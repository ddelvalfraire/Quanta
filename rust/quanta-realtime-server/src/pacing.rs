use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::debug;

use crate::session::Session;

const MAX_PACING_MULTIPLIER: f64 = 3.0;

pub struct DatagramBatch {
    pub datagrams: Vec<Bytes>,
}

/// A bounded sender that drains one item to make room when full.
///
/// Uses a cloned crossbeam `Receiver` to drain. This is best-effort:
/// under concurrent consumers the drained item may not be the oldest.
/// Designed for single-producer use (one island tick thread per client).
pub struct DropOldestSender<T> {
    tx: Sender<T>,
    drain_rx: Receiver<T>,
}

impl<T> DropOldestSender<T> {
    pub fn new(bound: usize) -> (Self, Receiver<T>) {
        let (tx, rx) = bounded(bound);
        let drain_rx = rx.clone();
        (Self { tx, drain_rx }, rx)
    }

    pub fn try_send(&self, item: T) -> Result<(), T> {
        match self.tx.try_send(item) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(item)) => {
                let _ = self.drain_rx.try_recv();
                debug!("pacing channel full, drained oldest batch");
                self.tx.try_send(item).map_err(|e| e.into_inner())
            }
            Err(TrySendError::Disconnected(item)) => Err(item),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PacingConfig {
    pub tick_period: Duration,
    pub channel_bound: usize,
    pub high_rtt_threshold: Duration,
    pub high_loss_threshold: f64,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            tick_period: Duration::from_millis(50),
            channel_bound: 4,
            high_rtt_threshold: Duration::from_millis(100),
            high_loss_threshold: 0.05,
        }
    }
}

pub struct PacingHandle {
    sender: DropOldestSender<DatagramBatch>,
    task: JoinHandle<()>,
}

impl PacingHandle {
    pub fn spawn(session: Arc<dyn Session>, config: PacingConfig) -> Self {
        let (sender, rx) = DropOldestSender::new(config.channel_bound);
        let task = tokio::spawn(pacing_loop(session, rx, config));
        Self { sender, task }
    }

    pub fn send_batch(&self, batch: DatagramBatch) {
        let _ = self.sender.try_send(batch);
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for PacingHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn pacing_loop(session: Arc<dyn Session>, rx: Receiver<DatagramBatch>, config: PacingConfig) {
    // Poll crossbeam rx on a 1ms interval since crossbeam channels are
    // synchronous and cannot be .await-ed. This bridges the sync tick
    // thread with the async pacing task.
    let mut poll_interval = tokio::time::interval(Duration::from_millis(1));
    poll_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        poll_interval.tick().await;

        let batch = match rx.try_recv() {
            Ok(b) => b,
            Err(crossbeam_channel::TryRecvError::Empty) => continue,
            Err(crossbeam_channel::TryRecvError::Disconnected) => return,
        };

        if batch.datagrams.is_empty() {
            continue;
        }

        let rtt = session.rtt();
        let stats = session.transport_stats();

        let loss_rate = if stats.sent_packets > 0 {
            stats.lost_packets as f64 / stats.sent_packets as f64
        } else {
            0.0
        };

        // Reduce datagram count when loss exceeds threshold.
        // Linear reduction from 100% at threshold to 50% at 2x threshold.
        let batch_len = batch.datagrams.len();
        let send_count = if loss_rate > config.high_loss_threshold {
            let excess =
                ((loss_rate - config.high_loss_threshold) / config.high_loss_threshold).min(1.0);
            let keep_fraction = 1.0 - 0.5 * excess;
            let count = ((keep_fraction * batch_len as f64).ceil() as usize).max(1);
            debug!(
                batch_len,
                send_count = count,
                loss_rate,
                "reducing datagram count due to packet loss"
            );
            count
        } else {
            batch_len
        };

        let interval = compute_pacing_period(send_count as u32, rtt, &config);

        let mut send_timer = tokio::time::interval(interval);
        send_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

        for datagram in batch.datagrams.iter().take(send_count) {
            send_timer.tick().await;
            if session.send_unreliable(datagram).is_ok() {
                crate::metrics::METRICS.datagrams_sent.inc();
                crate::metrics::METRICS
                    .bytes_sent
                    .inc_by(datagram.len() as u64);
            }
        }
    }
}

pub fn compute_pacing_period(
    datagram_count: u32,
    rtt: Duration,
    config: &PacingConfig,
) -> Duration {
    if datagram_count == 0 {
        return config.tick_period;
    }

    let base = config.tick_period / datagram_count;

    let rtt_multiplier = if rtt > config.high_rtt_threshold {
        let excess = rtt.saturating_sub(config.high_rtt_threshold).as_secs_f64();
        let threshold_secs = config.high_rtt_threshold.as_secs_f64();
        (1.0 + excess / threshold_secs).min(MAX_PACING_MULTIPLIER)
    } else {
        1.0
    };

    base.mul_f64(rtt_multiplier)
}
