use crate::command::IslandCommand;
use crate::island::state_machine::IslandState;
use crate::tick::ClientInput;
use crate::types::IslandId;
use crossbeam_channel::Sender;
use std::thread::JoinHandle;

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
}
