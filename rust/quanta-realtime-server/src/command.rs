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
}

#[derive(Debug)]
pub enum IslandCommand {
    Tick,
    Drain,
    Stop,
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
