use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use governor::{Quota, RateLimiter};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::auth::AuthValidator;
use crate::config::EndpointConfig;
use crate::connection::handle_connection;
use crate::error::EndpointError;
use crate::reconnect::ConnectedClient;
use crate::session_store::SessionStore;
use crate::tls::{build_server_config, TlsConfig};

pub struct QuicEndpoint {
    endpoint: quinn::Endpoint,
    config: EndpointConfig,
    // Per-IP rate limiter: each source IP gets its own bucket so one
    // noisy client can't exhaust the handshake budget for the rest
    // (review finding M-5). Keyed on `IpAddr` — callers derive the key
    // from `quinn::Incoming::remote_address()`.
    rate_limiter: governor::DefaultKeyedRateLimiter<IpAddr>,
    cert_sha256: [u8; 32],
}

impl QuicEndpoint {
    pub fn bind(
        addr: SocketAddr,
        config: EndpointConfig,
        tls: &TlsConfig,
    ) -> Result<Self, EndpointError> {
        let transport = config.build_transport_config();
        let (server_config, cert_sha256) = build_server_config(tls, transport)?;
        let endpoint = quinn::Endpoint::server(server_config, addr).map_err(EndpointError::Bind)?;

        let quota = Quota::per_second(
            NonZeroU32::new(config.rate_limit_per_sec).expect("rate_limit_per_sec must be > 0"),
        );
        let rate_limiter = RateLimiter::keyed(quota);

        info!(addr = %endpoint.local_addr().unwrap_or(addr), "QUIC endpoint bound");
        if !cfg!(target_os = "linux") {
            info!("GSO not available on this platform");
        }

        Ok(Self {
            endpoint,
            config,
            rate_limiter,
            cert_sha256,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, EndpointError> {
        self.endpoint.local_addr().map_err(EndpointError::Bind)
    }

    /// SHA-256 digest of the leaf certificate's DER encoding, suitable
    /// for the browser's `WebTransport.serverCertificateHashes` option.
    pub fn cert_sha256(&self) -> [u8; 32] {
        self.cert_sha256
    }

    /// Consume one cell from the rate limiter bucket for `ip`, returning
    /// `true` when the request is allowed and `false` when that IP's
    /// bucket is exhausted. Exposed so integration tests can assert
    /// per-IP keying semantics without driving a full QUIC handshake
    /// (M-5 regression guard).
    pub fn check_ip(&self, ip: IpAddr) -> bool {
        self.rate_limiter.check_key(&ip).is_ok()
    }

    pub async fn run(
        self,
        validator: Arc<dyn AuthValidator>,
        session_tx: mpsc::Sender<ConnectedClient>,
        session_store: Arc<Mutex<SessionStore>>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut purge_interval = tokio::time::interval(std::time::Duration::from_secs(1));
        purge_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else {
                        info!("endpoint closed");
                        break;
                    };

                    let remote_ip = incoming.remote_address().ip();
                    if self.rate_limiter.check_key(&remote_ip).is_err() {
                        warn!(
                            remote = %incoming.remote_address(),
                            "rate limited, refusing connection"
                        );
                        incoming.refuse();
                        continue;
                    }

                    let validator = validator.clone();
                    let config = self.config.clone();
                    let tx = session_tx.clone();
                    let store = session_store.clone();
                    tokio::spawn(async move {
                        match handle_connection(incoming, &*validator, &config, store).await {
                            Ok(client) => { let _ = tx.send(client).await; }
                            Err(e) => warn!(error = %e, "connection failed"),
                        }
                    });
                }
                _ = purge_interval.tick() => {
                    let mut store = match session_store.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    store.purge_expired();
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("shutdown signal received");
                        self.endpoint.close(0u32.into(), b"shutdown");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_with_self_signed_succeeds() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let config = EndpointConfig::default();
        let endpoint = QuicEndpoint::bind(addr, config, &TlsConfig::SelfSigned);
        assert!(endpoint.is_ok());
    }

    #[tokio::test]
    async fn local_addr_returns_bound_port() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let config = EndpointConfig::default();
        let endpoint = QuicEndpoint::bind(addr, config, &TlsConfig::SelfSigned).unwrap();
        let local = endpoint.local_addr().unwrap();
        assert_ne!(local.port(), 0);
    }
}
