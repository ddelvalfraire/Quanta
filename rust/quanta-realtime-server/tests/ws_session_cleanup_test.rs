//! H-1 regression test: WS sessions leak `client_registry` entries on
//! disconnect.
//!
//! ## Finding
//! `register_and_spawn_reader` in `server.rs` only spawns a close-watcher
//! that fires `DeregisterClient` when `client.quic_connection.is_some()`
//! (see server.rs ~321-357). WS sessions set `quic_connection: None`,
//! so nothing ever notifies the manager when a WebSocket peer closes.
//!
//! `sweep_dead_sessions` is the only other cleanup path, and it uses
//! `weak.upgrade().is_none()` — it only fires after every strong Arc is
//! dropped. In production the per-session reader task holds one strong
//! Arc for the lifetime of the loop; it never exits on WS close because
//! `WsSession::recv_datagram` cannot distinguish "empty queue" from
//! "peer disconnected". So the reader loops forever, the weak ref stays
//! upgradeable, and the `client_registry` entry leaks until the server
//! shuts down.
//!
//! ## Test strategy
//! We model the production wiring faithfully at the manager level:
//!
//!   1. Build an inline `IslandManager` (same pattern as
//!      `reader_panic_observable_test`).
//!   2. Activate an island.
//!   3. Register a client whose session is a real `WsSession` backed by
//!      tokio channels. Assert `client_registry_len() == 1`.
//!   4. Spawn a task that holds a strong `Arc<WsSession>` and loops on
//!      `recv_datagram()`, exactly like the production reader.
//!   5. Simulate a WS disconnect by dropping the peer sides of the
//!      outbound/inbound tokio channels — this is what `tokio_tungstenite`
//!      does when the TCP stream goes away.
//!   6. Assert `client_registry_len()` reaches 0.
//!
//! ## Why the test FAILS today
//! `WsSession` has no `on_closed()` close-notify API and ws_listener
//! never dispatches `DeregisterClient` on peer close. Compilation of
//! this test fails on the `on_closed()` call — that is the proof-of-bug.
//! After the H-1 fix (close-notify on WsSession + close-watcher in
//! ws_listener) the API exists and the test passes.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use quanta_realtime_server::command::ManagerCommand;
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::manager::{manager_channel, IslandManager};
use quanta_realtime_server::session::Session;
use quanta_realtime_server::stubs::StubBridge;
use quanta_realtime_server::tick::{NoopWasmExecutor, WasmExecutor};
use quanta_realtime_server::types::IslandId;
use quanta_realtime_server::ws_session::WsSession;
use quanta_realtime_server::ExecutorFactory;
use tokio::sync::{mpsc, oneshot};

use common::test_manifest_no_passivate;

// ---------------------------------------------------------------------------
// Helper: inline manager — same shape as reader_panic_observable_test.
// ---------------------------------------------------------------------------
fn build_inline_manager() -> (mpsc::Sender<ManagerCommand>, IslandManager) {
    let (tx, rx) = manager_channel(64);
    let bridge = Arc::new(StubBridge);
    let factory: ExecutorFactory =
        Arc::new(|| Box::new(NoopWasmExecutor) as Box<dyn WasmExecutor>);
    let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let mgr = IslandManager::new(
        ServerConfig::default(),
        rx,
        bridge,
        factory,
        None,
        shutdown_rx,
    );
    (tx, mgr)
}

fn flush(mgr: &mut IslandManager) {
    while mgr.process_one_command() {}
}

async fn poll_until<F: FnMut(&mut IslandManager) -> bool>(
    mut check: F,
    timeout: Duration,
    mgr: &mut IslandManager,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        flush(mgr);
        if check(mgr) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    flush(mgr);
    check(mgr)
}

// ---------------------------------------------------------------------------
// Test — FAILS today; PASSES after the fix.
//
// Models the full production wiring for a WS client:
//   - `WsSession` (real type) backed by real tokio channels.
//   - A reader task that owns a strong Arc and loops on `recv_datagram`,
//     mirroring `register_and_spawn_reader` in server.rs.
//   - A "close watcher" supplied via `WsSession::on_closed()` + a helper
//     that dispatches `DeregisterClient` — this is what the fix wires up
//     inside ws_listener/server.rs.
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_session_disconnect_reclaims_client_registry_slot() {
    let (cmd_tx, mut mgr) = build_inline_manager();

    // 1. Activate an island.
    let (act_tx, act_rx) = oneshot::channel();
    cmd_tx
        .send(ManagerCommand::Activate {
            manifest: test_manifest_no_passivate("h1-ws-island", 0),
            reply: act_tx,
        })
        .await
        .unwrap();
    flush(&mut mgr);
    act_rx.await.unwrap().expect("island activation");

    // 2. Build a WsSession backed by real tokio channels — exactly what
    //    `handle_ws_connection` produces.
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(16);
    let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(16);
    let ws = Arc::new(WsSession::new(outbound_tx, inbound_rx));
    let session: Arc<dyn Session> = ws.clone();

    // Stand-in for the tungstenite write task: just drains outbound.
    let write_task = tokio::spawn(async move {
        while outbound_rx.recv().await.is_some() {}
    });

    // 3. Register the client.
    let session_id: u64 = 42;
    let (reg_tx, reg_rx) = oneshot::channel();
    cmd_tx
        .send(ManagerCommand::RegisterClient {
            island_id: IslandId::from("h1-ws-island"),
            session_id,
            session: session.clone(),
            reply: reg_tx,
        })
        .await
        .unwrap();
    flush(&mut mgr);
    reg_rx.await.unwrap().expect("RegisterClient");

    assert_eq!(
        mgr.client_registry_len(),
        1,
        "client_registry must hold the WS entry after RegisterClient"
    );

    // 4. Spawn the production-shaped reader task.  Holds a strong Arc,
    //    loops on `recv_datagram`. The loop exits when the session's
    //    close-notify fires — this is the hook the fix adds to WsSession.
    let reader_session = ws.clone();
    let reader_task = tokio::spawn(async move {
        let closed = reader_session.on_closed();
        tokio::pin!(closed);
        loop {
            tokio::select! {
                _ = &mut closed => break,
                _ = tokio::time::sleep(Duration::from_millis(5)) => {
                    while reader_session.recv_datagram().is_some() {}
                }
            }
        }
        drop(reader_session);
    });

    // 5. Spawn the close-watcher that ws_listener must install after the fix.
    //    It waits for the session's close-notify and sends DeregisterClient.
    let watcher_session = ws.clone();
    let watcher_tx = cmd_tx.clone();
    let watcher_task = tokio::spawn(async move {
        watcher_session.on_closed().await;
        let _ = watcher_tx
            .send(ManagerCommand::DeregisterClient {
                island_id: IslandId::from("h1-ws-island"),
                session_id,
            })
            .await;
        drop(watcher_session);
    });

    // Drop the test's external Arcs — only the reader + watcher still hold one.
    drop(session);
    drop(ws);

    // 6. Simulate WS disconnect: tungstenite's write sink goes away (the
    //    TCP stream closed). The inbound tx dropping mirrors the read task
    //    exiting.  WsSession's close-notify must fire when either side
    //    tears down.
    drop(inbound_tx);
    write_task.abort();
    let _ = write_task.await;

    // 7. Reader and watcher observe the close, release their Arcs, and
    //    the watcher dispatches DeregisterClient.
    let _ = tokio::time::timeout(Duration::from_secs(2), reader_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), watcher_task).await;

    // 8. Registry entry must be reclaimed.
    let cleared = poll_until(
        |m| m.client_registry_len() == 0,
        Duration::from_secs(2),
        &mut mgr,
    )
    .await;

    assert!(
        cleared,
        "BUG H-1: WS session disconnect did not reclaim client_registry entry. \
         Found {} entry/entries after simulated WS close. \
         ws_listener must install a close-watcher that dispatches DeregisterClient \
         when the WebSocket transport tasks exit.",
        mgr.client_registry_len()
    );
}
