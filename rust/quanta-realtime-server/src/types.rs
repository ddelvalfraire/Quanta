use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a simulation island.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IslandId(pub String);

impl fmt::Display for IslandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for IslandId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Slot index for an entity within an island's ECS-style storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntitySlot(pub u32);

/// Raw delta-encoded bytes produced by the encoder.
pub type DeltaBytes = Vec<u8>;

/// Input received from a player for a given tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInput {
    pub player_id: String,
    pub payload: Vec<u8>,
}

/// Manifest describing an island to be activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifest {
    pub island_id: IslandId,
    pub entity_count: u32,
    pub wasm_module: String,
    pub initial_state: Vec<u8>,
}

/// Serialized snapshot of an island's full state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandSnapshot {
    pub island_id: IslandId,
    pub tick: u64,
    pub state: Vec<u8>,
}

/// An effect produced by a tick (e.g., spawning an entity, sending a message).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub payload: Vec<u8>,
}

/// Discriminant for effect types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectKind {
    SpawnEntity,
    DespawnEntity,
    SendMessage,
    EmitEvent,
}
