use std::fmt;

/// Bridge connection health states.
///
/// State machine:
/// ```text
/// Healthy -> Degraded  (1 missed heartbeat or buffer >= 50%)
/// Degraded -> Disconnected  (3 missed heartbeats or buffer >= 100%)
/// Disconnected -> Degraded  (heartbeat received)
/// Degraded -> Healthy  (3 consecutive heartbeats and buffer < 25%)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeHealthState {
    Healthy,
    Degraded,
    Disconnected,
}

impl fmt::Display for BridgeHealthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

pub struct BridgeHealthTracker {
    state: BridgeHealthState,
    missed_heartbeats: u32,
    consecutive_heartbeats: u32,
    buffer_ratio: f32,
}

impl BridgeHealthTracker {
    pub fn new() -> Self {
        Self {
            state: BridgeHealthState::Healthy,
            missed_heartbeats: 0,
            consecutive_heartbeats: 0,
            buffer_ratio: 0.0,
        }
    }

    pub fn state(&self) -> BridgeHealthState {
        self.state
    }

    pub fn missed_heartbeats(&self) -> u32 {
        self.missed_heartbeats
    }

    pub fn consecutive_heartbeats(&self) -> u32 {
        self.consecutive_heartbeats
    }

    pub fn record_heartbeat(&mut self) {
        self.missed_heartbeats = 0;
        self.consecutive_heartbeats += 1;

        match self.state {
            BridgeHealthState::Disconnected => {
                self.state = BridgeHealthState::Degraded;
                self.consecutive_heartbeats = 1;
            }
            BridgeHealthState::Degraded => {
                if self.consecutive_heartbeats >= 3 && self.buffer_ratio < 0.25 {
                    self.state = BridgeHealthState::Healthy;
                }
            }
            BridgeHealthState::Healthy => {}
        }
    }

    pub fn record_missed_heartbeat(&mut self) {
        self.missed_heartbeats += 1;
        self.consecutive_heartbeats = 0;

        match self.state {
            BridgeHealthState::Healthy => {
                self.state = BridgeHealthState::Degraded;
            }
            BridgeHealthState::Degraded => {
                if self.missed_heartbeats >= 3 {
                    self.state = BridgeHealthState::Disconnected;
                }
            }
            BridgeHealthState::Disconnected => {}
        }
    }

    pub fn update_buffer_ratio(&mut self, ratio: f32) {
        self.buffer_ratio = ratio.clamp(0.0, 1.0);

        match self.state {
            BridgeHealthState::Healthy => {
                if self.buffer_ratio >= 0.5 {
                    self.state = BridgeHealthState::Degraded;
                    self.consecutive_heartbeats = 0;
                }
            }
            BridgeHealthState::Degraded => {
                if self.buffer_ratio >= 1.0 {
                    self.state = BridgeHealthState::Disconnected;
                    self.consecutive_heartbeats = 0;
                }
            }
            BridgeHealthState::Disconnected => {}
        }
    }
}

impl Default for BridgeHealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_healthy() {
        let tracker = BridgeHealthTracker::new();
        assert_eq!(tracker.state(), BridgeHealthState::Healthy);
    }

    #[test]
    fn one_missed_heartbeat_degrades() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_missed_heartbeat();
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);
    }

    #[test]
    fn three_missed_heartbeats_disconnects() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_missed_heartbeat(); // Healthy → Degraded
        tracker.record_missed_heartbeat(); // still Degraded (2)
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);
        tracker.record_missed_heartbeat(); // Degraded → Disconnected (3)
        assert_eq!(tracker.state(), BridgeHealthState::Disconnected);
    }

    #[test]
    fn heartbeat_from_disconnected_goes_to_degraded() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_missed_heartbeat();
        tracker.record_missed_heartbeat();
        tracker.record_missed_heartbeat();
        assert_eq!(tracker.state(), BridgeHealthState::Disconnected);

        tracker.record_heartbeat();
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);
    }

    #[test]
    fn three_heartbeats_and_low_buffer_restores_healthy() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_missed_heartbeat(); // → Degraded
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);

        tracker.update_buffer_ratio(0.1); // buffer low
        tracker.record_heartbeat(); // 1
        tracker.record_heartbeat(); // 2
        assert_eq!(tracker.state(), BridgeHealthState::Degraded); // not yet
        tracker.record_heartbeat(); // 3 → Healthy
        assert_eq!(tracker.state(), BridgeHealthState::Healthy);
    }

    #[test]
    fn three_heartbeats_but_high_buffer_stays_degraded() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_missed_heartbeat(); // → Degraded

        tracker.update_buffer_ratio(0.30); // buffer >= 25%
        tracker.record_heartbeat();
        tracker.record_heartbeat();
        tracker.record_heartbeat();
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);
    }

    #[test]
    fn buffer_50_degrades_from_healthy() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.update_buffer_ratio(0.5);
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);
    }

    #[test]
    fn buffer_100_disconnects_from_degraded() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_missed_heartbeat(); // → Degraded
        tracker.update_buffer_ratio(1.0);
        assert_eq!(tracker.state(), BridgeHealthState::Disconnected);
    }

    #[test]
    fn recovery_from_disconnected_to_healthy() {
        let mut tracker = BridgeHealthTracker::new();
        // Go to Disconnected
        for _ in 0..3 {
            tracker.record_missed_heartbeat();
        }
        assert_eq!(tracker.state(), BridgeHealthState::Disconnected);

        // Heartbeat → Degraded
        tracker.record_heartbeat();
        assert_eq!(tracker.state(), BridgeHealthState::Degraded);

        // 3 consecutive heartbeats with low buffer → Healthy
        tracker.update_buffer_ratio(0.1);
        tracker.record_heartbeat(); // 2nd since recovery (1st was the one that moved to Degraded)
        tracker.record_heartbeat(); // 3rd
        assert_eq!(tracker.state(), BridgeHealthState::Healthy);
    }

    #[test]
    fn healthy_stays_healthy_on_heartbeat() {
        let mut tracker = BridgeHealthTracker::new();
        tracker.record_heartbeat();
        tracker.record_heartbeat();
        assert_eq!(tracker.state(), BridgeHealthState::Healthy);
    }

    #[test]
    fn missed_heartbeats_beyond_3_stay_disconnected() {
        let mut tracker = BridgeHealthTracker::new();
        for _ in 0..10 {
            tracker.record_missed_heartbeat();
        }
        assert_eq!(tracker.state(), BridgeHealthState::Disconnected);
    }
}
