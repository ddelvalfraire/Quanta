//! `ParticleFanout` — particle-world implementation of the platform's
//! `IslandFanout` trait. Owns per-client state (pacer + last-sent baselines)
//! and the island-level `InterestManager` + `PositionTable` used to choose
//! which entities each client should receive this tick.

use std::sync::Arc;

use bytes::Bytes;
use rustc_hash::FxHashMap;

use quanta_core_rs::delta::encoder::compute_delta;
use quanta_realtime_server::delta_envelope::{
    encode_delta_datagram, encode_delta_datagram_with_seq_ack, FLAG_FULL_STATE,
};
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

/// Result of computing a per-entity delta against the last sent state.
/// Separating this from the final `(flags, bytes)` tuple lets us treat
/// the "no change" branch specially for the client's own entity (it still
/// needs a seq-ack heartbeat each tick for server reconciliation).
enum DeltaOutcome {
    /// No prior baseline sent — ship the full state as FULL_STATE.
    Full(Vec<u8>),
    /// State changed — ship the delta payload.
    Delta(Vec<u8>),
    /// State byte-identical to the last sent state (idle or sub-quant).
    Unchanged,
}

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
        // Demo scope: subscribe every client to every entity AND force
        // every subscribed entity to Full LOD (sent every tick).
        // Distance-based LOD throttling is correct for production (it
        // saves bandwidth for distant/unimportant entities) but at 8-tick
        // granularity for the Low tier it produces visible stutter that
        // no client-side interpolation delay can smooth over. Demo values
        // motion smoothness over bandwidth.
        let cfg = InterestConfig {
            subscribe_radius: 20_000.0,
            unsubscribe_radius: 21_000.0,
            force_full_lod: true,
            ..InterestConfig::default()
        };
        Self {
            interest: InterestManager::new(cfg, MAX_CLIENTS, MAX_ENTITIES),
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
                let is_self = pe.entity == state.entity_slot;
                let last = state.last_sent.get(&pe.entity);
                let delta_outcome = match last {
                    None => DeltaOutcome::Full(entity.state.clone()),
                    Some(prev) => match compute_delta(schema, prev, &entity.state, None) {
                        Ok(d) if d.is_empty() => DeltaOutcome::Unchanged,
                        Ok(d) => DeltaOutcome::Delta(d),
                        Err(_) => {
                            // Baseline-length drift (e.g. schema grew): resync via FULL_STATE.
                            DeltaOutcome::Full(entity.state.clone())
                        }
                    },
                };

                let (flags, delta_bytes) = match delta_outcome {
                    DeltaOutcome::Full(b) => (FLAG_FULL_STATE, b),
                    DeltaOutcome::Delta(b) => (0u8, b),
                    DeltaOutcome::Unchanged => {
                        // For the client's OWN entity, ALWAYS ship a
                        // seq-ack heartbeat even when state is unchanged.
                        // Without this, an idle or sub-quantization motion
                        // entity sends no datagrams → the client's
                        // predictor never hears the ack → `pending` and
                        // `seq lag` grow unbounded. Empty delta bytes are
                        // fine: the header carries the `last_input_seq`
                        // and the receiver leaves state unmodified.
                        if is_self {
                            (0u8, Vec::new())
                        } else {
                            continue;
                        }
                    }
                };

                // If we're shipping this client their OWN entity, attach
                // the seq-ack prefix so the browser predictor can replay
                // its unack'd input buffer on reconcile. NPCs and other
                // players' entities get the plain envelope — no seq-ack
                // overhead.
                let bytes = if is_self {
                    encode_delta_datagram_with_seq_ack(
                        flags,
                        pe.entity.0,
                        snapshot.tick,
                        entity.last_input_seq,
                        &delta_bytes,
                    )
                } else {
                    encode_delta_datagram(flags, pe.entity.0, snapshot.tick, &delta_bytes)
                };
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
                last_input_seq: 0,
            }],
        };
        f.on_tick(&snap);
    }
}
