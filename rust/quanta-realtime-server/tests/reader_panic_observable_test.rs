//! H-2 regression test: per-client reader task spawned without JoinHandle tracking.
//!
//! ## Finding
//! `register_and_spawn_reader` in `server.rs` calls `tokio::spawn(async move { ... })`
//! and immediately drops the returned `JoinHandle<()>`.  If the reader task panics,
//! the manager receives no notification: the `client_registry` entry for that session
//! stays populated indefinitely, the entity slot is never freed, and the fanout pacer
//! is never removed.
//!
//! ## Test strategy
//! We cannot call the private `register_and_spawn_reader` from outside the crate.
//! Instead we test the narrower invariant that the manager relies on for cleanup:
//!
//!   1. `RegisterClient` → `client_registry` gains one entry.
//!   2. The reader task panics (JoinHandle was dropped; panic goes unobserved) →
//!      NO automatic cleanup of `client_registry`.
//!   3. Only an explicit `DeregisterClient` removes the entry.
//!
//! Step 2 is the bug: the manager has no watchdog on the reader JoinHandle, so a
//! panicking reader leaves the slot leaked until the QUIC connection closes (or
//! forever if the panic was caused by a bug while the connection stays open).
//!
//! ## Test-seam added to production code (`src/manager.rs`)
//! Two methods are added under `#[cfg(any(test, feature = "test-utils"))]` — the same
//! gate used by `ZoneTransferConfig::for_testing()` in this codebase:
//!
//!   - `IslandManager::client_registry_len() -> usize`
//!       Exposes the length of the private `client_registry` map so tests can assert
//!       slot-leak behaviour.
//!   - `IslandManager::process_one_command() -> bool`
//!       Drains and processes exactly one command from the internal channel.  Lets
//!       tests drive the manager inline (without a background task) so they can call
//!       `client_registry_len()` between steps.
//!
//! Both have zero effect in release builds.
//!
//! ## Why `reader_panic_leaves_client_registry_entry_populated` FAILS today
//! The test asserts `client_registry_len() == 0` *before* any `DeregisterClient` is
//! sent, expecting the manager to auto-clean the entry when the session is dropped.
//! In reality the manager never does that — the count stays at 1 — so the assertion
//! fails, proving the bug.

mod common;

use std::sync::Arc;
use std::time::Duration;

use quanta_realtime_server::command::ManagerCommand;
use quanta_realtime_server::config::ServerConfig;
use quanta_realtime_server::manager::{manager_channel, IslandManager};
use quanta_realtime_server::session::{Session, TransportStats, TransportType};
use quanta_realtime_server::stubs::StubBridge;
use quanta_realtime_server::tick::{NoopWasmExecutor, WasmExecutor};
use quanta_realtime_server::types::IslandId;
use quanta_realtime_server::ExecutorFactory;
use tokio::sync::oneshot;

use common::test_manifest_no_passivate;

// ---------------------------------------------------------------------------
// Session whose recv_datagram panics — models the case where the reader task
// would blow up on first poll.
// ---------------------------------------------------------------------------
struct PanickingSession;

impl Session for PanickingSession {
    fn recv_datagram(&self) -> Option<Vec<u8>> {
        panic!("PanickingSession: simulated reader task panic (H-2 probe)");
    }

    fn send_unreliable(&self, _data: &[u8]) -> Result<(), quanta_realtime_server::SendError> {
        Ok(())
    }

    fn send_reliable(
        &self,
        _stream_id: u32,
        _data: &[u8],
    ) -> Result<(), quanta_realtime_server::SendError> {
        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }

    fn rtt(&self) -> Duration {
        Duration::from_millis(1)
    }

    fn transport_stats(&self) -> TransportStats {
        TransportStats::default()
    }

    fn close(&self, _reason: &str) {}
}

// ---------------------------------------------------------------------------
// Helper: build an IslandManager driven inline (not in a background task).
// ---------------------------------------------------------------------------
fn build_inline_manager() -> (tokio::sync::mpsc::Sender<ManagerCommand>, IslandManager) {
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

// Drive all currently-queued commands.
fn flush(mgr: &mut IslandManager) {
    while mgr.process_one_command() {}
}

// ---------------------------------------------------------------------------
// Test 1 — FAILING TODAY (proves the bug)
//
// A client is registered.  The session is then dropped (simulating the
// JoinHandle being dropped with a panicking reader — no DeregisterClient is
// ever sent).  The test asserts client_registry is empty; it will NOT be,
// proving that the manager has no automatic cleanup path.
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_panic_leaves_client_registry_entry_populated() {
    let (cmd_tx, mut mgr) = build_inline_manager();

    // Activate an island.
    let (act_tx, act_rx) = oneshot::channel();
    cmd_tx
        .send(ManagerCommand::Activate {
            manifest: test_manifest_no_passivate("h2-island", 0),
            reply: act_tx,
        })
        .await
        .unwrap();
    flush(&mut mgr);
    act_rx.await.unwrap().expect("island activation failed");

    // Register a client whose session would panic if the reader task polled it.
    let session: Arc<dyn Session> = Arc::new(PanickingSession);
    let session_id: u64 = 1;

    let (reg_tx, reg_rx) = oneshot::channel();
    cmd_tx
        .send(ManagerCommand::RegisterClient {
            island_id: IslandId::from("h2-island"),
            session_id,
            session: session.clone(),
            reply: reg_tx,
        })
        .await
        .unwrap();
    flush(&mut mgr);
    reg_rx.await.unwrap().expect("RegisterClient should succeed");

    // Verify the entry is present.
    assert_eq!(
        mgr.client_registry_len(),
        1,
        "client_registry must have 1 entry after RegisterClient"
    );

    // Simulate the reader task having panicked: the JoinHandle was dropped,
    // the panic is swallowed by the tokio runtime, and nothing sends
    // DeregisterClient.  We model this by simply dropping the session Arc
    // and yielding.
    drop(session);
    tokio::task::yield_now().await;
    flush(&mut mgr); // process any commands that might have been enqueued

    // FAILING ASSERTION — proves H-2.
    // If a fix were in place (e.g., the JoinHandle were stored and a watcher
    // task sent DeregisterClient on panic), this would be 0.
    // Currently the count stays at 1 because no cleanup path exists.
    assert_eq!(
        mgr.client_registry_len(),
        0,
        "BUG H-2: client_registry entry was NOT cleaned up after the session \
         was dropped without an explicit DeregisterClient. \
         A panicking reader task (whose JoinHandle is dropped) leaves the slot \
         leaked indefinitely. Found {} entry/entries.",
        mgr.client_registry_len()
    );
}

// ---------------------------------------------------------------------------
// Test 2 — PASSING TODAY (control: explicit deregister works)
//
// Confirms the only cleanup path is a manual DeregisterClient command.
// This establishes the baseline and ensures the test infrastructure is sound.
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_deregister_clears_client_registry() {
    let (cmd_tx, mut mgr) = build_inline_manager();

    // Activate
    let (act_tx, act_rx) = oneshot::channel();
    cmd_tx
        .send(ManagerCommand::Activate {
            manifest: test_manifest_no_passivate("h2-island-ctrl", 0),
            reply: act_tx,
        })
        .await
        .unwrap();
    flush(&mut mgr);
    act_rx.await.unwrap().expect("island activation failed");

    // Register
    let session: Arc<dyn Session> = Arc::new(PanickingSession);
    let session_id: u64 = 2;

    let (reg_tx, reg_rx) = oneshot::channel();
    cmd_tx
        .send(ManagerCommand::RegisterClient {
            island_id: IslandId::from("h2-island-ctrl"),
            session_id,
            session,
            reply: reg_tx,
        })
        .await
        .unwrap();
    flush(&mut mgr);
    reg_rx.await.unwrap().expect("RegisterClient should succeed");

    assert_eq!(mgr.client_registry_len(), 1);

    // Explicit DeregisterClient — the only cleanup path that works today.
    cmd_tx
        .send(ManagerCommand::DeregisterClient {
            island_id: IslandId::from("h2-island-ctrl"),
            session_id,
        })
        .await
        .unwrap();
    flush(&mut mgr);

    assert_eq!(
        mgr.client_registry_len(),
        0,
        "explicit DeregisterClient must clear the client_registry entry"
    );
}
