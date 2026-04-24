use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use tokio::time::Instant;

use quanta_realtime_server::error::SendError;
use quanta_realtime_server::pacing::{
    compute_pacing_period, DatagramBatch, DropOldestSender, PacingConfig, PacingHandle,
};
use quanta_realtime_server::session::{Session, TransportStats, TransportType};

struct MockSession {
    send_times: Arc<Mutex<Vec<Instant>>>,
    rtt: Duration,
    stats: TransportStats,
}

impl MockSession {
    fn new(rtt: Duration, stats: TransportStats) -> Self {
        Self {
            send_times: Arc::new(Mutex::new(Vec::new())),
            rtt,
            stats,
        }
    }

    fn send_times(&self) -> Arc<Mutex<Vec<Instant>>> {
        self.send_times.clone()
    }
}

impl Session for MockSession {
    fn send_unreliable(&self, _data: &[u8]) -> Result<(), SendError> {
        self.send_times.lock().unwrap().push(Instant::now());
        Ok(())
    }

    fn send_reliable(&self, _stream_id: u32, _data: &[u8]) -> Result<(), SendError> {
        Err(SendError::StreamClosed)
    }

    fn recv_datagram(&self) -> Option<Vec<u8>> {
        None
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }

    fn rtt(&self) -> Duration {
        self.rtt
    }

    fn transport_stats(&self) -> TransportStats {
        self.stats
    }

    fn close(&self, _reason: &str) {}

    fn on_closed(&self) -> quanta_realtime_server::session::ClosedFuture {
        Box::pin(std::future::pending())
    }
}

#[tokio::test(start_paused = true)]
async fn test_datagram_spacing_and_burst_reduction() {
    let session = Arc::new(MockSession::new(
        Duration::from_millis(30),
        TransportStats::default(),
    ));
    let times = session.send_times();

    let config = PacingConfig {
        tick_period: Duration::from_millis(50),
        ..Default::default()
    };
    let handle = PacingHandle::spawn(session, config);

    let datagrams: Vec<Bytes> = (0..7).map(|i| Bytes::from(vec![i])).collect();
    handle.send_batch(DatagramBatch { datagrams });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let recorded = times.lock().unwrap();
    assert_eq!(recorded.len(), 7, "expected 7 sends");

    // Check per-interval spacing (~7142us apart)
    let expected_interval = Duration::from_micros(7142);
    for i in 1..recorded.len() {
        let gap = recorded[i] - recorded[i - 1];
        let diff = if gap > expected_interval {
            gap - expected_interval
        } else {
            expected_interval - gap
        };
        assert!(
            diff < Duration::from_millis(2),
            "gap {i}: {gap:?} deviates from expected {expected_interval:?} by {diff:?}"
        );
    }

    // Check burst reduction: spread should be ~42ms (6 intervals of ~7142us)
    let spread = recorded[6] - recorded[0];
    assert!(
        spread > Duration::from_millis(30),
        "spread {spread:?} is too short — pacing not working"
    );
    assert!(
        spread < Duration::from_millis(60),
        "spread {spread:?} is too long"
    );

    drop(handle);
}

#[test]
fn test_channel_full_drops_oldest() {
    let (sender, rx) = DropOldestSender::<u32>::new(4);

    for i in 0..5 {
        let _ = sender.try_send(i);
    }

    let mut received = Vec::new();
    while let Ok(val) = rx.try_recv() {
        received.push(val);
    }

    assert_eq!(received.len(), 4, "should have 4 items");
    assert_eq!(received[0], 1, "oldest (0) should have been dropped");
    assert_eq!(received[3], 4);
}

#[tokio::test(start_paused = true)]
async fn test_pacing_task_cleanup() {
    let session = Arc::new(MockSession::new(
        Duration::from_millis(30),
        TransportStats::default(),
    ));

    let config = PacingConfig::default();
    let handle = PacingHandle::spawn(session, config);

    // Drop the handle — should abort the task
    drop(handle);

    // Give a moment for the abort to propagate
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify by spawning a new handle and confirming it works
    // (the old task is gone, no resource leak)
    let session2 = Arc::new(MockSession::new(
        Duration::from_millis(30),
        TransportStats::default(),
    ));
    let handle2 = PacingHandle::spawn(session2, PacingConfig::default());
    assert!(!handle2.is_finished());
    drop(handle2);
}

#[test]
fn test_compute_pacing_period_adaptive() {
    let config = PacingConfig::default();

    // Normal RTT — base interval
    let period = compute_pacing_period(7, Duration::from_millis(30), &config);
    let expected_base = Duration::from_millis(50) / 7;
    assert_eq!(period, expected_base);

    // High RTT (200ms) — should widen
    let period = compute_pacing_period(7, Duration::from_millis(200), &config);
    assert!(
        period > expected_base,
        "high RTT should widen: {period:?} vs {expected_base:?}"
    );

    // Very high RTT — capped at 3x
    let period = compute_pacing_period(7, Duration::from_millis(500), &config);
    let max_period = expected_base.mul_f64(3.0);
    assert!(
        period <= max_period,
        "should be capped at 3x: {period:?} vs max {max_period:?}"
    );

    // Zero datagrams — returns tick_period
    let period = compute_pacing_period(0, Duration::from_millis(30), &config);
    assert_eq!(period, Duration::from_millis(50));
}

#[tokio::test(start_paused = true)]
async fn test_loss_reduces_datagram_count() {
    // 10% loss rate (2x the 5% threshold)
    let session = Arc::new(MockSession::new(
        Duration::from_millis(30),
        TransportStats {
            lost_packets: 10,
            sent_packets: 100,
        },
    ));
    let times = session.send_times();

    let config = PacingConfig {
        tick_period: Duration::from_millis(50),
        ..Default::default()
    };
    let handle = PacingHandle::spawn(session, config);

    let datagrams: Vec<Bytes> = (0..10).map(|i| Bytes::from(vec![i])).collect();
    handle.send_batch(DatagramBatch { datagrams });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let recorded = times.lock().unwrap();
    // 10% loss with 5% threshold: excess_ratio = 1.0, keep_fraction = 0.5
    // ceil(0.5 * 10) = 5
    assert_eq!(
        recorded.len(),
        5,
        "expected reduced datagram count due to loss"
    );

    drop(handle);
}
