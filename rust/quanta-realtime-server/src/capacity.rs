use crate::command::{ManagerCommand, ManagerMetrics};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Periodic capacity signal published to NATS for the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacitySignal {
    pub server_id: String,
    pub active_islands: u32,
    pub max_islands: u32,
    pub total_entities: u64,
    /// Placeholder: always 0.0 until sysinfo is wired.
    pub cpu_load: f64,
    /// Placeholder: always 0 until sysinfo is wired.
    pub memory_used: u64,
}

impl CapacitySignal {
    pub fn from_metrics(server_id: &str, max_islands: u32, metrics: &ManagerMetrics) -> Self {
        Self {
            server_id: server_id.to_owned(),
            active_islands: metrics.active_islands,
            max_islands,
            total_entities: metrics.total_entities,
            cpu_load: 0.0,
            memory_used: 0,
        }
    }
}

/// Run the capacity publisher loop. Queries the manager for metrics every
/// `interval` and publishes a JSON CapacitySignal to the given NATS subject.
pub async fn run_capacity_publisher(
    manager_tx: mpsc::Sender<ManagerCommand>,
    nats_client: async_nats::Client,
    subject: String,
    server_id: String,
    max_islands: u32,
    interval: std::time::Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if manager_tx
            .send(ManagerCommand::GetMetrics { reply: reply_tx })
            .await
            .is_err()
        {
            info!("manager channel closed, capacity publisher exiting");
            break;
        }

        let metrics = match reply_rx.await {
            Ok(m) => m,
            Err(_) => {
                warn!("failed to receive metrics from manager");
                continue;
            }
        };

        let signal = CapacitySignal::from_metrics(&server_id, max_islands, &metrics);
        match serde_json::to_vec(&signal) {
            Ok(payload) => {
                if let Err(e) = nats_client
                    .publish(subject.clone(), payload.into())
                    .await
                {
                    warn!(%e, "failed to publish capacity signal");
                }
            }
            Err(e) => {
                warn!(%e, "failed to serialize capacity signal");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::ManagerMetrics;

    #[test]
    fn capacity_signal_json_roundtrip() {
        let metrics = ManagerMetrics {
            active_islands: 5,
            total_islands: 7,
            total_entities: 1000,
        };
        let signal = CapacitySignal::from_metrics("srv-1", 200, &metrics);
        let json = serde_json::to_string(&signal).unwrap();
        let parsed: CapacitySignal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, signal);
        assert_eq!(parsed.active_islands, 5);
        assert_eq!(parsed.cpu_load, 0.0);
    }
}
