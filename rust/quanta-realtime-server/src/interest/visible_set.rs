use crate::spatial::PositionTable;
use crate::types::EntitySlot;
use rustc_hash::{FxHashMap, FxHashSet};

pub struct VisibleSetDiff {
    pub entered: Vec<EntitySlot>,
    pub left: Vec<EntitySlot>,
}

pub struct VisibleSet {
    current: FxHashSet<EntitySlot>,
    pending_leaves: FxHashMap<EntitySlot, u8>,
    leave_repeat_count: u8,
}

impl VisibleSet {
    pub fn new(leave_repeat_count: u8) -> Self {
        assert!(
            leave_repeat_count > 0,
            "leave_repeat_count must be at least 1"
        );
        Self {
            current: FxHashSet::default(),
            pending_leaves: FxHashMap::default(),
            leave_repeat_count,
        }
    }

    pub fn update(
        &mut self,
        cx: f32,
        cz: f32,
        subscribe_radius: f32,
        unsubscribe_radius: f32,
        positions: &PositionTable,
        all_entities: &[EntitySlot],
    ) -> VisibleSetDiff {
        let sub_r2 = subscribe_radius * subscribe_radius;
        let unsub_r2 = unsubscribe_radius * unsubscribe_radius;

        let mut new_visible = FxHashSet::default();

        for &entity in all_entities {
            let (ex, _, ez) = positions.get_position(entity);
            let dx = ex - cx;
            let dz = ez - cz;
            let dist2 = dx * dx + dz * dz;

            if self.current.contains(&entity) {
                if dist2 <= unsub_r2 {
                    new_visible.insert(entity);
                }
            } else if dist2 <= sub_r2 {
                new_visible.insert(entity);
            }
        }

        let entered: Vec<EntitySlot> = new_visible
            .iter()
            .filter(|e| !self.current.contains(e))
            .copied()
            .collect();

        let raw_left: Vec<EntitySlot> = self
            .current
            .iter()
            .filter(|e| !new_visible.contains(e))
            .copied()
            .collect();

        for entity in &raw_left {
            self.pending_leaves
                .entry(*entity)
                .or_insert(self.leave_repeat_count);
        }

        let mut left = Vec::new();
        self.pending_leaves.retain(|entity, remaining| {
            left.push(*entity);
            *remaining -= 1;
            *remaining > 0
        });

        self.current = new_visible;

        VisibleSetDiff { entered, left }
    }

    pub fn visible(&self) -> &FxHashSet<EntitySlot> {
        &self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_positions(entities: &[(EntitySlot, f32, f32, f32)]) -> PositionTable {
        let mut pt = PositionTable::new();
        for &(slot, x, y, z) in entities {
            pt.ensure_capacity(slot);
            pt.set_position(slot, x, y, z);
        }
        pt
    }

    #[test]
    fn entity_within_subscribe_radius_becomes_visible() {
        let mut vs = VisibleSet::new(3);
        let positions = setup_positions(&[(EntitySlot(0), 80.0, 0.0, 0.0)]);
        let entities = [EntitySlot(0)];

        let diff = vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
        assert!(diff.entered.contains(&EntitySlot(0)));
        assert!(vs.visible().contains(&EntitySlot(0)));
    }

    #[test]
    fn entity_beyond_subscribe_radius_not_visible() {
        let mut vs = VisibleSet::new(3);
        let positions = setup_positions(&[(EntitySlot(0), 160.0, 0.0, 0.0)]);
        let entities = [EntitySlot(0)];

        let diff = vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
        assert!(diff.entered.is_empty());
        assert!(!vs.visible().contains(&EntitySlot(0)));
    }

    #[test]
    fn hysteresis_prevents_flicker() {
        let mut vs = VisibleSet::new(3);
        let entities = [EntitySlot(0)];

        let positions = setup_positions(&[(EntitySlot(0), 99.0, 0.0, 0.0)]);
        vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
        assert!(vs.visible().contains(&EntitySlot(0)));

        let positions = setup_positions(&[(EntitySlot(0), 101.0, 0.0, 0.0)]);
        vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
        assert!(vs.visible().contains(&EntitySlot(0)));

        let positions = setup_positions(&[(EntitySlot(0), 99.0, 0.0, 0.0)]);
        vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
        assert!(vs.visible().contains(&EntitySlot(0)));
    }

    #[test]
    fn leave_repeated_for_n_ticks() {
        let mut vs = VisibleSet::new(3);
        let entities = [EntitySlot(0)];

        let positions = setup_positions(&[(EntitySlot(0), 50.0, 0.0, 0.0)]);
        vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);

        let positions = setup_positions(&[(EntitySlot(0), 200.0, 0.0, 0.0)]);

        for i in 0..3 {
            let diff = vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
            assert!(
                diff.left.contains(&EntitySlot(0)),
                "leave missing on tick {i}"
            );
        }

        let diff = vs.update(0.0, 0.0, 100.0, 150.0, &positions, &entities);
        assert!(!diff.left.contains(&EntitySlot(0)));
    }

    #[test]
    #[should_panic(expected = "leave_repeat_count must be at least 1")]
    fn zero_leave_repeat_count_panics() {
        VisibleSet::new(0);
    }
}
