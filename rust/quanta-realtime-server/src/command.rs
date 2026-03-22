use crate::types::IslandId;
use crate::types::IslandManifest;
use std::fmt;
use tokio::sync::oneshot;

pub enum ManagerCommand {
    Activate {
        manifest: IslandManifest,
        reply: oneshot::Sender<Result<(), ActivationError>>,
    },
    Drain {
        island_id: IslandId,
        reply: oneshot::Sender<Result<(), LifecycleError>>,
    },
    Stop {
        island_id: IslandId,
        reply: oneshot::Sender<Result<(), LifecycleError>>,
    },
    GetMetrics {
        reply: oneshot::Sender<ManagerMetrics>,
    },
    PlayerJoined {
        island_id: IslandId,
        reply: oneshot::Sender<Result<(), LifecycleError>>,
    },
    PlayerLeft {
        island_id: IslandId,
        reply: oneshot::Sender<Result<(), LifecycleError>>,
    },
    /// Route a bridge message to an island. Reactivates passivated islands on demand.
    BridgeMessage {
        island_id: IslandId,
        message: crate::tick::BridgeMessage,
        reply: oneshot::Sender<Result<(), LifecycleError>>,
    },
    /// Notify the manager of player input activity (resets idle timer).
    PlayerInput {
        island_id: IslandId,
        reply: oneshot::Sender<Result<(), LifecycleError>>,
    },
}

pub enum IslandCommand {
    Drain,
    Stop,
    /// Passivate: complete current tick, capture entity state snapshot, then stop.
    Passivate {
        snapshot_tx: crossbeam_channel::Sender<crate::types::IslandSnapshot>,
    },
}

impl std::fmt::Debug for IslandCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Drain => write!(f, "Drain"),
            Self::Stop => write!(f, "Stop"),
            Self::Passivate { .. } => write!(f, "Passivate"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActivationError {
    DuplicateIsland(IslandId),
    AtCapacity { max: u32 },
}

impl fmt::Display for ActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateIsland(id) => write!(f, "island already exists: {id}"),
            Self::AtCapacity { max } => write!(f, "server at capacity ({max} islands)"),
        }
    }
}

impl std::error::Error for ActivationError {}

#[derive(Debug, PartialEq, Eq)]
pub enum LifecycleError {
    NotFound(IslandId),
    InvalidTransition(String),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "island not found: {id}"),
            Self::InvalidTransition(msg) => write!(f, "invalid transition: {msg}"),
        }
    }
}

impl std::error::Error for LifecycleError {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagerMetrics {
    pub active_islands: u32,
    pub total_islands: u32,
    pub total_entities: u64,
}
