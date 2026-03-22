mod common;

use common::*;
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::types::EntitySlot;

fn fast_passivation_config() -> ServerConfig {
    ServerConfig {
        idle_timeout_secs: 2,
        grace_period_secs: 1,
        ..Default::default()
    }
}

#[tokio::test]
async fn idle_island_passivates_after_timeout() {
    let config = fast_passivation_config();
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("idle-1", 10)).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1);

    // idle_timeout (2s) + margin for the 1s check interval
    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0, "island should have been passivated");
}

#[tokio::test]
async fn passivation_writes_checkpoint() {
    let config = fast_passivation_config();
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("ckpt-1", 10)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0);

    bridge_message(&tx, "ckpt-1", EntitySlot(0), vec![1, 2, 3]).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1, "island should be reactivated from checkpoint");

    stop(&tx, "ckpt-1").await.unwrap();
}

#[tokio::test]
async fn reactivation_loads_correct_state_from_checkpoint() {
    let config = fast_passivation_config();
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("react-1", 5)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0, "island should have been passivated");

    bridge_message(&tx, "react-1", EntitySlot(0), vec![42]).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1, "island should be reactivated");

    stop(&tx, "react-1").await.unwrap();
}

#[tokio::test]
async fn grace_period_cancelled_by_player_join() {
    let config = ServerConfig {
        idle_timeout_secs: 10,
        grace_period_secs: 2,
        ..Default::default()
    };
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("grace-1", 10)).await.unwrap();

    player_joined(&tx, "grace-1").await.unwrap();
    player_left(&tx, "grace-1").await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Re-join within grace period cancels passivation
    player_joined(&tx, "grace-1").await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1, "passivation should have been cancelled by player join");

    stop(&tx, "grace-1").await.unwrap();
}

#[tokio::test]
async fn passivate_when_empty_false_keeps_island_running() {
    let config = fast_passivation_config();
    let tx = spawn_manager(config);

    activate(&tx, test_manifest_no_passivate("npc-zone", 10))
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(4000)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(
        m.active_islands, 1,
        "island with passivate_when_empty=false should stay running"
    );

    stop(&tx, "npc-zone").await.unwrap();
}

#[tokio::test]
async fn concurrent_passivation_50_islands_no_deadlock() {
    let config = fast_passivation_config();
    let tx = spawn_manager(config);

    let mut handles = Vec::new();
    for i in 0..50 {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            activate(&tx, test_manifest(&format!("pass-{i}"), 5))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 50);

    tokio::time::sleep(std::time::Duration::from_millis(4000)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(
        m.active_islands, 0,
        "all 50 islands should have been passivated"
    );
}

#[tokio::test]
async fn reactivated_island_resumes_from_correct_tick() {
    let config = ServerConfig {
        idle_timeout_secs: 1,
        grace_period_secs: 1,
        ..Default::default()
    };
    let tx = spawn_manager(config);

    activate(&tx, test_manifest("tick-resume", 5)).await.unwrap();

    // At 20Hz, ~40 ticks in 2s
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0, "island should be passivated");

    bridge_message(&tx, "tick-resume", EntitySlot(0), vec![1]).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1, "island should be reactivated");

    stop(&tx, "tick-resume").await.unwrap();
}
