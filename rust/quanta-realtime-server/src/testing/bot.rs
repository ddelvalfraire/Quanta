use crate::tick::*;
use crate::types::EntitySlot;

/// Action a bot can take each tick.
#[derive(Debug, Clone)]
pub enum BotAction {
    SendInput { entity: u32, payload: Vec<u8> },
    Idle,
}

/// Trait for implementing bot behaviors in load tests.
pub trait BotBehavior: Send {
    fn on_tick(&mut self, tick: u64, entity_states: &[(u32, &[u8])]) -> Vec<BotAction>;
}

/// Does nothing — tests passive entity load.
pub struct IdleBot;

impl BotBehavior for IdleBot {
    fn on_tick(&mut self, _tick: u64, _entity_states: &[(u32, &[u8])]) -> Vec<BotAction> {
        vec![BotAction::Idle]
    }
}

/// Generates random input payloads using a seeded PRNG.
pub struct RandomWalkBot {
    state: u64,
    entity: u32,
}

impl RandomWalkBot {
    pub fn new(rng_seed: u64, entity: u32) -> Self {
        Self {
            state: rng_seed,
            entity,
        }
    }

    /// Simple xorshift64 PRNG.
    fn next_u64(&mut self) -> u64 {
        let mut s = self.state;
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        self.state = s;
        s
    }
}

impl BotBehavior for RandomWalkBot {
    fn on_tick(&mut self, _tick: u64, _entity_states: &[(u32, &[u8])]) -> Vec<BotAction> {
        let val = self.next_u64();
        vec![BotAction::SendInput {
            entity: self.entity,
            payload: val.to_le_bytes().to_vec(),
        }]
    }
}

/// Sends maximum input rate — tests throughput under load.
pub struct StressBot {
    pub inputs_per_tick: u32,
    entity: u32,
}

impl StressBot {
    pub fn new(inputs_per_tick: u32, entity: u32) -> Self {
        Self {
            inputs_per_tick,
            entity,
        }
    }
}

impl BotBehavior for StressBot {
    fn on_tick(&mut self, tick: u64, _entity_states: &[(u32, &[u8])]) -> Vec<BotAction> {
        (0..self.inputs_per_tick)
            .map(|i| BotAction::SendInput {
                entity: self.entity,
                payload: [tick.to_le_bytes().as_slice(), &i.to_le_bytes()].concat(),
            })
            .collect()
    }
}

/// Metrics collected from a bot harness run.
#[derive(Debug, Clone, Default)]
pub struct BotMetrics {
    pub total_inputs: u64,
    pub ticks_run: u64,
}

/// Runs N bots against a TestHarness for M ticks, collecting metrics.
pub struct BotHarness {
    bots: Vec<Box<dyn BotBehavior>>,
    harness: super::TestHarness,
    metrics: BotMetrics,
    input_seq: u32,
}

impl BotHarness {
    pub fn new(harness: super::TestHarness, bots: Vec<Box<dyn BotBehavior>>) -> Self {
        Self {
            bots,
            harness,
            metrics: BotMetrics::default(),
            input_seq: 0,
        }
    }

    /// Run all bots for the given number of ticks.
    pub fn run(&mut self, ticks: u64) {
        for _ in 0..ticks {
            let tick = self.harness.current_tick();

            // Collect entity states for bots
            let entity_states: Vec<(u32, Vec<u8>)> = (0..self.harness.entity_count() as u32)
                .filter_map(|slot| {
                    self.harness
                        .get_entity_state(&EntitySlot(slot))
                        .map(|s| (slot, s.to_vec()))
                })
                .collect();

            let state_refs: Vec<(u32, &[u8])> =
                entity_states.iter().map(|(s, d)| (*s, d.as_slice())).collect();

            // Collect all actions from all bots
            let mut all_actions = Vec::new();
            for bot in &mut self.bots {
                all_actions.extend(bot.on_tick(tick, &state_refs));
            }

            // Execute actions
            for action in all_actions {
                match action {
                    BotAction::SendInput { entity, payload } => {
                        self.input_seq += 1;
                        self.harness.send_input(ClientInput {
                            session_id: SessionId::from("bot"),
                            entity_slot: EntitySlot(entity),
                            input_seq: self.input_seq,
                            payload,
                        });
                        self.metrics.total_inputs += 1;
                    }
                    BotAction::Idle => {}
                }
            }

            self.harness.tick();
            self.metrics.ticks_run += 1;
        }
    }

    pub fn metrics(&self) -> &BotMetrics {
        &self.metrics
    }

    pub fn harness(&self) -> &super::TestHarness {
        &self.harness
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::TestHarnessBuilder;

    #[test]
    fn idle_bots_run_without_errors() {
        let mut harness = TestHarnessBuilder::new().build();
        for i in 0..100 {
            harness.add_entity(EntitySlot(i), vec![0], None);
        }

        let bots: Vec<Box<dyn BotBehavior>> =
            (0..100).map(|_| Box::new(IdleBot) as Box<dyn BotBehavior>).collect();

        let mut bot_harness = BotHarness::new(harness, bots);
        bot_harness.run(60);

        assert_eq!(bot_harness.metrics().ticks_run, 60);
        assert_eq!(bot_harness.metrics().total_inputs, 0);
    }

    #[test]
    fn stress_bot_generates_expected_volume() {
        let mut harness = TestHarnessBuilder::new().build();
        harness.add_entity(EntitySlot(0), vec![0], None);

        let bots: Vec<Box<dyn BotBehavior>> =
            vec![Box::new(StressBot::new(10, 0))];

        let mut bot_harness = BotHarness::new(harness, bots);
        bot_harness.run(5);

        assert_eq!(bot_harness.metrics().ticks_run, 5);
        assert_eq!(bot_harness.metrics().total_inputs, 50); // 10 inputs × 5 ticks
    }

    #[test]
    fn random_walk_bot_is_deterministic() {
        let mut results = Vec::new();

        for _ in 0..2 {
            let wasm = IncrementWasm;
            let mut harness = TestHarnessBuilder::new()
                .wasm(Box::new(wasm))
                .build();
            harness.add_entity(EntitySlot(0), vec![0], None);

            let bots: Vec<Box<dyn BotBehavior>> =
                vec![Box::new(RandomWalkBot::new(42, 0))];

            let mut bot_harness = BotHarness::new(harness, bots);
            bot_harness.run(20);

            let state = bot_harness
                .harness()
                .get_entity_state(&EntitySlot(0))
                .unwrap()
                .to_vec();
            results.push(state);
        }

        assert_eq!(results[0], results[1], "same seed → same state");
    }

    struct IncrementWasm;

    impl WasmExecutor for IncrementWasm {
        fn call_handle_message(
            &mut self,
            _entity: EntitySlot,
            state: &[u8],
            _message: &TickMessage,
        ) -> Result<HandleResult, WasmTrap> {
            let mut new_state = state.to_vec();
            new_state[0] = new_state[0].wrapping_add(1);
            Ok(HandleResult {
                state: new_state,
                effects: vec![],
            })
        }
    }
}
