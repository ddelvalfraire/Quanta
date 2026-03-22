use crate::types::{EntitySlot, IslandId};

pub type CorrelationId = [u8; 16];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

pub struct EntityState {
    pub slot: EntitySlot,
    pub state: Vec<u8>,
    pub owner_session: Option<SessionId>,
    /// Set when state changes; cleared after checkpoint snapshot is taken.
    pub dirty: bool,
    pub init_state: Vec<u8>,
    pub checkpoint_state: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ClientInput {
    pub session_id: SessionId,
    pub entity_slot: EntitySlot,
    pub input_seq: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BridgeMessage {
    pub target_entity: EntitySlot,
    pub kind: BridgeMessageKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum BridgeMessageKind {
    OneWay,
    Request { correlation_id: CorrelationId },
    SagaFailed { correlation_id: CorrelationId },
}

/// Messages delivered to entities during a tick.
/// Added to per-entity queues in priority order: Timer > Bridge > Input > Deferred.
#[derive(Debug, Clone)]
pub enum TickMessage {
    Timer { name: String },
    Bridge { payload: Vec<u8> },
    Input {
        session_id: SessionId,
        input_seq: u32,
        payload: Vec<u8>,
    },
    Deferred {
        source: EntitySlot,
        payload: Vec<u8>,
    },
    BridgeRequest {
        correlation_id: CorrelationId,
        payload: Vec<u8>,
    },
    SagaFailed {
        correlation_id: CorrelationId,
    },
}

/// Effects returned from WASM handle_message execution.
#[derive(Debug, Clone)]
pub enum TickEffect {
    Send { target: EntitySlot, payload: Vec<u8> },
    SendRemote { target: String, payload: Vec<u8> },
    Persist,
    SetTimer { name: String, delay_ms: u32 },
    CancelTimer(String),
    EmitTelemetry { event: String },
    Reply(Vec<u8>),
    StopSelf,
    RequestRemote { target: String, payload: Vec<u8> },
    FireAndForget { target: String, payload: Vec<u8> },
    /// WASM emits this to initiate a zone transfer for a player.
    ZoneTransfer {
        player_id: String,
        target_zone: IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        /// Bitcode-encoded `Vec<BuffState>` — opaque to the tick engine.
        buffs: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct DeferredSend {
    pub source: EntitySlot,
    pub target: EntitySlot,
    pub payload: Vec<u8>,
}

/// Result of a WASM handle_message call.
pub struct HandleResult {
    pub state: Vec<u8>,
    pub effects: Vec<TickEffect>,
}

#[derive(Debug, Clone)]
pub enum WasmTrap {
    EpochDeadline,
    OutOfBounds,
    StackOverflow,
    StoreCorruption,
}

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EpochDeadline => write!(f, "epoch deadline exceeded"),
            Self::OutOfBounds => write!(f, "out-of-bounds memory access"),
            Self::StackOverflow => write!(f, "stack overflow"),
            Self::StoreCorruption => write!(f, "store corruption"),
        }
    }
}

impl std::error::Error for WasmTrap {}

/// Abstraction over WASM actor execution, for testability.
pub trait WasmExecutor: Send {
    fn call_handle_message(
        &mut self,
        entity: EntitySlot,
        state: &[u8],
        message: &TickMessage,
    ) -> Result<HandleResult, WasmTrap>;
}

/// No-op executor that returns state unchanged with no effects.
pub struct NoopWasmExecutor;

impl WasmExecutor for NoopWasmExecutor {
    fn call_handle_message(
        &mut self,
        _entity: EntitySlot,
        state: &[u8],
        _message: &TickMessage,
    ) -> Result<HandleResult, WasmTrap> {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![],
        })
    }
}

/// Effects routed outward from the tick engine (to bridge, checkpoint writer, etc.).
#[derive(Debug, Clone)]
pub enum BridgeEffect {
    SendRemote {
        target: String,
        payload: Vec<u8>,
    },
    Persist {
        entity_states: Vec<(EntitySlot, Vec<u8>)>,
    },
    EmitTelemetry {
        event: String,
    },
    RequestRemote {
        source_entity: EntitySlot,
        target: String,
        payload: Vec<u8>,
    },
    FireAndForget {
        target: String,
        payload: Vec<u8>,
    },
    BridgeReply {
        correlation_id: CorrelationId,
        payload: Vec<u8>,
    },
    EntityEvicted {
        entity: EntitySlot,
    },
    /// A zone transfer was requested by an entity's WASM logic.
    ZoneTransferRequest {
        player_id: String,
        source_entity: EntitySlot,
        target_zone: IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        buffs: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct TickEngineConfig {
    pub tick_rate_hz: u8,
    pub max_catchup_ticks: u32,
}

impl Default for TickEngineConfig {
    fn default() -> Self {
        Self {
            tick_rate_hz: 20,
            max_catchup_ticks: 3,
        }
    }
}
