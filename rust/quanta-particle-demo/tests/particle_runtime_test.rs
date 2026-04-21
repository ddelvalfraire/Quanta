//! In-process integration test: a deterministic bot sends directional
//! input datagrams, the TickEngine dispatches them to ParticleExecutor,
//! and the entity's position advances as expected.
//!
//! Phase 4 will replace BotHarness with a real WebTransport client that
//! speaks the same 25-byte datagram wire format via `encode_client_input`.

use quanta_particle_demo::executor::ParticleExecutor;
use quanta_particle_demo::input::{encode_datagram, ParticleInputPayload};
use quanta_particle_demo::schema::{initial_state, particle_field_indices, particle_schema};
use quanta_realtime_server::testing::{BotAction, BotBehavior, BotHarness, TestHarnessBuilder};
use quanta_realtime_server::types::EntitySlot;

use quanta_core_rs::delta::encoder::{dequantize, read_state};

struct RightwardBot {
    entity: u32,
    seq: u32,
}

impl BotBehavior for RightwardBot {
    fn on_tick(&mut self, _tick: u64, _entity_states: &[(u32, Vec<u8>)]) -> Vec<BotAction> {
        self.seq += 1;
        let payload = encode_datagram(&ParticleInputPayload {
            entity_slot: self.entity,
            input_seq: self.seq,
            dir_x: 1.0,
            dir_z: 0.0,
            actions: 0,
            dt_ms: 50,
        });
        vec![BotAction::SendInput {
            entity: self.entity,
            payload: payload.to_vec(),
        }]
    }
}

#[test]
fn particle_executor_moves_entity_monotonically() {
    let mut harness = TestHarnessBuilder::new()
        .wasm(Box::new(ParticleExecutor::default()))
        .build();
    harness.add_entity(EntitySlot(0), initial_state(), None);

    let bots: Vec<Box<dyn BotBehavior>> = vec![Box::new(RightwardBot { entity: 0, seq: 0 })];
    let mut bot_harness = BotHarness::new(harness, bots);
    bot_harness.run(10);

    let state = bot_harness
        .harness()
        .get_entity_state(&EntitySlot(0))
        .expect("entity present")
        .to_vec();

    let schema = particle_schema();
    let ix = particle_field_indices();
    let values = read_state(schema, &state).unwrap();
    let pos_x = dequantize(
        values[ix.pos_x],
        schema.fields[ix.pos_x].quantization.as_ref().unwrap(),
    ) as f32;
    let pos_z = dequantize(
        values[ix.pos_z],
        schema.fields[ix.pos_z].quantization.as_ref().unwrap(),
    ) as f32;

    assert!(pos_x > 1.0, "pos-x should advance after 10 ticks, got {pos_x}");
    assert!(pos_z.abs() < 0.5, "pos-z should stay near zero, got {pos_z}");
}
