//! Load harness: spawn N Tokio-driven WebTransport clients against a
//! running `particle-server`, drive WASD-style input, aggregate counters.
//!
//! Designed for validation of the "thousands of players on one node"
//! claim. The `run_load` entry point takes a [`LoadConfig`], ramps the
//! clients in over a configurable interval to avoid thundering-herd
//! connect spikes, and returns a [`Summary`] once the run duration
//! elapses. Feature-gated behind `load` because it depends on
//! `quanta-realtime-server/test-utils`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::watch;
use tracing::warn;

use quanta_realtime_server::auth::{AuthRequest, AuthResponse};
use quanta_realtime_server::delta_envelope::parse_delta_datagram;
use quanta_realtime_server::testing::endpoint_helpers::build_test_client;

use crate::input::{encode_datagram, ParticleInputPayload};

/// Matches the `DEFAULT_DEV_TOKEN` baked into `particle-server.rs`.
/// Kept in sync manually — if the server default changes, update here.
pub const DEFAULT_DEV_TOKEN: &str = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

#[derive(Debug, Clone)]
pub struct LoadConfig {
    pub addr: SocketAddr,
    pub clients: u32,
    pub duration: Duration,
    pub ramp: Duration,
    pub input_hz: u16,
    /// Bearer token presented in the auth handshake. Must match the
    /// server's validator — `DEFAULT_DEV_TOKEN` works against a default
    /// `particle-server` build.
    pub token: String,
}

#[derive(Debug, Default, Clone)]
pub struct Summary {
    pub connects_attempted: u64,
    pub connects_succeeded: u64,
    pub disconnects_midrun: u64,
    pub datagrams_received: u64,
    pub bytes_received: u64,
    pub datagrams_sent: u64,
    pub bytes_sent: u64,
}

pub async fn run_load(cfg: LoadConfig) -> Summary {
    let ep = Arc::new(build_test_client(&[b"quanta-v1"]));
    let (stop_tx, stop_rx) = watch::channel(false);

    let ca = Arc::new(AtomicU64::new(0));
    let cs = Arc::new(AtomicU64::new(0));
    let dc = Arc::new(AtomicU64::new(0));
    let dr = Arc::new(AtomicU64::new(0));
    let br = Arc::new(AtomicU64::new(0));
    let ds = Arc::new(AtomicU64::new(0));
    let bs = Arc::new(AtomicU64::new(0));

    let deadline = Instant::now() + cfg.duration;
    let ramp_step = if cfg.clients == 0 {
        Duration::ZERO
    } else {
        cfg.ramp / cfg.clients
    };
    let tick = Duration::from_millis((1000 / cfg.input_hz.max(1)) as u64);

    let mut handles = Vec::with_capacity(cfg.clients as usize);
    for i in 0..cfg.clients {
        if ramp_step > Duration::ZERO {
            tokio::time::sleep(ramp_step).await;
        }
        let ep = ep.clone();
        let addr = cfg.addr;
        let stop = stop_rx.clone();
        let token = cfg.token.clone();
        let ca = ca.clone();
        let cs = cs.clone();
        let dc = dc.clone();
        let dr = dr.clone();
        let br = br.clone();
        let ds = ds.clone();
        let bs = bs.clone();
        // PRNG seed must never be zero (xorshift stays at 0 forever).
        let seed = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).max(1);
        handles.push(tokio::spawn(async move {
            ca.fetch_add(1, Ordering::Relaxed);
            let conn = match ep.connect(addr, "localhost") {
                Ok(c) => match c.await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(i, error = %e, "connect failed");
                        return;
                    }
                },
                Err(e) => {
                    warn!(i, error = %e, "connect prep failed");
                    return;
                }
            };
            match authenticate(&conn, &token).await {
                Ok(resp) if resp.accepted => {}
                Ok(resp) => {
                    warn!(i, reason = %resp.reason, "auth rejected");
                    return;
                }
                Err(e) => {
                    warn!(i, error = %e, "auth handshake failed");
                    return;
                }
            }
            cs.fetch_add(1, Ordering::Relaxed);
            run_bot(conn, stop, seed, tick, dr, br, ds, bs, dc).await;
        }));
    }

    tokio::time::sleep_until(deadline.into()).await;
    let _ = stop_tx.send(true);
    for h in handles {
        let _ = tokio::time::timeout(Duration::from_secs(5), h).await;
    }

    Summary {
        connects_attempted: ca.load(Ordering::Relaxed),
        connects_succeeded: cs.load(Ordering::Relaxed),
        disconnects_midrun: dc.load(Ordering::Relaxed),
        datagrams_received: dr.load(Ordering::Relaxed),
        bytes_received: br.load(Ordering::Relaxed),
        datagrams_sent: ds.load(Ordering::Relaxed),
        bytes_sent: bs.load(Ordering::Relaxed),
    }
}

async fn authenticate(
    conn: &quinn::Connection,
    token: &str,
) -> Result<AuthResponse, Box<dyn std::error::Error + Send + Sync>> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let req = AuthRequest {
        token: token.to_string(),
        client_version: "0.1.0".into(),
        session_token: None,
        transfer_token: None,
    };
    let body = bitcode::encode(&req);
    send.write_all(&(body.len() as u32).to_be_bytes()).await?;
    send.write_all(&body).await?;

    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    let resp: AuthResponse = bitcode::decode(&buf)?;
    Ok(resp)
}

#[allow(clippy::too_many_arguments)]
async fn run_bot(
    conn: quinn::Connection,
    stop: watch::Receiver<bool>,
    mut seed: u64,
    tick: Duration,
    dgrams_recv: Arc<AtomicU64>,
    bytes_recv: Arc<AtomicU64>,
    dgrams_sent: Arc<AtomicU64>,
    bytes_sent: Arc<AtomicU64>,
    disconnects: Arc<AtomicU64>,
) {
    // Inbound reader task — exits on connection close or shutdown.
    {
        let conn = conn.clone();
        let dr = dgrams_recv.clone();
        let br = bytes_recv.clone();
        let mut stop = stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    res = conn.read_datagram() => match res {
                        Ok(bytes) => {
                            if parse_delta_datagram(&bytes).is_ok() {
                                dr.fetch_add(1, Ordering::Relaxed);
                                br.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                            }
                        }
                        Err(_) => break,
                    },
                    _ = stop.changed() => break,
                }
            }
        });
    }

    let mut input_seq: u32 = 0;
    let mut interval = tokio::time::interval(tick);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut stop = stop;
    loop {
        tokio::select! {
            _ = interval.tick() => {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let dir_x = if seed & 1 != 0 { 1.0 } else { -1.0 };
                let dir_z = if seed & 2 != 0 { 1.0 } else { -1.0 };
                input_seq = input_seq.wrapping_add(1);
                let payload = encode_datagram(&ParticleInputPayload {
                    entity_slot: 0,
                    input_seq,
                    dir_x,
                    dir_z,
                    actions: 0,
                    dt_ms: 50,
                });
                match conn.send_datagram(Bytes::copy_from_slice(&payload)) {
                    Ok(()) => {
                        dgrams_sent.fetch_add(1, Ordering::Relaxed);
                        bytes_sent.fetch_add(payload.len() as u64, Ordering::Relaxed);
                    }
                    Err(_) => {
                        disconnects.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                }
            }
            _ = stop.changed() => break,
        }
    }
    conn.close(0u32.into(), b"load done");
}
