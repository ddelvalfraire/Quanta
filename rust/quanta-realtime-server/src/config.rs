use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// NATS server URL.
    pub nats_url: String,
    /// Maximum number of concurrent islands on this server.
    pub max_islands: u32,
    /// Entity count threshold: islands above this get a dedicated thread.
    pub entity_threshold: u32,
    /// Interval in seconds between capacity signal publications.
    pub capacity_interval_secs: u64,
    /// NATS subject for capacity signals.
    pub capacity_subject: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            nats_url: "nats://127.0.0.1:4222".to_owned(),
            max_islands: 200,
            entity_threshold: 100,
            capacity_interval_secs: 5,
            capacity_subject: "quanta.capacity".to_owned(),
        }
    }
}
