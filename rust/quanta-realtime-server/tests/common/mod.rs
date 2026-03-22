use quanta_realtime_server::command::{ActivationError, LifecycleError, ManagerCommand};
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::manager::{manager_channel, IslandManager};
use quanta_realtime_server::types::{IslandId, IslandManifest};
use tokio::sync::oneshot;

pub fn test_manifest(id: &str, entity_count: u32) -> IslandManifest {
    IslandManifest {
        island_id: IslandId::from(id),
        entity_count,
        wasm_module: "test.wasm".into(),
        initial_state: vec![],
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

pub fn spawn_manager(config: ServerConfig) -> tokio::sync::mpsc::Sender<ManagerCommand> {
    let (tx, rx) = manager_channel(256);
    tokio::spawn(async move {
        let mut mgr = IslandManager::new(config, rx);
        mgr.run().await;
    });
    tx
}
