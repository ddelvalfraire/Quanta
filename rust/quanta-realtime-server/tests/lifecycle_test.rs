use quanta_realtime_server::command::{ActivationError, DrainError, ManagerCommand};
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::manager::{manager_channel, IslandManager};
use quanta_realtime_server::types::{IslandId, IslandManifest};
use tokio::sync::oneshot;

fn test_manifest(id: &str, entity_count: u32) -> IslandManifest {
    IslandManifest {
        island_id: IslandId::from(id),
        entity_count,
        wasm_module: "test.wasm".into(),
        initial_state: vec![],
    }
}

async fn activate(
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

async fn drain(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), DrainError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Drain {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

async fn stop(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    island_id: &str,
) -> Result<(), DrainError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Stop {
        island_id: IslandId::from(island_id),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap()
}

async fn get_metrics(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
) -> quanta_realtime_server::command::ManagerMetrics {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::GetMetrics { reply: reply_tx })
        .await
        .unwrap();
    reply_rx.await.unwrap()
}

/// Spawn a manager task and return the command sender. Drops the sender to
/// shut down the manager when the test ends.
fn spawn_manager(config: ServerConfig) -> tokio::sync::mpsc::Sender<ManagerCommand> {
    let (tx, rx) = manager_channel(256);
    tokio::spawn(async move {
        let mut mgr = IslandManager::new(config, rx);
        mgr.run().await;
    });
    tx
}

#[tokio::test]
async fn activate_and_drain_lifecycle() {
    let tx = spawn_manager(ServerConfig::default());

    activate(&tx, test_manifest("island-1", 50)).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1);
    assert_eq!(m.total_entities, 50);

    drain(&tx, "island-1").await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0);
}

#[tokio::test]
async fn stop_from_running() {
    let tx = spawn_manager(ServerConfig::default());
    activate(&tx, test_manifest("island-1", 10)).await.unwrap();
    stop(&tx, "island-1").await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0);
}

#[tokio::test]
async fn reject_duplicate_island() {
    let tx = spawn_manager(ServerConfig::default());
    activate(&tx, test_manifest("dup", 10)).await.unwrap();

    let err = activate(&tx, test_manifest("dup", 10)).await.unwrap_err();
    assert_eq!(err, ActivationError::DuplicateIsland(IslandId::from("dup")));

    // Cleanup
    stop(&tx, "dup").await.unwrap();
}

#[tokio::test]
async fn reject_at_max_capacity() {
    let config = ServerConfig {
        max_islands: 2,
        ..Default::default()
    };
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("a", 10)).await.unwrap();
    activate(&tx, test_manifest("b", 10)).await.unwrap();

    let err = activate(&tx, test_manifest("c", 10)).await.unwrap_err();
    assert_eq!(err, ActivationError::AtCapacity { max: 2 });

    // Cleanup
    stop(&tx, "a").await.unwrap();
    stop(&tx, "b").await.unwrap();
}

#[tokio::test]
async fn drain_nonexistent_island() {
    let tx = spawn_manager(ServerConfig::default());
    let err = drain(&tx, "ghost").await.unwrap_err();
    assert_eq!(err, DrainError::NotFound(IslandId::from("ghost")));
}

#[tokio::test]
async fn pooled_vs_dedicated_thread_model() {
    let config = ServerConfig {
        entity_threshold: 100,
        ..Default::default()
    };
    let tx = spawn_manager(config);

    // 50 entities -> Pooled, 200 entities -> Dedicated
    activate(&tx, test_manifest("small", 50)).await.unwrap();
    activate(&tx, test_manifest("big", 200)).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 2);
    assert_eq!(m.total_entities, 250);

    // Cleanup
    stop(&tx, "small").await.unwrap();
    stop(&tx, "big").await.unwrap();
}

#[tokio::test]
async fn concurrent_100_islands_no_deadlock() {
    let tx = spawn_manager(ServerConfig::default());

    // Activate 100 islands concurrently.
    let mut handles = Vec::new();
    for i in 0..100 {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            activate(&tx, test_manifest(&format!("island-{i}"), 5))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 100);
    assert_eq!(m.total_entities, 500);

    // Drain all concurrently.
    let mut handles = Vec::new();
    for i in 0..100 {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            drain(&tx, &format!("island-{i}")).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0);
}
