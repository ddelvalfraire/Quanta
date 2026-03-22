use crate::command::IslandCommand;
use crate::island::state_machine::IslandState;
use crate::types::IslandId;
use crossbeam_channel::Sender;
use std::thread::JoinHandle;

/// How an island thread is scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadModel {
    /// Island gets its own OS thread (high entity count).
    Dedicated,
    /// Island shares a thread pool (low entity count).
    Pooled,
}

/// Runtime handle for a live island, held by the manager.
pub struct IslandHandle {
    pub island_id: IslandId,
    pub state: IslandState,
    pub thread_model: ThreadModel,
    pub entity_count: u32,
    pub command_tx: Sender<IslandCommand>,
    pub join_handle: Option<JoinHandle<()>>,
}
