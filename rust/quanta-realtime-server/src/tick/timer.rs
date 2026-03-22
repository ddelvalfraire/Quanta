use crate::types::EntitySlot;
use std::collections::HashMap;

/// Manages named timers per entity. Timers fire at tick boundaries.
///
/// Timers are keyed by `(EntitySlot, name)` — setting a timer with the same
/// name overwrites the previous one (no duplicates).
pub struct TimerManager {
    timers: HashMap<(EntitySlot, String), u64>,
    current_tick: u64,
    tick_rate_hz: u8,
}

impl TimerManager {
    pub fn new(tick_rate_hz: u8) -> Self {
        Self {
            timers: HashMap::new(),
            current_tick: 0,
            tick_rate_hz,
        }
    }

    pub fn set_current_tick(&mut self, tick: u64) {
        self.current_tick = tick;
    }

    /// Schedule a timer. Overwrites any existing timer with the same (entity, name).
    /// Minimum delay is 1 tick.
    pub fn set_timer(&mut self, entity: EntitySlot, name: String, delay_ms: u32) {
        let tick_period_ms = 1000.0 / self.tick_rate_hz as f64;
        let delay_ticks = (delay_ms as f64 / tick_period_ms).ceil() as u64;
        let fire_tick = self.current_tick + delay_ticks.max(1);
        self.timers.insert((entity, name), fire_tick);
    }

    /// Cancel a timer. Returns true if the timer existed.
    pub fn cancel_timer(&mut self, entity: &EntitySlot, name: &str) -> bool {
        self.timers.remove(&(*entity, name.to_string())).is_some()
    }

    /// Fire all timers with `fire_tick <= current_tick`. Returns fired (entity, name) pairs.
    pub fn fire_elapsed(&mut self, current_tick: u64) -> Vec<(EntitySlot, String)> {
        let mut fired = Vec::new();
        self.timers.retain(|(entity, name), fire_tick| {
            if *fire_tick <= current_tick {
                fired.push((*entity, name.clone()));
                false
            } else {
                true
            }
        });
        fired
    }

    /// Remove all timers for a given entity.
    pub fn clear_entity(&mut self, entity: &EntitySlot) {
        self.timers.retain(|(e, _), _| e != entity);
    }

    pub fn timer_count(&self) -> usize {
        self.timers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(n: u32) -> EntitySlot {
        EntitySlot(n)
    }

    #[test]
    fn set_and_fire_timer() {
        let mut tm = TimerManager::new(20); // 50ms per tick
        tm.set_current_tick(0);
        tm.set_timer(slot(1), "attack".into(), 100); // 100ms = 2 ticks at 20Hz

        assert_eq!(tm.fire_elapsed(0), vec![]); // tick 0: not yet
        assert_eq!(tm.fire_elapsed(1), vec![]); // tick 1: not yet
        let fired = tm.fire_elapsed(2); // tick 2: fires
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0], (slot(1), "attack".to_string()));
    }

    #[test]
    fn timer_fires_exactly_on_target_tick() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(5);
        tm.set_timer(slot(1), "t".into(), 50); // 50ms = 1 tick at 20Hz

        assert!(tm.fire_elapsed(5).is_empty()); // set on tick 5, fires on tick 6
        let fired = tm.fire_elapsed(6);
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn minimum_delay_is_one_tick() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(10);
        tm.set_timer(slot(1), "t".into(), 0); // 0ms → still 1 tick minimum

        assert!(tm.fire_elapsed(10).is_empty());
        assert_eq!(tm.fire_elapsed(11).len(), 1);
    }

    #[test]
    fn cancel_timer() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(0);
        tm.set_timer(slot(1), "heal".into(), 100);

        assert!(tm.cancel_timer(&slot(1), "heal"));
        assert!(tm.fire_elapsed(100).is_empty()); // cancelled, never fires
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let mut tm = TimerManager::new(20);
        assert!(!tm.cancel_timer(&slot(1), "nope"));
    }

    #[test]
    fn overwrite_timer_with_same_name() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(0);
        tm.set_timer(slot(1), "t".into(), 50); // fires tick 1
        tm.set_timer(slot(1), "t".into(), 150); // overwrites → fires tick 3

        assert!(tm.fire_elapsed(1).is_empty());
        assert!(tm.fire_elapsed(2).is_empty());
        assert_eq!(tm.fire_elapsed(3).len(), 1);
    }

    #[test]
    fn clear_entity_removes_all_timers() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(0);
        tm.set_timer(slot(1), "a".into(), 50);
        tm.set_timer(slot(1), "b".into(), 100);
        tm.set_timer(slot(2), "a".into(), 50);

        tm.clear_entity(&slot(1));
        assert_eq!(tm.timer_count(), 1); // only slot(2)'s timer remains
    }

    #[test]
    fn multiple_timers_fire_same_tick() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(0);
        tm.set_timer(slot(1), "a".into(), 100);
        tm.set_timer(slot(2), "b".into(), 100);

        let fired = tm.fire_elapsed(2);
        assert_eq!(fired.len(), 2);
    }

    #[test]
    fn late_fire_catches_up() {
        let mut tm = TimerManager::new(20);
        tm.set_current_tick(0);
        tm.set_timer(slot(1), "t".into(), 50); // fires tick 1

        // Skipped tick 1, now at tick 5 — timer should still fire
        let fired = tm.fire_elapsed(5);
        assert_eq!(fired.len(), 1);
    }
}
