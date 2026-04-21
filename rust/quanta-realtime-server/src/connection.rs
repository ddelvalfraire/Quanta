use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tracing::{info, warn};

use crate::auth::{run_auth_handshake, AuthValidator};
use crate::config::EndpointConfig;
use crate::error::EndpointError;
use crate::reconnect::{ConnectedClient, ReconnectTier};
use crate::session::QuicSession;
use crate::session_store::SessionStore;
use crate::webtransport_session::WebTransportSession;

const CLOSE_AUTH_FAILURE: quinn::VarInt = quinn::VarInt::from_u32(2);

static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(1);

pub async fn handle_connection(
    incoming: quinn::Incoming,
    validator: &dyn AuthValidator,
    config: &EndpointConfig,
    session_store: Arc<Mutex<SessionStore>>,
) -> Result<ConnectedClient, EndpointError> {
    let connection = incoming.await.map_err(EndpointError::Quinn)?;

    let alpn = connection
        .handshake_data()
        .and_then(|hd| hd.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|hd| hd.protocol)
        .map(|p| p.to_vec());

    let alpn_str = alpn
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_else(|| "none".into());

    let conn_id = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
    info!(
        remote = %connection.remote_address(),
        alpn = %alpn_str,
        conn_id,
        "connection accepted"
    );

    match alpn.as_deref() {
        Some(b"quanta-v1") => handle_quanta_v1(connection, validator, config, session_store).await,
        Some(b"h3") => handle_h3_webtransport(connection, validator, config, session_store).await,
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
    session_store: Arc<Mutex<SessionStore>>,
) -> Result<ConnectedClient, EndpointError> {
    let result = tokio::time::timeout(config.auth_timeout, async {
        let (mut send, mut recv) = connection.accept_bi().await.map_err(EndpointError::Quinn)?;
        run_auth_handshake(&mut send, &mut recv, validator).await
    })
    .await;

    let (response, session_token) = match result {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => {
            connection.close(CLOSE_AUTH_FAILURE, b"auth failed");
            return Err(e);
        }
        Err(_elapsed) => {
            connection.close(CLOSE_AUTH_FAILURE, b"auth timeout");
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

    let reconnect_tier = classify_reconnect(&session_store, response.session_id, session_token);

    Ok(ConnectedClient {
        quic_connection: Some(connection.clone()),
        session: Arc::new(QuicSession::new(connection)),
        session_id: response.session_id,
        reconnect_tier,
    })
}

async fn handle_h3_webtransport(
    connection: quinn::Connection,
    validator: &dyn AuthValidator,
    config: &EndpointConfig,
    session_store: Arc<Mutex<SessionStore>>,
) -> Result<ConnectedClient, EndpointError> {
    let result = tokio::time::timeout(config.auth_timeout, async {
        let request = web_transport_quinn::Request::accept(connection.clone())
            .await
            .map_err(|e| EndpointError::WebTransport(e.to_string()))?;

        let session = request
            .ok()
            .await
            .map_err(|e| EndpointError::WebTransport(e.to_string()))?;

        let (mut send, mut recv) = session
            .accept_bi()
            .await
            .map_err(|e| EndpointError::WebTransport(e.to_string()))?;

        let (response, session_token) = run_auth_handshake(&mut send, &mut recv, validator).await?;
        Ok::<_, EndpointError>((session, response, session_token))
    })
    .await;

    let (session, response, session_token) = match result {
        Ok(Ok(tuple)) => tuple,
        Ok(Err(e)) => {
            connection.close(CLOSE_AUTH_FAILURE, b"auth failed");
            return Err(e);
        }
        Err(_elapsed) => {
            connection.close(CLOSE_AUTH_FAILURE, b"auth timeout");
            return Err(EndpointError::Auth("auth timeout".into()));
        }
    };

    if !response.accepted {
        session.close(CLOSE_AUTH_FAILURE.into_inner() as u32, b"auth rejected");
        return Err(EndpointError::Auth(format!(
            "rejected: {}",
            response.reason
        )));
    }

    info!(
        session_id = response.session_id,
        remote = %connection.remote_address(),
        "webtransport auth succeeded"
    );

    let reconnect_tier = classify_reconnect(&session_store, response.session_id, session_token);

    Ok(ConnectedClient {
        quic_connection: Some(connection),
        session: Arc::new(WebTransportSession::new(session)),
        session_id: response.session_id,
        reconnect_tier,
    })
}

/// Classify the reconnection tier by checking the session store for a retained
/// session and verifying the client's session_token matches.
fn classify_reconnect(
    store: &Mutex<SessionStore>,
    session_id: u64,
    client_token: Option<u64>,
) -> ReconnectTier {
    let mut store = match store.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("session store mutex poisoned, recovering");
            poisoned.into_inner()
        }
    };

    match store.take(session_id) {
        Some(retained) => {
            // Verify the client-provided session_token matches what we stored.
            if client_token == Some(retained.session_token) {
                info!(session_id, "fast reconnect (tier 2)");
                ReconnectTier::Fast { retained }
            } else {
                warn!(session_id, "session token mismatch, falling back to cold");
                ReconnectTier::Cold
            }
        }
        None => ReconnectTier::Cold,
    }
}
