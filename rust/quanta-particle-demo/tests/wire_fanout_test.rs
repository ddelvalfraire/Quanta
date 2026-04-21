//! End-to-end fanout test: two QUIC clients authenticate, one sends WASD
//! input datagrams for ~1 second, and the second observes delta datagrams
//! flowing back through the fanout path.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::watch;

use quanta_particle_demo::input::{encode_datagram, ParticleInputPayload};
use quanta_particle_demo::{particle_executor_factory, particle_fanout_factory};
use quanta_realtime_server::auth::AcceptAllValidator;
use quanta_realtime_server::command::ManagerCommand;
use quanta_realtime_server::config::{EndpointConfig, ServerConfig};
use quanta_realtime_server::delta_envelope::parse_delta_datagram;
use quanta_realtime_server::testing::endpoint_helpers::{build_test_client, client_auth};
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::types::{IslandId, IslandManifest};
use quanta_realtime_server::{run_server, RunServerArgs};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_clients_exchange_deltas() {
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
        server_id: "srv-fanout".into(),
        executor_factory: Some(particle_executor_factory(20)),
        fanout_factory: Some(particle_fanout_factory()),
        default_island_id: Some(IslandId::from("particle-world")),
    })
    .await
    .expect("run_server");

    let (tx, rx) = tokio::sync::oneshot::channel();
    running
        .manager_tx
        .send(ManagerCommand::Activate {
            manifest: IslandManifest {
                island_id: IslandId::from("particle-world"),
                entity_count: 0,
                wasm_module: "particle".into(),
                initial_state: Vec::new(),
                passivate_when_empty: false,
            },
            reply: tx,
        })
        .await
        .unwrap();
    rx.await.unwrap().expect("activate");

    let client = build_test_client(&[b"quanta-v1"]);
    let conn_a = client
        .connect(running.quic_addr, "localhost")
        .unwrap()
        .await
        .unwrap();
    let _ = client_auth(&conn_a).await;

    let conn_b = client
        .connect(running.quic_addr, "localhost")
        .unwrap()
        .await
        .unwrap();
    let _ = client_auth(&conn_b).await;

    tokio::time::sleep(Duration::from_millis(250)).await;

    let received = Arc::new(AtomicU32::new(0));
    let reader_received = received.clone();
    let conn_b_reader = conn_b.clone();
    tokio::spawn(async move {
        while let Ok(bytes) = conn_b_reader.read_datagram().await {
            if parse_delta_datagram(&bytes).is_ok() {
                reader_received.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let start = Instant::now();
    let mut seq = 0u32;
    while start.elapsed() < Duration::from_secs(1) {
        seq += 1;
        let payload = encode_datagram(&ParticleInputPayload {
            entity_slot: 0,
            input_seq: seq,
            dir_x: 1.0,
            dir_z: 0.0,
            actions: 0,
            dt_ms: 50,
        });
        let _ = conn_a.send_datagram(bytes::Bytes::copy_from_slice(&payload));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    let count = received.load(Ordering::Relaxed);
    assert!(
        count >= 10,
        "observer should receive ≥10 deltas, got {count}"
    );

    conn_a.close(0u32.into(), b"done");
    conn_b.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    for t in running.tasks {
        let _ = tokio::time::timeout(Duration::from_secs(2), t).await;
    }
}
