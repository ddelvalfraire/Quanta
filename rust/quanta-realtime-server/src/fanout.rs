//! Per-island fanout: takes `TickSnapshot`s from the engine and sends
//! client-specific datagrams over `Session::send_unreliable` via per-client
//! `PacingHandle`s. The trait is generic — demo crates provide a concrete
//! impl that knows how to encode their state into delta bytes.
//!
//! The loop bridges a sync crossbeam snapshot channel and an async tokio
//! command channel inside a `tokio::select!`, which is why the snapshot
//! drain runs on a 5ms poll cadence (matches the 20Hz tick rate with
//! margin).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::session::Session;
use crate::tick::types::TickSnapshot;
use crate::types::{ClientIndex, EntitySlot};

pub enum FanoutCommand {
    ClientJoined {
        client_index: ClientIndex,
        entity_slot: EntitySlot,
        session: Arc<dyn Session>,
    },
    ClientLeft {
        client_index: ClientIndex,
    },
}

pub trait IslandFanout: Send {
    fn on_client_joined(
        &mut self,
        client_index: ClientIndex,
        entity_slot: EntitySlot,
        session: Arc<dyn Session>,
    );
    fn on_client_left(&mut self, client_index: ClientIndex);
    fn on_tick(&mut self, snapshot: &TickSnapshot);
}

pub type FanoutFactory = Arc<dyn Fn() -> Box<dyn IslandFanout> + Send + Sync>;

const SNAPSHOT_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub async fn fanout_loop(
    mut fanout: Box<dyn IslandFanout>,
    snapshot_rx: crossbeam_channel::Receiver<TickSnapshot>,
    mut cmd_rx: mpsc::Receiver<FanoutCommand>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(FanoutCommand::ClientJoined { client_index, entity_slot, session }) => {
                        fanout.on_client_joined(client_index, entity_slot, session);
                    }
                    Some(FanoutCommand::ClientLeft { client_index }) => {
                        fanout.on_client_left(client_index);
                    }
                    None => break,
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(SNAPSHOT_POLL_INTERVAL) => {
                while let Ok(snapshot) = snapshot_rx.try_recv() {
                    fanout.on_tick(&snapshot);
                }
            }
        }
    }
}
