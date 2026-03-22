use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use governor::{Quota, RateLimiter};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::auth::{AuthRequest, AuthValidator};
use crate::config::EndpointConfig;
use crate::error::EndpointError;
use crate::session::Session;
use crate::ws_session::{decode_frame, WsSession};

pub struct WsListener {
    listener: TcpListener,
    config: EndpointConfig,
    rate_limiter: governor::DefaultDirectRateLimiter,
}

impl WsListener {
    pub async fn bind(addr: SocketAddr, config: EndpointConfig) -> Result<Self, EndpointError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(EndpointError::Bind)?;

        let quota = Quota::per_second(
            NonZeroU32::new(config.rate_limit_per_sec)
                .expect("rate_limit_per_sec must be > 0"),
        );
        let rate_limiter = RateLimiter::direct(quota);

        info!(addr = %listener.local_addr().unwrap_or(addr), "WebSocket listener bound");
        Ok(Self {
            listener,
            config,
            rate_limiter,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, EndpointError> {
        self.listener.local_addr().map_err(EndpointError::Bind)
    }

    pub async fn run(
        self,
        validator: Arc<dyn AuthValidator>,
        session_tx: mpsc::Sender<Box<dyn Session>>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                result = self.listener.accept() => {
                    let (stream, addr) = match result {
                        Ok(pair) => pair,
                        Err(e) => {
                            warn!(error = %e, "TCP accept failed");
                            continue;
                        }
                    };

                    if self.rate_limiter.check().is_err() {
                        warn!(remote = %addr, "ws rate limited, dropping connection");
                        drop(stream);
                        continue;
                    }

                    let validator = validator.clone();
                    let config = self.config.clone();
                    let tx = session_tx.clone();
                    tokio::spawn(async move {
                        match handle_ws_connection(stream, addr, &*validator, &config).await {
                            Ok(session) => { let _ = tx.send(session).await; }
                            Err(e) => warn!(remote = %addr, error = %e, "ws connection failed"),
                        }
                    });
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("ws listener shutdown");
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_ws_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    validator: &dyn AuthValidator,
    config: &EndpointConfig,
) -> Result<Box<dyn Session>, EndpointError> {
    let ws_stream = tokio::time::timeout(
        config.auth_timeout,
        tokio_tungstenite::accept_async(stream),
    )
    .await
    .map_err(|_| EndpointError::Auth("ws handshake timeout".into()))?
    .map_err(|e| EndpointError::WebSocket(e.to_string()))?;

    let (mut sink, mut stream) = ws_stream.split();

    // Auth: first binary message is the raw bitcode AuthRequest (no length prefix).
    let auth_result = tokio::time::timeout(config.auth_timeout, async {
        while let Some(msg) = stream.next().await {
            let msg = msg.map_err(|e| EndpointError::WebSocket(e.to_string()))?;
            match msg {
                Message::Binary(data) => {
                    let req: AuthRequest = bitcode::decode(&data)
                        .map_err(|e| EndpointError::Auth(format!("decode auth: {e}")))?;
                    let response = validator.validate(&req)?;
                    let resp_bytes = bitcode::encode(&response);
                    sink.send(Message::Binary(resp_bytes.into()))
                        .await
                        .map_err(|e| EndpointError::WebSocket(e.to_string()))?;
                    return Ok(response);
                }
                Message::Close(_) => {
                    return Err(EndpointError::Auth("client closed before auth".into()));
                }
                _ => continue, // skip text/ping/pong during auth
            }
        }
        Err(EndpointError::Auth("stream ended before auth".into()))
    })
    .await
    .map_err(|_| EndpointError::Auth("ws auth timeout".into()))?;

    let response = auth_result?;
    if !response.accepted {
        let _ = sink.send(Message::Close(None)).await;
        return Err(EndpointError::Auth(format!(
            "rejected: {}",
            response.reason
        )));
    }

    info!(
        session_id = response.session_id,
        remote = %addr,
        "ws auth succeeded"
    );

    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(256);
    let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(256);

    // Background write task: outbound channel -> WS sink.
    // An empty Vec is the shutdown sentinel from WsSession::close().
    tokio::spawn(async move {
        while let Some(data) = outbound_rx.recv().await {
            if data.is_empty() {
                break;
            }
            if sink.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });

    // Background read task: WS stream -> inbound channel.
    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Binary(data) => match decode_frame(&data) {
                    Some(payload) => {
                        if inbound_tx.send(payload.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        warn!("ws: dropping malformed frame ({} bytes)", data.len());
                    }
                },
                Message::Close(_) => break,
                _ => continue,
            }
        }
    });

    Ok(Box::new(WsSession::new(outbound_tx, inbound_rx)))
}
