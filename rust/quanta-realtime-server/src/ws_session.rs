use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::{mpsc, Notify};

use crate::error::SendError;
use crate::session::{ClosedFuture, Session, TransportStats, TransportType};

pub const FLAG_RELIABLE: u8 = 0x00;
pub const FLAG_UNRELIABLE: u8 = 0x01;

/// Static RTT estimate for WebSocket connections.
const WS_RTT_ESTIMATE: Duration = Duration::from_millis(100);

/// Minimum valid frame size: 1-byte flags header + at least 1 byte of payload.
pub const MIN_FRAME_LEN: usize = 2;

pub fn encode_frame(flags: u8, data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(1 + data.len());
    frame.push(flags);
    frame.extend_from_slice(data);
    frame
}

pub fn decode_frame(data: &[u8]) -> Option<&[u8]> {
    if data.len() >= MIN_FRAME_LEN {
        Some(&data[1..])
    } else {
        None
    }
}

pub struct WsSession {
    outbound_tx: mpsc::Sender<Vec<u8>>,
    datagram_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Shared close-notify. `on_closed()` returns a future that waits on
    /// `notified()`; `trigger_close()` fires `notify_waiters()` and is
    /// idempotent (subsequent waiters are served by the `notified`-after-
    /// `notify_waiters` race only if we track a "closed" flag too).
    ///
    /// We pair the Notify with a `closed` flag so late subscribers (tasks
    /// that call `on_closed()` *after* the close already fired) still
    /// resolve immediately — `Notify::notify_waiters` only wakes waiters
    /// that are already parked at fire time.
    close_notify: Arc<Notify>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl WsSession {
    pub fn new(outbound_tx: mpsc::Sender<Vec<u8>>, datagram_rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            outbound_tx,
            datagram_rx: Mutex::new(datagram_rx),
            close_notify: Arc::new(Notify::new()),
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Fire the close-notify. Safe to call multiple times; only the first
    /// call flips the `closed` flag, subsequent calls are no-ops.
    ///
    /// Called from `ws_listener.rs` when either the transport read or
    /// write task exits — the peer has disconnected (or we're shutting
    /// down), so every holder of `on_closed()` should proceed to cleanup.
    ///
    /// Public so integration tests can simulate a WS teardown without
    /// running a full `ws_listener` + tungstenite stack.
    pub fn trigger_close(&self) {
        if !self
            .closed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            self.close_notify.notify_waiters();
        }
    }

    /// Returns a shareable handle that fires the same close-notify — used
    /// by `ws_listener` to give each spawned transport task a cheap way
    /// to signal close without holding the whole `Arc<WsSession>`.
    pub fn close_trigger(&self) -> CloseTrigger {
        CloseTrigger {
            notify: self.close_notify.clone(),
            closed: self.closed.clone(),
        }
    }
}

/// Lightweight handle that fires the session's close-notify.
///
/// Cloned into each ws_listener transport task so they can signal close
/// when they exit without capturing the full `Arc<WsSession>` (which
/// would keep the session alive beyond its transport).
#[derive(Clone)]
pub struct CloseTrigger {
    notify: Arc<Notify>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl CloseTrigger {
    pub fn fire(&self) {
        if !self
            .closed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            self.notify.notify_waiters();
        }
    }
}

impl Session for WsSession {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError> {
        self.outbound_tx
            .try_send(encode_frame(FLAG_UNRELIABLE, data))
            .map_err(|_| SendError::ConnectionLost("ws outbound channel full".into()))
    }

    fn send_reliable(&self, _stream_id: u32, data: &[u8]) -> Result<(), SendError> {
        self.outbound_tx
            .try_send(encode_frame(FLAG_RELIABLE, data))
            .map_err(|_| SendError::ConnectionLost("ws outbound channel full".into()))
    }

    fn recv_datagram(&self) -> Option<Vec<u8>> {
        self.datagram_rx.lock().ok()?.try_recv().ok()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }

    fn rtt(&self) -> Duration {
        WS_RTT_ESTIMATE
    }

    fn transport_stats(&self) -> TransportStats {
        TransportStats::default()
    }

    /// Sends a shutdown sentinel to the background write task, which closes the socket.
    fn close(&self, _reason: &str) {
        let _ = self.outbound_tx.try_send(Vec::new());
        // Also fire the close-notify so any reader/watcher task waiting on
        // `on_closed()` unblocks immediately — without this, a caller that
        // drives the close path has to wait for the write task to exit
        // and propagate back.
        self.trigger_close();
    }

    fn on_closed(&self) -> ClosedFuture {
        let notify = self.close_notify.clone();
        let closed = self.closed.clone();
        Box::pin(async move {
            // Fast path: already closed. Avoid parking on `notified()`
            // after a `notify_waiters()` has already fired (which only
            // wakes waiters parked at fire time).
            if closed.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            // Register a notified() permit BEFORE re-checking the flag,
            // otherwise we'd race with a concurrent trigger that fires
            // between the load and the await.
            let waiter = notify.notified();
            tokio::pin!(waiter);
            if closed.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            waiter.await;
        })
    }
}
