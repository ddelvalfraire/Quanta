mod common;

use quanta_realtime_server::tick::*;
use quanta_realtime_server::types::IslandId;
use quanta_realtime_server::zone_transfer::*;

use common::{slot, test_engine, MockWasm};

fn zone(s: &str) -> IslandId {
    IslandId::from(s)
}

#[tokio::test(start_paused = true)]
async fn same_server_prepare_accept_acknowledge() {
    let mut mgr = ZoneTransferManager::new(ZoneTransferConfig::for_testing());

    let pos = [42.0, 10.0, -7.5];
    let vel = [1.0, 0.0, -0.5];
    let buffs = vec![BuffState {
        buff_id: "shield".into(),
        remaining_ms: 3000,
        data: vec![10, 20],
    }];

    let token = mgr
        .prepare_transfer(
            "player-1".into(),
            zone("island-a"),
            zone("island-b"),
            pos,
            vel,
            buffs.clone(),
        )
        .unwrap();

    assert!(mgr.is_transferring("player-1"));

    let ts = token.timestamp;
    let transferred = mgr
        .accept_transfer_at(&token, &zone("island-b"), ts + 10)
        .unwrap();

    assert_eq!(transferred.player_id, "player-1");
    assert_eq!(transferred.source_zone, zone("island-a"));
    assert_eq!(transferred.position, pos);
    assert_eq!(transferred.velocity, vel);
    assert_eq!(transferred.buffs, buffs);

    mgr.acknowledge_transfer("player-1").unwrap();
    assert!(!mgr.is_transferring("player-1"));
}

#[test]
fn cross_server_token_survives_serialization() {
    let signer = TokenSigner::new(b"cross-server-shared-key-xxxxxxx", 10_000);
    let ts = 2_000_000u64;

    let token = signer.sign_at(
        "player-cross".into(),
        zone("server-a:zone-1"),
        zone("server-b:zone-3"),
        [999.0, 50.0, -100.0],
        [10.0, 0.0, -5.0],
        vec![BuffState {
            buff_id: "armor".into(),
            remaining_ms: 9000,
            data: vec![42],
        }],
        ts,
    );

    let wire_bytes = token.to_bytes();
    let received = ZoneTransferToken::from_bytes(&wire_bytes).unwrap();

    assert!(signer
        .validate_at(&received, &zone("server-b:zone-3"), ts + 500)
        .is_ok());
    assert_eq!(received, token);
}

#[test]
fn zone_transfer_effect_routed_to_bridge() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::ZoneTransfer {
                player_id: "player-1".into(),
                target_zone: IslandId::from("zone-b"),
                position: [10.0, 0.0, 20.0],
                velocity: [1.0, 0.0, -1.0],
                buffs: vec![],
            }],
        })
    });

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(0), vec![1, 2, 3], None);

    input_tx
        .send(ClientInput {
            session_id: SessionId::from("sess-1"),
            entity_slot: slot(0),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();

    engine.tick();

    let effects = engine.take_effects();
    let transfers: Vec<_> = effects
        .iter()
        .filter(|e| matches!(e, BridgeEffect::ZoneTransferRequest { .. }))
        .collect();

    assert_eq!(transfers.len(), 1);
    if let BridgeEffect::ZoneTransferRequest {
        player_id,
        source_entity,
        target_zone,
        position,
        velocity,
        ..
    } = &transfers[0]
    {
        assert_eq!(player_id, "player-1");
        assert_eq!(*source_entity, slot(0));
        assert_eq!(*target_zone, IslandId::from("zone-b"));
        assert_eq!(*position, [10.0, 0.0, 20.0]);
        assert_eq!(*velocity, [1.0, 0.0, -1.0]);
    } else {
        panic!("expected ZoneTransferRequest");
    }
}
