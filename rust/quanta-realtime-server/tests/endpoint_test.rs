use std::time::Duration;

use quanta_realtime_server::reconnect::ReconnectTier;
use quanta_realtime_server::testing::endpoint_helpers::*;
use quanta_realtime_server::EndpointConfig;

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quic_handshake_with_quanta_v1_alpn() {
    let (addr, _session_rx, shutdown_tx, handle, _store) =
        start_test_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let hd = connection
        .handshake_data()
        .and_then(|hd| hd.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|hd| hd.protocol);

    assert_eq!(hd.as_deref(), Some(b"quanta-v1".as_slice()));

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn h3_without_webtransport_handshake_times_out() {
    let mut config = EndpointConfig::default();
    config.auth_timeout = Duration::from_millis(200);

    let (addr, _session_rx, shutdown_tx, handle, _store) = start_test_server(config).await;
    let client = build_test_client(&[b"h3"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let err = connection.closed().await;
    assert!(
        matches!(err, quinn::ConnectionError::ApplicationClosed(_)),
        "expected ApplicationClosed, got {err:?}"
    );

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn auth_flow_valid_token() {
    let (addr, mut session_rx, shutdown_tx, handle, _store) =
        start_test_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);
    assert!(resp.session_id > 0);

    // Verify ConnectedClient was delivered to the application layer
    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client should arrive")
        .expect("channel should not be closed");
    assert_eq!(
        connected.session.transport_type(),
        quanta_realtime_server::TransportType::Quic
    );
    assert!(matches!(connected.reconnect_tier, ReconnectTier::Cold));

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn auth_timeout_closes_connection() {
    let mut config = EndpointConfig::default();
    config.auth_timeout = Duration::from_millis(200);

    let (addr, _session_rx, shutdown_tx, handle, _store) = start_test_server(config).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    // Don't open a bidi stream and don't send auth — server should timeout
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        connection.close_reason().is_some(),
        "connection should be closed after auth timeout"
    );

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn datagram_send_after_auth() {
    let (addr, mut session_rx, shutdown_tx, handle, _store) =
        start_test_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    // Wait for the server-side ConnectedClient to be delivered
    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client should arrive")
        .expect("channel should not be closed");

    // Send a datagram from client to server
    let payload = b"hello-quanta";
    connection
        .send_datagram(bytes::Bytes::from_static(payload))
        .expect("send datagram");

    // Wait for the datagram to arrive on the server-side session
    let mut received = None;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if let Some(data) = connected.session.recv_datagram() {
            received = Some(data);
            break;
        }
    }
    assert_eq!(received.as_deref(), Some(payload.as_slice()));

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn self_signed_tls_handshake_succeeds() {
    let (addr, _session_rx, shutdown_tx, handle, _store) =
        start_test_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("TLS handshake should succeed with self-signed cert");

    let _ = client_auth(&connection).await;
    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn rate_limiting_excess_refused() {
    let mut config = EndpointConfig::default();
    config.rate_limit_per_sec = 2;

    let (addr, _session_rx, shutdown_tx, handle, _store) = start_test_server(config).await;

    let mut successes = 0u32;
    let mut failures = 0u32;

    for _ in 0..10 {
        let client = build_test_client(&[b"quanta-v1"]);
        match tokio::time::timeout(
            Duration::from_millis(500),
            client.connect(addr, "localhost").unwrap(),
        )
        .await
        {
            Ok(Ok(_conn)) => successes += 1,
            _ => failures += 1,
        }
    }

    assert!(successes > 0, "at least one connection should succeed");
    assert!(
        failures > 0,
        "at least one connection should be rate-limited"
    );

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn connected_client_includes_quic_connection() {
    let (addr, mut session_rx, shutdown_tx, handle, _store) =
        start_test_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let _ = client_auth(&connection).await;

    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client should arrive")
        .expect("channel should not be closed");

    assert!(connected.quic_connection.is_some());
    assert!(connected.session_id > 0);

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}
