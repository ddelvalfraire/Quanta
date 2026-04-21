use crate::tick::*;
use crate::types::EntitySlot;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum BotAction {
    SendInput { entity: u32, payload: Vec<u8> },
}

pub trait BotBehavior: Send {
    fn on_tick(&mut self, tick: u64, entity_states: &[(u32, Vec<u8>)]) -> Vec<BotAction>;
}

pub struct IdleBot;

impl BotBehavior for IdleBot {
    fn on_tick(&mut self, _tick: u64, _entity_states: &[(u32, Vec<u8>)]) -> Vec<BotAction> {
        vec![]
    }
}

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
    fn on_tick(&mut self, _tick: u64, _entity_states: &[(u32, Vec<u8>)]) -> Vec<BotAction> {
        let val = self.next_u64();
        vec![BotAction::SendInput {
            entity: self.entity,
            payload: val.to_le_bytes().to_vec(),
        }]
    }
}

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
    fn on_tick(&mut self, tick: u64, _entity_states: &[(u32, Vec<u8>)]) -> Vec<BotAction> {
        (0..self.inputs_per_tick)
            .map(|i| BotAction::SendInput {
                entity: self.entity,
                payload: [tick.to_le_bytes().as_slice(), &i.to_le_bytes()].concat(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct BotMetrics {
    pub total_inputs: u64,
    pub ticks_run: u64,
}

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

    pub fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            let tick = self.harness.current_tick();

            let entity_states: Vec<(u32, Vec<u8>)> = self
                .harness
                .entity_slots()
                .iter()
                .filter_map(|slot| {
                    self.harness
                        .get_entity_state(slot)
                        .map(|s| (slot.0, s.to_vec()))
                })
                .collect();

            for (bot_idx, bot) in self.bots.iter_mut().enumerate() {
                for action in bot.on_tick(tick, &entity_states) {
                    match action {
                        BotAction::SendInput { entity, payload } => {
                            self.input_seq += 1;
                            self.harness.send_input(ClientInput {
                                session_id: SessionId::from(format!("bot-{bot_idx}").as_str()),
                                entity_slot: EntitySlot(entity),
                                input_seq: self.input_seq,
                                payload,
                            });
                            self.metrics.total_inputs += 1;
                        }
                    }
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
    use super::super::test_executors::IncrementWasm;
    use super::super::TestHarnessBuilder;
    use super::*;

    #[test]
    fn idle_bots_run_without_errors() {
        let mut harness = TestHarnessBuilder::new().build();
        for i in 0..100 {
            harness.add_entity(EntitySlot(i), vec![0], None);
        }

        let bots: Vec<Box<dyn BotBehavior>> = (0..100)
            .map(|_| Box::new(IdleBot) as Box<dyn BotBehavior>)
            .collect();

        let mut bot_harness = BotHarness::new(harness, bots);
        bot_harness.run(60);

        assert_eq!(bot_harness.metrics().ticks_run, 60);
        assert_eq!(bot_harness.metrics().total_inputs, 0);
    }

    #[test]
    fn stress_bot_generates_expected_volume() {
        let mut harness = TestHarnessBuilder::new().build();
        harness.add_entity(EntitySlot(0), vec![0], None);

        let bots: Vec<Box<dyn BotBehavior>> = vec![Box::new(StressBot::new(10, 0))];

        let mut bot_harness = BotHarness::new(harness, bots);
        bot_harness.run(5);

        assert_eq!(bot_harness.metrics().ticks_run, 5);
        assert_eq!(bot_harness.metrics().total_inputs, 50);
    }

    #[test]
    fn random_walk_bot_is_deterministic() {
        let mut results = Vec::new();

        for _ in 0..2 {
            let mut harness = TestHarnessBuilder::new()
                .wasm(Box::new(IncrementWasm))
                .build();
            harness.add_entity(EntitySlot(0), vec![0], None);

            let bots: Vec<Box<dyn BotBehavior>> = vec![Box::new(RandomWalkBot::new(42, 0))];

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

    #[test]
    fn sparse_entity_slots_handled_correctly() {
        let mut harness = TestHarnessBuilder::new().build();
        harness.add_entity(EntitySlot(10), vec![0], None);
        harness.add_entity(EntitySlot(50), vec![0], None);

        let bots: Vec<Box<dyn BotBehavior>> = vec![Box::new(IdleBot)];

        let mut bot_harness = BotHarness::new(harness, bots);
        bot_harness.run(5);

        assert_eq!(bot_harness.metrics().ticks_run, 5);
    }
}
