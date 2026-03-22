#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use quanta_realtime_server::command::{ActivationError, IslandCommand, LifecycleError, ManagerCommand};
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::manager::{manager_channel, IslandManager};
use quanta_realtime_server::stubs::StubBridge;
use quanta_realtime_server::tick::*;
use quanta_realtime_server::types::{EntitySlot, IslandId, IslandManifest};
use tokio::sync::oneshot;

pub fn test_manifest(id: &str, entity_count: u32) -> IslandManifest {
    IslandManifest {
        island_id: IslandId::from(id),
        entity_count,
        wasm_module: "test.wasm".into(),
        initial_state: vec![],
        passivate_when_empty: true,
    }
}

pub async fn activate(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    manifest: IslandManifest,
) -> Result<(), ActivationError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Activate {
        manifest,
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub async fn drain(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), LifecycleError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Drain {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub async fn stop(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), LifecycleError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Stop {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub async fn get_metrics(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
) -> quanta_realtime_server::command::ManagerMetrics {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::GetMetrics { reply: reply_tx })
        .await
        .unwrap();
    reply_rx.await.unwrap()
}

pub async fn player_joined(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), LifecycleError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::PlayerJoined {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub async fn player_left(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), LifecycleError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::PlayerLeft {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub async fn bridge_message(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
    payload: Vec<u8>,
) -> Result<(), LifecycleError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::BridgeMessage {
        island_id: IslandId::from(island_id),
        payload,
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub fn test_manifest_no_passivate(id: &str, entity_count: u32) -> IslandManifest {
    IslandManifest {
        island_id: IslandId::from(id),
        entity_count,
        wasm_module: "test.wasm".into(),
        initial_state: vec![],
        passivate_when_empty: false,
    }
}

pub async fn player_input(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), LifecycleError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::PlayerInput {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

pub fn spawn_manager(config: ServerConfig) -> tokio::sync::mpsc::Sender<ManagerCommand> {
    let (tx, rx) = manager_channel(256);
    let bridge = Arc::new(StubBridge);
    tokio::spawn(async move {
        let mut mgr = IslandManager::new(config, rx, bridge);
        mgr.run().await;
    });
    tx
}

// ── Tick engine test helpers ───────────────────────────────────────

pub fn slot(n: u32) -> EntitySlot {
    EntitySlot(n)
}

pub struct MockWasm {
    handler:
        Box<dyn FnMut(EntitySlot, &[u8], &TickMessage) -> Result<HandleResult, WasmTrap> + Send>,
}

impl MockWasm {
    pub fn new<F>(handler: F) -> Self
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

pub fn test_engine(
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
    let heartbeat = Arc::new(AtomicU64::new(0));
    let engine = TickEngine::new(
        IslandId::from("test-island"),
        config,
        wasm,
        input_rx,
        bridge_rx,
        cmd_rx,
        shutdown,
        heartbeat,
    );
    (engine, input_tx, cmd_tx, bridge_tx)
}

pub fn noop_engine() -> (
    TickEngine,
    crossbeam_channel::Sender<ClientInput>,
    crossbeam_channel::Sender<IslandCommand>,
    crossbeam_channel::Sender<BridgeMessage>,
) {
    test_engine(Box::new(NoopWasmExecutor))
}
