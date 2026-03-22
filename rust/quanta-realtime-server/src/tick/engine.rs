use crate::checkpoint::codec::{CheckpointEntity, CheckpointPayload};
use crate::checkpoint::writer::{CheckpointHandle, CheckpointRequest};
use crate::command::IslandCommand;
use crate::types::{EntitySlot, IslandId};

use super::fault::{ActorHealthState, FaultTracker, TrapResponse};
use super::timer::TimerManager;
use super::types::*;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// Maximum effects a single WASM `handle_message` call may produce.
const MAX_EFFECTS_PER_CALL: usize = 64;

/// Maximum deferred sends that can accumulate between ticks.
const MAX_DEFERRED_SENDS: usize = 4096;

/// Maximum entity count accepted from a passivation snapshot.
const MAX_SNAPSHOT_ENTITIES: usize = 10_000;

/// Maximum entity state size accepted from a passivation snapshot (1 MB).
const MAX_SNAPSHOT_ENTITY_STATE: usize = 1_024 * 1_024;

/// Below this threshold, busy-wait is cheaper than the OS scheduler overhead.
const MIN_SLEEP_DURATION: Duration = Duration::from_micros(500);

fn route_effects(
    effects: Vec<TickEffect>,
    bridge_cid: Option<CorrelationId>,
) -> (Vec<BridgeEffect>, Vec<TickEffect>) {
    let Some(cid) = bridge_cid else {
        return (Vec::new(), effects);
    };
    let mut bridge = Vec::new();
    let mut remaining = Vec::new();
    for effect in effects {
        if let TickEffect::Reply(payload) = effect {
            bridge.push(BridgeEffect::BridgeReply {
                correlation_id: cid,
                payload,
            });
        } else {
            remaining.push(effect);
        }
    }
    (bridge, remaining)
}

/// The core tick loop engine for a simulation island.
///
/// Executes a 6-phase tick loop with fixed timestep (Gaffer on Games pattern):
/// 1. Drain inputs (crossbeam MPSC → per-entity input buffer)
/// 2. Fire timers
/// 3. Build per-entity message queues (priority: Timer > Bridge > Input > Deferred)
/// 4. Simulate (WASM handle_message per entity, BTreeMap order)
/// 5. Batch effects (persist coalescing, deferred sends)
/// 6. Compute deltas (hand off to async runtime)

/// Checkpoint snapshot buffer entry. Stores entity state as plain types
/// (String instead of SessionId) to match the checkpoint codec format.
struct SnapshotEntry {
    state: Vec<u8>,
    owner_session: Option<String>,
}

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
    bridge_rx: crossbeam_channel::Receiver<BridgeMessage>,
    cmd_rx: crossbeam_channel::Receiver<IslandCommand>,
    effects_out: Vec<BridgeEffect>,
    last_input_seq: HashMap<SessionId, u32>,
    shutdown: Arc<AtomicBool>,
    // Checkpoint state
    checkpoint_handle: Option<CheckpointHandle>,
    checkpoint_interval_ticks: u64,
    last_checkpoint_tick: u64,
    snapshot_buffer: BTreeMap<EntitySlot, SnapshotEntry>,
    heartbeat: Arc<AtomicU64>,
    effect_tx: Option<crate::effect_io::EffectSender>,
}

impl TickEngine {
    pub fn new(
        island_id: IslandId,
        config: TickEngineConfig,
        wasm: Box<dyn WasmExecutor>,
        input_rx: crossbeam_channel::Receiver<ClientInput>,
        bridge_rx: crossbeam_channel::Receiver<BridgeMessage>,
        cmd_rx: crossbeam_channel::Receiver<IslandCommand>,
        shutdown: Arc<AtomicBool>,
        heartbeat: Arc<AtomicU64>,
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
            bridge_rx,
            cmd_rx,
            effects_out: Vec::new(),
            last_input_seq: HashMap::new(),
            shutdown,
            checkpoint_handle: None,
            checkpoint_interval_ticks: 0,
            last_checkpoint_tick: 0,
            snapshot_buffer: BTreeMap::new(),
            heartbeat,
            effect_tx: None,
        }
    }

    /// Restore entity states from a passivation snapshot, resuming at the stored tick.
    pub fn restore_from_snapshot(&mut self, snapshot: &crate::types::IslandSnapshot) {
        self.tick = snapshot.tick;
        match bitcode::decode::<Vec<(u32, Vec<u8>, Option<String>)>>(&snapshot.state) {
            Ok(entries) => {
                if entries.len() > MAX_SNAPSHOT_ENTITIES {
                    warn!(
                        island_id = %self.island_id,
                        count = entries.len(),
                        max = MAX_SNAPSHOT_ENTITIES,
                        "snapshot entity count exceeds limit, rejecting"
                    );
                    return;
                }
                for (slot_id, state, owner) in entries {
                    if state.len() > MAX_SNAPSHOT_ENTITY_STATE {
                        warn!(
                            island_id = %self.island_id,
                            slot = slot_id,
                            size = state.len(),
                            "snapshot entity state too large, skipping"
                        );
                        continue;
                    }
                    let init_state = state.clone();
                    self.entities.insert(
                        EntitySlot(slot_id),
                        EntityState {
                            slot: EntitySlot(slot_id),
                            state,
                            owner_session: owner.map(SessionId),
                            dirty: false,
                            init_state,
                            checkpoint_state: None,
                        },
                    );
                }
            }
            Err(e) => {
                warn!(
                    island_id = %self.island_id,
                    tick = snapshot.tick,
                    error = %e,
                    "failed to decode passivation snapshot, starting with empty state"
                );
            }
        }
    }

    pub fn set_effect_sender(&mut self, tx: crate::effect_io::EffectSender) {
        self.effect_tx = Some(tx);
    }

    pub fn add_entity(&mut self, slot: EntitySlot, state: Vec<u8>, owner: Option<SessionId>) {
        let init_state = state.clone();
        self.entities.insert(
            slot,
            EntityState {
                slot,
                state,
                owner_session: owner,
                dirty: true,
                init_state,
                checkpoint_state: None,
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

    fn flush_effects(&mut self) {
        if let Some(tx) = &self.effect_tx {
            let effects = std::mem::take(&mut self.effects_out);
            if !effects.is_empty() {
                tx.send_batch(effects);
            }
        }
    }

    pub fn set_timer(&mut self, entity: EntitySlot, name: String, delay_ms: u32) {
        self.timers.set_timer(entity, name, delay_ms);
    }

    pub fn cancel_timer(&mut self, entity: &EntitySlot, name: &str) -> bool {
        self.timers.cancel_timer(entity, name)
    }

    /// Attach a checkpoint writer. `interval_secs` controls periodic checkpoints (0 = disabled).
    pub fn set_checkpoint_handle(&mut self, handle: CheckpointHandle, interval_secs: u32) {
        self.checkpoint_interval_ticks =
            (interval_secs as u64) * (self.config.tick_rate_hz as u64);
        self.checkpoint_handle = Some(handle);
    }

    /// Restore engine state from a decoded checkpoint payload.
    /// Clears all transient state (deferred sends, timers, faults, input tracking).
    pub fn restore_from_checkpoint(&mut self, tick: u64, payload: &CheckpointPayload) {
        self.tick = tick;
        self.entities.clear();
        self.snapshot_buffer.clear();
        self.deferred_sends.clear();
        self.timers = TimerManager::new(self.config.tick_rate_hz);
        self.fault_tracker = FaultTracker::new();
        self.last_input_seq.clear();
        self.effects_out.clear();
        for entity in &payload.entities {
            let slot = EntitySlot(entity.slot);
            let state = entity.state.clone();
            let owner = entity.owner_session.clone();
            self.snapshot_buffer.insert(
                slot,
                SnapshotEntry {
                    state: state.clone(),
                    owner_session: owner.clone(),
                },
            );
            self.entities.insert(
                slot,
                EntityState {
                    slot,
                    init_state: state.clone(),
                    state,
                    owner_session: owner.map(SessionId),
                    dirty: false,
                    checkpoint_state: None,
                },
            );
        }
        self.last_checkpoint_tick = tick;
    }

    /// Build a checkpoint snapshot using the copy-on-update optimization.
    /// Only clones state for dirty entities; clean entities reuse the buffer.
    pub fn build_snapshot(&mut self) -> CheckpointPayload {
        for (slot, entity) in &mut self.entities {
            if entity.dirty {
                self.snapshot_buffer.insert(
                    *slot,
                    SnapshotEntry {
                        state: entity.state.clone(),
                        owner_session: entity.owner_session.as_ref().map(|s| s.0.clone()),
                    },
                );
                entity.dirty = false;
            }
        }

        // Remove entities that no longer exist
        self.snapshot_buffer
            .retain(|slot, _| self.entities.contains_key(slot));

        let entities = self
            .snapshot_buffer
            .iter()
            .map(|(slot, entry)| CheckpointEntity {
                slot: slot.0,
                state: entry.state.clone(),
                owner_session: entry.owner_session.clone(),
            })
            .collect();

        CheckpointPayload { entities }
    }

    pub fn tick(&mut self) {
        self.execute_tick();
        self.tick += 1;
    }

    pub fn tick_n(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    pub fn run(&mut self) -> bool {
        let tick_period = Duration::from_secs_f64(1.0 / self.config.tick_rate_hz as f64);
        let max_catchup = self.config.max_catchup_ticks;
        let mut accumulator = Duration::ZERO;
        let mut last_time = Instant::now();

        info!(
            island_id = %self.island_id,
            tick_rate = self.config.tick_rate_hz,
            "tick engine started"
        );

        'outer: while !self.shutdown.load(Ordering::Relaxed) {
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
                let tick_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.execute_tick();
                }));

                match tick_result {
                    Ok(()) => {
                        self.tick += 1;
                        self.heartbeat.store(self.tick, Ordering::Relaxed);
                        self.flush_effects();
                    }
                    Err(panic_info) => {
                        let bt = std::backtrace::Backtrace::force_capture();
                        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic_info.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "unknown panic".to_string()
                        };
                        tracing::error!(
                            island_id = %self.island_id,
                            tick = self.tick,
                            panic = %msg,
                            backtrace = %bt,
                            "island thread panicked during tick execution"
                        );
                        self.effects_out.clear();
                        self.effects_out.push(BridgeEffect::EmitTelemetry {
                            event: format!(
                                "island_panic:{}:tick={}",
                                self.island_id, self.tick
                            ),
                        });
                        self.flush_effects();
                        self.write_final_checkpoint();
                        return false;
                    }
                }

                accumulator -= tick_period;

                if self.drain_commands() {
                    break 'outer;
                }
            }

            let sleep_time = tick_period - accumulator;
            if sleep_time > MIN_SLEEP_DURATION {
                std::thread::sleep(sleep_time);
            }
        }

        self.write_final_checkpoint();

        info!(
            island_id = %self.island_id,
            tick = self.tick,
            "tick engine stopped"
        );
        true
    }

    fn drain_commands(&mut self) -> bool {
        loop {
            match self.cmd_rx.try_recv() {
                Ok(IslandCommand::Drain) | Ok(IslandCommand::Stop) => return true,
                Ok(IslandCommand::Passivate { snapshot_tx }) => {
                    self.send_passivation_snapshot(snapshot_tx);
                    return true;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return false,
                Err(crossbeam_channel::TryRecvError::Disconnected) => return true,
            }
        }
    }

    fn send_passivation_snapshot(
        &self,
        tx: crossbeam_channel::Sender<crate::types::IslandSnapshot>,
    ) {
        let state = self.serialize_entity_states();
        let snapshot = crate::types::IslandSnapshot {
            island_id: self.island_id.clone(),
            tick: self.tick,
            state,
        };
        let _ = tx.send(snapshot);
    }

    fn serialize_entity_states(&self) -> Vec<u8> {
        let entries: Vec<(u32, Vec<u8>, Option<String>)> = self
            .entities
            .iter()
            .map(|(slot, e)| (slot.0, e.state.clone(), e.owner_session.as_ref().map(|s| s.0.clone())))
            .collect();
        bitcode::encode(&entries)
    }

    fn execute_tick(&mut self) {
        self.effects_out.clear();
        self.timers.set_current_tick(self.tick);

        let inputs = self.phase1_drain_inputs();
        let timer_messages = self.phase2_fire_timers();
        let actor_queues = self.phase3_build_queues(&inputs, &timer_messages);
        let effect_batch = self.phase4_simulate(&actor_queues);
        self.phase5_batch_effects(effect_batch);
        self.check_checkpoint_triggers();
    }

    fn check_checkpoint_triggers(&mut self) {
        if self.checkpoint_handle.is_none() {
            return;
        }

        let has_persist = self
            .effects_out
            .iter()
            .any(|e| matches!(e, BridgeEffect::Persist { .. }));

        let periodic_due = self.checkpoint_interval_ticks > 0
            && self.tick >= self.last_checkpoint_tick + self.checkpoint_interval_ticks;

        if has_persist || periodic_due {
            self.write_checkpoint();
        }
    }

    fn write_checkpoint(&mut self) {
        if self.checkpoint_handle.is_none() {
            return;
        }
        let payload = self.build_snapshot();
        let data = crate::checkpoint::codec::encode_checkpoint(self.tick, &payload);
        if let Some(handle) = &self.checkpoint_handle {
            handle.try_send(CheckpointRequest {
                island_id: self.island_id.clone(),
                tick: self.tick,
                data,
                ack: None,
            });
        }
        self.last_checkpoint_tick = self.tick;
    }

    /// Synchronous final checkpoint for pre-passivation. Blocks until the write is acknowledged.
    ///
    /// # Panics
    /// Panics if called from within a Tokio async runtime. This method is designed
    /// to be called from the island's dedicated OS thread.
    fn write_final_checkpoint(&mut self) {
        if self.checkpoint_handle.is_none() {
            return;
        }
        let payload = self.build_snapshot();
        let data = crate::checkpoint::codec::encode_checkpoint(self.tick, &payload);
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        let req = CheckpointRequest {
            island_id: self.island_id.clone(),
            tick: self.tick,
            data,
            ack: Some(ack_tx),
        };
        let handle = self.checkpoint_handle.as_ref().unwrap();
        if handle.tx.blocking_send(req).is_err() {
            warn!(
                island_id = %self.island_id,
                tick = self.tick,
                "final checkpoint send failed: writer closed"
            );
            return;
        }
        if ack_rx.blocking_recv().is_err() {
            warn!(
                island_id = %self.island_id,
                tick = self.tick,
                "final checkpoint ack lost: writer dropped sender"
            );
        }
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

        for (entity, name) in timer_messages {
            queues
                .entry(*entity)
                .or_default()
                .push(TickMessage::Timer { name: name.clone() });
        }

        // Priority 2: Bridge messages
        while let Ok(msg) = self.bridge_rx.try_recv() {
            let tick_msg = match msg.kind {
                BridgeMessageKind::OneWay => TickMessage::Bridge {
                    payload: msg.payload,
                },
                BridgeMessageKind::Request { correlation_id } => TickMessage::BridgeRequest {
                    correlation_id,
                    payload: msg.payload,
                },
                BridgeMessageKind::SagaFailed { correlation_id } => {
                    TickMessage::SagaFailed { correlation_id }
                }
            };
            queues.entry(msg.target_entity).or_default().push(tick_msg);
        }

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
        let mut entities_to_evict = Vec::new();

        let entities = &mut self.entities;
        let wasm = &mut self.wasm;
        let fault_tracker = &mut self.fault_tracker;
        let effects_out = &mut self.effects_out;
        let tick = self.tick;

        for (entity_slot, messages) in actor_queues {
            if !fault_tracker.should_tick(entity_slot, tick) {
                continue;
            }

            if !entities.contains_key(entity_slot) {
                continue;
            }

            for msg in messages {
                let bridge_cid = if let TickMessage::BridgeRequest {
                    correlation_id, ..
                } = msg
                {
                    Some(*correlation_id)
                } else {
                    None
                };

                let current_state = entities[entity_slot].state.clone();
                match wasm.call_handle_message(*entity_slot, &current_state, msg) {
                    Ok(mut result) => {
                        if let Some(e) = entities.get_mut(entity_slot) {
                            e.dirty = e.state != result.state || e.dirty;
                            e.state = result.state;
                        }

                        if result.effects.len() > MAX_EFFECTS_PER_CALL {
                            warn!(
                                entity = entity_slot.0,
                                count = result.effects.len(),
                                max = MAX_EFFECTS_PER_CALL,
                                "effect budget exceeded, truncating"
                            );
                            result.effects.truncate(MAX_EFFECTS_PER_CALL);
                        }

                        if !result.effects.is_empty() {
                            let (bridge_effects, remaining) =
                                route_effects(result.effects, bridge_cid);
                            effects_out.extend(bridge_effects);
                            if !remaining.is_empty() {
                                effect_batch.push((*entity_slot, remaining));
                            }
                        }

                        fault_tracker.record_success(entity_slot);
                    }
                    Err(trap) => {
                        warn!(
                            entity = entity_slot.0,
                            trap = %trap,
                            "WASM trap"
                        );
                        let response = fault_tracker.record_fault(entity_slot, tick);
                        match response {
                            TrapResponse::Skip => {}
                            TrapResponse::Reset => {
                                if let Some(e) = entities.get_mut(entity_slot) {
                                    let restore = e
                                        .checkpoint_state
                                        .clone()
                                        .unwrap_or_else(|| e.init_state.clone());
                                    e.state = restore;
                                    warn!(entity = entity_slot.0, "entity reset to checkpoint");
                                }
                            }
                            TrapResponse::Recreate => {
                                if let Some(e) = entities.get_mut(entity_slot) {
                                    e.state = e.init_state.clone();
                                    e.checkpoint_state = None;
                                    warn!(entity = entity_slot.0, "entity recreated with init state");
                                }
                            }
                            TrapResponse::Evict => {
                                entities_to_evict.push(*entity_slot);
                                warn!(entity = entity_slot.0, "entity evicted");
                            }
                        }
                        break;
                    }
                }
            }
        }

        for slot in entities_to_evict {
            self.entities.remove(&slot);
            self.timers.clear_entity(&slot);
            self.effects_out
                .push(BridgeEffect::EntityEvicted { entity: slot });
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
                        if self.deferred_sends.len() < MAX_DEFERRED_SENDS {
                            self.deferred_sends.push(DeferredSend {
                                source: source_slot,
                                target,
                                payload,
                            });
                        } else {
                            warn!("deferred sends at capacity, dropping send");
                        }
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
                    TickEffect::Reply(_) => {
                        // Reply effects are routed in phase4 via route_effects().
                        // This arm should not be reachable.
                    }
                    TickEffect::RequestRemote { target, payload } => {
                        self.effects_out.push(BridgeEffect::RequestRemote {
                            source_entity: source_slot,
                            target,
                            payload,
                        });
                    }
                    TickEffect::FireAndForget { target, payload } => {
                        self.effects_out
                            .push(BridgeEffect::FireAndForget { target, payload });
                    }
                    TickEffect::StopSelf => {
                        entities_to_remove.push(source_slot);
                    }
                }
            }
        }

        if !persist_entities.is_empty() {
            for (slot, state) in &persist_entities {
                if let Some(entity) = self.entities.get_mut(slot) {
                    entity.checkpoint_state = Some(state.clone());
                }
            }
            self.effects_out.push(BridgeEffect::Persist {
                entity_states: persist_entities,
            });
        }

        for slot in entities_to_remove {
            self.remove_entity(&slot);
        }
    }
}
