use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::watch;
use tracing::info;

use quanta_realtime_server::auth::DevTokenValidator;
use quanta_realtime_server::config::{EndpointConfig, ServerConfig};
use quanta_realtime_server::ids::generate_server_id;
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::{run_server, RunServerArgs};

const DEFAULT_DEV_TOKEN: &str = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quanta_realtime_server=info".into()),
        )
        .init();

    let quic_addr: SocketAddr = std::env::var("QUANTA_QUIC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:4443".into())
        .parse()?;
    let ws_addr: Option<SocketAddr> = std::env::var("QUANTA_WS_ADDR")
        .ok()
        .map(|s| s.parse())
        .transpose()?;

    let server_config = ServerConfig {
        nats_url: std::env::var("QUANTA_NATS_URL").ok(),
        ..ServerConfig::default()
    };

    let server_id = generate_server_id("srv");
    let dev_token = std::env::var("QUANTA_DEV_TOKEN").unwrap_or_else(|_| DEFAULT_DEV_TOKEN.into());
    let validator = DevTokenValidator::new(dev_token) as Arc<_>;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    info!(
        %server_id,
        %quic_addr,
        ?ws_addr,
        nats = ?server_config.nats_url,
        "starting quanta-realtime-server"
    );

    let running = run_server(RunServerArgs {
        server_config,
        endpoint_config: EndpointConfig::default(),
        quic_addr,
        ws_addr,
        tls: TlsConfig::SelfSigned,
        validator,
        shutdown_rx,
        server_id,
        executor_factory: None,
    })
    .await?;

    info!(
        quic_addr = %running.quic_addr,
        ws_addr = ?running.ws_addr,
        "server running — press Ctrl-C to stop"
    );

    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");
    let _ = shutdown_tx.send(true);

    for task in running.tasks {
        let _ = task.await;
    }
    Ok(())
}
