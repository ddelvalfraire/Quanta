use quanta_realtime_server::command::ManagerCommand;
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

fn spawn_manager(config: ServerConfig) -> tokio::sync::mpsc::Sender<ManagerCommand> {
    let (tx, rx) = manager_channel(256);
    tokio::spawn(async move {
        let mut mgr = IslandManager::new(config, rx);
        mgr.run().await;
    });
    tx
}

#[tokio::test]
async fn get_metrics_reflects_mutations() {
    let tx = spawn_manager(ServerConfig::default());

    // Initially empty.
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::GetMetrics { reply: reply_tx })
        .await
        .unwrap();
    let m = reply_rx.await.unwrap();
    assert_eq!(m.active_islands, 0);
    assert_eq!(m.total_entities, 0);

    // Activate two islands.
    for (id, count) in [("a", 30), ("b", 70)] {
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(ManagerCommand::Activate {
            manifest: test_manifest(id, count),
            reply: reply_tx,
        })
        .await
        .unwrap();
        reply_rx.await.unwrap().unwrap();
    }

    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::GetMetrics { reply: reply_tx })
        .await
        .unwrap();
    let m = reply_rx.await.unwrap();
    assert_eq!(m.active_islands, 2);
    assert_eq!(m.total_islands, 2);
    assert_eq!(m.total_entities, 100);

    // Drain one island.
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Drain {
        island_id: IslandId::from("a"),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap().unwrap();

    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::GetMetrics { reply: reply_tx })
        .await
        .unwrap();
    let m = reply_rx.await.unwrap();
    assert_eq!(m.active_islands, 1);
    assert_eq!(m.total_islands, 2); // stopped island still in registry
    assert_eq!(m.total_entities, 100); // entity count unchanged

    // Cleanup
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(ManagerCommand::Stop {
        island_id: IslandId::from("b"),
        reply: reply_tx,
    })
    .await
    .unwrap();
    reply_rx.await.unwrap().unwrap();
}

#[tokio::test]
async fn capacity_signal_serialization() {
    use quanta_realtime_server::capacity::CapacitySignal;
    use quanta_realtime_server::command::ManagerMetrics;

    let metrics = ManagerMetrics {
        active_islands: 3,
        total_islands: 5,
        total_entities: 500,
    };
    let signal = CapacitySignal::from_metrics("srv-test", 200, &metrics);
    let json = serde_json::to_string_pretty(&signal).unwrap();

    assert!(json.contains("\"server_id\": \"srv-test\""));
    assert!(json.contains("\"active_islands\": 3"));
    assert!(json.contains("\"max_islands\": 200"));
    assert!(json.contains("\"total_entities\": 500"));
    assert!(json.contains("\"cpu_load\": 0.0"));
    assert!(json.contains("\"memory_used\": 0"));
}
