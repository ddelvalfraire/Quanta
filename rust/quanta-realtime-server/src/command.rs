use crate::reconnect::ConnectedClient;
use crate::types::IslandId;
use crate::types::IslandManifest;
use crate::zone_transfer::{BuffState, TransferError, TransferredPlayer};
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
    /// Phase 1: Create a signed transfer token and mark the player as leaving.
    PrepareZoneTransfer {
        player_id: String,
        source_island: IslandId,
        target_island: IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        buffs: Vec<BuffState>,
        reply: oneshot::Sender<Result<Vec<u8>, ZoneTransferError>>,
    },
    /// Phase 2: Validate a transfer token and mark the player as arriving.
    AcceptZoneTransfer {
        token_bytes: Vec<u8>,
        target_island: IslandId,
        reply: oneshot::Sender<Result<TransferredPlayer, ZoneTransferError>>,
    },
    /// Route an authenticated client to the manager. Phase 1 stores it in
    /// a flat vec as a placeholder; Phase 3 binds per-island.
    ClientConnected {
        client: ConnectedClient,
        reply: oneshot::Sender<Result<u64, ClientConnectedError>>,
    },
    /// Notify the manager that a previously-connected client's underlying
    /// transport has closed. Phase 1 removes the entry from the placeholder
    /// vec; Phase 3 will evict from the per-island registry.
    ClientDisconnected {
        session_id: u64,
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

#[derive(Debug, PartialEq, Eq)]
pub enum ZoneTransferError {
    NotConfigured,
    SourceNotFound(IslandId),
    SourceNotRunning(IslandId),
    TargetNotFound(IslandId),
    TargetNotRunning(IslandId),
    Transfer(TransferError),
}

impl fmt::Display for ZoneTransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "zone transfer not configured"),
            Self::SourceNotFound(id) => write!(f, "source island not found: {id}"),
            Self::SourceNotRunning(id) => write!(f, "source island not running: {id}"),
            Self::TargetNotFound(id) => write!(f, "target island not found: {id}"),
            Self::TargetNotRunning(id) => write!(f, "target island not running: {id}"),
            Self::Transfer(e) => write!(f, "transfer error: {e}"),
        }
    }
}

impl std::error::Error for ZoneTransferError {}

impl From<TransferError> for ZoneTransferError {
    fn from(e: TransferError) -> Self {
        Self::Transfer(e)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagerMetrics {
    pub active_islands: u32,
    pub total_islands: u32,
    pub total_entities: u64,
    #[serde(default)]
    pub connected_clients: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClientConnectedError {
    AtCapacity { max: usize },
}

impl fmt::Display for ClientConnectedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtCapacity { max } => write!(f, "server at client capacity ({max})"),
        }
    }
}

impl std::error::Error for ClientConnectedError {}
