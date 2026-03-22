use crate::command::{
    ActivationError, IslandCommand, LifecycleError, ManagerCommand, ManagerMetrics,
};
use crate::config::ServerConfig;
use crate::island::handle::{IslandHandle, ThreadModel};
use crate::island::registry::IslandRegistry;
use crate::island::state_machine::IslandState;
use crate::tick::types::{BridgeMessage, ClientInput, NoopWasmExecutor, TickEngineConfig};
use crate::tick::TickEngine;
use crate::traits::Bridge;
use crate::types::{IslandId, IslandManifest, IslandSnapshot};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct IslandManager {
    config: ServerConfig,
    registry: IslandRegistry,
    cmd_rx: mpsc::Receiver<ManagerCommand>,
    shutdown: Arc<AtomicBool>,
    bridge: Arc<dyn Bridge>,
    passivated: FxHashMap<String, PassivatedIsland>,
}

struct PassivatedIsland {
    snapshot: IslandSnapshot,
    manifest: IslandManifest,
}

impl IslandManager {
    pub fn new(
        config: ServerConfig,
        cmd_rx: mpsc::Receiver<ManagerCommand>,
        bridge: Arc<dyn Bridge>,
    ) -> Self {
        Self {
            config,
            registry: IslandRegistry::new(),
            cmd_rx,
            shutdown: Arc::new(AtomicBool::new(false)),
            bridge,
            passivated: FxHashMap::default(),
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
                payload,
                reply,
            } => {
                let result = self.handle_bridge_message(&island_id, payload);
                let _ = reply.send(result);
            }
            ManagerCommand::PlayerInput { island_id, reply } => {
                let result = self.handle_player_input(&island_id);
                let _ = reply.send(result);
            }
        }
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

        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<IslandCommand>();
        let (input_tx, input_rx) = crossbeam_channel::bounded::<ClientInput>(256);
        let (_bridge_tx, bridge_rx) = crossbeam_channel::bounded::<BridgeMessage>(256);
        let island_id = manifest.island_id.clone();
        let passivate_when_empty = manifest.passivate_when_empty;
        let shutdown = self.shutdown.clone();

        let heartbeat = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let engine_heartbeat = heartbeat.clone();
        let engine_island_id = island_id.clone();
        let panicked = Arc::new(AtomicBool::new(false));
        let engine_panicked = panicked.clone();
        let join_handle = std::thread::spawn(move || {
            let config = TickEngineConfig::default();
            let wasm = Box::new(NoopWasmExecutor);
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

            if let Some(snap) = snapshot {
                engine.restore_from_snapshot(&snap);
            }

            let clean = engine.run();
            if !clean {
                engine_panicked.store(true, Ordering::Relaxed);
            }
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

    fn handle_drain(
        &mut self,
        island_id: &IslandId,
    ) -> Result<(), LifecycleError> {
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

    fn handle_stop(
        &mut self,
        island_id: &IslandId,
    ) -> Result<(), LifecycleError> {
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
        _payload: Vec<u8>,
    ) -> Result<(), LifecycleError> {
        if let Some(handle) = self.registry.get_mut(island_id) {
            if handle.player_count == 0 && handle.passivate_when_empty {
                let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
                handle.passivation_deadline = Some(Instant::now() + idle_timeout);
            }
            return Ok(());
        }

        if let Some(passivated) = self.passivated.remove(&island_id.0) {
            info!(%island_id, tick = passivated.snapshot.tick, "reactivating passivated island");
            self.spawn_island(passivated.manifest, Some(passivated.snapshot));
            return Ok(());
        }

        Err(LifecycleError::NotFound(island_id.clone()))
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
        let _ = handle.command_tx.send(IslandCommand::Passivate { snapshot_tx });
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
                self.passivated.insert(
                    island_id.0.clone(),
                    PassivatedIsland { snapshot, manifest },
                );
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
        }
    }
}

pub fn manager_channel(
    buffer: usize,
) -> (
    mpsc::Sender<ManagerCommand>,
    mpsc::Receiver<ManagerCommand>,
) {
    mpsc::channel(buffer)
}
