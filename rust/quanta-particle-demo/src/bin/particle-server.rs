//! Particle World server binary.
//!
//! Wires Quanta's platform runtime (`quanta-realtime-server`) with this
//! crate's `ParticleExecutor` via the public `executor_factory` hook.
//! The platform library knows nothing about particles — all demo-specific
//! code lives in this crate.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use quanta_particle_demo::server_info;
use tokio::sync::watch;
use tracing::{info, warn};

use quanta_particle_demo::{particle_executor_factory, particle_fanout_factory};
use quanta_realtime_server::auth::DevTokenValidator;
use quanta_realtime_server::command::ManagerCommand;
use quanta_realtime_server::config::{EndpointConfig, ServerConfig};
use quanta_realtime_server::ids::generate_server_id;
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::types::{IslandId, IslandManifest};
use quanta_realtime_server::{run_server, RunServerArgs};

const DEFAULT_DEV_TOKEN: &str = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";
const DEMO_TICK_RATE_HZ: u8 = 20;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "particle_server=info,quanta_particle_demo=info,quanta_realtime_server=info".into()
            }),
        )
        .init();

    let quic_addr: SocketAddr = std::env::var("QUANTA_QUIC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:4443".into())
        .parse()?;

    // Prometheus /metrics endpoint. Unset → default 127.0.0.1:9090; set
    // to empty string to disable; otherwise the parsed address.
    let metrics_addr: Option<SocketAddr> = match std::env::var("QUANTA_METRICS_ADDR") {
        Ok(s) if s.is_empty() => None,
        Ok(s) => Some(s.parse()?),
        Err(_) => Some("127.0.0.1:9090".parse().unwrap()),
    };

    let server_config = ServerConfig {
        nats_url: std::env::var("QUANTA_NATS_URL").ok(),
        ..ServerConfig::default()
    };

    let server_id = generate_server_id("particle");
    let dev_token = std::env::var("QUANTA_DEV_TOKEN").unwrap_or_else(|_| DEFAULT_DEV_TOKEN.into());
    let validator = DevTokenValidator::new(dev_token) as Arc<_>;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    info!(%server_id, %quic_addr, "starting particle-world server");

    let running = run_server(RunServerArgs {
        server_config,
        endpoint_config: EndpointConfig::default(),
        quic_addr,
        ws_addr: None,
        tls: TlsConfig::SelfSigned,
        validator,
        shutdown_rx,
        server_id,
        executor_factory: Some(particle_executor_factory(DEMO_TICK_RATE_HZ)),
        fanout_factory: Some(particle_fanout_factory()),
        default_island_id: Some(IslandId::from("particle-world")),
        metrics_addr,
    })
    .await?;

    // Activate the default "particle-world" island so incoming clients have
    // somewhere to land. `passivate_when_empty: false` keeps the island up
    // before the first client joins.
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
        .await?;
    rx.await??;
    info!(island_id = "particle-world", "default island activated");

    // Publish connection info for the browser demo. Relative default path
    // assumes the server runs from the `rust/` directory.
    let info_path = std::env::var("QUANTA_SERVER_INFO_FILE")
        .unwrap_or_else(|_| "../examples/particle-world/public/server-info.json".into());
    if let Err(e) = server_info::write_server_info(
        Path::new(&info_path),
        running.quic_addr,
        running.cert_sha256,
    ) {
        warn!(error = %e, path = %info_path, "failed to write server-info.json");
    }

    info!(
        quic_addr = %running.quic_addr,
        tick_rate_hz = DEMO_TICK_RATE_HZ,
        "particle-world server running — press Ctrl-C to stop"
    );

    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");
    let _ = shutdown_tx.send(true);

    for task in running.tasks {
        let _ = task.await;
    }
    Ok(())
}
