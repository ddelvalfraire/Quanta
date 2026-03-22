mod common;

use quanta_realtime_server::command::IslandCommand;
use quanta_realtime_server::tick::*;
use quanta_realtime_server::types::{EntitySlot, IslandId};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use common::{noop_engine, slot, test_engine, MockWasm};

// ── Deterministic tick ordering ────────────────────────────────────

#[test]
fn deterministic_entity_processing_order() {
    let mut runs: Vec<Vec<u32>> = Vec::new();

    for _ in 0..2 {
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let order_clone = order.clone();

        let wasm = MockWasm::new(move |entity, state, _msg| {
            order_clone.lock().unwrap().push(entity.0);
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        });

        let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
        // Add entities out of natural order
        engine.add_entity(slot(5), vec![0], None);
        engine.add_entity(slot(1), vec![0], None);
        engine.add_entity(slot(3), vec![0], None);

        for s in [1, 3, 5] {
            input_tx
                .send(ClientInput {
                    session_id: SessionId::from(format!("p{s}").as_str()),
                    entity_slot: slot(s),
                    input_seq: 1,
                    payload: vec![],
                })
                .unwrap();
        }

        engine.tick();
        runs.push(order.lock().unwrap().clone());
    }

    assert_eq!(runs[0], vec![1, 3, 5], "entities processed in BTreeMap order");
    assert_eq!(runs[0], runs[1], "two runs produce identical processing order");
}

// ── Timer accuracy ─────────────────────────────────────────────────

#[test]
fn timer_fires_exactly_on_target_tick() {
    let timer_fired = Arc::new(std::sync::Mutex::new(false));
    let fired = timer_fired.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if matches!(msg, TickMessage::Timer { .. }) {
            *fired.lock().unwrap() = true;
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Set timer for 100ms at 20Hz (50ms/tick) → fires on tick 2
    engine.set_timer(slot(1), "attack".into(), 100);

    engine.tick(); // tick 0 → no fire
    assert!(!*timer_fired.lock().unwrap());

    engine.tick(); // tick 1 → no fire
    assert!(!*timer_fired.lock().unwrap());

    engine.tick(); // tick 2 → fires!
    assert!(*timer_fired.lock().unwrap());
}

// ── Timer cancellation ─────────────────────────────────────────────

#[test]
fn cancelled_timer_does_not_fire() {
    let timer_fired = Arc::new(std::sync::Mutex::new(false));
    let fired = timer_fired.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if matches!(msg, TickMessage::Timer { .. }) {
            *fired.lock().unwrap() = true;
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    engine.set_timer(slot(1), "heal".into(), 100);
    engine.cancel_timer(&slot(1), "heal");

    engine.tick_n(10);
    assert!(!*timer_fired.lock().unwrap());
}

// ── Input stale detection ──────────────────────────────────────────

#[test]
fn stale_input_dropped_new_input_processed() {
    let processed_seqs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seqs = processed_seqs.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if let TickMessage::Input { input_seq, .. } = msg {
            seqs.lock().unwrap().push(*input_seq);
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Send inputs: seq 3, then stale seq 1, then new seq 5
    for seq in [3, 1, 5] {
        input_tx
            .send(ClientInput {
                session_id: SessionId::from("player1"),
                entity_slot: slot(1),
                input_seq: seq,
                payload: vec![seq as u8],
            })
            .unwrap();
    }

    engine.tick();

    let seqs = processed_seqs.lock().unwrap();
    assert_eq!(*seqs, vec![3, 5]); // seq 1 was stale, dropped
}

// ── Effect batching: persist coalescing ────────────────────────────

#[test]
fn persist_effects_coalesced_into_one_checkpoint() {
    // 3 entities all emit Persist → should produce exactly 1 BridgeEffect::Persist
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::Persist],
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    for i in 0..3 {
        engine.add_entity(slot(i), vec![i as u8], None);
        input_tx
            .send(ClientInput {
                session_id: SessionId::from("p1"),
                entity_slot: slot(i),
                input_seq: i + 1, // incrementing seq to avoid stale detection
                payload: vec![],
            })
            .unwrap();
    }

    engine.tick();

    let effects = engine.take_effects();
    let persist_count = effects
        .iter()
        .filter(|e| matches!(e, BridgeEffect::Persist { .. }))
        .count();
    assert_eq!(persist_count, 1, "should be exactly 1 coalesced persist");

    // The single persist should contain all 3 entities
    if let BridgeEffect::Persist { entity_states } = &effects[0] {
        assert_eq!(entity_states.len(), 3);
    } else {
        panic!("expected Persist effect");
    }
}

// ── Deferred sends ─────────────────────────────────────────────────

#[test]
fn same_island_send_deferred_to_next_tick() {
    let b_received = Arc::new(std::sync::Mutex::new(false));
    let received = b_received.clone();

    let wasm = MockWasm::new(move |entity, state, msg| {
        if entity == EntitySlot(1) {
            // Entity 1 sends to entity 2
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![TickEffect::Send {
                    target: slot(2),
                    payload: vec![42],
                }],
            })
        } else if entity == EntitySlot(2) {
            if let TickMessage::Deferred { payload, .. } = msg {
                if payload == &[42] {
                    *received.lock().unwrap() = true;
                }
            }
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        } else {
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        }
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);
    engine.add_entity(slot(2), vec![0], None);

    // Give entity 1 an input to trigger its handler
    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();

    engine.tick(); // tick 0: entity 1 sends to entity 2, deferred
    assert!(!*b_received.lock().unwrap(), "should not deliver same tick");
    assert_eq!(engine.deferred_send_count(), 1);

    engine.tick(); // tick 1: deferred send delivered to entity 2
    assert!(*b_received.lock().unwrap(), "should deliver next tick");
}

// ── Message priority ───────────────────────────────────────────────

#[test]
fn timer_messages_processed_before_inputs() {
    let message_order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let order = message_order.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        let label = match msg {
            TickMessage::Timer { .. } => "timer",
            TickMessage::Input { .. } => "input",
            TickMessage::Bridge { .. } => "bridge",
            TickMessage::Deferred { .. } => "deferred",
            TickMessage::BridgeRequest { .. } => "bridge_request",
            TickMessage::SagaFailed { .. } => "saga_failed",
        };
        order.lock().unwrap().push(label);
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Set timer that fires on tick 1
    engine.set_timer(slot(1), "t".into(), 50);

    // Also inject an input for tick 1
    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();

    // Advance to tick 1 where timer fires
    engine.tick(); // tick 0: input processed (timer fires on tick 1)
    message_order.lock().unwrap().clear();

    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 2,
            payload: vec![],
        })
        .unwrap();

    engine.tick(); // tick 1: timer + input both delivered

    let order = message_order.lock().unwrap();
    assert_eq!(order[0], "timer", "timer should come before input");
    assert_eq!(order[1], "input");
}

// ── Epoch interruption / WASM trap ─────────────────────────────────

#[test]
fn wasm_trap_skips_remaining_messages_and_records_fault() {
    let call_count = Arc::new(std::sync::Mutex::new(0u32));
    let count = call_count.clone();

    let wasm = MockWasm::new(move |_entity, _state, _msg| {
        let mut c = count.lock().unwrap();
        *c += 1;
        if *c == 1 {
            Err(WasmTrap::EpochDeadline) // first message traps
        } else {
            Ok(HandleResult {
                state: vec![],
                effects: vec![],
            })
        }
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Send 3 inputs — only the first should be attempted (then trap breaks)
    for seq in 1..=3 {
        input_tx
            .send(ClientInput {
                session_id: SessionId::from("p1"),
                entity_slot: slot(1),
                input_seq: seq,
                payload: vec![],
            })
            .unwrap();
    }

    engine.tick();

    assert_eq!(*call_count.lock().unwrap(), 1, "should stop after trap");
    assert_eq!(
        engine.fault_state(&slot(1)),
        ActorHealthState::Quarantined {
            consecutive_faults: 1,
            resume_at_tick: 1,
        }
    );
}

// ── StopSelf effect removes entity ─────────────────────────────────

#[test]
fn stop_self_removes_entity() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::StopSelf],
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);
    engine.add_entity(slot(2), vec![0], None);

    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();

    engine.tick();

    assert_eq!(engine.entity_count(), 1);
    assert!(engine.get_entity_state(&slot(1)).is_none());
    assert!(engine.get_entity_state(&slot(2)).is_some());
}

// ── State mutation persists across ticks ────────────────────────────

#[test]
fn wasm_state_mutation_persists() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        // Increment the first byte
        let mut new_state = state.to_vec();
        new_state[0] = new_state[0].wrapping_add(1);
        Ok(HandleResult {
            state: new_state,
            effects: vec![],
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    for seq in 1..=3 {
        input_tx
            .send(ClientInput {
                session_id: SessionId::from("p1"),
                entity_slot: slot(1),
                input_seq: seq,
                payload: vec![],
            })
            .unwrap();
        engine.tick();
    }

    assert_eq!(engine.get_entity_state(&slot(1)), Some(&[3u8][..]));
}

// ── SetTimer effect from WASM creates timer ────────────────────────

#[test]
fn set_timer_effect_creates_timer() {
    let timer_fired = Arc::new(std::sync::Mutex::new(false));
    let fired = timer_fired.clone();

    let call_count = Arc::new(std::sync::Mutex::new(0u32));
    let count = call_count.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        let mut c = count.lock().unwrap();
        *c += 1;

        if *c == 1 {
            // First call: set a timer
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![TickEffect::SetTimer {
                    name: "boom".into(),
                    delay_ms: 100,
                }],
            })
        } else {
            // Subsequent calls: check if it's a timer message
            if matches!(msg, TickMessage::Timer { name } if name == "boom") {
                *fired.lock().unwrap() = true;
            }
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        }
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();

    engine.tick(); // tick 0: entity sets timer (fires on tick 2)
    assert!(!*timer_fired.lock().unwrap());

    engine.tick(); // tick 1: nothing
    assert!(!*timer_fired.lock().unwrap());

    engine.tick(); // tick 2: timer fires
    assert!(*timer_fired.lock().unwrap());
}

// ── Run loop stops on Drain command ────────────────────────────────

#[test]
fn run_loop_stops_on_drain() {
    let (mut engine, _input_tx, cmd_tx, _bridge_tx) = noop_engine();
    engine.add_entity(slot(1), vec![0], None);

    // Send Drain immediately so the loop exits on first command check
    cmd_tx.send(IslandCommand::Drain).unwrap();

    engine.run(); // should return quickly
    assert!(engine.current_tick() <= 1);
}

// ── Run loop stops on shutdown flag ────────────────────────────────

#[test]
fn run_loop_stops_on_shutdown_flag() {
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    let (bridge_tx, bridge_rx) = crossbeam_channel::unbounded();
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let config = TickEngineConfig {
        tick_rate_hz: 20,
        max_catchup_ticks: 3,
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let mut engine = TickEngine::new(
        IslandId::from("test"),
        config,
        Box::new(NoopWasmExecutor),
        input_rx,
        bridge_rx,
        cmd_rx,
        shutdown,
        Arc::new(AtomicU64::new(0)),
    );

    // Set shutdown after a brief delay
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        shutdown_clone.store(true, Ordering::Relaxed);
    });

    engine.run(); // should stop within ~100ms
    drop(input_tx);
    drop(bridge_tx);
    drop(cmd_tx);
}

// ── Tick counter advances correctly ────────────────────────────────

#[test]
fn tick_counter_advances() {
    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = noop_engine();
    assert_eq!(engine.current_tick(), 0);
    engine.tick();
    assert_eq!(engine.current_tick(), 1);
    engine.tick_n(5);
    assert_eq!(engine.current_tick(), 6);
}

// ── Entities without messages are not processed ────────────────────

#[test]
fn idle_entities_not_processed() {
    let call_count = Arc::new(std::sync::Mutex::new(0u32));
    let count = call_count.clone();

    let wasm = MockWasm::new(move |_entity, state, _msg| {
        *count.lock().unwrap() += 1;
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);
    engine.add_entity(slot(2), vec![0], None);

    engine.tick(); // no inputs, no timers → no WASM calls
    assert_eq!(*call_count.lock().unwrap(), 0);
}

// ── WASM trap tier 1 (skip): entity processes next tick ────────────

#[test]
fn wasm_trap_tier1_skip_processes_next_tick() {
    let fault_count = Arc::new(std::sync::Mutex::new(0u32));
    let fc = fault_count.clone();

    let wasm = MockWasm::new(move |_entity, state, _msg| {
        let mut c = fc.lock().unwrap();
        *c += 1;
        if *c == 1 {
            Err(WasmTrap::EpochDeadline) // first call traps
        } else {
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        }
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![42], None);

    // Tick 0: send input, entity traps → Skip, quarantine 1 tick (resume at tick 1)
    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();
    engine.tick();
    assert_eq!(engine.fault_state(&slot(1)), ActorHealthState::Quarantined {
        consecutive_faults: 1,
        resume_at_tick: 1,
    });
    // Entity state unchanged (trap occurred before mutation)
    assert_eq!(engine.get_entity_state(&slot(1)), Some(&[42u8][..]));

    // Tick 1: entity resumes (quarantine expired), processes normally
    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 2,
            payload: vec![],
        })
        .unwrap();
    engine.tick();
    assert_eq!(*fault_count.lock().unwrap(), 2, "entity should have been called again");
}

// ── WASM trap tier 2 (reset): 3 consecutive traps reset to checkpoint ──

#[test]
fn wasm_trap_tier2_reset_to_checkpoint() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        // First modify state, then trap
        let mut new_state = state.to_vec();
        new_state[0] = new_state[0].wrapping_add(1);
        Err(WasmTrap::EpochDeadline)
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![10], None);

    // Create a checkpoint by simulating a persist
    // We need to set checkpoint state directly since the wasm always traps
    // The init_state is [10], so after 3 traps it should reset to init_state

    // Fault 1 (tick 0): Skip
    input_tx.send(ClientInput {
        session_id: SessionId::from("p1"),
        entity_slot: slot(1),
        input_seq: 1,
        payload: vec![],
    }).unwrap();
    engine.tick();

    // Fault 2 (tick 1): quarantine expires at tick 1, so entity can tick
    input_tx.send(ClientInput {
        session_id: SessionId::from("p1"),
        entity_slot: slot(1),
        input_seq: 2,
        payload: vec![],
    }).unwrap();
    engine.tick();

    // Tick 2: quarantine doesn't expire until tick 3
    // Tick 3: quarantine expires (resume_at_tick = 1 + 2 = 3)
    input_tx.send(ClientInput {
        session_id: SessionId::from("p1"),
        entity_slot: slot(1),
        input_seq: 3,
        payload: vec![],
    }).unwrap();
    engine.tick(); // tick 2: should_tick(2) >= 3 → false, skipped
    engine.tick(); // tick 3: should_tick(3) >= 3 → true, Fault 3 → Reset

    // After 3rd fault with Reset response, entity should be reset to init state [10]
    assert_eq!(engine.get_entity_state(&slot(1)), Some(&[10u8][..]));
}

// ── WASM trap tier 3 (recreate): 5 consecutive traps recreate entity ──

#[test]
fn wasm_trap_tier3_recreate_after_5_traps() {
    let wasm = MockWasm::new(|_entity, _state, _msg| {
        Err(WasmTrap::EpochDeadline)
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![99], None); // init state = [99]

    let mut seq = 1u32;
    // Accumulate 5 consecutive faults, respecting quarantine:
    // Fault 1 at tick 0: resume at 1
    // Fault 2 at tick 1: resume at 3
    // Fault 3 at tick 3: resume at 7 (Reset → init state since no checkpoint)
    // Fault 4 at tick 7: resume at 15
    // Fault 5 at tick 15: resume at 31 (Recreate → init state)
    let fault_ticks = [0u32, 1, 3, 7, 15];
    let mut current_tick = 0u32;

    for &target_tick in &fault_ticks {
        while current_tick < target_tick {
            engine.tick();
            current_tick += 1;
        }
        input_tx.send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: seq,
            payload: vec![],
        }).unwrap();
        seq += 1;
        engine.tick();
        current_tick += 1;
    }

    // After 5th fault (Recreate), entity should still exist with init state
    assert!(engine.get_entity_state(&slot(1)).is_some(), "recreated entity should still exist");
    assert_eq!(engine.get_entity_state(&slot(1)), Some(&[99u8][..]), "should be reset to init state");
    assert_eq!(
        engine.fault_state(&slot(1)),
        ActorHealthState::Quarantined {
            consecutive_faults: 5,
            resume_at_tick: 31,
        }
    );
}

// ── Entity eviction after 6 consecutive traps ──────────────────────

#[test]
fn entity_evicted_after_6_consecutive_traps() {
    let wasm = MockWasm::new(|_entity, _state, _msg| {
        Err(WasmTrap::EpochDeadline)
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);
    engine.add_entity(slot(2), vec![0], None);

    let mut seq = 1u32;
    // Fault 1 at tick 0: resume at 1
    // Fault 2 at tick 1: resume at 3
    // Fault 3 at tick 3: resume at 7 (Reset)
    // Fault 4 at tick 7: resume at 15
    // Fault 5 at tick 15: resume at 31 (Recreate)
    // Fault 6 at tick 31: Evict
    let fault_ticks = [0u32, 1, 3, 7, 15, 31];
    let mut current_tick = 0u32;

    for &target_tick in &fault_ticks {
        while current_tick < target_tick {
            engine.tick();
            current_tick += 1;
        }
        input_tx.send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: seq,
            payload: vec![],
        }).unwrap();
        seq += 1;
        engine.tick();
        current_tick += 1;
    }

    assert_eq!(engine.fault_state(&slot(1)), ActorHealthState::Evicted);
    assert!(engine.get_entity_state(&slot(1)).is_none(), "evicted entity should be removed");
    assert_eq!(engine.entity_count(), 1, "only entity 2 should remain");

    let effects = engine.take_effects();
    assert!(
        effects.iter().any(|e| matches!(e, BridgeEffect::EntityEvicted { entity } if *entity == slot(1))),
        "bridge should be notified of eviction"
    );
}

// ── Poison actor reset: 100 clean ticks resets fault counter ────────

#[test]
fn poison_actor_resets_after_100_clean_ticks() {
    let fault_count = Arc::new(std::sync::Mutex::new(0u32));
    let fc = fault_count.clone();

    let wasm = MockWasm::new(move |_entity, state, _msg| {
        let mut c = fc.lock().unwrap();
        *c += 1;
        if *c <= 2 {
            Err(WasmTrap::EpochDeadline)
        } else {
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        }
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Fault 1 (tick 0)
    input_tx.send(ClientInput {
        session_id: SessionId::from("p1"),
        entity_slot: slot(1),
        input_seq: 1,
        payload: vec![],
    }).unwrap();
    engine.tick();

    // Fault 2 (tick 1)
    input_tx.send(ClientInput {
        session_id: SessionId::from("p1"),
        entity_slot: slot(1),
        input_seq: 2,
        payload: vec![],
    }).unwrap();
    engine.tick();

    // Now faults stop. Need 100 clean ticks to reset.
    // Quarantine from 2nd fault: resume at tick 1 + 2 = 3
    // Send inputs for 100+ ticks starting from tick 3
    for i in 0..103 {
        input_tx.send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 3 + i,
            payload: vec![],
        }).unwrap();
        engine.tick();
    }

    // After 100+ clean ticks, fault state should be reset to Healthy
    assert_eq!(engine.fault_state(&slot(1)), ActorHealthState::Healthy);
}
