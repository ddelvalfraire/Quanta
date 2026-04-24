//! Failing test that proves Finding H-3: unbounded `snapshot_subscribers` Vec.
//!
//! `TickEngine::add_snapshot_subscriber` appends to a `Vec` with no capacity
//! limit. A misbehaving or buggy caller can push arbitrarily many senders,
//! growing memory without bound and amplifying per-tick clone overhead.
//!
//! This test FAILS today because no cap exists — all N+1 calls succeed and
//! every subscriber receives a snapshot on the next tick.
//!
//! After the fix, `add_snapshot_subscriber` should silently ignore (or return
//! an error for) the (N+1)th call, and the overflow receiver should stay empty
//! after a tick.

mod common;

use crossbeam_channel::unbounded;
use quanta_realtime_server::TickSnapshot;
use common::noop_engine;

/// Proposed maximum number of snapshot subscribers.
/// Chosen to be small enough to be reasonable in production yet large enough
/// not to break legitimate multi-consumer demos (swarm + fanout = 2 today).
const MAX_SNAPSHOT_SUBSCRIBERS: usize = 16;

#[test]
fn snapshot_subscriber_count_is_capped() {
    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = noop_engine();

    // Keep receivers alive so the channel is not disconnected and the engine
    // does not silently drop those senders during emit_snapshot's retain pass.
    let mut _live_receivers: Vec<crossbeam_channel::Receiver<TickSnapshot>> = Vec::new();

    // Register exactly MAX_SNAPSHOT_SUBSCRIBERS — all must succeed.
    for _ in 0..MAX_SNAPSHOT_SUBSCRIBERS {
        let (tx, rx) = unbounded::<TickSnapshot>();
        _live_receivers.push(rx);
        engine.add_snapshot_subscriber(tx);
    }

    // Attempt to register one additional subscriber beyond the proposed cap.
    let (overflow_tx, overflow_rx) = unbounded::<TickSnapshot>();
    engine.add_snapshot_subscriber(overflow_tx);

    // Tick once so the engine emits snapshots to all registered subscribers.
    // The engine also needs at least one entity to emit a non-empty snapshot;
    // for a bare noop engine with no entities, emit_snapshot still fires when
    // snapshot_subscribers is non-empty (it checks the len before early-exit).
    engine.tick();

    // If the cap had rejected the overflow sender, overflow_rx stays empty.
    // EXPECTED (after fix): try_recv() returns Err (no snapshot delivered).
    // ACTUAL TODAY: a snapshot IS delivered because add_snapshot_subscriber
    //   pushed unconditionally — try_recv() returns Ok, and this assert FAILS,
    //   proving Finding H-3.
    let received = overflow_rx.try_recv();
    assert!(
        received.is_err(),
        "BUG (H-3): add_snapshot_subscriber has no cap. After registering {} \
         subscribers, the {}th call still succeeded and a snapshot was delivered \
         to the overflow receiver on the next tick. \
         The Vec length is unbounded, enabling memory exhaustion and unbounded \
         per-tick clone amplification (one TickSnapshot::clone per subscriber \
         per tick). \
         Expected: overflow subscriber rejected — try_recv() returns Err. \
         Actual: snapshot received (try_recv() returned Ok).",
        MAX_SNAPSHOT_SUBSCRIBERS,
        MAX_SNAPSHOT_SUBSCRIBERS + 1,
    );
}
