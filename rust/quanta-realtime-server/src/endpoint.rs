use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use governor::{Quota, RateLimiter};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::auth::AuthValidator;
use crate::config::EndpointConfig;
use crate::connection::handle_connection;
use crate::error::EndpointError;
use crate::session::Session;
use crate::tls::{build_server_config, TlsConfig};

pub struct QuicEndpoint {
    endpoint: quinn::Endpoint,
    config: EndpointConfig,
    rate_limiter: governor::DefaultDirectRateLimiter,
}

impl QuicEndpoint {
    pub fn bind(
        addr: SocketAddr,
        config: EndpointConfig,
        tls: &TlsConfig,
    ) -> Result<Self, EndpointError> {
        let transport = config.build_transport_config();
        let server_config = build_server_config(tls, transport)?;
        let endpoint = quinn::Endpoint::server(server_config, addr)
            .map_err(EndpointError::Bind)?;

        let quota = Quota::per_second(
            NonZeroU32::new(config.rate_limit_per_sec)
                .expect("rate_limit_per_sec must be > 0"),
        );
        let rate_limiter = RateLimiter::direct(quota);

        info!(addr = %endpoint.local_addr().unwrap_or(addr), "QUIC endpoint bound");

        Ok(Self {
            endpoint,
            config,
            rate_limiter,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, EndpointError> {
        self.endpoint
            .local_addr()
            .map_err(EndpointError::Bind)
    }

    pub async fn run(
        self,
        validator: Arc<dyn AuthValidator>,
        session_tx: mpsc::Sender<Box<dyn Session>>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else {
                        info!("endpoint closed");
                        break;
                    };

                    if self.rate_limiter.check().is_err() {
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
                    tokio::spawn(async move {
                        match handle_connection(incoming, &*validator, &config).await {
                            Ok(session) => { let _ = tx.send(session).await; }
                            Err(e) => warn!(error = %e, "connection failed"),
                        }
                    });
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
