//! Prometheus metrics + `/metrics` HTTP endpoint.
//!
//! The [`METRICS`] lazy static owns a single [`Registry`] holding all
//! server metric families. Call sites access the typed metric handles
//! (`tick_duration`, `clients_connected`, `datagrams_sent`, `bytes_sent`)
//! directly; the registry is only exposed for the text-format encoder.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use once_cell::sync::Lazy;
use prometheus::{Encoder, Histogram, HistogramOpts, IntCounter, IntGauge, Registry, TextEncoder};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{info, warn};

pub static METRICS: Lazy<ServerMetrics> = Lazy::new(ServerMetrics::new);

pub struct ServerMetrics {
    pub registry: Registry,
    pub tick_duration: Histogram,
    pub clients_connected: IntGauge,
    pub datagrams_sent: IntCounter,
    pub bytes_sent: IntCounter,
}

impl ServerMetrics {
    /// Construct a fresh, independent metrics set. Intended for tests
    /// that must not race with the global [`METRICS`] singleton. Production
    /// code should always use `METRICS.*`.
    pub fn new() -> Self {
        let registry = Registry::new();

        // Buckets tuned for a 20 Hz (50 ms) tick loop — need resolution
        // across 0.5–10 ms.
        let tick_duration = Histogram::with_opts(
            HistogramOpts::new(
                "tick_duration_seconds",
                "Wall-clock duration of one island tick",
            )
            .buckets(vec![0.0005, 0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.1]),
        )
        .expect("valid histogram opts");

        let clients_connected = IntGauge::new(
            "clients_connected",
            "Clients currently registered with the manager",
        )
        .expect("valid gauge opts");

        let datagrams_sent = IntCounter::new(
            "datagrams_sent_total",
            "Unreliable datagrams sent by fanout/pacing",
        )
        .expect("valid counter opts");

        let bytes_sent = IntCounter::new("bytes_sent_total", "Bytes sent via unreliable datagrams")
            .expect("valid counter opts");

        registry
            .register(Box::new(tick_duration.clone()))
            .expect("register tick_duration");
        registry
            .register(Box::new(clients_connected.clone()))
            .expect("register clients_connected");
        registry
            .register(Box::new(datagrams_sent.clone()))
            .expect("register datagrams_sent");
        registry
            .register(Box::new(bytes_sent.clone()))
            .expect("register bytes_sent");

        Self {
            registry,
            tick_duration,
            clients_connected,
            datagrams_sent,
            bytes_sent,
        }
    }

    /// Encode the registered metric families in the Prometheus text
    /// exposition format. Allocates on every call — fine at scrape
    /// intervals (seconds) but don't call per-tick.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        encoder.encode(&families, &mut buf).expect("encode metrics");
        buf
    }
}

/// Max concurrent /metrics HTTP connections. Prevents a misbehaving
/// scraper from spawning unbounded connection tasks.
const MAX_METRICS_CONNECTIONS: usize = 8;

/// Bind the `/metrics` HTTP endpoint on `addr` and serve until
/// `shutdown_rx` fires. Bind failure warns and returns — the rest of the
/// server keeps running.
pub async fn metrics_serve(addr: SocketAddr, mut shutdown_rx: watch::Receiver<bool>) {
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            warn!(%addr, error = %e, "metrics bind failed; metrics disabled");
            return;
        }
    };
    info!(%addr, "metrics endpoint bound");

    let active = Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(error = %e, "metrics accept failed");
                        continue;
                    }
                };
                if active.load(Ordering::Relaxed) >= MAX_METRICS_CONNECTIONS {
                    warn!(%peer, "metrics at connection cap; rejecting");
                    drop(stream); // RST to peer
                    continue;
                }
                let active_task = active.clone();
                active_task.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let _ = http1::Builder::new()
                        .serve_connection(io, service_fn(handle))
                        .await;
                    active_task.fetch_sub(1, Ordering::Relaxed);
                });
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("metrics endpoint shutting down");
                    break;
                }
            }
        }
    }
}

async fn handle(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.uri().path() != "/metrics" {
        let mut resp = Response::new(Full::new(Bytes::from_static(b"not found\n")));
        *resp.status_mut() = StatusCode::NOT_FOUND;
        return Ok(resp);
    }
    let body = METRICS.encode();
    let resp = Response::builder()
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(Full::new(Bytes::from(body)))
        .expect("valid response");
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests operate on a fresh `ServerMetrics` instance so parallel
    // cargo-test runners don't race on the global `METRICS` singleton.
    #[test]
    fn encode_contains_tick_duration_buckets() {
        let m = ServerMetrics::new();
        m.tick_duration.observe(0.0002);
        m.tick_duration.observe(0.003);
        let body = String::from_utf8(m.encode()).expect("utf8 metrics");
        assert!(body.contains("tick_duration_seconds_bucket"));
        assert!(body.contains("clients_connected"));
        assert!(body.contains("datagrams_sent_total"));
        assert!(body.contains("bytes_sent_total"));
    }

    #[test]
    fn client_gauge_inc_dec() {
        let m = ServerMetrics::new();
        m.clients_connected.inc();
        m.clients_connected.inc();
        m.clients_connected.dec();
        assert_eq!(m.clients_connected.get(), 1);
    }
}
