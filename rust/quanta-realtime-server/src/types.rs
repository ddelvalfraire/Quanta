use serde::{Deserialize, Serialize};
use std::fmt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntitySlot(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientIndex(pub u16);

pub type DeltaBytes = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInput {
    pub player_id: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifest {
    pub island_id: IslandId,
    pub entity_count: u32,
    pub wasm_module: String,
    pub initial_state: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandSnapshot {
    pub island_id: IslandId,
    pub tick: u64,
    pub state: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectKind {
    SpawnEntity,
    DespawnEntity,
    SendMessage,
    EmitEvent,
}
