use crate::command::{
    ActivationError, ClientConnectedError, IslandCommand, LifecycleError, ManagerCommand,
    ManagerMetrics, RegisterClientError, RegisterClientResult, ZoneTransferError,
};
use crate::config::ServerConfig;
use crate::effect_io;
use crate::fanout::{fanout_loop, FanoutCommand, FanoutFactory};
use crate::island::handle::{IslandHandle, ThreadModel};
use crate::island::registry::IslandRegistry;
use crate::island::state_machine::IslandState;
use crate::reconnect::ConnectedClient;
use crate::server::ExecutorFactory;
use crate::session::Session;
use crate::tick::types::{
    BridgeEffect, BridgeMessage, ClientInput, TickEngineConfig, TickSnapshot,
};
use crate::tick::TickEngine;
use crate::traits::Bridge;
use crate::types::{ClientIndex, EntitySlot, IslandId, IslandManifest, IslandSnapshot};
use crate::zone_transfer::{ZoneTransferManager, ZoneTransferToken};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

pub struct IslandManager {
    config: ServerConfig,
    registry: IslandRegistry,
    cmd_rx: mpsc::Receiver<ManagerCommand>,
    shutdown: Arc<AtomicBool>,
    bridge: Arc<dyn Bridge>,
    passivated: FxHashMap<String, PassivatedIsland>,
    zone_transfer: Option<ZoneTransferManager>,
    /// Phase 1 placeholder — flat list of authenticated clients.
    /// Phase 3 replaces with per-island HashMap<SessionId, Arc<dyn Session>>.
    connected_clients: Vec<ConnectedClient>,
    max_clients: usize,
    /// Constructs a fresh `WasmExecutor` for each island this manager spawns.
    executor_factory: ExecutorFactory,
    /// Phase 3: optional fanout factory. If `None`, no fanout task is spawned
    /// per island (engine still emits snapshots, they're dropped).
    fanout_factory: Option<FanoutFactory>,
    /// Phase 3: entity-slot allocator per island id.
    /// `next` grows monotonically; `free` holds reclaimed slot ids from
    /// departed clients so steady-state churn never exhausts the u16 range.
    slot_allocators: FxHashMap<String, SlotAllocator>,
    /// Phase 3: per-island fanout command channel, keyed by island id string.
    fanout_tx: FxHashMap<String, mpsc::Sender<FanoutCommand>>,
    /// Phase 3: `session_id -> (island_id, slot, client_index)` for deregister lookup.
    client_registry: FxHashMap<u64, (IslandId, EntitySlot, ClientIndex)>,
    /// Propagated to fanout tasks so they exit on shutdown.
    shutdown_watch: watch::Receiver<bool>,
}

struct PassivatedIsland {
    snapshot: IslandSnapshot,
    manifest: IslandManifest,
}

/// Per-island entity-slot allocator. Hands out slot ids from the free list
/// first (LIFO reuse) then from the monotonic counter.
#[derive(Default)]
struct SlotAllocator {
    next: u32,
    free: Vec<u32>,
}

impl SlotAllocator {
    /// Returns the next slot id, or `None` if it would exceed `u16::MAX`.
    /// `ClientIndex` narrows `u32` → `u16`; rejecting above the cap prevents
    /// silent collision with an earlier client still registered in fanout.
    fn allocate(&mut self) -> Option<u32> {
        if let Some(s) = self.free.pop() {
            return Some(s);
        }
        if self.next > u16::MAX as u32 {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        Some(slot)
    }

    fn release(&mut self, slot: u32) {
        self.free.push(slot);
    }
}

impl IslandManager {
    pub fn new(
        config: ServerConfig,
        cmd_rx: mpsc::Receiver<ManagerCommand>,
        bridge: Arc<dyn Bridge>,
        executor_factory: ExecutorFactory,
        fanout_factory: Option<FanoutFactory>,
        shutdown_watch: watch::Receiver<bool>,
    ) -> Self {
        let zone_transfer = config
            .zone_transfer
            .as_ref()
            .map(|ztc| ZoneTransferManager::new(ztc.clone()));
        Self {
            config,
            registry: IslandRegistry::new(),
            cmd_rx,
            shutdown: Arc::new(AtomicBool::new(false)),
            bridge,
            passivated: FxHashMap::default(),
            zone_transfer,
            connected_clients: Vec::new(),
            max_clients: 4096,
            executor_factory,
            fanout_factory,
            slot_allocators: FxHashMap::default(),
            fanout_tx: FxHashMap::default(),
            client_registry: FxHashMap::default(),
            shutdown_watch,
        }
    }

    pub async fn run(&mut self) {
        info!("island manager started");
        let mut idle_check = tokio::time::interval(Duration::from_secs(1));
        idle_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => self.handle_command(cmd),
                        None => break,
                    }
                }
                _ = idle_check.tick() => {
                    self.check_passivation();
                    self.check_zone_transfer_timeouts();
                }
            }
        }

        self.shutdown.store(true, Ordering::Relaxed);
        info!("island manager shutting down");
    }

    fn handle_command(&mut self, cmd: ManagerCommand) {
        match cmd {
            ManagerCommand::Activate { manifest, reply } => {
                let result = self.handle_activate(manifest);
                let _ = reply.send(result);
            }
            ManagerCommand::Drain { island_id, reply } => {
                let result = self.handle_drain(&island_id);
                let _ = reply.send(result);
            }
            ManagerCommand::Stop { island_id, reply } => {
                let result = self.handle_stop(&island_id);
                let _ = reply.send(result);
            }
            ManagerCommand::GetMetrics { reply } => {
                let _ = reply.send(self.collect_metrics());
            }
            ManagerCommand::PlayerJoined { island_id, reply } => {
                let result = self.handle_player_joined(&island_id);
                let _ = reply.send(result);
            }
            ManagerCommand::PlayerLeft { island_id, reply } => {
                let result = self.handle_player_left(&island_id);
                let _ = reply.send(result);
            }
            ManagerCommand::BridgeMessage {
                island_id,
                message,
                reply,
            } => {
                let result = self.handle_bridge_message(&island_id, message);
                let _ = reply.send(result);
            }
            ManagerCommand::PlayerInput { island_id, reply } => {
                let result = self.handle_player_input(&island_id);
                let _ = reply.send(result);
            }
            ManagerCommand::PrepareZoneTransfer {
                player_id,
                source_island,
                target_island,
                position,
                velocity,
                buffs,
                reply,
            } => {
                let result = self.handle_prepare_zone_transfer(
                    player_id,
                    &source_island,
                    &target_island,
                    position,
                    velocity,
                    buffs,
                );
                let _ = reply.send(result);
            }
            ManagerCommand::AcceptZoneTransfer {
                token_bytes,
                target_island,
                reply,
            } => {
                let result = self.handle_accept_zone_transfer(&token_bytes, &target_island);
                let _ = reply.send(result);
            }
            ManagerCommand::ClientConnected { client, reply } => {
                let result = self.handle_client_connected(client);
                let _ = reply.send(result);
            }
            ManagerCommand::ClientDisconnected { session_id } => {
                self.handle_client_disconnected(session_id);
            }
            ManagerCommand::RegisterClient {
                island_id,
                session_id,
                session,
                reply,
            } => {
                let result = self.handle_register_client(&island_id, session_id, session);
                let _ = reply.send(result);
            }
            ManagerCommand::DeregisterClient {
                island_id,
                session_id,
            } => {
                self.handle_deregister_client(&island_id, session_id);
            }
            ManagerCommand::AllocateEntitySlot { island_id, reply } => {
                let result = self.handle_allocate_entity_slot(&island_id);
                let _ = reply.send(result);
            }
            ManagerCommand::SubscribeSnapshots { island_id, reply } => {
                let result = self.handle_subscribe_snapshots(&island_id);
                let _ = reply.send(result);
            }
        }
    }

    fn handle_subscribe_snapshots(
        &mut self,
        island_id: &IslandId,
    ) -> Result<crossbeam_channel::Receiver<TickSnapshot>, LifecycleError> {
        let handle = self
            .registry
            .get(island_id)
            .ok_or_else(|| LifecycleError::NotFound(island_id.clone()))?;
        if handle.state != IslandState::Running {
            return Err(LifecycleError::InvalidTransition(format!(
                "island {} not running",
                island_id
            )));
        }
        // Bounded to tolerate a slow subscriber without pinning memory
        // unboundedly. The swarm mind reads every tick so 4 is plenty.
        let (tx, rx) = crossbeam_channel::bounded::<TickSnapshot>(4);
        let _ = handle
            .command_tx
            .send(IslandCommand::AddSnapshotSubscriber { tx });
        Ok(rx)
    }

    fn handle_allocate_entity_slot(
        &mut self,
        island_id: &IslandId,
    ) -> Result<
        (EntitySlot, crossbeam_channel::Sender<ClientInput>),
        RegisterClientError,
    > {
        let handle = self
            .registry
            .get(island_id)
            .ok_or_else(|| RegisterClientError::IslandNotFound(island_id.clone()))?;
        if handle.state != IslandState::Running {
            return Err(RegisterClientError::IslandNotRunning(island_id.clone()));
        }
        let allocator = self
            .slot_allocators
            .entry(island_id.0.clone())
            .or_default();
        let slot_id = allocator
            .allocate()
            .ok_or(RegisterClientError::AtSlotCapacity)?;
        let slot = EntitySlot(slot_id);
        let _ = handle.command_tx.send(IslandCommand::AddEntity {
            slot,
            initial_state: Vec::new(),
            owner: Some(crate::tick::SessionId::from(
                format!("npc-{}", slot_id).as_str(),
            )),
        });
        Ok((slot, handle.input_tx.clone()))
    }

    fn handle_client_connected(
        &mut self,
        client: ConnectedClient,
    ) -> Result<u64, ClientConnectedError> {
        if self.connected_clients.len() >= self.max_clients {
            return Err(ClientConnectedError::AtCapacity {
                max: self.max_clients,
            });
        }
        let session_id = client.session_id;
        info!(
            session_id,
            total_connected = self.connected_clients.len() + 1,
            "client connected"
        );
        self.connected_clients.push(client);
        Ok(session_id)
    }

    fn handle_client_disconnected(&mut self, session_id: u64) {
        let before = self.connected_clients.len();
        self.connected_clients
            .retain(|c| c.session_id != session_id);
        if self.connected_clients.len() < before {
            info!(
                session_id,
                total_connected = self.connected_clients.len(),
                "client disconnected"
            );
        }
    }

    fn handle_register_client(
        &mut self,
        island_id: &IslandId,
        session_id: u64,
        session: Arc<dyn Session>,
    ) -> RegisterClientResult {
        let handle = self
            .registry
            .get(island_id)
            .ok_or_else(|| RegisterClientError::IslandNotFound(island_id.clone()))?;
        if handle.state != IslandState::Running {
            return Err(RegisterClientError::IslandNotRunning(island_id.clone()));
        }

        let allocator = self.slot_allocators.entry(island_id.0.clone()).or_default();
        let slot_id = allocator
            .allocate()
            .ok_or(RegisterClientError::AtSlotCapacity)?;
        let slot = EntitySlot(slot_id);
        // `slot_id <= u16::MAX` is guaranteed by `SlotAllocator::allocate`.
        let client_index = ClientIndex(slot_id as u16);

        let _ = handle.command_tx.send(IslandCommand::AddEntity {
            slot,
            initial_state: Vec::new(),
            owner: Some(crate::tick::SessionId::from(
                session_id.to_string().as_str(),
            )),
        });
        let input_tx = handle.input_tx.clone();

        if let Some(fan) = self.fanout_tx.get(&island_id.0) {
            if fan
                .try_send(FanoutCommand::ClientJoined {
                    client_index,
                    entity_slot: slot,
                    session,
                })
                .is_err()
            {
                error!(
                    %island_id,
                    session_id,
                    slot = slot.0,
                    "fanout ClientJoined dropped — client will tick without receiving datagrams"
                );
            }
        }

        self.client_registry
            .insert(session_id, (island_id.clone(), slot, client_index));
        crate::metrics::METRICS.clients_connected.inc();

        info!(%island_id, session_id, slot = slot.0, "client registered with island");
        Ok((slot, client_index, input_tx))
    }

    fn handle_deregister_client(&mut self, island_id: &IslandId, session_id: u64) {
        let Some((_, slot, client_index)) = self.client_registry.remove(&session_id) else {
            return;
        };
        if let Some(handle) = self.registry.get(island_id) {
            let _ = handle.command_tx.send(IslandCommand::RemoveEntity { slot });
        }
        if let Some(fan) = self.fanout_tx.get(&island_id.0) {
            if fan
                .try_send(FanoutCommand::ClientLeft { client_index })
                .is_err()
            {
                warn!(
                    %island_id,
                    session_id,
                    "fanout ClientLeft dropped — fanout will retain stale pacer until island stops"
                );
            }
        }
        if let Some(allocator) = self.slot_allocators.get_mut(&island_id.0) {
            allocator.release(slot.0);
        }
        crate::metrics::METRICS.clients_connected.dec();
        info!(%island_id, session_id, slot = slot.0, "client deregistered from island");
    }

    fn handle_activate(&mut self, manifest: IslandManifest) -> Result<(), ActivationError> {
        if self.registry.contains(&manifest.island_id) {
            return Err(ActivationError::DuplicateIsland(manifest.island_id.clone()));
        }
        if self.registry.active_count() >= self.config.max_islands {
            return Err(ActivationError::AtCapacity {
                max: self.config.max_islands,
            });
        }

        self.spawn_island(manifest, None);
        Ok(())
    }

    /// Spawn an island thread. If `snapshot` is provided, the engine restores from it.
    fn spawn_island(&mut self, manifest: IslandManifest, snapshot: Option<IslandSnapshot>) {
        let thread_model = if manifest.entity_count >= self.config.entity_threshold {
            ThreadModel::Dedicated
        } else {
            ThreadModel::Pooled
        };

        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<IslandCommand>(32);
        // Sized to hold ~4 ticks of input bursts at the current particle-
        // world demo scale (300 swarm NPCs + N players, 30 Hz tick). The
        // old 256-slot budget overflows once NPC count exceeds ~250 and
        // the swarm task fires twice between engine drains — dropped
        // inputs manifest as "clusters freeze" (NPCs missing a tick-step)
        // AND "player rubber-bands on move" (your WASD input gets evicted
        // too, server never acks, `pending` balloons, drift spikes on the
        // next successful burst).
        let (input_tx, input_rx) = crossbeam_channel::bounded::<ClientInput>(4096);
        let (bridge_tx, bridge_rx) = crossbeam_channel::bounded::<BridgeMessage>(4096);
        let island_id = manifest.island_id.clone();
        let passivate_when_empty = manifest.passivate_when_empty;
        let shutdown = self.shutdown.clone();

        let (effect_tx, effect_rx) = effect_io::effect_channel();
        let (snap_tx, snap_rx) = crossbeam_channel::bounded::<TickSnapshot>(4);

        let heartbeat = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let engine_heartbeat = heartbeat.clone();
        let engine_island_id = island_id.clone();
        let panicked = Arc::new(AtomicBool::new(false));
        let engine_panicked = panicked.clone();
        let factory = self.executor_factory.clone();
        let engine_snap_tx = snap_tx;
        let engine_tick_rate_hz = self.config.tick_rate_hz;
        let join_handle = std::thread::spawn(move || {
            let config = TickEngineConfig {
                tick_rate_hz: engine_tick_rate_hz,
                ..TickEngineConfig::default()
            };
            let wasm = factory();
            let mut engine = TickEngine::new(
                engine_island_id,
                config,
                wasm,
                input_rx,
                bridge_rx,
                cmd_rx,
                shutdown,
                engine_heartbeat,
            );
            engine.set_effect_sender(effect_tx);
            engine.set_snapshot_sender(engine_snap_tx);

            if let Some(snap) = snapshot {
                engine.restore_from_snapshot(&snap);
            }

            let clean = engine.run();
            if !clean {
                engine_panicked.store(true, Ordering::Relaxed);
            }
        });

        // Spawn per-island fanout task, if a factory is configured.
        if let Some(factory) = self.fanout_factory.clone() {
            let fanout = factory();
            // Sized to absorb burst reconnects without dropping ClientJoined.
            // A single fanout tick polls all pending commands, so the 5ms
            // poll interval drains this quickly in steady state.
            let (fan_cmd_tx, fan_cmd_rx) = mpsc::channel::<FanoutCommand>(1024);
            let shutdown_rx = self.shutdown_watch.clone();
            tokio::spawn(fanout_loop(fanout, snap_rx, fan_cmd_rx, shutdown_rx));
            self.fanout_tx.insert(island_id.0.clone(), fan_cmd_tx);
        } else {
            // Drop receiver — engine's snapshot try_send becomes Err(Disconnected)
            // and is ignored. Keeps the engine code path uniform whether or
            // not a fanout is configured.
            drop(snap_rx);
        }

        // Spawn effect consumer task — routes effects to their handlers.
        // Exits when the island thread drops the EffectSender.
        let effect_island_id = island_id.clone();
        let effect_bridge = self.bridge.clone();
        tokio::spawn(async move {
            effect_io::run_effect_drain(effect_rx, move |effect| {
                handle_effect(&effect_island_id, &*effect_bridge, effect);
            })
            .await;
        });

        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
        let passivation_deadline = if passivate_when_empty {
            Some(Instant::now() + idle_timeout)
        } else {
            None
        };

        let handle = IslandHandle {
            island_id: island_id.clone(),
            state: IslandState::Initializing,
            thread_model,
            entity_count: manifest.entity_count,
            command_tx: cmd_tx,
            input_tx,
            bridge_tx,
            join_handle: Some(join_handle),
            manifest,
            player_count: 0,
            passivation_deadline,
            passivate_when_empty,
            heartbeat,
            panicked,
        };

        self.registry.insert(handle);

        let h = self.registry.get_mut(&island_id).unwrap();
        h.state = h.state.transition(IslandState::Running).unwrap();

        info!(%island_id, ?thread_model, "island activated");
    }

    fn handle_drain(&mut self, island_id: &IslandId) -> Result<(), LifecycleError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| LifecycleError::NotFound(island_id.clone()))?;

        handle.state = handle
            .state
            .transition(IslandState::Draining)
            .map_err(|e| LifecycleError::InvalidTransition(e.to_string()))?;

        let _ = handle.command_tx.send(IslandCommand::Drain);
        info!(%island_id, "island draining");

        self.finish_stop(island_id);
        Ok(())
    }

    fn handle_stop(&mut self, island_id: &IslandId) -> Result<(), LifecycleError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| LifecycleError::NotFound(island_id.clone()))?;

        if handle.state == IslandState::Running {
            handle.state = handle
                .state
                .transition(IslandState::Draining)
                .map_err(|e| LifecycleError::InvalidTransition(e.to_string()))?;
        }

        let _ = handle.command_tx.send(IslandCommand::Stop);
        info!(%island_id, "island stopping");

        self.finish_stop(island_id);
        Ok(())
    }

    fn handle_player_joined(&mut self, island_id: &IslandId) -> Result<(), LifecycleError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| LifecycleError::NotFound(island_id.clone()))?;

        handle.player_count += 1;
        handle.passivation_deadline = None;

        Ok(())
    }

    fn handle_player_left(&mut self, island_id: &IslandId) -> Result<(), LifecycleError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| LifecycleError::NotFound(island_id.clone()))?;

        handle.player_count = handle.player_count.saturating_sub(1);

        if handle.player_count == 0 && handle.passivate_when_empty {
            let grace = Duration::from_secs(self.config.grace_period_secs);
            handle.passivation_deadline = Some(Instant::now() + grace);
        }

        Ok(())
    }

    fn handle_player_input(&mut self, island_id: &IslandId) -> Result<(), LifecycleError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| LifecycleError::NotFound(island_id.clone()))?;

        if handle.player_count == 0 && handle.passivate_when_empty {
            let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
            handle.passivation_deadline = Some(Instant::now() + idle_timeout);
        }

        Ok(())
    }

    fn handle_bridge_message(
        &mut self,
        island_id: &IslandId,
        message: BridgeMessage,
    ) -> Result<(), LifecycleError> {
        if let Some(handle) = self.registry.get_mut(island_id) {
            if handle.player_count == 0 && handle.passivate_when_empty {
                let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
                handle.passivation_deadline = Some(Instant::now() + idle_timeout);
            }
            let _ = handle.bridge_tx.try_send(message);
            return Ok(());
        }

        if let Some(passivated) = self.passivated.remove(&island_id.0) {
            info!(%island_id, tick = passivated.snapshot.tick, "reactivating passivated island");
            self.spawn_island(passivated.manifest, Some(passivated.snapshot));
            return Ok(());
        }

        Err(LifecycleError::NotFound(island_id.clone()))
    }

    fn handle_prepare_zone_transfer(
        &mut self,
        player_id: String,
        source_island: &IslandId,
        target_island: &IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        buffs: Vec<crate::zone_transfer::BuffState>,
    ) -> Result<Vec<u8>, ZoneTransferError> {
        if self.zone_transfer.is_none() {
            return Err(ZoneTransferError::NotConfigured);
        }

        let handle = self
            .registry
            .get(source_island)
            .ok_or_else(|| ZoneTransferError::SourceNotFound(source_island.clone()))?;
        if handle.state != IslandState::Running {
            return Err(ZoneTransferError::SourceNotRunning(source_island.clone()));
        }

        let zt = self.zone_transfer.as_mut().unwrap();
        let token = zt.prepare_transfer(
            player_id,
            source_island.clone(),
            target_island.clone(),
            position,
            velocity,
            buffs,
        )?;

        if let Some(h) = self.registry.get_mut(source_island) {
            h.player_count = h.player_count.saturating_sub(1);
            if h.player_count == 0 && h.passivate_when_empty {
                let grace = Duration::from_secs(self.config.grace_period_secs);
                h.passivation_deadline = Some(Instant::now() + grace);
            }
        }

        info!(
            player_id = token.player_id.as_str(),
            source = %source_island,
            target = %target_island,
            "zone transfer prepared"
        );
        Ok(token.to_bytes())
    }

    fn handle_accept_zone_transfer(
        &mut self,
        token_bytes: &[u8],
        target_island: &IslandId,
    ) -> Result<crate::zone_transfer::TransferredPlayer, ZoneTransferError> {
        if self.zone_transfer.is_none() {
            return Err(ZoneTransferError::NotConfigured);
        }

        let handle = self
            .registry
            .get(target_island)
            .ok_or_else(|| ZoneTransferError::TargetNotFound(target_island.clone()))?;
        if handle.state != IslandState::Running {
            return Err(ZoneTransferError::TargetNotRunning(target_island.clone()));
        }

        let token =
            ZoneTransferToken::from_bytes(token_bytes).map_err(ZoneTransferError::Transfer)?;
        let zt = self.zone_transfer.as_mut().unwrap();
        let transferred = zt.accept_transfer(&token, target_island)?;

        if let Some(h) = self.registry.get_mut(target_island) {
            h.player_count += 1;
            h.passivation_deadline = None;
        }

        info!(
            player_id = transferred.player_id.as_str(),
            target = %target_island,
            "zone transfer accepted"
        );
        Ok(transferred)
    }

    fn check_zone_transfer_timeouts(&mut self) {
        if let Some(zt) = &mut self.zone_transfer {
            let rolled_back = zt.check_timeouts();
            for player_id in &rolled_back {
                info!(player_id, "zone transfer timed out, rolled back");
            }
        }
    }

    fn check_passivation(&mut self) {
        let now = Instant::now();
        let mut to_passivate = Vec::new();

        for (id, handle) in self.registry.iter() {
            if handle.state != IslandState::Running {
                continue;
            }
            if let Some(deadline) = handle.passivation_deadline {
                if now >= deadline {
                    to_passivate.push(id.clone());
                }
            }
        }

        for island_id in to_passivate {
            self.passivate_island(&island_id);
        }
    }

    /// Execute the passivation sequence for an island:
    /// Draining -> complete tick -> checkpoint -> notify bridge -> release thread -> Stopped -> remove.
    fn passivate_island(&mut self, island_id: &IslandId) {
        let handle = match self.registry.get_mut(island_id) {
            Some(h) => h,
            None => return,
        };

        match handle.state.transition(IslandState::Draining) {
            Ok(new_state) => handle.state = new_state,
            Err(_) => return,
        }

        let (snapshot_tx, snapshot_rx) = crossbeam_channel::bounded(1);
        let _ = handle
            .command_tx
            .send(IslandCommand::Passivate { snapshot_tx });
        let manifest = handle.manifest.clone();
        let jh = handle.join_handle.take();

        info!(%island_id, "island passivating");

        let panicked = match jh {
            Some(jh) => jh.join().is_err(),
            None => false,
        };

        if panicked {
            error!(%island_id, "island thread panicked during passivation");
            self.registry.remove(island_id);
            return;
        }

        match snapshot_rx.try_recv() {
            Ok(snapshot) => {
                info!(%island_id, checkpoint_tick = snapshot.tick, "island passivated");
                self.bridge
                    .report_island_passivated(island_id, snapshot.tick);
                self.passivated
                    .insert(island_id.0.clone(), PassivatedIsland { snapshot, manifest });
            }
            Err(_) => {
                error!(%island_id, "failed to receive passivation snapshot");
            }
        }

        if let Some(handle) = self.registry.get_mut(island_id) {
            if let Ok(s) = handle.state.transition(IslandState::Stopped) {
                handle.state = s;
            }
        }
        self.registry.remove(island_id);
    }

    fn finish_stop(&mut self, island_id: &IslandId) {
        if let Some(mut handle) = self.registry.remove(island_id) {
            if let Some(jh) = handle.join_handle.take() {
                let _ = jh.join();
            }
            info!(%island_id, "island stopped");
        }
    }

    fn collect_metrics(&self) -> ManagerMetrics {
        ManagerMetrics {
            active_islands: self.registry.active_count(),
            total_islands: self.registry.len(),
            total_entities: self.registry.total_entities(),
            connected_clients: self.connected_clients.len(),
        }
    }
}

pub fn manager_channel(
    buffer: usize,
) -> (mpsc::Sender<ManagerCommand>, mpsc::Receiver<ManagerCommand>) {
    mpsc::channel(buffer)
}

/// Test-only accessor: returns the number of entries in `client_registry`.
///
/// Used by `reader_panic_observable_test` to assert whether a dead reader
/// task's entity slot has been reclaimed. The field is private to enforce
/// the command-channel interface in production; exposed only under the
/// same gate as `ZoneTransferConfig::for_testing()` so integration test
/// binaries can call it without affecting release builds.
#[cfg(any(test, feature = "test-utils"))]
impl IslandManager {
    /// Returns the number of session entries currently tracked in
    /// `client_registry`.  Used by tests to detect slot leaks.
    pub fn client_registry_len(&self) -> usize {
        self.client_registry.len()
    }

    /// Drain exactly one pending command from the internal channel and
    /// process it synchronously.  Returns `true` if a command was processed,
    /// `false` if the channel was empty.
    ///
    /// Lets tests drive the manager step-by-step without spawning a
    /// background task, which in turn lets them call `client_registry_len()`
    /// between steps.
    pub fn process_one_command(&mut self) -> bool {
        match self.cmd_rx.try_recv() {
            Ok(cmd) => {
                self.handle_command(cmd);
                true
            }
            Err(_) => false,
        }
    }
}

/// Route a single BridgeEffect to the appropriate handler.
///
/// Currently handles Persist (via checkpoint — TODO: wire checkpoint handle),
/// EmitTelemetry (log), and EntityEvicted (log). Remote sends and bridge
/// replies require Bridge trait extensions (future tickets).
fn handle_effect(island_id: &IslandId, _bridge: &dyn Bridge, effect: BridgeEffect) {
    match effect {
        BridgeEffect::Persist { entity_states } => {
            info!(
                %island_id,
                entity_count = entity_states.len(),
                "persist effect received (checkpoint routing TODO)"
            );
        }
        BridgeEffect::EmitTelemetry { event } => {
            info!(%island_id, %event, "telemetry");
        }
        BridgeEffect::EntityEvicted { entity } => {
            info!(%island_id, ?entity, "entity evicted");
        }
        BridgeEffect::ZoneTransferRequest {
            player_id,
            source_entity,
            target_zone,
            ..
        } => {
            info!(
                %island_id,
                %player_id,
                ?source_entity,
                %target_zone,
                "zone transfer request from entity"
            );
        }
        BridgeEffect::SendRemote { target, .. } => {
            info!(%island_id, %target, "remote send (bridge routing TODO)");
        }
        BridgeEffect::RequestRemote { target, .. } => {
            info!(%island_id, %target, "remote request (bridge routing TODO)");
        }
        BridgeEffect::FireAndForget { target, .. } => {
            info!(%island_id, %target, "fire-and-forget (bridge routing TODO)");
        }
        BridgeEffect::BridgeReply { correlation_id, .. } => {
            info!(%island_id, ?correlation_id, "bridge reply (routing TODO)");
        }
    }
}
