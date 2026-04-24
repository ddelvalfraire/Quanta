//! Regression guard for M-2: `phase1_drain_inputs` must cap the number of
//! client inputs it drains per tick. Without a cap, a single session can push
//! up to the channel's capacity (4096) into the engine in one tick by sending
//! a burst of messages with monotonically increasing `input_seq` values — the
//! dedupe check only filters *stale* seqs, not fresh ones.
//!
//! The fix exposes `quanta_realtime_server::tick::engine::MAX_INPUTS_PER_TICK`
//! and bounds the drain loop at that constant. This test fails until the
//! constant exists and is actually applied inside `phase1_drain_inputs`.

mod common;

use common::{slot, test_engine, MockWasm};

use quanta_realtime_server::tick::engine::MAX_INPUTS_PER_TICK;
use quanta_realtime_server::tick::{ClientInput, HandleResult, SessionId, TickMessage};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Flood the engine with significantly more inputs than the cap in a single
/// batch, run one tick, and assert that at most `MAX_INPUTS_PER_TICK`
/// inputs reached the executor. This guarantees the cap is applied at drain
/// time, not downstream.
#[test]
fn phase1_drain_inputs_respects_max_inputs_per_tick() {
    // Sanity check the constant — the fix should set this well below the
    // 4096 channel capacity so a burst can be absorbed across multiple ticks.
    assert!(
        MAX_INPUTS_PER_TICK > 0 && MAX_INPUTS_PER_TICK <= 4096,
        "MAX_INPUTS_PER_TICK must be a sensible positive cap; got {MAX_INPUTS_PER_TICK}"
    );

    let input_calls = Arc::new(AtomicUsize::new(0));
    let counter = input_calls.clone();
    let wasm = MockWasm::new(move |_entity, state, msg| {
        if matches!(msg, TickMessage::Input { .. }) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: Vec::new(),
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));

    // The engine only dispatches to entities that actually exist. Register
    // one entity so `TickMessage::Input` calls get routed to the executor.
    let entity = slot(1);
    engine.add_entity(entity, vec![0u8; 4], Some(SessionId::from("session-a")));

    // Push roughly 4x the cap in one burst. The dedupe step keeps only
    // strictly increasing `input_seq` values, so seeding 1..=N lets every
    // input survive dedupe and test the drain cap specifically.
    let burst = MAX_INPUTS_PER_TICK.saturating_mul(4).max(2048) as u32;
    for seq in 1..=burst {
        input_tx
            .send(ClientInput {
                session_id: SessionId::from("session-a"),
                entity_slot: entity,
                input_seq: seq,
                payload: vec![],
            })
            .expect("input channel should be open");
    }

    engine.tick();

    let processed = input_calls.load(Ordering::Relaxed);
    assert!(
        processed <= MAX_INPUTS_PER_TICK,
        "phase1_drain_inputs exceeded MAX_INPUTS_PER_TICK: processed {processed}, cap {MAX_INPUTS_PER_TICK}"
    );
}
