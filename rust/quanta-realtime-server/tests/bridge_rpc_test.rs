use quanta_realtime_server::command::IslandCommand;
use quanta_realtime_server::tick::*;
use quanta_realtime_server::types::{EntitySlot, IslandId};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

fn slot(n: u32) -> EntitySlot {
    EntitySlot(n)
}

/// Create a TickEngine with a custom WasmExecutor for testing.
fn test_engine(
    wasm: Box<dyn WasmExecutor>,
) -> (
    TickEngine,
    crossbeam_channel::Sender<ClientInput>,
    crossbeam_channel::Sender<IslandCommand>,
    crossbeam_channel::Sender<BridgeMessage>,
) {
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (bridge_tx, bridge_rx) = crossbeam_channel::unbounded();
    let config = TickEngineConfig {
        tick_rate_hz: 20,
        max_catchup_ticks: 3,
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    let engine = TickEngine::new(
        IslandId::from("test-island"),
        config,
        wasm,
        input_rx,
        bridge_rx,
        cmd_rx,
        shutdown,
        Arc::new(AtomicU64::new(0)),
    );
    (engine, input_tx, cmd_tx, bridge_tx)
}

struct MockWasm {
    handler:
        Box<dyn FnMut(EntitySlot, &[u8], &TickMessage) -> Result<HandleResult, WasmTrap> + Send>,
}

impl MockWasm {
    fn new<F>(handler: F) -> Self
    where
        F: FnMut(EntitySlot, &[u8], &TickMessage) -> Result<HandleResult, WasmTrap>
            + Send
            + 'static,
    {
        Self {
            handler: Box::new(handler),
        }
    }
}

impl WasmExecutor for MockWasm {
    fn call_handle_message(
        &mut self,
        entity: EntitySlot,
        state: &[u8],
        message: &TickMessage,
    ) -> Result<HandleResult, WasmTrap> {
        (self.handler)(entity, state, message)
    }
}

// ── Request-reply: bridge request → entity Reply → BridgeReply effect ──

#[test]
fn bridge_request_reply_produces_bridge_reply_effect() {
    let cid = [0xAA; 16];

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if let TickMessage::BridgeRequest { payload, .. } = msg {
            // Entity processes the request and replies
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![TickEffect::Reply(payload.clone())],
            })
        } else {
            Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            })
        }
    });

    let (mut engine, _input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    bridge_tx
        .send(BridgeMessage {
            target_entity: slot(1),
            kind: BridgeMessageKind::Request {
                correlation_id: cid,
            },
            payload: b"check-inventory".to_vec(),
        })
        .unwrap();

    engine.tick();

    let effects = engine.take_effects();
    let reply = effects
        .iter()
        .find(|e| matches!(e, BridgeEffect::BridgeReply { .. }));
    assert!(reply.is_some(), "should produce a BridgeReply effect");

    if let BridgeEffect::BridgeReply {
        correlation_id,
        payload,
    } = reply.unwrap()
    {
        assert_eq!(*correlation_id, cid, "correlation_id must match");
        assert_eq!(payload, b"check-inventory");
    }
}

// ── One-way bridge message delivered as TickMessage::Bridge ────────

#[test]
fn bridge_one_way_message_delivered_to_entity() {
    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recv = received.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if let TickMessage::Bridge { payload } = msg {
            recv.lock().unwrap().extend_from_slice(payload);
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    bridge_tx
        .send(BridgeMessage {
            target_entity: slot(1),
            kind: BridgeMessageKind::OneWay,
            payload: b"state-update".to_vec(),
        })
        .unwrap();

    engine.tick();
    assert_eq!(*received.lock().unwrap(), b"state-update");
}

// ── Bridge messages have priority 2 (after timers, before inputs) ──

#[test]
fn bridge_messages_processed_between_timers_and_inputs() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ord = order.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        let label = match msg {
            TickMessage::Timer { .. } => "timer",
            TickMessage::Bridge { .. } => "bridge",
            TickMessage::Input { .. } => "input",
            TickMessage::Deferred { .. } => "deferred",
            TickMessage::BridgeRequest { .. } => "bridge_request",
            TickMessage::SagaFailed { .. } => "saga_failed",
        };
        ord.lock().unwrap().push(label);
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Set timer that fires on tick 1
    engine.set_timer(slot(1), "t".into(), 50);

    // Consume tick 0 with an input to advance
    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();
    engine.tick(); // tick 0
    order.lock().unwrap().clear();

    // On tick 1: timer fires, bridge message, and input all present
    bridge_tx
        .send(BridgeMessage {
            target_entity: slot(1),
            kind: BridgeMessageKind::OneWay,
            payload: b"bridge-data".to_vec(),
        })
        .unwrap();

    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 2,
            payload: vec![],
        })
        .unwrap();

    engine.tick(); // tick 1

    let labels = order.lock().unwrap().clone();
    assert_eq!(labels, vec!["timer", "bridge", "input"]);
}

// ── RequestRemote effect produces BridgeEffect::RequestRemote ──────

#[test]
fn request_remote_effect_routed_to_bridge() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::RequestRemote {
                target: "actor_type.validate".into(),
                payload: b"validate-this".to_vec(),
            }],
        })
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

    engine.tick();

    let effects = engine.take_effects();
    let req = effects.iter().find(|e| {
        matches!(e, BridgeEffect::RequestRemote { .. })
    });
    assert!(req.is_some(), "should emit RequestRemote bridge effect");

    if let BridgeEffect::RequestRemote {
        source_entity,
        target,
        payload,
    } = req.unwrap()
    {
        assert_eq!(*source_entity, slot(1));
        assert_eq!(target, "actor_type.validate");
        assert_eq!(payload, b"validate-this");
    }
}

// ── FireAndForget effect produces BridgeEffect::FireAndForget ──────

#[test]
fn fire_and_forget_effect_routed_to_bridge() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::FireAndForget {
                target: "vfx.particles".into(),
                payload: b"explosion".to_vec(),
            }],
        })
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

    engine.tick();

    let effects = engine.take_effects();
    let ff = effects.iter().find(|e| {
        matches!(e, BridgeEffect::FireAndForget { .. })
    });
    assert!(ff.is_some(), "should emit FireAndForget bridge effect");

    if let BridgeEffect::FireAndForget { target, payload } = ff.unwrap() {
        assert_eq!(target, "vfx.particles");
        assert_eq!(payload, b"explosion");
    }
}

// ── SagaFailed message delivered to entity ─────────────────────────

#[test]
fn saga_failed_delivered_to_entity() {
    let saga_cid = [0xBB; 16];
    let received_cid = Arc::new(std::sync::Mutex::new(None));
    let recv = received_cid.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if let TickMessage::SagaFailed { correlation_id } = msg {
            *recv.lock().unwrap() = Some(*correlation_id);
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Bridge layer delivers SagaFailed after detecting a request timeout.
    // In production: RequestReplyMap::remove_expired() → BridgeMessage with SagaFailed kind.
    bridge_tx
        .send(BridgeMessage {
            target_entity: slot(1),
            kind: BridgeMessageKind::SagaFailed {
                correlation_id: saga_cid,
            },
            payload: vec![],
        })
        .unwrap();

    engine.tick();

    let cid = received_cid.lock().unwrap();
    assert_eq!(*cid, Some(saga_cid), "entity should receive saga_failed with matching correlation_id");
}

// ── BridgeRequest with no Reply effect produces no BridgeReply ─────

#[test]
fn bridge_request_without_reply_produces_no_bridge_reply() {
    let cid = [0xCC; 16];

    let wasm = MockWasm::new(|_entity, state, _msg| {
        // Entity processes request but does NOT reply
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::EmitTelemetry {
                event: "processed".into(),
            }],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    bridge_tx
        .send(BridgeMessage {
            target_entity: slot(1),
            kind: BridgeMessageKind::Request {
                correlation_id: cid,
            },
            payload: b"request".to_vec(),
        })
        .unwrap();

    engine.tick();

    let effects = engine.take_effects();
    let has_reply = effects
        .iter()
        .any(|e| matches!(e, BridgeEffect::BridgeReply { .. }));
    assert!(!has_reply, "no Reply effect → no BridgeReply");

    let has_telemetry = effects
        .iter()
        .any(|e| matches!(e, BridgeEffect::EmitTelemetry { .. }));
    assert!(has_telemetry, "other effects still routed normally");
}

// ── Multiple bridge messages to same entity in one tick ────────────

#[test]
fn multiple_bridge_messages_all_processed() {
    let count = Arc::new(std::sync::Mutex::new(0u32));
    let cnt = count.clone();

    let wasm = MockWasm::new(move |_entity, state, msg| {
        if matches!(msg, TickMessage::Bridge { .. } | TickMessage::BridgeRequest { .. }) {
            *cnt.lock().unwrap() += 1;
        }
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    for i in 0..3 {
        bridge_tx
            .send(BridgeMessage {
                target_entity: slot(1),
                kind: BridgeMessageKind::OneWay,
                payload: vec![i],
            })
            .unwrap();
    }

    engine.tick();
    assert_eq!(*count.lock().unwrap(), 3);
}

// ── Bridge message to nonexistent entity is silently dropped ───────

#[test]
fn bridge_message_to_nonexistent_entity_dropped() {
    let call_count = Arc::new(std::sync::Mutex::new(0u32));
    let cnt = call_count.clone();

    let wasm = MockWasm::new(move |_entity, state, _msg| {
        *cnt.lock().unwrap() += 1;
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    });

    let (mut engine, _input_tx, _cmd_tx, bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);

    // Send to entity 99 which doesn't exist
    bridge_tx
        .send(BridgeMessage {
            target_entity: slot(99),
            kind: BridgeMessageKind::OneWay,
            payload: b"lost".to_vec(),
        })
        .unwrap();

    engine.tick();
    assert_eq!(*call_count.lock().unwrap(), 0, "no WASM calls for missing entity");
}
