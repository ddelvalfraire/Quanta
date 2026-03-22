mod common;

use common::*;
use quanta_realtime_server::command::ZoneTransferError;
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::types::IslandId;
use quanta_realtime_server::zone_transfer::BuffState;

#[tokio::test]
async fn same_server_prepare_then_accept() {
    let tx = spawn_manager(zone_transfer_config());
    activate(&tx, test_manifest("zone-a", 10)).await.unwrap();
    activate(&tx, test_manifest("zone-b", 10)).await.unwrap();
    player_joined(&tx, "zone-a").await.unwrap();

    let token_bytes = prepare_zone_transfer(
        &tx,
        "player-1",
        "zone-a",
        "zone-b",
        [1.0, 2.0, 3.0],
        [0.5, 0.0, -0.5],
        vec![BuffState {
            buff_id: "shield".into(),
            remaining_ms: 5000,
            data: vec![1],
        }],
    )
    .await
    .unwrap();

    let transferred = accept_zone_transfer(&tx, token_bytes, "zone-b")
        .await
        .unwrap();

    assert_eq!(transferred.player_id, "player-1");
    assert_eq!(transferred.source_zone, IslandId::from("zone-a"));
    assert_eq!(transferred.position, [1.0, 2.0, 3.0]);
    assert_eq!(transferred.velocity, [0.5, 0.0, -0.5]);
    assert_eq!(transferred.buffs[0].buff_id, "shield");

    stop(&tx, "zone-a").await.unwrap();
    stop(&tx, "zone-b").await.unwrap();
}

#[tokio::test]
async fn prepare_to_remote_target_succeeds() {
    let tx = spawn_manager(zone_transfer_config());
    activate(&tx, test_manifest("src", 10)).await.unwrap();
    player_joined(&tx, "src").await.unwrap();

    // Target doesn't exist locally — that's fine, it may be on another server.
    let result = prepare_zone_transfer(
        &tx,
        "player-1",
        "src",
        "remote-island",
        [0.0; 3],
        [0.0; 3],
        vec![],
    )
    .await;

    assert!(result.is_ok());
    stop(&tx, "src").await.unwrap();
}

#[tokio::test]
async fn accept_with_garbage_token_fails() {
    let tx = spawn_manager(zone_transfer_config());
    activate(&tx, test_manifest("target", 10)).await.unwrap();

    let result = accept_zone_transfer(&tx, vec![0xFF, 0xFE, 0xFD], "target").await;

    assert!(matches!(result, Err(ZoneTransferError::Transfer(_))));
    stop(&tx, "target").await.unwrap();
}

#[tokio::test]
async fn accept_on_nonexistent_target_fails() {
    let tx = spawn_manager(zone_transfer_config());

    let result = accept_zone_transfer(&tx, vec![1, 2, 3], "ghost").await;

    assert!(matches!(result, Err(ZoneTransferError::TargetNotFound(_))));
}

#[tokio::test]
async fn zone_transfer_not_configured_returns_error() {
    let tx = spawn_manager(ServerConfig::default());
    activate(&tx, test_manifest("island", 10)).await.unwrap();

    let result = prepare_zone_transfer(
        &tx,
        "player-1",
        "island",
        "other",
        [0.0; 3],
        [0.0; 3],
        vec![],
    )
    .await;

    assert!(matches!(result, Err(ZoneTransferError::NotConfigured)));
    stop(&tx, "island").await.unwrap();
}

#[tokio::test]
async fn prepare_from_nonexistent_source_fails() {
    let tx = spawn_manager(zone_transfer_config());

    let result = prepare_zone_transfer(
        &tx,
        "player-1",
        "ghost",
        "target",
        [0.0; 3],
        [0.0; 3],
        vec![],
    )
    .await;

    assert!(matches!(result, Err(ZoneTransferError::SourceNotFound(_))));
}
