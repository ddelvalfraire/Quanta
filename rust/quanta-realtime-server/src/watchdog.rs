use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct HeartbeatWatchdog {
    heartbeat: Arc<AtomicU64>,
    last_seen: u64,
    last_check: Instant,
    stale_threshold: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogStatus {
    Alive,
    Stale { since: Duration },
    Hung { since: Duration },
}

impl HeartbeatWatchdog {
    pub fn new(heartbeat: Arc<AtomicU64>, stale_threshold: Duration) -> Self {
        let current = heartbeat.load(Ordering::Relaxed);
        Self {
            heartbeat,
            last_seen: current,
            last_check: Instant::now(),
            stale_threshold,
        }
    }

    pub fn check(&mut self) -> WatchdogStatus {
        let current = self.heartbeat.load(Ordering::Relaxed);
        let now = Instant::now();

        if current != self.last_seen {
            self.last_seen = current;
            self.last_check = now;
            WatchdogStatus::Alive
        } else {
            let since = now - self.last_check;
            if since >= self.stale_threshold {
                WatchdogStatus::Hung { since }
            } else {
                WatchdogStatus::Stale { since }
            }
        }
    }

    pub fn last_heartbeat(&self) -> u64 {
        self.last_seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alive_when_heartbeat_advances() {
        let hb = Arc::new(AtomicU64::new(0));
        let mut wd = HeartbeatWatchdog::new(hb.clone(), Duration::from_secs(5));

        hb.store(1, Ordering::Relaxed);
        assert_eq!(wd.check(), WatchdogStatus::Alive);

        hb.store(2, Ordering::Relaxed);
        assert_eq!(wd.check(), WatchdogStatus::Alive);
    }

    #[test]
    fn stale_when_heartbeat_unchanged() {
        let hb = Arc::new(AtomicU64::new(0));
        let mut wd = HeartbeatWatchdog::new(hb.clone(), Duration::from_secs(5));

        // Heartbeat doesn't change — should be Stale (not Hung yet)
        let status = wd.check();
        match status {
            WatchdogStatus::Stale { .. } | WatchdogStatus::Alive => {}
            WatchdogStatus::Hung { .. } => panic!("should not be hung immediately"),
        }
    }

    #[test]
    fn hung_after_threshold() {
        let hb = Arc::new(AtomicU64::new(0));
        // Use a very short threshold so we can test it quickly
        let mut wd = HeartbeatWatchdog::new(hb.clone(), Duration::from_millis(10));

        std::thread::sleep(Duration::from_millis(15));
        let status = wd.check();
        assert!(
            matches!(status, WatchdogStatus::Hung { .. }),
            "should be hung after threshold, got: {status:?}"
        );
    }

    #[test]
    fn recovery_from_stale() {
        let hb = Arc::new(AtomicU64::new(0));
        let mut wd = HeartbeatWatchdog::new(hb.clone(), Duration::from_secs(5));

        // Check without advancing — Stale
        wd.check();

        // Now advance — should be Alive again
        hb.store(1, Ordering::Relaxed);
        assert_eq!(wd.check(), WatchdogStatus::Alive);
    }

    #[test]
    fn last_heartbeat_tracks_value() {
        let hb = Arc::new(AtomicU64::new(0));
        let mut wd = HeartbeatWatchdog::new(hb.clone(), Duration::from_secs(5));
        assert_eq!(wd.last_heartbeat(), 0);

        hb.store(42, Ordering::Relaxed);
        wd.check();
        assert_eq!(wd.last_heartbeat(), 42);
    }
}
