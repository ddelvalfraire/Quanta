//! End-to-end wiring test: run the full realtime server in-process and verify
//! that QUIC accept -> auth -> ConnectedClient -> IslandManager path works.
//!
//! The existing `endpoint_test.rs` and `webtransport_test.rs` verify pieces
//! in isolation (QuicEndpoint alone). This test exercises the `run_server()`
//! composition that `main.rs` uses, catching regressions where the listener
//! is wired but never drained into the manager.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use quanta_realtime_server::auth::AcceptAllValidator;
use quanta_realtime_server::command::ManagerCommand;
use quanta_realtime_server::config::{EndpointConfig, ServerConfig};
use quanta_realtime_server::testing::endpoint_helpers::{build_test_client, client_auth};
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::{run_server, RunServerArgs};

async fn get_connected_clients(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
) -> usize {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(ManagerCommand::GetMetrics { reply: reply_tx })
        .await
        .expect("manager channel");
    reply_rx.await.expect("metrics").connected_clients
}

async fn wait_for_connected(
    tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    target: usize,
    max_wait: Duration,
) -> usize {
    let deadline = std::time::Instant::now() + max_wait;
    loop {
        let n = get_connected_clients(tx).await;
        if n == target || std::time::Instant::now() >= deadline {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn server_accepts_quic_and_routes_to_manager() {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let validator = AcceptAllValidator::new() as Arc<_>;

    let running = run_server(RunServerArgs {
        server_config: ServerConfig::default(), // nats_url: None by default
        endpoint_config: EndpointConfig::default(),
        quic_addr: "127.0.0.1:0".parse().unwrap(),
        ws_addr: None,
        tls: TlsConfig::SelfSigned,
        validator,
        shutdown_rx,
        server_id: "srv-test".into(),
    })
    .await
    .expect("run_server");

    let client = build_test_client(&[b"quanta-v1"]);
    let connection = client
        .connect(running.quic_addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);
    assert!(resp.session_id > 0);

    // Second client, verifies routing is idempotent.
    let connection2 = client
        .connect(running.quic_addr, "localhost")
        .expect("connect2")
        .await
        .expect("handshake2");
    let resp2 = client_auth(&connection2).await;
    assert!(resp2.accepted);
    assert_ne!(resp.session_id, resp2.session_id);

    // Give the drain loop a moment to forward both clients to the manager.
    tokio::time::sleep(Duration::from_millis(100)).await;

    connection.close(0u32.into(), b"done");
    connection2.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    for task in running.tasks {
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
    }
}

#[tokio::test]
async fn client_disconnect_shrinks_manager_vec() {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let validator = AcceptAllValidator::new() as Arc<_>;

    let running = run_server(RunServerArgs {
        server_config: ServerConfig::default(),
        endpoint_config: EndpointConfig::default(),
        quic_addr: "127.0.0.1:0".parse().unwrap(),
        ws_addr: None,
        tls: TlsConfig::SelfSigned,
        validator,
        shutdown_rx,
        server_id: "srv-test".into(),
    })
    .await
    .expect("run_server");

    let client = build_test_client(&[b"quanta-v1"]);
    let connection = client
        .connect(running.quic_addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    // Manager receives the ClientConnected within a few ticks of the drain loop.
    let before = wait_for_connected(&running.manager_tx, 1, Duration::from_secs(2)).await;
    assert_eq!(before, 1, "client should be tracked after auth");

    // Close the QUIC connection from the client side — the server-side monitor
    // task should observe `connection.closed()` and emit `ClientDisconnected`,
    // which removes the entry from the manager's placeholder vec.
    connection.close(0u32.into(), b"bye");
    drop(connection);

    let after = wait_for_connected(&running.manager_tx, 0, Duration::from_secs(2)).await;
    assert_eq!(after, 0, "manager should drop client entry on disconnect");

    let _ = shutdown_tx.send(true);
    for task in running.tasks {
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
    }
}

#[tokio::test]
async fn server_runs_without_nats() {
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let validator = AcceptAllValidator::new() as Arc<_>;
    let cfg = ServerConfig {
        nats_url: None,
        ..ServerConfig::default()
    };

    let running = run_server(RunServerArgs {
        server_config: cfg,
        endpoint_config: EndpointConfig::default(),
        quic_addr: "127.0.0.1:0".parse().unwrap(),
        ws_addr: None,
        tls: TlsConfig::SelfSigned,
        validator,
        shutdown_rx,
        server_id: "srv-test".into(),
    })
    .await
    .expect("run_server with no NATS");

    assert!(running.quic_addr.port() > 0);
}
