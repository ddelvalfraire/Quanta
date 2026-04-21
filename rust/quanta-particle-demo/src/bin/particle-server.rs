//! Particle World server binary.
//!
//! Wires Quanta's platform runtime (`quanta-realtime-server`) with this
//! crate's `ParticleExecutor` via the public `executor_factory` hook.
//! The platform library knows nothing about particles — all demo-specific
//! code lives in this crate.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use quanta_particle_demo::input::{encode_datagram, ParticleInputPayload};
use quanta_particle_demo::server_info;
use quanta_realtime_server::tick::{ClientInput, SessionId};
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

    // Optional NPC swarm — spawns N server-side entities doing
    // deterministic random walks so a single browser tab can see the
    // platform at scale.
    let npc_count: u32 = std::env::var("QUANTA_NPC_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if npc_count > 0 {
        spawn_npc_swarm(&running.manager_tx, npc_count, shutdown_tx.subscribe()).await?;
    }

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

/// Allocate `count` NPC entity slots on the default island and spawn a
/// driver task per slot that feeds random-walk `ClientInput`s at 20 Hz.
/// Each NPC shows up in `TickSnapshot` just like a real client-owned
/// entity, so the fanout sends it to every watching tab.
async fn spawn_npc_swarm(
    manager_tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    count: u32,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    use quanta_realtime_server::types::EntitySlot;
    info!(npc_count = count, "spawning NPC swarm");
    let island_id = IslandId::from("particle-world");

    for i in 0..count {
        let (tx, rx) = tokio::sync::oneshot::channel();
        manager_tx
            .send(ManagerCommand::AllocateEntitySlot {
                island_id: island_id.clone(),
                reply: tx,
            })
            .await?;
        let (slot, input_tx) = rx.await??;
        let seed = ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)).max(1);
        let mut sd = shutdown_rx.clone();
        tokio::spawn(async move {
            drive_npc(slot, seed, input_tx, &mut sd).await;
        });
        if i % 100 == 0 {
            // Let the manager + tokio scheduler breathe during bulk spawn.
            tokio::task::yield_now().await;
        }
        // Swallow the unused-binding lint for the EntitySlot newtype
        // destructure on some clippy settings.
        let _ = EntitySlot(slot.0);
    }

    info!(npc_count = count, "NPC swarm live");
    // Drop the shutdown_rx clone held only to trigger close on exit.
    let _ = shutdown_rx.changed();
    Ok(())
}

async fn drive_npc(
    slot: quanta_realtime_server::types::EntitySlot,
    mut seed: u64,
    input_tx: crossbeam_channel::Sender<ClientInput>,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    let session_id = SessionId::from(format!("npc-{}", slot.0).as_str());
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input_seq: u32 = 0;
    let mut dir_x: f32 = 0.0;
    let mut dir_z: f32 = 0.0;
    let mut step: u32 = 0;
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Change direction roughly every 2 s (40 ticks) so the
                // swarm looks like lazy drift, not chaos.
                if step % 40 == 0 {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let angle = (seed as f32) * 9.313e-10; // u32 → radians-ish
                    dir_x = angle.cos();
                    dir_z = angle.sin();
                }
                step = step.wrapping_add(1);
                input_seq = input_seq.wrapping_add(1);
                let payload = encode_datagram(&ParticleInputPayload {
                    entity_slot: slot.0,
                    input_seq,
                    dir_x,
                    dir_z,
                    actions: 0,
                    dt_ms: 50,
                });
                if input_tx
                    .try_send(ClientInput {
                        session_id: session_id.clone(),
                        entity_slot: slot,
                        input_seq,
                        payload: payload.to_vec(),
                    })
                    .is_err()
                {
                    // Channel full — drop this tick, keep the NPC alive.
                }
                // Bytes import keeps the compiler from whining if this
                // helper is reshaped later.
                let _ = Bytes::new();
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
        }
    }
}
