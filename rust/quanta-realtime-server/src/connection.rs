use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{info, warn};

use crate::auth::{run_auth_handshake, AuthValidator};
use crate::config::EndpointConfig;
use crate::error::EndpointError;
use crate::session::QuicSession;

/// Monotonic session counter for 0-RTT replay protection.
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Handle an incoming QUIC connection.
///
/// 1. Accepts the connection
/// 2. Reads ALPN from handshake data
/// 3. Runs auth handshake on the first bidi stream
/// 4. Creates a QuicSession on success
pub async fn handle_connection(
    incoming: quinn::Incoming,
    validator: &dyn AuthValidator,
    config: &EndpointConfig,
) -> Result<QuicSession, EndpointError> {
    let connection = incoming.await.map_err(EndpointError::Quinn)?;

    let alpn = connection
        .handshake_data()
        .and_then(|hd| {
            hd.downcast::<quinn::crypto::rustls::HandshakeData>()
                .ok()
        })
        .and_then(|hd| hd.protocol)
        .map(|p| p.to_vec());

    let alpn_str = alpn
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_else(|| "none".into());

    let session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    info!(
        remote = %connection.remote_address(),
        alpn = %alpn_str,
        session_id,
        "connection accepted"
    );

    // Dispatch by ALPN
    match alpn.as_deref() {
        Some(b"quanta-v1") => {
            handle_quanta_v1(connection, validator, config).await
        }
        Some(b"h3") => {
            // WebTransport upgrade — stub for T49
            warn!(remote = %connection.remote_address(), "h3 ALPN not yet implemented");
            connection.close(1u32.into(), b"h3 not implemented");
            Err(EndpointError::Auth("h3 ALPN not yet implemented".into()))
        }
        _ => {
            warn!(remote = %connection.remote_address(), alpn = %alpn_str, "unknown ALPN");
            connection.close(1u32.into(), b"unknown ALPN");
            Err(EndpointError::Auth(format!("unknown ALPN: {alpn_str}")))
        }
    }
}

async fn handle_quanta_v1(
    connection: quinn::Connection,
    validator: &dyn AuthValidator,
    config: &EndpointConfig,
) -> Result<QuicSession, EndpointError> {
    // Auth on first bidi stream, with the full auth timeout covering accept_bi + handshake
    let result = tokio::time::timeout(config.auth_timeout, async {
        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(EndpointError::Quinn)?;
        run_auth_handshake(&mut send, &mut recv, validator, config.auth_timeout).await
    })
    .await;

    let response = match result {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            connection.close(2u32.into(), b"auth failed");
            return Err(e);
        }
        Err(_elapsed) => {
            connection.close(2u32.into(), b"auth timeout");
            return Err(EndpointError::Auth("auth timeout".into()));
        }
    };

    if !response.accepted {
        connection.close(2u32.into(), b"auth rejected");
        return Err(EndpointError::Auth(format!(
            "rejected: {}",
            response.reason
        )));
    }

    info!(
        session_id = response.session_id,
        remote = %connection.remote_address(),
        "auth succeeded"
    );

    Ok(QuicSession::new(connection))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_counter_increments() {
        let a = SESSION_COUNTER.load(Ordering::Relaxed);
        let b = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        assert_eq!(a, b);
        let c = SESSION_COUNTER.load(Ordering::Relaxed);
        assert_eq!(c, b + 1);
    }
}
