//! Ten-client, five-second smoke of the load harness — keeps CI honest
//! that the N-client Tokio path connects, receives datagrams, and exits
//! cleanly without disconnects.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use quanta_particle_demo::load::{run_load, LoadConfig};
use quanta_particle_demo::{particle_executor_factory, particle_fanout_factory};
use quanta_realtime_server::auth::AcceptAllValidator;
use quanta_realtime_server::command::ManagerCommand;
use quanta_realtime_server::config::{EndpointConfig, ServerConfig};
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::types::{IslandId, IslandManifest};
use quanta_realtime_server::{run_server, RunServerArgs};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn load_ten_clients_five_seconds() {
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
        server_id: "srv-load".into(),
        executor_factory: Some(particle_executor_factory(20)),
        fanout_factory: Some(particle_fanout_factory()),
        default_island_id: Some(IslandId::from("particle-world")),
        metrics_addr: None,
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

    let summary = run_load(LoadConfig {
        addr: running.quic_addr,
        clients: 10,
        duration: Duration::from_secs(5),
        ramp: Duration::from_secs(1),
        input_hz: 20,
        token: "load-smoke-token".into(),
    })
    .await;

    assert_eq!(summary.connects_attempted, 10);
    assert_eq!(
        summary.connects_succeeded, 10,
        "all 10 should connect, got {:?}",
        summary
    );
    assert_eq!(summary.disconnects_midrun, 0, "no mid-run disconnects");
    assert!(
        summary.datagrams_received >= 100,
        "expected >=100 deltas in 5s, got {}",
        summary.datagrams_received
    );

    let _ = shutdown_tx.send(true);
    for t in running.tasks {
        let _ = tokio::time::timeout(Duration::from_secs(2), t).await;
    }
}
