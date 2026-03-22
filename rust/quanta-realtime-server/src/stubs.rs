use crate::traits::{Bridge, IslandRuntime, Session, SpatialIndex, TickResult};
use crate::types::{DeltaBytes, IslandId, IslandManifest, IslandSnapshot, PlayerInput};

/// Stub runtime that does nothing. Used for testing the manager lifecycle.
pub struct StubIslandRuntime {
    tick_count: u64,
}

impl StubIslandRuntime {
    pub fn new() -> Self {
        Self { tick_count: 0 }
    }
}

impl Default for StubIslandRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandRuntime for StubIslandRuntime {
    fn init(&mut self, _manifest: &IslandManifest) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn tick(&mut self, _inputs: &[PlayerInput]) -> Result<TickResult, Box<dyn std::error::Error>> {
        self.tick_count += 1;
        Ok(TickResult {
            tick: self.tick_count,
            delta: DeltaBytes::new(),
            effects: vec![],
        })
    }

    fn snapshot(&self) -> Result<IslandSnapshot, Box<dyn std::error::Error>> {
        Ok(IslandSnapshot {
            island_id: IslandId::from("stub"),
            tick: self.tick_count,
            state: vec![],
        })
    }

    fn restore(
        &mut self,
        snapshot: &IslandSnapshot,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.tick_count = snapshot.tick;
        Ok(())
    }
}

/// Stub session that discards all sends.
pub struct StubSession;

impl Session for StubSession {
    fn send_delta(&self, _island_id: &IslandId, _player_id: &str, _delta: &[u8]) {}
    fn disconnect(&self, _player_id: &str) {}
}

/// Stub bridge that does nothing.
pub struct StubBridge;

impl Bridge for StubBridge {
    fn report_island_stopped(&self, _island_id: &IslandId) {}
    fn request_passivation(&self, _island_id: &IslandId) {}
}

/// Stub spatial index that returns empty results.
pub struct StubSpatialIndex;

impl SpatialIndex for StubSpatialIndex {
    fn update_position(&self, _island_id: &IslandId, _entity: u32, _x: f32, _y: f32, _z: f32) {}
    fn query_nearby(&self, _x: f32, _y: f32, _z: f32, _radius: f32) -> Vec<IslandId> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_runtime_tick_increments() {
        let mut rt = StubIslandRuntime::new();
        let manifest = IslandManifest {
            island_id: IslandId::from("test"),
            entity_count: 1,
            wasm_module: "test.wasm".into(),
            initial_state: vec![],
        };
        rt.init(&manifest).unwrap();
        let r1 = rt.tick(&[]).unwrap();
        assert_eq!(r1.tick, 1);
        let r2 = rt.tick(&[]).unwrap();
        assert_eq!(r2.tick, 2);
    }

    #[test]
    fn stub_runtime_snapshot_restore() {
        let mut rt = StubIslandRuntime::new();
        rt.tick(&[]).unwrap();
        rt.tick(&[]).unwrap();
        let snap = rt.snapshot().unwrap();
        assert_eq!(snap.tick, 2);

        let mut rt2 = StubIslandRuntime::new();
        rt2.restore(&snap).unwrap();
        let r = rt2.tick(&[]).unwrap();
        assert_eq!(r.tick, 3);
    }
}
