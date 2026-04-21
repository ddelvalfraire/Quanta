use std::fmt;

/// Maximum tick rate for WebSocket clients (Hz).
pub const WS_TICK_RATE: u32 = 10;

/// Maximum number of entities replicated to a WebSocket client.
pub const WS_MAX_ENTITIES: u32 = 100;

/// Maximum outbound bytes per second to a WebSocket client.
pub const WS_MAX_BYTES_PER_SEC: u32 = 10_240;

/// 4-level graceful degradation based on CPU load.
///
/// | Level | Trigger | Action |
/// |-------|---------|--------|
/// | Normal | CPU < 70% | Full processing |
/// | Elevated | CPU 70-85% | Reduce LOD tier 3 divisor, skip cosmetic effects |
/// | Stressed | CPU 85-95% | Reduce max visible entities by 25%, skip tier 3 |
/// | Critical | CPU > 95% | Emergency passivate idle islands, reject new activations |
///
/// Tick rate is **never** reduced — gameplay fidelity is non-negotiable.
/// Level transitions require 3 consecutive measurements (debounce).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DegradationLevel {
    Normal,
    Elevated,
    Stressed,
    Critical,
}

impl fmt::Display for DegradationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl DegradationLevel {
    fn for_cpu(cpu: f32) -> Self {
        if cpu >= 0.95 {
            Self::Critical
        } else if cpu >= 0.85 {
            Self::Stressed
        } else if cpu >= 0.70 {
            Self::Elevated
        } else {
            Self::Normal
        }
    }
}

const DEBOUNCE_THRESHOLD: u32 = 3;

pub struct DegradationController {
    level: DegradationLevel,
    pending_level: DegradationLevel,
    consecutive_count: u32,
}

impl DegradationController {
    pub fn new() -> Self {
        Self {
            level: DegradationLevel::Normal,
            pending_level: DegradationLevel::Normal,
            consecutive_count: 0,
        }
    }

    pub fn current_level(&self) -> DegradationLevel {
        self.level
    }

    pub fn record_cpu_sample(&mut self, cpu: f32) -> Option<DegradationLevel> {
        let target = DegradationLevel::for_cpu(cpu);

        if target == self.pending_level {
            self.consecutive_count += 1;
        } else {
            self.pending_level = target;
            self.consecutive_count = 1;
        }

        if self.consecutive_count >= DEBOUNCE_THRESHOLD && self.pending_level != self.level {
            self.level = self.pending_level;
            Some(self.level)
        } else {
            None
        }
    }

    pub fn should_reject_activations(&self) -> bool {
        self.level == DegradationLevel::Critical
    }

    pub fn should_skip_cosmetics(&self) -> bool {
        self.level >= DegradationLevel::Elevated
    }

    pub fn should_skip_tier3(&self) -> bool {
        self.level >= DegradationLevel::Stressed
    }
}

impl Default for DegradationController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_normal() {
        let ctrl = DegradationController::new();
        assert_eq!(ctrl.current_level(), DegradationLevel::Normal);
        assert!(!ctrl.should_reject_activations());
        assert!(!ctrl.should_skip_cosmetics());
        assert!(!ctrl.should_skip_tier3());
    }

    #[test]
    fn single_spike_no_transition() {
        let mut ctrl = DegradationController::new();
        assert_eq!(ctrl.record_cpu_sample(0.90), None); // 1 sample
        assert_eq!(ctrl.current_level(), DegradationLevel::Normal);
    }

    #[test]
    fn two_spikes_no_transition() {
        let mut ctrl = DegradationController::new();
        ctrl.record_cpu_sample(0.90); // 1
        assert_eq!(ctrl.record_cpu_sample(0.90), None); // 2
        assert_eq!(ctrl.current_level(), DegradationLevel::Normal);
    }

    #[test]
    fn three_consecutive_transitions() {
        let mut ctrl = DegradationController::new();
        ctrl.record_cpu_sample(0.90); // 1
        ctrl.record_cpu_sample(0.90); // 2
        let result = ctrl.record_cpu_sample(0.90); // 3 → transition
        assert_eq!(result, Some(DegradationLevel::Stressed));
        assert_eq!(ctrl.current_level(), DegradationLevel::Stressed);
    }

    #[test]
    fn interrupted_spike_resets_debounce() {
        let mut ctrl = DegradationController::new();
        ctrl.record_cpu_sample(0.90); // Stressed candidate, count=1
        ctrl.record_cpu_sample(0.90); // count=2
        ctrl.record_cpu_sample(0.50); // Normal candidate, count=1 (resets)
        ctrl.record_cpu_sample(0.90); // Stressed candidate, count=1 (resets)
        assert_eq!(ctrl.record_cpu_sample(0.90), None); // count=2, not yet
        assert_eq!(ctrl.current_level(), DegradationLevel::Normal);
    }

    #[test]
    fn normal_to_elevated() {
        let mut ctrl = DegradationController::new();
        ctrl.record_cpu_sample(0.75);
        ctrl.record_cpu_sample(0.75);
        let result = ctrl.record_cpu_sample(0.75);
        assert_eq!(result, Some(DegradationLevel::Elevated));
        assert!(ctrl.should_skip_cosmetics());
        assert!(!ctrl.should_skip_tier3());
        assert!(!ctrl.should_reject_activations());
    }

    #[test]
    fn normal_to_critical() {
        let mut ctrl = DegradationController::new();
        ctrl.record_cpu_sample(0.96);
        ctrl.record_cpu_sample(0.96);
        let result = ctrl.record_cpu_sample(0.96);
        assert_eq!(result, Some(DegradationLevel::Critical));
        assert!(ctrl.should_reject_activations());
        assert!(ctrl.should_skip_cosmetics());
        assert!(ctrl.should_skip_tier3());
    }

    #[test]
    fn stressed_back_to_normal() {
        let mut ctrl = DegradationController::new();
        // Go to Stressed
        ctrl.record_cpu_sample(0.90);
        ctrl.record_cpu_sample(0.90);
        ctrl.record_cpu_sample(0.90);
        assert_eq!(ctrl.current_level(), DegradationLevel::Stressed);

        // Cool down to Normal
        ctrl.record_cpu_sample(0.50);
        ctrl.record_cpu_sample(0.50);
        let result = ctrl.record_cpu_sample(0.50);
        assert_eq!(result, Some(DegradationLevel::Normal));
    }

    #[test]
    fn same_level_after_transition_no_repeat() {
        let mut ctrl = DegradationController::new();
        ctrl.record_cpu_sample(0.75);
        ctrl.record_cpu_sample(0.75);
        ctrl.record_cpu_sample(0.75); // → Elevated
                                      // Continue at same CPU — no new transition
        assert_eq!(ctrl.record_cpu_sample(0.75), None);
        assert_eq!(ctrl.record_cpu_sample(0.75), None);
    }

    #[test]
    fn level_thresholds_exact_boundaries() {
        assert_eq!(DegradationLevel::for_cpu(0.69), DegradationLevel::Normal);
        assert_eq!(DegradationLevel::for_cpu(0.70), DegradationLevel::Elevated);
        assert_eq!(DegradationLevel::for_cpu(0.84), DegradationLevel::Elevated);
        assert_eq!(DegradationLevel::for_cpu(0.85), DegradationLevel::Stressed);
        assert_eq!(DegradationLevel::for_cpu(0.94), DegradationLevel::Stressed);
        assert_eq!(DegradationLevel::for_cpu(0.95), DegradationLevel::Critical);
        assert_eq!(DegradationLevel::for_cpu(1.0), DegradationLevel::Critical);
    }
}
