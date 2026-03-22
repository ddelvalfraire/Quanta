use super::{LodTier, PrioritizedEntity};
use crate::types::{ClientIndex, EntitySlot};

const DISTANCE_ATTENUATION: f32 = 0.01;
const VELOCITY_WEIGHT: f32 = 0.1;
const INTERACTION_BOOST: f32 = 3.0;

pub struct PriorityAccumulator {
    priorities: Vec<f32>,
    max_clients: usize,
    max_entities: usize,
}

impl PriorityAccumulator {
    pub fn new(max_clients: usize, max_entities: usize) -> Self {
        let len = max_clients
            .checked_mul(max_entities)
            .expect("PriorityAccumulator: max_clients * max_entities overflows usize");
        Self {
            priorities: vec![0.0; len],
            max_clients,
            max_entities,
        }
    }

    fn index(&self, client: ClientIndex, entity: EntitySlot) -> usize {
        let c = client.0 as usize;
        let e = entity.0 as usize;
        debug_assert!(
            c < self.max_clients && e < self.max_entities,
            "PriorityAccumulator index out of bounds: client={c}, entity={e}, \
             max_clients={}, max_entities={}",
            self.max_clients,
            self.max_entities,
        );
        c * self.max_entities + e
    }

    pub fn accumulate(
        &mut self,
        client: ClientIndex,
        entity: EntitySlot,
        distance: f32,
        velocity: f32,
        interacted: bool,
    ) {
        let distance_factor = 1.0 / (1.0 + distance * DISTANCE_ATTENUATION);
        let velocity_factor = 1.0 + velocity * VELOCITY_WEIGHT;
        let interaction_factor = if interacted { INTERACTION_BOOST } else { 1.0 };

        let idx = self.index(client, entity);
        self.priorities[idx] += distance_factor * velocity_factor * interaction_factor;
    }

    pub fn sorted_by_priority(
        &self,
        client: ClientIndex,
        entity_lods: &[(EntitySlot, LodTier)],
        tick: u64,
    ) -> Vec<PrioritizedEntity> {
        let base = (client.0 as usize) * self.max_entities;

        let mut result: Vec<PrioritizedEntity> = entity_lods
            .iter()
            .filter(|(_, lod)| tick.is_multiple_of(lod.tick_divisor()))
            .map(|&(entity, lod_tier)| {
                let priority = self.priorities[base + entity.0 as usize];
                PrioritizedEntity {
                    entity,
                    lod_tier,
                    priority,
                }
            })
            .collect();

        result.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result
    }

    pub fn reset(&mut self, client: ClientIndex, entity: EntitySlot) {
        let idx = self.index(client, entity);
        self.priorities[idx] = 0.0;
    }

    pub fn clear_client(&mut self, client: ClientIndex, max_entities: usize) {
        let base = (client.0 as usize) * self.max_entities;
        let end = base + max_entities.min(self.max_entities);
        for p in &mut self.priorities[base..end] {
            *p = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_increases_priority() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);
        let entity = EntitySlot(0);

        acc.accumulate(client, entity, 50.0, 0.0, false);
        let list = acc.sorted_by_priority(client, &[(entity, LodTier::Full)], 0);
        assert_eq!(list.len(), 1);
        assert!(list[0].priority > 0.0);
    }

    #[test]
    fn reset_zeroes_priority() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);
        let entity = EntitySlot(0);

        acc.accumulate(client, entity, 50.0, 0.0, false);
        acc.reset(client, entity);

        let list = acc.sorted_by_priority(client, &[(entity, LodTier::Full)], 0);
        assert_eq!(list[0].priority, 0.0);
    }

    #[test]
    fn distance_factor_formula() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);

        acc.accumulate(client, EntitySlot(0), 0.0, 0.0, false);
        acc.accumulate(client, EntitySlot(1), 100.0, 0.0, false);

        let lods = [
            (EntitySlot(0), LodTier::Full),
            (EntitySlot(1), LodTier::Full),
        ];
        let list = acc.sorted_by_priority(client, &lods, 0);

        assert!(list[0].entity == EntitySlot(0));
        assert!((list[0].priority - 1.0).abs() < 0.001);
        assert!((list[1].priority - 0.5).abs() < 0.001);
    }

    #[test]
    fn velocity_factor_formula() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);

        acc.accumulate(client, EntitySlot(0), 0.0, 10.0, false);
        let list = acc.sorted_by_priority(client, &[(EntitySlot(0), LodTier::Full)], 0);
        assert!((list[0].priority - 2.0).abs() < 0.001);
    }

    #[test]
    fn interaction_factor_formula() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);

        acc.accumulate(client, EntitySlot(0), 0.0, 0.0, true);
        let list = acc.sorted_by_priority(client, &[(EntitySlot(0), LodTier::Full)], 0);
        assert!((list[0].priority - 3.0).abs() < 0.001);
    }

    #[test]
    fn sorted_descending() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);

        acc.accumulate(client, EntitySlot(0), 100.0, 0.0, false);
        acc.accumulate(client, EntitySlot(1), 0.0, 0.0, false);

        let lods = [
            (EntitySlot(0), LodTier::Full),
            (EntitySlot(1), LodTier::Full),
        ];
        let list = acc.sorted_by_priority(client, &lods, 0);
        assert!(list[0].priority >= list[1].priority);
    }

    #[test]
    fn tick_divisor_filters_entities() {
        let mut acc = PriorityAccumulator::new(1, 10);
        let client = ClientIndex(0);

        acc.accumulate(client, EntitySlot(0), 0.0, 0.0, false);
        acc.accumulate(client, EntitySlot(1), 0.0, 0.0, false);

        let lods = [
            (EntitySlot(0), LodTier::Full),
            (EntitySlot(1), LodTier::Medium),
        ];

        let list = acc.sorted_by_priority(client, &lods, 1);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].entity, EntitySlot(0));

        let list = acc.sorted_by_priority(client, &lods, 4);
        assert_eq!(list.len(), 2);
    }

    #[test]
    #[should_panic(expected = "overflows usize")]
    fn overflow_panics() {
        PriorityAccumulator::new(usize::MAX, 2);
    }
}
