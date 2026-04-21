pub mod priority;
pub mod visible_set;

pub use priority::PriorityAccumulator;
pub use visible_set::{VisibleSet, VisibleSetDiff};

use crate::spatial::PositionTable;
use crate::types::{ClientIndex, EntitySlot};
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LodTier {
    /// 0–30m: full fidelity, every tick
    Full = 0,
    /// 30–70m: high fidelity, every 2 ticks
    High = 1,
    /// 70–100m: medium fidelity, every 4 ticks
    Medium = 2,
    /// 100–150m: low fidelity, every 8 ticks
    Low = 3,
}

impl LodTier {
    pub fn from_distance(distance: f32) -> Self {
        if distance <= 30.0 {
            LodTier::Full
        } else if distance <= 70.0 {
            LodTier::High
        } else if distance <= 100.0 {
            LodTier::Medium
        } else {
            LodTier::Low
        }
    }

    pub fn tick_divisor(self) -> u64 {
        match self {
            LodTier::Full => 1,
            LodTier::High => 2,
            LodTier::Medium => 4,
            LodTier::Low => 8,
        }
    }

    pub fn field_group_mask(self) -> u8 {
        match self {
            LodTier::Full => 0xFF,
            LodTier::High => 0x0F,
            LodTier::Medium => 0x03,
            LodTier::Low => 0x01,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrioritizedEntity {
    pub entity: EntitySlot,
    pub lod_tier: LodTier,
    pub priority: f32,
}

pub struct ClientTickResult {
    pub client: ClientIndex,
    pub sends: Vec<PrioritizedEntity>,
    pub enters: Vec<EntitySlot>,
    pub leaves: Vec<EntitySlot>,
    pub batch_enters: bool,
}

pub struct InterestConfig {
    pub subscribe_radius: f32,
    pub unsubscribe_radius: f32,
    pub batch_enter_threshold: usize,
    pub leave_repeat_count: u8,
    /// When true, all visible entities get `LodTier::Full` regardless of
    /// distance — every visible entity's state is sent every tick. Off by
    /// default (distance-based throttling saves bandwidth in production),
    /// on for visual demos where smooth interpolation matters more than
    /// bytes. Client-side interpolation expects at most ~2 ticks between
    /// snapshots; the default Low tier ships every 8 ticks, which reads
    /// as visible stutter no matter what interp delay the client picks.
    pub force_full_lod: bool,
}

impl Default for InterestConfig {
    fn default() -> Self {
        Self {
            subscribe_radius: 100.0,
            unsubscribe_radius: 150.0,
            batch_enter_threshold: 5,
            leave_repeat_count: 3,
            force_full_lod: false,
        }
    }
}

pub struct InterestManager {
    config: InterestConfig,
    max_clients: usize,
    max_entities: usize,
    visible_sets: Vec<Option<VisibleSet>>,
    client_positions: Vec<(f32, f32)>,
    priority: PriorityAccumulator,
    interactions: FxHashSet<(ClientIndex, EntitySlot)>,
}

impl InterestManager {
    pub fn new(config: InterestConfig, max_clients: usize, max_entities: usize) -> Self {
        Self {
            config,
            max_clients,
            max_entities,
            visible_sets: (0..max_clients).map(|_| None).collect(),
            client_positions: vec![(0.0, 0.0); max_clients],
            priority: PriorityAccumulator::new(max_clients, max_entities),
            interactions: FxHashSet::default(),
        }
    }

    pub fn register_client(&mut self, client: ClientIndex, x: f32, z: f32) {
        let idx = client.0 as usize;
        if idx < self.max_clients {
            self.visible_sets[idx] = Some(VisibleSet::new(self.config.leave_repeat_count));
            self.client_positions[idx] = (x, z);
        }
    }

    pub fn unregister_client(&mut self, client: ClientIndex) {
        let idx = client.0 as usize;
        if idx < self.max_clients {
            self.visible_sets[idx] = None;
            self.client_positions[idx] = (0.0, 0.0);
            self.priority.clear_client(client, self.max_entities);
        }
    }

    pub fn record_interaction(&mut self, client: ClientIndex, entity: EntitySlot) {
        self.interactions.insert((client, entity));
    }

    pub fn set_client_position(&mut self, client: ClientIndex, x: f32, z: f32) {
        let idx = client.0 as usize;
        if idx < self.max_clients {
            self.client_positions[idx] = (x, z);
        }
    }

    pub fn update(
        &mut self,
        tick: u64,
        positions: &PositionTable,
        all_entities: &[EntitySlot],
    ) -> Vec<ClientTickResult> {
        let mut results = Vec::new();

        for ci in 0..self.max_clients {
            let Some(vis) = self.visible_sets[ci].as_mut() else {
                continue;
            };
            let client = ClientIndex(ci as u16);
            let (cx, cz) = self.client_positions[ci];

            let diff = vis.update(
                cx,
                cz,
                self.config.subscribe_radius,
                self.config.unsubscribe_radius,
                positions,
                all_entities,
            );

            let entity_lods = accumulate_visible(
                &mut self.priority,
                &self.interactions,
                client,
                cx,
                cz,
                vis.visible(),
                positions,
                self.config.force_full_lod,
            );

            let sends = self.priority.sorted_by_priority(client, &entity_lods, tick);

            for pe in &sends {
                self.priority.reset(client, pe.entity);
            }

            let batch_enters = diff.entered.len() >= self.config.batch_enter_threshold;

            results.push(ClientTickResult {
                client,
                sends,
                enters: diff.entered,
                leaves: diff.left,
                batch_enters,
            });
        }

        self.interactions.clear();
        results
    }
}

fn accumulate_visible(
    priority: &mut PriorityAccumulator,
    interactions: &FxHashSet<(ClientIndex, EntitySlot)>,
    client: ClientIndex,
    cx: f32,
    cz: f32,
    visible: &FxHashSet<EntitySlot>,
    positions: &PositionTable,
    force_full_lod: bool,
) -> Vec<(EntitySlot, LodTier)> {
    let mut entity_lods = Vec::with_capacity(visible.len());
    for &entity in visible {
        let (ex, _, ez) = positions.get_position(entity);
        let dx = ex - cx;
        let dz = ez - cz;
        let distance = (dx * dx + dz * dz).sqrt();
        let (vx, _, vz) = positions.get_velocity(entity);
        let velocity = (vx * vx + vz * vz).sqrt();
        let interacted = interactions.contains(&(client, entity));

        priority.accumulate(client, entity, distance, velocity, interacted);

        let tier = if force_full_lod {
            LodTier::Full
        } else {
            LodTier::from_distance(distance)
        };
        entity_lods.push((entity, tier));
    }
    entity_lods
}

#[cfg(test)]
mod mod_tests {
    use super::*;

    fn fixture(force_full_lod: bool, positions: &[(EntitySlot, f32, f32)]) -> Vec<(EntitySlot, LodTier)> {
        let mut pri = PriorityAccumulator::new(4, 16);
        let interactions = FxHashSet::default();
        let mut pos = PositionTable::new();
        let mut visible = FxHashSet::default();
        for &(slot, x, z) in positions {
            pos.ensure_capacity(slot);
            pos.set_position(slot, x, 0.0, z);
            visible.insert(slot);
        }
        // Client sits at origin.
        accumulate_visible(
            &mut pri,
            &interactions,
            ClientIndex(0),
            0.0,
            0.0,
            &visible,
            &pos,
            force_full_lod,
        )
    }

    #[test]
    fn default_lod_throttles_distant_entities() {
        let close = EntitySlot(0);
        let medium = EntitySlot(1);
        let far = EntitySlot(2);
        let lods = fixture(
            false,
            &[(close, 10.0, 0.0), (medium, 80.0, 0.0), (far, 500.0, 0.0)],
        );
        let get = |s: EntitySlot| lods.iter().find(|(e, _)| *e == s).unwrap().1;
        assert_eq!(get(close), LodTier::Full);
        assert_eq!(get(medium), LodTier::Medium);
        assert_eq!(get(far), LodTier::Low);
    }

    #[test]
    fn force_full_lod_overrides_distance_throttle() {
        // Same setup, but with force_full_lod=true every entity must be
        // Full tier — this is what the particle-world demo relies on for
        // smooth client-side snapshot interpolation.
        let close = EntitySlot(0);
        let medium = EntitySlot(1);
        let far = EntitySlot(2);
        let lods = fixture(
            true,
            &[(close, 10.0, 0.0), (medium, 80.0, 0.0), (far, 500.0, 0.0)],
        );
        for (_slot, tier) in &lods {
            assert_eq!(*tier, LodTier::Full, "every entity must be Full tier");
        }
    }
}
