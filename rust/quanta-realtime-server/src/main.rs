use quanta_realtime_server::capacity::run_capacity_publisher;
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::manager::{manager_channel, IslandManager};
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quanta_realtime_server=info".into()),
        )
        .init();

    let config = ServerConfig::default();
    let server_id = generate_server_id();

    info!(%server_id, nats_url = %config.nats_url, "starting quanta-realtime-server");

    let nats_client = async_nats::connect(&config.nats_url).await?;
    info!("connected to NATS");

    let (cmd_tx, cmd_rx) = manager_channel(256);

    let capacity_subject = format!("{}.{}", config.capacity_subject, server_id);
    let capacity_interval = Duration::from_secs(config.capacity_interval_secs);
    let max_islands = config.max_islands;
    let server_id_clone = server_id.clone();
    let capacity_tx = cmd_tx.clone();

    tokio::spawn(async move {
        run_capacity_publisher(
            capacity_tx,
            nats_client,
            capacity_subject,
            server_id_clone,
            max_islands,
            capacity_interval,
        )
        .await;
    });

    let bridge = std::sync::Arc::new(quanta_realtime_server::stubs::StubBridge);
    let mut manager = IslandManager::new(config, cmd_rx, bridge);
    manager.run().await;

    Ok(())
}

fn generate_server_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    format!("srv-{:08x}{:04x}", (nanos / 1_000_000) as u32, pid as u16)
}
