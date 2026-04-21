use std::time::Duration;

use rustc_hash::FxHashMap;

use quanta_realtime_server::reconnect::ReconnectTier;
use quanta_realtime_server::session_store::RetainedSession;
use quanta_realtime_server::sync::{
    recv_initial_state_stream, send_baseline_ack, send_initial_state_stream, BaselineAck,
    EntityPayload, InitialStateMessage,
};
use quanta_realtime_server::testing::endpoint_helpers::*;
use quanta_realtime_server::types::EntitySlot;
use quanta_realtime_server::EndpointConfig;

fn make_entities(count: usize) -> Vec<EntityPayload> {
    (0..count)
        .map(|i| EntityPayload {
            entity_slot: i as u32,
            state: vec![(i & 0xFF) as u8; 8],
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bulk_transfer_200_entities() {
    let (addr, mut session_rx, shutdown_tx, handle, _store) =
        start_test_server(EndpointConfig::default()).await;

    let client_endpoint = build_test_client(&[b"quanta-v1"]);
    let connection = client_endpoint
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    // Wait for the server-side ConnectedClient.
    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client should arrive")
        .expect("channel should not be closed");
    let server_conn = connected
        .quic_connection
        .expect("should have quic_connection");

    // Server sends initial state, client receives it — run concurrently.
    let msg = InitialStateMessage {
        baseline_tick: 42,
        flags: 0,
        schema_version: 1,
        compiled_schema: None,
        entities: make_entities(200),
    };

    let msg_clone = msg.clone();
    let timeout = Duration::from_secs(5);

    let client_fut = async {
        let (received, mut send) = recv_initial_state_stream(&connection, timeout).await?;
        send_baseline_ack(
            &mut send,
            &BaselineAck {
                baseline_tick: received.baseline_tick,
            },
        )
        .await?;
        Ok::<_, quanta_realtime_server::sync::SyncError>(received)
    };

    let (server_result, client_result) = tokio::join!(
        send_initial_state_stream(&server_conn, &msg_clone, timeout),
        client_fut,
    );

    let ack = server_result.expect("server should get ack");
    assert_eq!(ack.baseline_tick, 42);

    let received_msg = client_result.expect("client should receive message");
    assert_eq!(received_msg, msg);

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn fast_reconnect_with_retained_session() {
    let config = EndpointConfig::default();
    let (addr, mut session_rx, shutdown_tx, handle, store) = start_test_server(config).await;

    // First connection.
    let client = build_test_client(&[b"quanta-v1"]);
    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp = client_auth(&connection).await;
    assert!(resp.accepted);
    let session_id = resp.session_id;

    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");
    assert!(matches!(connected.reconnect_tier, ReconnectTier::Cold));

    connection.close(0u32.into(), b"disconnect");

    // Insert a retained session for the next expected session_id (TestValidator
    // auto-increments). The session_token must match what the client sends.
    let next_session_id = session_id + 1;
    {
        let mut s = store.lock().unwrap();
        s.insert(
            next_session_id,
            RetainedSession {
                baseline_tick: 500,
                visible_entities: vec![EntitySlot(0), EntitySlot(1)],
                input_seq: 10,
                session_token: next_session_id,
                client_capabilities: FxHashMap::default(),
                created_at: tokio::time::Instant::now(),
            },
        );
    }

    // Reconnect with session_token matching the retained session_token.
    let client2 = build_test_client(&[b"quanta-v1"]);
    let connection2 = client2
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp2 = client_auth_with_token(&connection2, Some(next_session_id)).await;
    assert!(resp2.accepted);
    assert_eq!(resp2.session_id, next_session_id);

    let connected2 = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");

    match connected2.reconnect_tier {
        ReconnectTier::Fast { retained } => {
            assert_eq!(retained.baseline_tick, 500);
            assert_eq!(retained.input_seq, 10);
        }
        ReconnectTier::Cold => panic!("expected Fast reconnect tier"),
    }

    connection2.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn cold_reconnect_after_session_expired() {
    let mut config = EndpointConfig::default();
    config.session_retain_duration = Duration::from_millis(200);

    let (addr, mut session_rx, shutdown_tx, handle, store) = start_test_server(config).await;

    // First connection.
    let client = build_test_client(&[b"quanta-v1"]);
    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp = client_auth(&connection).await;
    let session_id = resp.session_id;

    let _connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");

    // Insert a retained session.
    let next_session_id = session_id + 1;
    {
        let mut s = store.lock().unwrap();
        s.insert(
            next_session_id,
            RetainedSession {
                baseline_tick: 100,
                visible_entities: vec![],
                input_seq: 5,
                session_token: next_session_id,
                client_capabilities: FxHashMap::default(),
                created_at: tokio::time::Instant::now(),
            },
        );
    }

    connection.close(0u32.into(), b"disconnect");

    // Wait for the session to expire (200ms retain + margin).
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Reconnect — session should have expired, resulting in Cold tier.
    let client2 = build_test_client(&[b"quanta-v1"]);
    let connection2 = client2
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp2 = client_auth_with_token(&connection2, Some(next_session_id)).await;
    assert!(resp2.accepted);

    let connected2 = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");

    assert!(
        matches!(connected2.reconnect_tier, ReconnectTier::Cold),
        "expected Cold tier after session expiry"
    );

    connection2.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}
