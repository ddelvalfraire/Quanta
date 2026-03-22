use crate::command::{
    ActivationError, DrainError, IslandCommand, ManagerCommand, ManagerMetrics,
};
use crate::config::ServerConfig;
use crate::island::handle::{IslandHandle, ThreadModel};
use crate::island::registry::IslandRegistry;
use crate::island::state_machine::IslandState;
use crate::types::IslandManifest;
use tokio::sync::mpsc;
use tracing::info;

pub struct IslandManager {
    config: ServerConfig,
    registry: IslandRegistry,
    cmd_rx: mpsc::Receiver<ManagerCommand>,
}

impl IslandManager {
    pub fn new(config: ServerConfig, cmd_rx: mpsc::Receiver<ManagerCommand>) -> Self {
        Self {
            config,
            registry: IslandRegistry::new(),
            cmd_rx,
        }
    }

    /// Run the manager loop, processing commands until the channel closes.
    pub async fn run(&mut self) {
        info!("island manager started");
        while let Some(cmd) = self.cmd_rx.recv().await {
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
            }
        }
        info!("island manager shutting down");
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

        let thread_model = if manifest.entity_count >= self.config.entity_threshold {
            ThreadModel::Dedicated
        } else {
            ThreadModel::Pooled
        };

        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<IslandCommand>();
        let island_id = manifest.island_id.clone();

        let join_handle = std::thread::spawn(move || {
            island_thread_loop(cmd_rx);
        });

        let handle = IslandHandle {
            island_id: island_id.clone(),
            state: IslandState::Initializing,
            thread_model,
            entity_count: manifest.entity_count,
            command_tx: cmd_tx,
            join_handle: Some(join_handle),
        };

        self.registry.insert(handle);

        // Transition to Running immediately (init is a no-op stub for now).
        let h = self.registry.get_mut(&island_id).unwrap();
        h.state = h.state.transition(IslandState::Running).unwrap();

        info!(%island_id, ?thread_model, entities = manifest.entity_count, "island activated");
        Ok(())
    }

    fn handle_drain(
        &mut self,
        island_id: &crate::types::IslandId,
    ) -> Result<(), DrainError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| DrainError::NotFound(island_id.clone()))?;

        handle.state = handle
            .state
            .transition(IslandState::Draining)
            .map_err(|e| DrainError::InvalidTransition(e.to_string()))?;

        let _ = handle.command_tx.send(IslandCommand::Drain);
        info!(%island_id, "island draining");

        // Join the thread to complete the stop transition.
        self.finish_stop(island_id);
        Ok(())
    }

    fn handle_stop(
        &mut self,
        island_id: &crate::types::IslandId,
    ) -> Result<(), DrainError> {
        let handle = self
            .registry
            .get_mut(island_id)
            .ok_or_else(|| DrainError::NotFound(island_id.clone()))?;

        // Allow stop from Running (go through Draining first) or from Draining.
        if handle.state == IslandState::Running {
            handle.state = handle
                .state
                .transition(IslandState::Draining)
                .map_err(|e| DrainError::InvalidTransition(e.to_string()))?;
        }

        let _ = handle.command_tx.send(IslandCommand::Stop);
        info!(%island_id, "island stopping");

        self.finish_stop(island_id);
        Ok(())
    }

    fn finish_stop(&mut self, island_id: &crate::types::IslandId) {
        if let Some(handle) = self.registry.get_mut(island_id) {
            if let Some(jh) = handle.join_handle.take() {
                let _ = jh.join();
            }
            // Transition Draining -> Stopped
            if let Ok(new_state) = handle.state.transition(IslandState::Stopped) {
                handle.state = new_state;
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

/// Stub island thread loop. Blocks on crossbeam recv; exits on Drain or Stop.
fn island_thread_loop(rx: crossbeam_channel::Receiver<IslandCommand>) {
    loop {
        match rx.recv() {
            Ok(IslandCommand::Tick) => {
                // Stub: no-op tick
            }
            Ok(IslandCommand::Drain) | Ok(IslandCommand::Stop) => break,
            Err(_) => break, // channel closed
        }
    }
}

/// Create a (sender, receiver) pair for sending commands to the manager.
pub fn manager_channel(buffer: usize) -> (mpsc::Sender<ManagerCommand>, mpsc::Receiver<ManagerCommand>) {
    mpsc::channel(buffer)
}
