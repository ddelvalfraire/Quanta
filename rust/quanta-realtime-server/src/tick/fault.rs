use crate::types::EntitySlot;
use std::collections::HashMap;

/// Health state of an actor entity for progressive fault quarantine.
///
/// Escalation ladder (by consecutive `record_fault` calls):
/// - 1-2 faults: Warned (logged, no skip)
/// - 3 faults: Quarantined 10 ticks
/// - 4 faults: Quarantined 60 ticks
/// - 5+ faults: Evicted (removed from island)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorHealthState {
    Healthy,
    Warned { consecutive_faults: u32 },
    Quarantined { consecutive_faults: u32, resume_at_tick: u64 },
    Evicted,
}

/// Tracks per-entity faults and applies progressive quarantine.
pub struct FaultTracker {
    faults: HashMap<EntitySlot, ActorHealthState>,
}

impl FaultTracker {
    pub fn new() -> Self {
        Self {
            faults: HashMap::new(),
        }
    }

    pub fn record_fault(&mut self, entity: &EntitySlot, current_tick: u64) {
        const EVICTION_THRESHOLD: u32 = 5;
        const SHORT_QUARANTINE_TICKS: u64 = 10;
        const LONG_QUARANTINE_TICKS: u64 = 60;

        let entry = self
            .faults
            .entry(*entity)
            .or_insert(ActorHealthState::Healthy);
        *entry = match entry {
            ActorHealthState::Healthy => ActorHealthState::Warned {
                consecutive_faults: 1,
            },
            ActorHealthState::Warned {
                consecutive_faults,
            } => {
                let n = *consecutive_faults + 1;
                if n >= EVICTION_THRESHOLD {
                    ActorHealthState::Evicted
                } else if n < 3 {
                    ActorHealthState::Warned {
                        consecutive_faults: n,
                    }
                } else {
                    let skip = if n == 3 {
                        SHORT_QUARANTINE_TICKS
                    } else {
                        LONG_QUARANTINE_TICKS
                    };
                    ActorHealthState::Quarantined {
                        consecutive_faults: n,
                        resume_at_tick: current_tick + skip,
                    }
                }
            }
            ActorHealthState::Quarantined {
                consecutive_faults, ..
            } => {
                let n = *consecutive_faults + 1;
                if n >= EVICTION_THRESHOLD {
                    ActorHealthState::Evicted
                } else {
                    ActorHealthState::Quarantined {
                        consecutive_faults: n,
                        resume_at_tick: current_tick + LONG_QUARANTINE_TICKS,
                    }
                }
            }
            ActorHealthState::Evicted => ActorHealthState::Evicted,
        };
    }

    /// A successful tick resets the entity to Healthy.
    pub fn record_success(&mut self, entity: &EntitySlot) {
        self.faults.remove(entity);
    }

    pub fn should_tick(&self, entity: &EntitySlot, current_tick: u64) -> bool {
        match self.faults.get(entity) {
            None
            | Some(ActorHealthState::Healthy)
            | Some(ActorHealthState::Warned { .. }) => true,
            Some(ActorHealthState::Quarantined {
                resume_at_tick, ..
            }) => current_tick >= *resume_at_tick,
            Some(ActorHealthState::Evicted) => false,
        }
    }

    pub fn get_state(&self, entity: &EntitySlot) -> ActorHealthState {
        self.faults
            .get(entity)
            .cloned()
            .unwrap_or(ActorHealthState::Healthy)
    }
}

impl Default for FaultTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(n: u32) -> EntitySlot {
        EntitySlot(n)
    }

    #[test]
    fn healthy_by_default() {
        let ft = FaultTracker::new();
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Healthy);
        assert!(ft.should_tick(&slot(1), 0));
    }

    #[test]
    fn first_fault_warns() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Warned {
                consecutive_faults: 1
            }
        );
        assert!(ft.should_tick(&slot(1), 0)); // warned but still ticks
    }

    #[test]
    fn two_faults_still_warned() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0); // Warned(1)
        ft.record_fault(&slot(1), 1); // Warned(2)
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Warned {
                consecutive_faults: 2
            }
        );
        assert!(ft.should_tick(&slot(1), 1));
    }

    #[test]
    fn three_faults_quarantine_10_ticks() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0); // Warned(1)
        ft.record_fault(&slot(1), 1); // Warned(2)
        ft.record_fault(&slot(1), 2); // Quarantined(3, resume=12)
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 3,
                resume_at_tick: 12
            }
        );
        assert!(!ft.should_tick(&slot(1), 5));
        assert!(ft.should_tick(&slot(1), 12));
    }

    #[test]
    fn four_faults_quarantine_60_ticks() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0); // Warned(1)
        ft.record_fault(&slot(1), 1); // Warned(2)
        ft.record_fault(&slot(1), 2); // Quarantined(3, 12)
        ft.record_fault(&slot(1), 20); // Quarantined(4, 80)
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 4,
                resume_at_tick: 80
            }
        );
    }

    #[test]
    fn five_faults_evicts() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0); // Warned(1)
        ft.record_fault(&slot(1), 1); // Warned(2)
        ft.record_fault(&slot(1), 2); // Quarantined(3)
        ft.record_fault(&slot(1), 20); // Quarantined(4)
        ft.record_fault(&slot(1), 100); // Evicted
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Evicted);
        assert!(!ft.should_tick(&slot(1), 10000));
    }

    #[test]
    fn success_resets_to_healthy() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        ft.record_success(&slot(1));
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Healthy);
    }

    #[test]
    fn evicted_stays_evicted() {
        let mut ft = FaultTracker::new();
        for i in 0..10 {
            ft.record_fault(&slot(1), i);
        }
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Evicted);
        ft.record_fault(&slot(1), 100); // still evicted
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Evicted);
    }
}
