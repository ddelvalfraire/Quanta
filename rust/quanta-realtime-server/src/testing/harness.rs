use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::command::IslandCommand;
use crate::tick::*;
use crate::types::{EntitySlot, IslandId};

/// A test harness wrapping a `TickEngine` for deterministic testing.
///
/// Owns the channel senders and shutdown flag so tests only need to
/// interact with high-level convenience methods.
pub struct TestHarness {
    engine: TickEngine,
    input_tx: crossbeam_channel::Sender<ClientInput>,
    #[allow(dead_code)]
    cmd_tx: crossbeam_channel::Sender<IslandCommand>,
    shutdown: Arc<AtomicBool>,
}

impl TestHarness {
    /// Execute one tick and advance the counter.
    pub fn tick(&mut self) {
        self.engine.tick();
    }

    /// Execute N ticks.
    pub fn tick_n(&mut self, n: u32) {
        self.engine.tick_n(n);
    }

    /// Inject a client input into the engine's input channel.
    pub fn send_input(&self, input: ClientInput) {
        self.input_tx.send(input).expect("input channel disconnected");
    }

    /// Add an entity to the engine.
    pub fn add_entity(&mut self, slot: EntitySlot, state: Vec<u8>, owner: Option<SessionId>) {
        self.engine.add_entity(slot, state, owner);
    }

    /// Take all bridge effects emitted during the last tick.
    pub fn take_effects(&mut self) -> Vec<BridgeEffect> {
        self.engine.take_effects()
    }

    /// Current tick number.
    pub fn current_tick(&self) -> u64 {
        self.engine.current_tick()
    }

    /// Get entity state by slot.
    pub fn get_entity_state(&self, slot: &EntitySlot) -> Option<&[u8]> {
        self.engine.get_entity_state(slot)
    }

    /// Number of entities in the engine.
    pub fn entity_count(&self) -> usize {
        self.engine.entity_count()
    }

    /// Signal shutdown (useful if tests spawn the run loop in a thread).
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Direct access to the underlying engine.
    pub fn engine(&self) -> &TickEngine {
        &self.engine
    }

    /// Mutable access to the underlying engine.
    pub fn engine_mut(&mut self) -> &mut TickEngine {
        &mut self.engine
    }
}

/// Builder for constructing a `TestHarness`.
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
        let shutdown = Arc::new(AtomicBool::new(false));

        let config = TickEngineConfig {
            tick_rate_hz: self.tick_rate_hz,
            max_catchup_ticks: self.max_catchup_ticks,
        };

        let wasm = self.wasm.unwrap_or_else(|| Box::new(NoopWasmExecutor));

        let engine = TickEngine::new(
            self.island_id,
            config,
            wasm,
            input_rx,
            cmd_rx,
            shutdown.clone(),
        );

        TestHarness {
            engine,
            input_tx,
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

    #[test]
    fn builder_defaults_produce_working_harness() {
        let mut harness = TestHarnessBuilder::new().build();
        assert_eq!(harness.current_tick(), 0);
        harness.tick();
        assert_eq!(harness.current_tick(), 1);
    }

    #[test]
    fn deterministic_ticks_same_inputs_same_outputs() {
        // Run the same sequence twice — entity state must match at every tick.
        let mut results = Vec::new();

        for _ in 0..2 {
            let wasm = CountingWasm { counter: 0 };
            let mut harness = TestHarnessBuilder::new()
                .wasm(Box::new(wasm))
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

    /// A deterministic WASM executor that increments a counter in state.
    struct CountingWasm {
        counter: u32,
    }

    impl WasmExecutor for CountingWasm {
        fn call_handle_message(
            &mut self,
            _entity: EntitySlot,
            _state: &[u8],
            _message: &TickMessage,
        ) -> Result<HandleResult, WasmTrap> {
            self.counter += 1;
            Ok(HandleResult {
                state: self.counter.to_le_bytes().to_vec(),
                effects: vec![],
            })
        }
    }
}
