use crate::command::IslandCommand;
use crate::island::state_machine::IslandState;
use crate::tick::ClientInput;
use crate::types::{IslandId, IslandManifest};
use crossbeam_channel::Sender;
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadModel {
    Dedicated,
    Pooled,
}

pub struct IslandHandle {
    pub island_id: IslandId,
    pub state: IslandState,
    pub thread_model: ThreadModel,
    pub entity_count: u32,
    pub command_tx: Sender<IslandCommand>,
    pub input_tx: Sender<ClientInput>,
    pub join_handle: Option<JoinHandle<()>>,
    pub manifest: IslandManifest,
    pub player_count: u32,
    /// Deadline after which the island should be passivated (None = not scheduled).
    pub passivation_deadline: Option<Instant>,
    pub passivate_when_empty: bool,
}
