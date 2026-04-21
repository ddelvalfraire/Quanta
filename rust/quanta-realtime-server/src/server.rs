//! Compose the full realtime server: QUIC + optional WebSocket + `IslandManager`
//! + optional NATS capacity publisher.
//!
//! [`run_server`] is the library entry point shared by `main.rs` and integration
//! tests. It binds all listeners synchronously (returning any bind errors) and
//! then spawns background tasks that run until `shutdown_rx` signals `true`.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::auth::AuthValidator;
use crate::capacity::run_capacity_publisher;
use crate::command::ManagerCommand;
use crate::config::{EndpointConfig, ServerConfig};
use crate::endpoint::QuicEndpoint;
use crate::error::EndpointError;
use crate::manager::{manager_channel, IslandManager};
use crate::reconnect::ConnectedClient;
use crate::session_store::SessionStore;
use crate::stubs::StubBridge;
use crate::tls::TlsConfig;
use crate::ws_listener::WsListener;

pub struct RunServerArgs {
    pub server_config: ServerConfig,
    pub endpoint_config: EndpointConfig,
    pub quic_addr: SocketAddr,
    pub ws_addr: Option<SocketAddr>,
    pub tls: TlsConfig,
    pub validator: Arc<dyn AuthValidator>,
    pub shutdown_rx: watch::Receiver<bool>,
    pub server_id: String,
}

pub struct RunningServer {
    pub quic_addr: SocketAddr,
    pub ws_addr: Option<SocketAddr>,
    pub manager_tx: mpsc::Sender<ManagerCommand>,
    pub tasks: Vec<JoinHandle<()>>,
}

/// Compose QUIC + optional WS + manager + optional NATS.
///
/// Returns once all listeners are bound. The server runs until `shutdown_rx`
/// signals `true` — all spawned tasks observe shutdown cooperatively.
pub async fn run_server(args: RunServerArgs) -> Result<RunningServer, EndpointError> {
    let RunServerArgs {
        server_config,
        endpoint_config,
        quic_addr,
        ws_addr,
        tls,
        validator,
        shutdown_rx,
        server_id,
    } = args;

    let session_store = Arc::new(Mutex::new(SessionStore::new(
        endpoint_config.session_retain_duration,
        endpoint_config.max_retained_sessions,
    )));

    // 1. Bind QUIC synchronously so bind failures surface as Err.
    let quic_endpoint = QuicEndpoint::bind(quic_addr, endpoint_config.clone(), &tls)?;
    let bound_quic = quic_endpoint.local_addr()?;
    info!(addr = %bound_quic, "QUIC endpoint bound");

    // 2. Optionally bind WS.
    let ws_bound = match ws_addr {
        Some(addr) => {
            let listener = WsListener::bind(addr, endpoint_config.clone()).await?;
            let bound = listener.local_addr()?;
            info!(addr = %bound, "WebSocket listener bound");
            Some((listener, bound))
        }
        None => None,
    };
    let bound_ws = ws_bound.as_ref().map(|(_, a)| *a);

    // 3. Shared session channel — both listeners push ConnectedClient here.
    let (session_tx, mut session_rx) = mpsc::channel::<ConnectedClient>(256);

    // 4. Spawn QUIC accept loop.
    let quic_handle = {
        let validator = validator.clone();
        let store = session_store.clone();
        let shutdown = shutdown_rx.clone();
        let tx = session_tx.clone();
        tokio::spawn(async move {
            quic_endpoint.run(validator, tx, store, shutdown).await;
        })
    };

    // 5. Spawn optional WS accept loop using the same session channel.
    let ws_handle = ws_bound.map(|(listener, _addr)| {
        let validator = validator.clone();
        let shutdown = shutdown_rx.clone();
        let tx = session_tx.clone();
        tokio::spawn(async move {
            listener.run(validator, tx, shutdown).await;
        })
    });

    // 6. Spawn IslandManager.
    let (manager_tx, manager_rx) = manager_channel(256);
    let manager_config = server_config.clone();
    let bridge = Arc::new(StubBridge);
    let manager_handle = tokio::spawn(async move {
        let mut manager = IslandManager::new(manager_config, manager_rx, bridge);
        manager.run().await;
    });

    // 7. Drain session channel → forward each ConnectedClient to the manager.
    //    On successful ack, spawn a monitor task that sends `ClientDisconnected`
    //    when the underlying QUIC connection closes, so the placeholder vec in
    //    IslandManager doesn't leak entries for dead clients.
    let drain_handle = {
        let drain_tx = manager_tx.clone();
        let mut drain_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    maybe = session_rx.recv() => {
                        match maybe {
                            Some(client) => {
                                let session_id = client.session_id;
                                let quic_conn = client.quic_connection.clone();
                                let (reply_tx, reply_rx) = oneshot::channel();
                                if drain_tx
                                    .send(ManagerCommand::ClientConnected {
                                        client,
                                        reply: reply_tx,
                                    })
                                    .await
                                    .is_err()
                                {
                                    warn!("manager channel closed, session drain exiting");
                                    break;
                                }
                                match reply_rx.await {
                                    Ok(Ok(_)) => {
                                        if let Some(conn) = quic_conn {
                                            let notify_tx = drain_tx.clone();
                                            tokio::spawn(async move {
                                                let _ = conn.closed().await;
                                                let _ = notify_tx
                                                    .send(ManagerCommand::ClientDisconnected {
                                                        session_id,
                                                    })
                                                    .await;
                                            });
                                        }
                                    }
                                    Ok(Err(e)) => warn!(error = %e, "manager rejected client"),
                                    Err(_) => warn!("manager reply dropped"),
                                }
                            }
                            None => {
                                info!("session channel closed, session drain exiting");
                                break;
                            }
                        }
                    }
                    _ = drain_shutdown.changed() => {
                        if *drain_shutdown.borrow() {
                            info!("session drain shutdown");
                            break;
                        }
                    }
                }
            }
        })
    };

    // 8. Optional NATS capacity publisher — non-fatal if connect fails.
    let capacity_handle = if let Some(url) = server_config.nats_url.as_ref() {
        match async_nats::connect(url).await {
            Ok(nats_client) => {
                info!(url = %url, "connected to NATS");
                let subject = format!("{}.{}", server_config.capacity_subject, server_id);
                let interval = Duration::from_secs(server_config.capacity_interval_secs);
                let max_islands = server_config.max_islands;
                let server_id_clone = server_id.clone();
                let tx = manager_tx.clone();
                Some(tokio::spawn(async move {
                    run_capacity_publisher(
                        tx,
                        nats_client,
                        subject,
                        server_id_clone,
                        max_islands,
                        interval,
                    )
                    .await;
                }))
            }
            Err(e) => {
                warn!(
                    error = %e,
                    url = %url,
                    "NATS connect failed; running without capacity publisher"
                );
                None
            }
        }
    } else {
        info!("NATS disabled (no url configured)");
        None
    };

    let mut tasks = vec![quic_handle, manager_handle, drain_handle];
    if let Some(h) = ws_handle {
        tasks.push(h);
    }
    if let Some(h) = capacity_handle {
        tasks.push(h);
    }

    Ok(RunningServer {
        quic_addr: bound_quic,
        ws_addr: bound_ws,
        manager_tx,
        tasks,
    })
}
