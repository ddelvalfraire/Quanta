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

    let conn_id = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
    info!(
        remote = %connection.remote_address(),
        alpn = %alpn_str,
        conn_id,
        "connection accepted"
    );

    match alpn.as_deref() {
        Some(b"quanta-v1") => {
            handle_quanta_v1(connection, validator, config, session_store).await
        }
        Some(b"h3") => {
            handle_h3_webtransport(connection, validator, config, session_store).await
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
    session_store: Arc<Mutex<SessionStore>>,
) -> Result<ConnectedClient, EndpointError> {
    let result = tokio::time::timeout(config.auth_timeout, async {
        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(EndpointError::Quinn)?;
        run_auth_handshake(&mut send, &mut recv, validator).await
    })
    .await;

    let response = match result {
        Ok(Ok(resp)) => resp,
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

    let reconnect_tier = classify_reconnect(&session_store, &response);

    Ok(ConnectedClient {
        quic_connection: Some(connection.clone()),
        session: Box::new(QuicSession::new(connection)),
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

        let response = run_auth_handshake(&mut send, &mut recv, validator).await?;
        Ok::<_, EndpointError>((session, response))
    })
    .await;

    let (session, response) = match result {
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

    let reconnect_tier = classify_reconnect(&session_store, &response);

    Ok(ConnectedClient {
        quic_connection: Some(connection),
        session: Box::new(WebTransportSession::new(session)),
        session_id: response.session_id,
        reconnect_tier,
    })
}

/// Classify the reconnection tier based on whether the auth request carried
/// a session token and whether we still have a retained session for it.
fn classify_reconnect(
    store: &Mutex<SessionStore>,
    response: &crate::auth::AuthResponse,
) -> ReconnectTier {
    // The session_token from the auth request is not available in the
    // AuthResponse. The validator can use the token to map to a session_id.
    // For reconnection we check if we have a retained session for the
    // assigned session_id.
    let mut store = store.lock().expect("session store lock poisoned");
    match store.take(response.session_id) {
        Some(retained) => {
            info!(session_id = response.session_id, "fast reconnect (tier 2)");
            ReconnectTier::Fast { retained }
        }
        None => ReconnectTier::Cold,
    }
}
