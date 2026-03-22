use crate::command::IslandCommand;
use crate::types::{EntitySlot, IslandId};

use super::fault::{ActorHealthState, FaultTracker};
use super::timer::TimerManager;
use super::types::*;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// The core tick loop engine for a simulation island.
///
/// Executes a 6-phase tick loop with fixed timestep (Gaffer on Games pattern):
/// 1. Drain inputs (crossbeam MPSC → per-entity input buffer)
/// 2. Fire timers
/// 3. Build per-entity message queues (priority: Timer > Bridge > Input > Deferred)
/// 4. Simulate (WASM handle_message per entity, BTreeMap order)
/// 5. Batch effects (persist coalescing, deferred sends)
/// 6. Compute deltas (hand off to async runtime)
pub struct TickEngine {
    island_id: IslandId,
    config: TickEngineConfig,
    tick: u64,
    entities: BTreeMap<EntitySlot, EntityState>,
    timers: TimerManager,
    fault_tracker: FaultTracker,
    deferred_sends: Vec<DeferredSend>,
    wasm: Box<dyn WasmExecutor>,
    input_rx: crossbeam_channel::Receiver<ClientInput>,
    cmd_rx: crossbeam_channel::Receiver<IslandCommand>,
    effects_out: Vec<BridgeEffect>,
    last_input_seq: HashMap<SessionId, u32>,
    shutdown: Arc<AtomicBool>,
}

impl TickEngine {
    pub fn new(
        island_id: IslandId,
        config: TickEngineConfig,
        wasm: Box<dyn WasmExecutor>,
        input_rx: crossbeam_channel::Receiver<ClientInput>,
        cmd_rx: crossbeam_channel::Receiver<IslandCommand>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let tick_rate_hz = config.tick_rate_hz;
        Self {
            island_id,
            config,
            tick: 0,
            entities: BTreeMap::new(),
            timers: TimerManager::new(tick_rate_hz),
            fault_tracker: FaultTracker::new(),
            deferred_sends: Vec::new(),
            wasm,
            input_rx,
            cmd_rx,
            effects_out: Vec::new(),
            last_input_seq: HashMap::new(),
            shutdown,
        }
    }

    pub fn add_entity(&mut self, slot: EntitySlot, state: Vec<u8>, owner: Option<SessionId>) {
        self.entities.insert(
            slot,
            EntityState {
                slot,
                state,
                owner_session: owner,
            },
        );
    }

    pub fn remove_entity(&mut self, slot: &EntitySlot) {
        self.entities.remove(slot);
        self.timers.clear_entity(slot);
    }

    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    pub fn get_entity_state(&self, slot: &EntitySlot) -> Option<&[u8]> {
        self.entities.get(slot).map(|e| e.state.as_slice())
    }

    /// Take all bridge effects emitted during the last tick.
    pub fn take_effects(&mut self) -> Vec<BridgeEffect> {
        std::mem::take(&mut self.effects_out)
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn entity_slots(&self) -> Vec<EntitySlot> {
        self.entities.keys().copied().collect()
    }

    pub fn fault_state(&self, entity: &EntitySlot) -> ActorHealthState {
        self.fault_tracker.get_state(entity)
    }

    pub fn deferred_send_count(&self) -> usize {
        self.deferred_sends.len()
    }

    /// Set a timer on an entity. Delegates to the internal TimerManager.
    pub fn set_timer(&mut self, entity: EntitySlot, name: String, delay_ms: u32) {
        self.timers.set_timer(entity, name, delay_ms);
    }

    /// Cancel a timer on an entity.
    pub fn cancel_timer(&mut self, entity: &EntitySlot, name: &str) -> bool {
        self.timers.cancel_timer(entity, name)
    }

    /// Execute one tick and advance the tick counter.
    pub fn tick(&mut self) {
        self.execute_tick();
        self.tick += 1;
    }

    /// Execute N ticks.
    pub fn tick_n(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Run the fixed-timestep outer loop. Blocks until shutdown or Stop/Drain command.
    pub fn run(&mut self) {
        let tick_period = Duration::from_secs_f64(1.0 / self.config.tick_rate_hz as f64);
        let max_catchup = self.config.max_catchup_ticks;
        let mut accumulator = Duration::ZERO;
        let mut last_time = Instant::now();

        info!(
            island_id = %self.island_id,
            tick_rate = self.config.tick_rate_hz,
            "tick engine started"
        );

        while !self.shutdown.load(Ordering::Relaxed) {
            if self.drain_commands() {
                break;
            }

            let now = Instant::now();
            let frame_time = now - last_time;
            last_time = now;

            accumulator += frame_time;

            let max_accumulator = tick_period * max_catchup;
            if accumulator > max_accumulator {
                warn!(
                    island_id = %self.island_id,
                    tick = self.tick,
                    "tick overrun: capping at {} catch-up ticks",
                    max_catchup
                );
                accumulator = max_accumulator;
            }

            while accumulator >= tick_period {
                self.execute_tick();
                self.tick += 1;
                accumulator -= tick_period;

                if self.drain_commands() {
                    return;
                }
            }

            let sleep_time = tick_period - accumulator;
            if sleep_time > Duration::from_micros(500) {
                std::thread::sleep(sleep_time);
            }
        }

        info!(
            island_id = %self.island_id,
            tick = self.tick,
            "tick engine stopped"
        );
    }

    /// Check for pending commands. Returns true if the engine should stop.
    fn drain_commands(&self) -> bool {
        loop {
            match self.cmd_rx.try_recv() {
                Ok(IslandCommand::Tick) => {} // legacy no-op
                Ok(IslandCommand::Drain) | Ok(IslandCommand::Stop) => return true,
                Err(crossbeam_channel::TryRecvError::Empty) => return false,
                Err(crossbeam_channel::TryRecvError::Disconnected) => return true,
            }
        }
    }

    /// Execute one tick (6 phases). Does NOT advance the tick counter.
    fn execute_tick(&mut self) {
        self.effects_out.clear();
        self.timers.set_current_tick(self.tick);

        // Phase 1: Drain inputs
        let inputs = self.phase1_drain_inputs();

        // Phase 2: Fire timers
        let timer_messages = self.phase2_fire_timers();

        // Phase 3: Build per-entity message queues
        let actor_queues = self.phase3_build_queues(&inputs, &timer_messages);

        // Phase 4: Simulate
        let effect_batch = self.phase4_simulate(&actor_queues);

        // Phase 5: Batch effects
        self.phase5_batch_effects(effect_batch);

        // Phase 6: Compute deltas (state snapshot for async processing)
        // Full implementation sends DeltaWorkItem to Tokio runtime.
        // Wired up when transport layer is connected.
    }

    fn phase1_drain_inputs(&mut self) -> Vec<ClientInput> {
        let mut inputs = Vec::new();
        while let Ok(input) = self.input_rx.try_recv() {
            let last_seq = self
                .last_input_seq
                .entry(input.session_id.clone())
                .or_insert(0);
            if input.input_seq > *last_seq {
                *last_seq = input.input_seq;
                inputs.push(input);
            }
        }
        inputs
    }

    fn phase2_fire_timers(&mut self) -> Vec<(EntitySlot, String)> {
        self.timers.fire_elapsed(self.tick)
    }

    fn phase3_build_queues(
        &mut self,
        inputs: &[ClientInput],
        timer_messages: &[(EntitySlot, String)],
    ) -> BTreeMap<EntitySlot, Vec<TickMessage>> {
        let mut queues: BTreeMap<EntitySlot, Vec<TickMessage>> = BTreeMap::new();

        // Priority 1: Timers
        for (entity, name) in timer_messages {
            queues
                .entry(*entity)
                .or_default()
                .push(TickMessage::Timer { name: name.clone() });
        }

        // Priority 2: Bridge messages (separate channel, TODO for bridge integration)

        // Priority 3: Client inputs
        for input in inputs {
            queues
                .entry(input.entity_slot)
                .or_default()
                .push(TickMessage::Input {
                    session_id: input.session_id.clone(),
                    input_seq: input.input_seq,
                    payload: input.payload.clone(),
                });
        }

        // Priority 4: Deferred sends from previous tick
        let deferred = std::mem::take(&mut self.deferred_sends);
        for send in deferred {
            queues
                .entry(send.target)
                .or_default()
                .push(TickMessage::Deferred {
                    source: send.source,
                    payload: send.payload,
                });
        }

        queues
    }

    fn phase4_simulate(
        &mut self,
        actor_queues: &BTreeMap<EntitySlot, Vec<TickMessage>>,
    ) -> Vec<(EntitySlot, Vec<TickEffect>)> {
        let mut effect_batch = Vec::new();

        // Split borrows to satisfy the borrow checker:
        // we need &mut self.wasm, &mut self.entities, and &mut self.fault_tracker simultaneously.
        let entities = &mut self.entities;
        let wasm = &mut self.wasm;
        let fault_tracker = &mut self.fault_tracker;
        let tick = self.tick;

        for (entity_slot, messages) in actor_queues {
            if !fault_tracker.should_tick(entity_slot, tick) {
                continue;
            }

            if !entities.contains_key(entity_slot) {
                continue;
            }

            for msg in messages {
                let current_state = entities[entity_slot].state.clone();
                match wasm.call_handle_message(*entity_slot, &current_state, msg) {
                    Ok(result) => {
                        if let Some(e) = entities.get_mut(entity_slot) {
                            e.state = result.state;
                        }
                        if !result.effects.is_empty() {
                            effect_batch.push((*entity_slot, result.effects));
                        }
                        fault_tracker.record_success(entity_slot);
                    }
                    Err(trap) => {
                        warn!(
                            entity = entity_slot.0,
                            trap = %trap,
                            "WASM trap"
                        );
                        fault_tracker.record_fault(entity_slot, tick);
                        break;
                    }
                }
            }
        }

        effect_batch
    }

    fn phase5_batch_effects(&mut self, effect_batch: Vec<(EntitySlot, Vec<TickEffect>)>) {
        let mut persist_entities: Vec<(EntitySlot, Vec<u8>)> = Vec::new();
        let mut entities_to_remove: Vec<EntitySlot> = Vec::new();

        for (source_slot, effects) in effect_batch {
            for effect in effects {
                match effect {
                    TickEffect::Send { target, payload } => {
                        self.deferred_sends.push(DeferredSend {
                            source: source_slot,
                            target,
                            payload,
                        });
                    }
                    TickEffect::SendRemote { target, payload } => {
                        self.effects_out
                            .push(BridgeEffect::SendRemote { target, payload });
                    }
                    TickEffect::Persist => {
                        if let Some(entity) = self.entities.get(&source_slot) {
                            persist_entities.push((source_slot, entity.state.clone()));
                        }
                    }
                    TickEffect::SetTimer { name, delay_ms } => {
                        self.timers.set_timer(source_slot, name, delay_ms);
                    }
                    TickEffect::CancelTimer(name) => {
                        self.timers.cancel_timer(&source_slot, &name);
                    }
                    TickEffect::EmitTelemetry { event } => {
                        self.effects_out
                            .push(BridgeEffect::EmitTelemetry { event });
                    }
                    TickEffect::Reply(_payload) => {
                        // Route to client session (wired when transport is connected)
                    }
                    TickEffect::StopSelf => {
                        entities_to_remove.push(source_slot);
                    }
                }
            }
        }

        // Coalesce persist: one checkpoint per island per tick
        if !persist_entities.is_empty() {
            self.effects_out.push(BridgeEffect::Persist {
                entity_states: persist_entities,
            });
        }

        for slot in entities_to_remove {
            self.remove_entity(&slot);
        }
    }
}
