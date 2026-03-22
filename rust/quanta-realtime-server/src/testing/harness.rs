use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::command::IslandCommand;
use crate::tick::*;
use crate::types::{EntitySlot, IslandId};

pub struct TestHarness {
    engine: TickEngine,
    input_tx: crossbeam_channel::Sender<ClientInput>,
    bridge_tx: crossbeam_channel::Sender<BridgeMessage>,
    #[allow(dead_code)]
    cmd_tx: crossbeam_channel::Sender<IslandCommand>,
    shutdown: Arc<AtomicBool>,
}

impl TestHarness {
    pub fn tick(&mut self) {
        self.engine.tick();
    }

    pub fn tick_n(&mut self, n: u32) {
        self.engine.tick_n(n);
    }

    pub fn send_input(&self, input: ClientInput) {
        self.input_tx.send(input).expect("input channel disconnected");
    }

    pub fn send_bridge(&self, msg: BridgeMessage) {
        self.bridge_tx.send(msg).expect("bridge channel disconnected");
    }

    pub fn add_entity(&mut self, slot: EntitySlot, state: Vec<u8>, owner: Option<SessionId>) {
        self.engine.add_entity(slot, state, owner);
    }

    pub fn take_effects(&mut self) -> Vec<BridgeEffect> {
        self.engine.take_effects()
    }

    pub fn current_tick(&self) -> u64 {
        self.engine.current_tick()
    }

    pub fn last_completed_tick(&self) -> u64 {
        self.engine.current_tick().saturating_sub(1)
    }

    pub fn get_entity_state(&self, slot: &EntitySlot) -> Option<&[u8]> {
        self.engine.get_entity_state(slot)
    }

    pub fn entity_count(&self) -> usize {
        self.engine.entity_count()
    }

    pub fn entity_slots(&self) -> Vec<EntitySlot> {
        self.engine.entity_slots()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

pub struct TestHarnessBuilder {
    tick_rate_hz: u8,
    max_catchup_ticks: u32,
    wasm: Option<Box<dyn WasmExecutor>>,
    island_id: IslandId,
}

impl TestHarnessBuilder {
    pub fn new() -> Self {
        Self {
            tick_rate_hz: 20,
            max_catchup_ticks: 3,
            wasm: None,
            island_id: IslandId::from("test-island"),
        }
    }

    pub fn tick_rate(mut self, hz: u8) -> Self {
        self.tick_rate_hz = hz;
        self
    }

    pub fn max_catchup(mut self, n: u32) -> Self {
        self.max_catchup_ticks = n;
        self
    }

    pub fn wasm(mut self, executor: Box<dyn WasmExecutor>) -> Self {
        self.wasm = Some(executor);
        self
    }

    pub fn island_id(mut self, id: &str) -> Self {
        self.island_id = IslandId::from(id);
        self
    }

    pub fn build(self) -> TestHarness {
        let (input_tx, input_rx) = crossbeam_channel::unbounded();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (bridge_tx, bridge_rx) = crossbeam_channel::unbounded();
        let shutdown = Arc::new(AtomicBool::new(false));

        let config = TickEngineConfig {
            tick_rate_hz: self.tick_rate_hz,
            max_catchup_ticks: self.max_catchup_ticks,
        };

        let wasm = self.wasm.unwrap_or_else(|| Box::new(NoopWasmExecutor));

        let heartbeat = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let engine = TickEngine::new(
            self.island_id,
            config,
            wasm,
            input_rx,
            bridge_rx,
            cmd_rx,
            shutdown.clone(),
            heartbeat,
        );

        TestHarness {
            engine,
            input_tx,
            bridge_tx,
            cmd_tx,
            shutdown,
        }
    }
}

impl Default for TestHarnessBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_executors::IncrementWasm;

    #[test]
    fn builder_defaults_produce_working_harness() {
        let mut harness = TestHarnessBuilder::new().build();
        assert_eq!(harness.current_tick(), 0);
        harness.tick();
        assert_eq!(harness.current_tick(), 1);
    }

    #[test]
    fn deterministic_ticks_same_inputs_same_outputs() {
        let mut results = Vec::new();

        for _ in 0..2 {
            let mut harness = TestHarnessBuilder::new()
                .wasm(Box::new(IncrementWasm))
                .build();

            harness.add_entity(EntitySlot(1), vec![0], None);

            for seq in 1..=100 {
                harness.send_input(ClientInput {
                    session_id: SessionId::from("p1"),
                    entity_slot: EntitySlot(1),
                    input_seq: seq,
                    payload: vec![],
                });
                harness.tick();
            }

            let state = harness.get_entity_state(&EntitySlot(1)).unwrap().to_vec();
            results.push(state);
        }

        assert_eq!(results[0], results[1], "deterministic: same inputs → same state");
    }

    #[test]
    fn tick_n_advances_correctly() {
        let mut harness = TestHarnessBuilder::new().build();
        harness.tick_n(10);
        assert_eq!(harness.current_tick(), 10);
    }

    #[test]
    fn custom_tick_rate() {
        let harness = TestHarnessBuilder::new()
            .tick_rate(60)
            .build();
        assert_eq!(harness.current_tick(), 0);
    }

    #[test]
    fn last_completed_tick_tracks_correctly() {
        let mut harness = TestHarnessBuilder::new().build();
        assert_eq!(harness.last_completed_tick(), 0);
        harness.tick();
        assert_eq!(harness.last_completed_tick(), 0);
        harness.tick();
        assert_eq!(harness.last_completed_tick(), 1);
    }
}
