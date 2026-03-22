mod common;

use common::*;
use quanta_realtime_server::command::{ActivationError, LifecycleError};
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::types::IslandId;

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
    assert_eq!(m.total_entities, 0);
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

    stop(&tx, "a").await.unwrap();
    stop(&tx, "b").await.unwrap();
}

#[tokio::test]
async fn drain_nonexistent_island() {
    let tx = spawn_manager(ServerConfig::default());
    let err = drain(&tx, "ghost").await.unwrap_err();
    assert_eq!(err, LifecycleError::NotFound(IslandId::from("ghost")));
}

#[tokio::test]
async fn pooled_vs_dedicated_thread_model() {
    let config = ServerConfig {
        entity_threshold: 100,
        ..Default::default()
    };
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("small", 50)).await.unwrap();
    activate(&tx, test_manifest("big", 200)).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 2);
    assert_eq!(m.total_entities, 250);

    stop(&tx, "small").await.unwrap();
    stop(&tx, "big").await.unwrap();
}

#[tokio::test]
async fn concurrent_100_islands_no_deadlock() {
    let tx = spawn_manager(ServerConfig::default());

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
    assert_eq!(m.total_entities, 0);
}
