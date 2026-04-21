//! `ParticleFanout` — particle-world implementation of the platform's
//! `IslandFanout` trait. Owns per-client state (pacer + last-sent baselines)
//! and the island-level `InterestManager` + `PositionTable` used to choose
//! which entities each client should receive this tick.

use std::sync::Arc;

use bytes::Bytes;
use rustc_hash::FxHashMap;

use quanta_core_rs::delta::encoder::compute_delta;
use quanta_realtime_server::delta_envelope::{encode_delta_datagram, FLAG_FULL_STATE};
use quanta_realtime_server::fanout::IslandFanout;
use quanta_realtime_server::interest::{InterestConfig, InterestManager};
use quanta_realtime_server::pacing::{DatagramBatch, PacingConfig, PacingHandle};
use quanta_realtime_server::session::Session;
use quanta_realtime_server::spatial::PositionTable;
use quanta_realtime_server::tick::types::TickSnapshot;
use quanta_realtime_server::types::{ClientIndex, EntitySlot};

use crate::schema::particle_schema;

const MAX_CLIENTS: usize = 4096;
const MAX_ENTITIES: usize = 4096;

struct ClientState {
    pacer: PacingHandle,
    entity_slot: EntitySlot,
    last_sent: FxHashMap<EntitySlot, Vec<u8>>,
}

pub struct ParticleFanout {
    interest: InterestManager,
    position_table: PositionTable,
    clients: FxHashMap<ClientIndex, ClientState>,
}

impl ParticleFanout {
    pub fn new() -> Self {
        Self {
            interest: InterestManager::new(InterestConfig::default(), MAX_CLIENTS, MAX_ENTITIES),
            position_table: PositionTable::new(),
            clients: FxHashMap::default(),
        }
    }
}

impl Default for ParticleFanout {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandFanout for ParticleFanout {
    fn on_client_joined(
        &mut self,
        client_index: ClientIndex,
        entity_slot: EntitySlot,
        session: Arc<dyn Session>,
    ) {
        let pacer = PacingHandle::spawn(session, PacingConfig::default());
        self.clients.insert(
            client_index,
            ClientState {
                pacer,
                entity_slot,
                last_sent: FxHashMap::default(),
            },
        );
        self.interest.register_client(client_index, 0.0, 0.0);
    }

    fn on_client_left(&mut self, client_index: ClientIndex) {
        self.clients.remove(&client_index);
        self.interest.unregister_client(client_index);
    }

    fn on_tick(&mut self, snapshot: &TickSnapshot) {
        // TODO(phase-5): build an FxHashMap<EntitySlot, &EntitySnapshot> once
        // here and reuse below. The per-client `snapshot.entities.iter().find`
        // and the per-send lookup are each O(M) and dominate at high N·M.
        for e in &snapshot.entities {
            self.position_table.ensure_capacity(e.slot);
            self.position_table
                .set_position(e.slot, e.pos_x, 0.0, e.pos_z);
            self.position_table
                .set_velocity(e.slot, e.vel_x, 0.0, e.vel_z);
        }
        for (&client_index, state) in self.clients.iter() {
            if let Some(e) = snapshot
                .entities
                .iter()
                .find(|e| e.slot == state.entity_slot)
            {
                self.interest
                    .set_client_position(client_index, e.pos_x, e.pos_z);
            }
        }

        let all_entities: Vec<EntitySlot> = snapshot.entities.iter().map(|e| e.slot).collect();
        let results = self
            .interest
            .update(snapshot.tick, &self.position_table, &all_entities);

        let schema = particle_schema();
        for r in results {
            let Some(state) = self.clients.get_mut(&r.client) else {
                continue;
            };
            let mut batch = DatagramBatch {
                datagrams: Vec::new(),
            };
            for pe in &r.sends {
                let Some(entity) = snapshot.entities.iter().find(|e| e.slot == pe.entity) else {
                    continue;
                };
                // Entities whose executor hasn't materialized a real state yet
                // (empty bytes) have nothing to send. Wait for a tick where
                // inputs produce an actual state.
                if entity.state.is_empty() {
                    continue;
                }
                let last = state.last_sent.get(&pe.entity);
                let (flags, delta_bytes) = match last {
                    None => (FLAG_FULL_STATE, entity.state.clone()),
                    Some(prev) => match compute_delta(schema, prev, &entity.state, None) {
                        Ok(d) if d.is_empty() => continue,
                        Ok(d) => (0u8, d),
                        Err(_) => {
                            // Baseline-length drift (e.g. schema grew): resync via FULL_STATE.
                            (FLAG_FULL_STATE, entity.state.clone())
                        }
                    },
                };
                let bytes = encode_delta_datagram(flags, pe.entity.0, snapshot.tick, &delta_bytes);
                batch.datagrams.push(Bytes::from(bytes));
                state.last_sent.insert(pe.entity, entity.state.clone());
            }
            if !batch.datagrams.is_empty() {
                state.pacer.send_batch(batch);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quanta_realtime_server::tick::types::EntitySnapshot;

    #[test]
    fn on_tick_without_clients_is_noop() {
        let mut f = ParticleFanout::new();
        let snap = TickSnapshot {
            tick: 0,
            entities: vec![EntitySnapshot {
                slot: EntitySlot(0),
                state: vec![],
                pos_x: 0.0,
                pos_z: 0.0,
                vel_x: 0.0,
                vel_z: 0.0,
            }],
        };
        f.on_tick(&snap);
    }
}
