use crate::types::EntitySlot;
use std::collections::BTreeMap;

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
}

#[derive(Debug, Clone)]
pub struct ClientInput {
    pub session_id: SessionId,
    pub entity_slot: EntitySlot,
    pub input_seq: u32,
    pub payload: Vec<u8>,
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
}

/// Effects returned from WASM handle_message execution.
#[derive(Debug, Clone)]
pub enum TickEffect {
    Reply(Vec<u8>),
    Send { target: EntitySlot, payload: Vec<u8> },
    SendRemote { target: String, payload: Vec<u8> },
    Persist,
    SetTimer { name: String, delay_ms: u32 },
    CancelTimer(String),
    EmitTelemetry { event: String },
    StopSelf,
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

/// Work item sent from island thread to async runtime for delta encoding.
#[derive(Debug)]
pub struct DeltaWorkItem {
    pub tick: u64,
    pub entity_states: BTreeMap<EntitySlot, Vec<u8>>,
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
