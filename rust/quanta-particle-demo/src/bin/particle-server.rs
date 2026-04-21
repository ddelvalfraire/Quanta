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
/// Server tick rate in Hz. Bumped 20→30 to close the gap between server
/// updates and 60 fps client rendering. Must match `TICK_RATE_HZ` in
/// `examples/particle-world/src/state.ts` — the client uses it to convert
/// server ticks to time for snapshot interpolation.
const DEMO_TICK_RATE_HZ: u8 = 30;

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
        tick_rate_hz: DEMO_TICK_RATE_HZ,
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

    // Optional NPC swarm — spawns N server-side entities driven by a
    // single shared boids-lite simulator that observes the engine's
    // per-tick snapshot so NPCs can flock and follow the player.
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

/// Allocate `count` NPC entity slots and spawn a single swarm-mind task
/// that subscribes to the engine's tick snapshot and feeds smooth
/// direction inputs each tick — gentle flocking + weak player attraction
/// so NPCs follow whoever's playing.
async fn spawn_npc_swarm(
    manager_tx: &tokio::sync::mpsc::Sender<ManagerCommand>,
    count: u32,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    use quanta_realtime_server::types::EntitySlot;
    info!(npc_count = count, "spawning NPC swarm");
    let island_id = IslandId::from("particle-world");

    let mut npc_slots: Vec<EntitySlot> = Vec::with_capacity(count as usize);
    let mut input_tx_opt: Option<crossbeam_channel::Sender<ClientInput>> = None;

    for i in 0..count {
        let (tx, rx) = tokio::sync::oneshot::channel();
        manager_tx
            .send(ManagerCommand::AllocateEntitySlot {
                island_id: island_id.clone(),
                reply: tx,
            })
            .await?;
        let (slot, input_tx) = rx.await??;
        npc_slots.push(slot);
        if input_tx_opt.is_none() {
            input_tx_opt = Some(input_tx);
        }
        if i % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    let input_tx = input_tx_opt.expect("at least one NPC → have an input_tx");

    // Subscribe to the engine's tick snapshot.
    let (sub_tx, sub_rx) = tokio::sync::oneshot::channel();
    manager_tx
        .send(ManagerCommand::SubscribeSnapshots {
            island_id: island_id.clone(),
            reply: sub_tx,
        })
        .await?;
    let snap_rx = sub_rx.await??;

    info!(npc_count = count, "NPC swarm live");
    tokio::spawn(async move {
        run_swarm_mind(npc_slots, input_tx, snap_rx, shutdown_rx).await;
    });
    Ok(())
}

/// Drive every NPC each tick from the authoritative snapshot. Each NPC
/// has a *current heading* that turns smoothly (bounded angular velocity)
/// toward a *desired heading* computed from:
///
/// - weak pull toward the nearest real-client entity (player attraction),
/// - a small repulsion from the nearest neighbour (separation),
/// - tiny per-NPC wander noise so still scenes don't stack.
///
/// Continuous heading → continuous velocity → no direction teleports, so
/// the client's linear extrapolation between 20 Hz ticks is accurate.
async fn run_swarm_mind(
    slots: Vec<quanta_realtime_server::types::EntitySlot>,
    input_tx: crossbeam_channel::Sender<ClientInput>,
    snap_rx: crossbeam_channel::Receiver<quanta_realtime_server::tick::types::TickSnapshot>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    use std::collections::HashSet;
    use tokio::time::{interval, Duration as TD, MissedTickBehavior};

    let npc_set: HashSet<u32> = slots.iter().map(|s| s.0).collect();
    let mut headings: std::collections::HashMap<u32, f32> =
        slots.iter().map(|s| (s.0, 0.0)).collect();
    // Per-NPC independent noise phase.
    let mut phase: std::collections::HashMap<u32, f32> = slots
        .iter()
        .map(|s| (s.0, (s.0 as f32) * 0.7531_f32))
        .collect();
    let mut input_seq: u32 = 0;

    // Reynolds (1986) boids rules, plus weak player attraction + per-NPC
    // wander so still scenes don't stack. Weights blend as a vector sum
    // whose angle becomes the desired heading — NPCs steer toward it at
    // a bounded turn rate for continuous motion.
    //
    // Tuned assuming `DEMO_TICK_RATE_HZ = 30` on the server. If the tick
    // rate changes, MAX_TURN_RATE * TICK_PERIOD_S is the per-tick
    // angular step; keep it small enough that flocks curve smoothly.
    const TICK_PERIOD_S: f32 = 1.0 / 30.0;
    const MAX_TURN_RATE: f32 = 5.0; // radians/sec ≈ 290°/s
    /// Topological neighbourhood size (Ballerini et al., PNAS 2008:
    /// "Interaction ruling animal collective behavior depends on
    /// topological rather than metric distance"). Each NPC considers
    /// exactly K nearest neighbours regardless of cluster density —
    /// this is what real starlings do, and it's what kills the
    /// "jittery when clustered" problem: a metric (radius-based)
    /// neighbourhood has a hard boundary, and in a dense cluster ~50
    /// neighbours are constantly crossing that boundary tick-to-tick,
    /// flipping which ones contribute to the force sum. Topological
    /// K is always exactly K, so the cardinality of the interaction
    /// set is stable and the force angle changes smoothly as neighbours
    /// reshuffle by rank instead of blinking in and out of existence.
    const TOPOLOGICAL_K: usize = 7;
    /// Absolute max distance — ignored unless a neighbour is outside this
    /// bound (e.g. the whole flock is nearby; this just prevents the
    /// algorithm from picking the player across the world if there are
    /// fewer than K nearby NPCs).
    const CONSIDER_RADIUS: f32 = 1500.0;
    const CONSIDER_R2: f32 = CONSIDER_RADIUS * CONSIDER_RADIUS;
    /// Radius for the separation force specifically. Only the subset
    /// of the K neighbours that are inside this distance push apart
    /// — lets the flock be tight without every pair separating.
    const SEPARATION_RADIUS: f32 = 110.0;
    const SEPARATION_R2: f32 = SEPARATION_RADIUS * SEPARATION_RADIUS;

    // Reynolds' three classical flocking weights. Separation dominates
    // (keeps birds from merging), alignment and cohesion are softer
    // grouping forces.
    const SEPARATION_WEIGHT: f32 = 1.8;
    const ALIGNMENT_WEIGHT: f32 = 1.0;
    const COHESION_WEIGHT: f32 = 0.8;
    /// Player acts as a predator / disturbance: NPCs scatter when the
    /// player gets close, regroup behind them. Unlike an attraction
    /// force this keeps density DOWN around the player (where the
    /// jitter was worst) and makes the player's movement visually
    /// meaningful — the swarm reacts to you instead of piling on you.
    const PREDATOR_RADIUS: f32 = 300.0;
    const PREDATOR_R2: f32 = PREDATOR_RADIUS * PREDATOR_RADIUS;
    const PREDATOR_WEIGHT: f32 = 2.2;
    const WANDER_WEIGHT: f32 = 0.25;

    // Use `tokio::time::interval` (not `sleep` in a loop) for the
    // swarm cadence. `sleep(33ms)` fires `33ms + process_time` after
    // each iteration — over many cycles the effective period drifts
    // above 33.3 ms and swarm-mind falls behind the server tick. When
    // it catches up two inputs land in one server tick, producing
    // 2× physics steps for those NPCs (visible micro-stutter). `interval`
    // schedules ticks at absolute times and self-corrects for drift.
    // `Skip` missed-tick behavior avoids firing a burst after a long
    // stall (process suspended, debug pause, etc.).
    let mut ticker = interval(TD::from_micros(33_333));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            _ = ticker.tick() => {
                // Drain any snapshots that queued up.
                let mut latest = None;
                while let Ok(s) = snap_rx.try_recv() {
                    latest = Some(s);
                }
                let Some(snapshot) = latest else { continue };

                // Pick one real-client entity as the attractor. If several
                // players are in the tab, pick the one with the largest
                // velocity magnitude — drama.
                let mut player = None;
                let mut best_speed = -1.0f32;
                for e in &snapshot.entities {
                    if npc_set.contains(&e.slot.0) { continue; }
                    let sp = (e.pos_x * e.pos_x + e.pos_z * e.pos_z).sqrt();
                    // Fall back to position distance from origin if vel
                    // isn't in the snapshot (it currently isn't).
                    if sp > best_speed {
                        best_speed = sp;
                        player = Some((e.pos_x, e.pos_z));
                    }
                }
                // If no player is connected, `player` stays `None` and the
                // predator-repulsion branch below is skipped — the flock
                // just flocks on its own rather than fleeing an imaginary
                // predator at the world origin.
                let player_pos: Option<(f32, f32)> = player;

                // Build slot → (x, z) lookup once per tick so separation
                // is O(N) not O(N²). We still O(N²) through it for the
                // cheap nearest-neighbour scan — fine at N=500.
                let mut positions: Vec<(u32, f32, f32)> =
                    Vec::with_capacity(snapshot.entities.len());
                for e in &snapshot.entities {
                    positions.push((e.slot.0, e.pos_x, e.pos_z));
                }

                for &(slot_id, sx, sz) in &positions {
                    if !npc_set.contains(&slot_id) { continue; }

                    // NOTE (experiment reverted): there used to be a
                    // `tokio::task::yield_now().await` every 128 NPCs
                    // here to cooperate with other tokio tasks at large
                    // N. It introduced split-tick input firing: the
                    // first 128 NPC inputs landed in the engine's
                    // current tick drain, the rest landed in the
                    // next — causing the exact 2×-speed bursts we
                    // originally eliminated by switching to
                    // `tokio::time::interval`. At N=300 the whole loop
                    // is <1 ms; yielding is unnecessary and harmful.
                    // If N grows much past ~500 and the QUIC loops
                    // start starving, consider spawning this loop on
                    // `tokio::task::spawn_blocking` rather than
                    // yielding mid-batch.

                    // Predator repulsion: when a connected player is
                    // inside `PREDATOR_RADIUS`, steer away with a force
                    // that scales linearly with proximity. Outside the
                    // radius the player is invisible to the swarm, so
                    // the flock does its own thing until you disturb it.
                    let predator_vec = player_pos.and_then(|(px, pz)| {
                        let dx_p = sx - px; // away from player
                        let dz_p = sz - pz;
                        let d2_p = dx_p * dx_p + dz_p * dz_p;
                        if d2_p < PREDATOR_R2 && d2_p > 1e-3 {
                            // Linear fall-off: weight 1 at d=0, 0 at
                            // radius. Normalised direction × weight,
                            // not raw displacement, so close encounters
                            // don't explode the force magnitude.
                            let d = d2_p.sqrt();
                            let weight = 1.0 - d / PREDATOR_RADIUS;
                            Some((dx_p / d * weight, dz_p / d * weight))
                        } else {
                            None
                        }
                    });

                    // Canonical Reynolds boids separation — pure
                    // displacement-vector sum (NOT 1/distance). A 1/d
                    // weighting has a singularity as d → 0: when two
                    // boids cluster close, the force angle flips from
                    // tick to tick based on micro-motion, which the
                    // client's interpolation renders as visible jitter
                    // (the classic "smooth when spread, jittery when
                    // clustered" signature). The unweighted sum stays
                    // finite and changes smoothly with neighbour count
                    // and geometry. See Van Hunter Adams' reference
                    // implementation, Wikipedia Boids, and the Pico boids
                    // algorithm write-up.
                    //
                    // Combined one-pass accumulator:
                    //   - separation: sum of (self - other) for d < SEPARATION_RADIUS
                    //   - cohesion:   mean position of neighbours inside NEIGHBOUR_RADIUS
                    //   - alignment:  mean unit-heading of neighbouring NPCs
                    let mut sep_x = 0.0f32;
                    let mut sep_z = 0.0f32;
                    let mut cohesion_x_sum = 0.0f32;
                    let mut cohesion_z_sum = 0.0f32;
                    let mut cohesion_n = 0u32;
                    let mut align_x_sum = 0.0f32;
                    let mut align_z_sum = 0.0f32;
                    let mut align_n = 0u32;
                    // Topological K-nearest: find the K closest OTHER NPCs
                    // by distance. Uses a tiny fixed-size max-heap
                    // (represented as a K-element array kept sorted by
                    // distance² descending so the "worst" is at index 0).
                    // O(N) distance computations + O(K log K) updates per
                    // NPC. At N=1000, K=7 that's 7000 heap ops per NPC per
                    // tick = 7M ops/tick total — fine.
                    let mut knn: [(f32, u32, f32, f32); TOPOLOGICAL_K] =
                        [(f32::INFINITY, 0, 0.0, 0.0); TOPOLOGICAL_K];
                    let mut knn_len: usize = 0;
                    let mut knn_worst: f32 = f32::INFINITY;
                    for &(other_id, ox, oz) in &positions {
                        if other_id == slot_id { continue; }
                        if !npc_set.contains(&other_id) { continue; }
                        let dx = sx - ox;
                        let dz = sz - oz;
                        let d2 = dx * dx + dz * dz;
                        if d2 > CONSIDER_R2 { continue; }
                        if knn_len < TOPOLOGICAL_K {
                            knn[knn_len] = (d2, other_id, ox, oz);
                            knn_len += 1;
                            if knn_len == TOPOLOGICAL_K {
                                knn_worst = knn.iter().map(|e| e.0).fold(0.0f32, f32::max);
                            }
                        } else if d2 < knn_worst {
                            // Replace the current worst.
                            let mut worst_idx = 0;
                            let mut worst_val = knn[0].0;
                            for i in 1..TOPOLOGICAL_K {
                                if knn[i].0 > worst_val {
                                    worst_val = knn[i].0;
                                    worst_idx = i;
                                }
                            }
                            knn[worst_idx] = (d2, other_id, ox, oz);
                            knn_worst = knn.iter().map(|e| e.0).fold(0.0f32, f32::max);
                        }
                    }

                    // Apply all three rules to the K-nearest set.
                    for i in 0..knn_len {
                        let (d2, other_id, ox, oz) = knn[i];
                        let dx = sx - ox;
                        let dz = sz - oz;
                        // Separation: only the subset inside SEPARATION_RADIUS.
                        if d2 < SEPARATION_R2 {
                            sep_x += dx;
                            sep_z += dz;
                        }
                        // Cohesion + alignment always count all K — the
                        // topological neighbourhood IS the reference set.
                        cohesion_x_sum += ox;
                        cohesion_z_sum += oz;
                        cohesion_n += 1;
                        if let Some(h) = headings.get(&other_id) {
                            align_x_sum += h.cos();
                            align_z_sum += h.sin();
                            align_n += 1;
                        }
                    }
                    let sep_angle = if sep_x.abs() + sep_z.abs() > 1e-5 {
                        Some(sep_z.atan2(sep_x))
                    } else {
                        None
                    };
                    let cohesion_angle = if cohesion_n > 0 {
                        let cx = cohesion_x_sum / cohesion_n as f32;
                        let cz = cohesion_z_sum / cohesion_n as f32;
                        Some((cz - sz).atan2(cx - sx))
                    } else {
                        None
                    };
                    let alignment_angle = if align_n > 0
                        && align_x_sum.abs() + align_z_sum.abs() > 1e-5
                    {
                        Some(align_z_sum.atan2(align_x_sum))
                    } else {
                        None
                    };

                    // Per-NPC wander: sinusoidal phase so each NPC has a
                    // unique slow drift axis — keeps idle flocks from
                    // collapsing onto a single point.
                    let ph = phase.entry(slot_id).or_insert(0.0);
                    *ph += 0.01;
                    let wander_angle = *ph;

                    // Reynolds-style weighted vector sum. Each force
                    // contributes a unit vector at its angle scaled by
                    // its weight; the resultant's angle is the desired
                    // heading.
                    let mut vx = WANDER_WEIGHT * wander_angle.cos();
                    let mut vz = WANDER_WEIGHT * wander_angle.sin();
                    if let Some((rx, rz)) = predator_vec {
                        vx += PREDATOR_WEIGHT * rx;
                        vz += PREDATOR_WEIGHT * rz;
                    }
                    if let Some(sa) = sep_angle {
                        vx += SEPARATION_WEIGHT * sa.cos();
                        vz += SEPARATION_WEIGHT * sa.sin();
                    }
                    if let Some(ca) = cohesion_angle {
                        vx += COHESION_WEIGHT * ca.cos();
                        vz += COHESION_WEIGHT * ca.sin();
                    }
                    if let Some(aa) = alignment_angle {
                        vx += ALIGNMENT_WEIGHT * aa.cos();
                        vz += ALIGNMENT_WEIGHT * aa.sin();
                    }
                    let desired = vz.atan2(vx);

                    // Turn current heading toward desired at max turn rate.
                    let current = headings.entry(slot_id).or_insert(0.0);
                    let mut diff = desired - *current;
                    // Wrap to [-PI, PI]
                    while diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
                    while diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
                    let max_step = MAX_TURN_RATE * TICK_PERIOD_S;
                    let step = diff.clamp(-max_step, max_step);
                    *current += step;

                    let dir_x = current.cos();
                    let dir_z = current.sin();
                    input_seq = input_seq.wrapping_add(1);
                    let payload = encode_datagram(&ParticleInputPayload {
                        entity_slot: slot_id,
                        input_seq,
                        dir_x,
                        dir_z,
                        actions: 0,
                        // 33 ms = one server tick at DEMO_TICK_RATE_HZ = 30.
                        dt_ms: 33,
                    });
                    let _ = input_tx.try_send(ClientInput {
                        session_id: SessionId::from(format!("npc-{}", slot_id).as_str()),
                        entity_slot: quanta_realtime_server::types::EntitySlot(slot_id),
                        input_seq,
                        payload: payload.to_vec(),
                    });
                    // Keeps Bytes import active if the helper is reshaped.
                    let _ = Bytes::new();
                }
            }
        }
    }
}
