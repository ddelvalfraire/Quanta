use crate::types::EntitySlot;
use std::collections::HashMap;

/// What action the tick engine should take when a WASM trap occurs.
///
/// Escalation ladder by consecutive fault count:
/// - Fault 1, 2, 4: Skip (entity skipped this tick, retried after quarantine)
/// - Fault 3: Reset (entity state restored to last checkpoint)
/// - Fault 5: Recreate (entity destroyed and recreated with init state)
/// - Fault 6+: Evict (entity permanently removed from island, bridge notified)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapResponse {
    Skip,
    Reset,
    Recreate,
    Evict,
}

/// Health state of an actor entity for progressive fault quarantine.
///
/// Quarantine schedule (exponential backoff by consecutive faults):
/// - 1st fault: skip 1 tick
/// - 2nd fault: skip 2 ticks
/// - 3rd fault: skip 4 ticks (+ reset to checkpoint)
/// - 4th fault: skip 8 ticks
/// - 5th fault: skip 16 ticks (+ recreate with init state)
/// - 6th+ fault: evict entity from island
///
/// Fault counter resets after 100 consecutive clean ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorHealthState {
    Healthy,
    Quarantined {
        consecutive_faults: u32,
        resume_at_tick: u64,
    },
    Evicted,
}

#[derive(Debug, Clone)]
struct FaultEntry {
    consecutive_faults: u32,
    resume_at_tick: u64,
    clean_ticks: u32,
}

const QUARANTINE_TICKS: [u64; 5] = [1, 2, 4, 8, 16];
const RECREATE_THRESHOLD: u32 = 5;
const EVICTION_THRESHOLD: u32 = 6;
const CLEAN_TICK_RESET: u32 = 100;

pub struct FaultTracker {
    entries: HashMap<EntitySlot, FaultEntry>,
}

impl FaultTracker {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn record_fault(&mut self, entity: &EntitySlot, current_tick: u64) -> TrapResponse {
        let entry = self.entries.entry(*entity).or_insert(FaultEntry {
            consecutive_faults: 0,
            resume_at_tick: 0,
            clean_ticks: 0,
        });

        entry.consecutive_faults += 1;
        entry.clean_ticks = 0;

        if entry.consecutive_faults >= EVICTION_THRESHOLD {
            entry.resume_at_tick = u64::MAX;
            return TrapResponse::Evict;
        }

        let idx = (entry.consecutive_faults - 1) as usize;
        let skip = QUARANTINE_TICKS[idx.min(QUARANTINE_TICKS.len() - 1)];
        entry.resume_at_tick = current_tick + skip;

        match entry.consecutive_faults {
            3 => TrapResponse::Reset,
            n if n >= RECREATE_THRESHOLD => TrapResponse::Recreate,
            _ => TrapResponse::Skip,
        }
    }

    /// Resets fault state after 100 consecutive clean ticks.
    pub fn record_success(&mut self, entity: &EntitySlot) {
        if let Some(entry) = self.entries.get_mut(entity) {
            if entry.consecutive_faults >= EVICTION_THRESHOLD {
                return;
            }
            entry.clean_ticks += 1;
            if entry.clean_ticks >= CLEAN_TICK_RESET {
                self.entries.remove(entity);
            }
        }
    }

    pub fn should_tick(&self, entity: &EntitySlot, current_tick: u64) -> bool {
        match self.entries.get(entity) {
            None => true,
            Some(entry) => {
                if entry.consecutive_faults >= EVICTION_THRESHOLD {
                    false
                } else {
                    current_tick >= entry.resume_at_tick
                }
            }
        }
    }

    pub fn get_state(&self, entity: &EntitySlot) -> ActorHealthState {
        match self.entries.get(entity) {
            None => ActorHealthState::Healthy,
            Some(entry) => {
                if entry.consecutive_faults >= EVICTION_THRESHOLD {
                    ActorHealthState::Evicted
                } else {
                    ActorHealthState::Quarantined {
                        consecutive_faults: entry.consecutive_faults,
                        resume_at_tick: entry.resume_at_tick,
                    }
                }
            }
        }
    }

    pub fn remove_entity(&mut self, entity: &EntitySlot) {
        self.entries.remove(entity);
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
    fn first_fault_skip_quarantine_1() {
        let mut ft = FaultTracker::new();
        let resp = ft.record_fault(&slot(1), 10);
        assert_eq!(resp, TrapResponse::Skip);
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 1,
                resume_at_tick: 11,
            }
        );
        assert!(!ft.should_tick(&slot(1), 10));
        assert!(ft.should_tick(&slot(1), 11));
    }

    #[test]
    fn second_fault_skip_quarantine_2() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        let resp = ft.record_fault(&slot(1), 1);
        assert_eq!(resp, TrapResponse::Skip);
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 2,
                resume_at_tick: 3,
            }
        );
        assert!(!ft.should_tick(&slot(1), 2));
        assert!(ft.should_tick(&slot(1), 3));
    }

    #[test]
    fn third_fault_reset_quarantine_4() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        ft.record_fault(&slot(1), 1);
        let resp = ft.record_fault(&slot(1), 3);
        assert_eq!(resp, TrapResponse::Reset);
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 3,
                resume_at_tick: 7,
            }
        );
        assert!(!ft.should_tick(&slot(1), 6));
        assert!(ft.should_tick(&slot(1), 7));
    }

    #[test]
    fn fourth_fault_skip_quarantine_8() {
        let mut ft = FaultTracker::new();
        for i in 0..3 {
            ft.record_fault(&slot(1), i);
        }
        let resp = ft.record_fault(&slot(1), 10);
        assert_eq!(resp, TrapResponse::Skip);
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 4,
                resume_at_tick: 18,
            }
        );
    }

    #[test]
    fn fifth_fault_recreates_quarantine_16() {
        let mut ft = FaultTracker::new();
        for i in 0..4 {
            ft.record_fault(&slot(1), i);
        }
        let resp = ft.record_fault(&slot(1), 20);
        assert_eq!(resp, TrapResponse::Recreate);
        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 5,
                resume_at_tick: 36,
            }
        );
        assert!(!ft.should_tick(&slot(1), 35));
        assert!(ft.should_tick(&slot(1), 36));
    }

    #[test]
    fn six_faults_evicts() {
        let mut ft = FaultTracker::new();
        for i in 0..5 {
            ft.record_fault(&slot(1), i);
        }
        let resp = ft.record_fault(&slot(1), 50);
        assert_eq!(resp, TrapResponse::Evict);
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Evicted);
        assert!(!ft.should_tick(&slot(1), u64::MAX - 1));
    }

    #[test]
    fn evicted_stays_evicted() {
        let mut ft = FaultTracker::new();
        for i in 0..10 {
            ft.record_fault(&slot(1), i);
        }
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Evicted);
        ft.record_success(&slot(1));
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Evicted);
    }

    #[test]
    fn success_decrements_toward_healthy() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        ft.record_fault(&slot(1), 1);
        // 2 consecutive faults, now succeed
        for _ in 0..99 {
            ft.record_success(&slot(1));
        }
        // Not yet reset (99 < 100)
        assert!(matches!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined { .. }
        ));

        ft.record_success(&slot(1)); // 100th clean tick
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Healthy);
    }

    #[test]
    fn fault_resets_clean_tick_counter() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        // Accumulate 50 clean ticks
        for _ in 0..50 {
            ft.record_success(&slot(1));
        }
        // Fault again resets clean counter
        ft.record_fault(&slot(1), 100);
        // Now need another 100 clean ticks
        for _ in 0..99 {
            ft.record_success(&slot(1));
        }
        assert!(matches!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined { .. }
        ));
        ft.record_success(&slot(1));
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Healthy);
    }

    #[test]
    fn multiple_entities_independent() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        ft.record_fault(&slot(2), 0);
        ft.record_fault(&slot(2), 1);

        assert_eq!(
            ft.get_state(&slot(1)),
            ActorHealthState::Quarantined {
                consecutive_faults: 1,
                resume_at_tick: 1,
            }
        );
        assert_eq!(
            ft.get_state(&slot(2)),
            ActorHealthState::Quarantined {
                consecutive_faults: 2,
                resume_at_tick: 3,
            }
        );
    }

    #[test]
    fn remove_entity_clears_tracking() {
        let mut ft = FaultTracker::new();
        ft.record_fault(&slot(1), 0);
        ft.remove_entity(&slot(1));
        assert_eq!(ft.get_state(&slot(1)), ActorHealthState::Healthy);
    }
}
