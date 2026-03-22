use crate::command::IslandCommand;
use crate::island::state_machine::IslandState;
use crate::types::IslandId;
use crossbeam_channel::Sender;
use std::thread::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadModel {
    Dedicated,
    /// TODO(T45): Pooled islands currently use std::thread::spawn like Dedicated.
    /// Wire up a thread pool (rayon or custom work-stealing) for this variant.
    Pooled,
}

pub struct IslandHandle {
    pub island_id: IslandId,
    pub state: IslandState,
    pub thread_model: ThreadModel,
    pub entity_count: u32,
    pub command_tx: Sender<IslandCommand>,
    pub join_handle: Option<JoinHandle<()>>,
}
