use crate::types::{DeltaBytes, Effect, IslandId, IslandManifest, IslandSnapshot, PlayerInput};

/// Result of a single simulation tick.
#[derive(Debug)]
pub struct TickResult {
    pub tick: u64,
    pub delta: DeltaBytes,
    pub effects: Vec<Effect>,
}

/// Core simulation runtime that drives an island's WASM actor.
/// Implemented per-game; stubbed for T44.
pub trait IslandRuntime: Send + 'static {
    fn init(&mut self, manifest: &IslandManifest) -> Result<(), Box<dyn std::error::Error>>;
    fn tick(&mut self, inputs: &[PlayerInput]) -> Result<TickResult, Box<dyn std::error::Error>>;
    fn snapshot(&self) -> Result<IslandSnapshot, Box<dyn std::error::Error>>;
    fn restore(&mut self, snapshot: &IslandSnapshot)
        -> Result<(), Box<dyn std::error::Error>>;
}

/// Player session interface for the transport layer (T48).
pub trait Session: Send + Sync + 'static {
    fn send_delta(&self, island_id: &IslandId, player_id: &str, delta: &[u8]);
    fn disconnect(&self, player_id: &str);
}

/// Bridge to the Elixir control plane (T54).
pub trait Bridge: Send + Sync + 'static {
    fn report_island_stopped(&self, island_id: &IslandId);
    fn request_passivation(&self, island_id: &IslandId);
}

/// Spatial index for interest management (T52).
pub trait SpatialIndex: Send + Sync + 'static {
    fn update_position(&self, island_id: &IslandId, entity: u32, x: f32, y: f32, z: f32);
    fn query_nearby(&self, x: f32, y: f32, z: f32, radius: f32) -> Vec<IslandId>;
}
