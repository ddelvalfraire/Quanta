use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::types::IslandId;

/// Request sent from island thread to checkpoint writer.
pub struct CheckpointRequest {
    pub island_id: IslandId,
    pub tick: u64,
    pub data: Vec<u8>,
    /// If set, writer sends acknowledgement after persisting (used for pre-passivation).
    pub ack: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Latest completed checkpoint, readable via ArcSwap without blocking the tick loop.
#[derive(Debug, Clone)]
pub struct CheckpointSnapshot {
    pub tick: u64,
    pub data: Vec<u8>,
}

/// Abstraction over KV store operations for testability.
pub trait CheckpointStore: Send + 'static {
    fn put(
        &mut self,
        key: String,
        value: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    fn get(
        &mut self,
        key: String,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, String>> + Send + '_>>;
}

/// Handle held by the island thread. Provides non-blocking send and lock-free snapshot reads.
pub struct CheckpointHandle {
    pub tx: mpsc::Sender<CheckpointRequest>,
    pub latest: Arc<ArcSwap<Option<CheckpointSnapshot>>>,
}

impl CheckpointHandle {
    /// Try to enqueue a checkpoint request. If the channel is full, log and move on (coalescing).
    pub fn try_send(&self, request: CheckpointRequest) {
        match self.tx.try_send(request) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(req)) => {
                warn!(
                    island_id = %req.island_id,
                    tick = req.tick,
                    "checkpoint channel full, request dropped (coalescing)"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("checkpoint writer channel closed");
            }
        }
    }

    /// Read the latest completed checkpoint without blocking.
    pub fn latest_snapshot(&self) -> Option<CheckpointSnapshot> {
        let guard = self.latest.load();
        guard.as_ref().clone()
    }
}

/// Async checkpoint writer. Receives requests via channel, coalesces pending writes,
/// persists to a `CheckpointStore`, and updates ArcSwap on success.
pub struct CheckpointWriter<S> {
    rx: mpsc::Receiver<CheckpointRequest>,
    latest: Arc<ArcSwap<Option<CheckpointSnapshot>>>,
    store: S,
}

impl<S: CheckpointStore> CheckpointWriter<S> {
    /// Create a writer and its handle. `capacity` sets the bounded channel size (default: 16).
    pub fn new(store: S, capacity: usize) -> (Self, CheckpointHandle) {
        let (tx, rx) = mpsc::channel(capacity);
        let latest = Arc::new(ArcSwap::from_pointee(None));
        let handle = CheckpointHandle {
            tx,
            latest: Arc::clone(&latest),
        };
        let writer = Self { rx, latest, store };
        (writer, handle)
    }

    /// Run the writer loop until the channel closes.
    pub async fn run(mut self) {
        while let Some(mut request) = self.rx.recv().await {
            // Coalesce: drain pending requests, keep the latest.
            // Ack any dropped requests so callers don't hang.
            while let Ok(newer) = self.rx.try_recv() {
                if let Some(ack) = request.ack.take() {
                    let _ = ack.send(());
                }
                request = newer;
            }

            let key = request.island_id.0.clone();
            let tick = request.tick;
            let data = request.data.clone();

            match self.store.put(key, data.clone()).await {
                Ok(()) => {
                    self.latest
                        .store(Arc::new(Some(CheckpointSnapshot { tick, data })));
                    info!(island_id = %request.island_id, tick, "checkpoint written");
                }
                Err(e) => {
                    warn!(
                        island_id = %request.island_id,
                        tick,
                        error = %e,
                        "checkpoint write failed"
                    );
                }
            }

            if let Some(ack) = request.ack.take() {
                let _ = ack.send(());
            }
        }
    }
}
