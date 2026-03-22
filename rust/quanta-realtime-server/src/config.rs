use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub nats_url: String,
    pub max_islands: u32,
    pub entity_threshold: u32,
    pub capacity_interval_secs: u64,
    pub capacity_subject: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            nats_url: "nats://127.0.0.1:4222".to_owned(),
            max_islands: 200,
            entity_threshold: 100,
            capacity_interval_secs: 5,
            capacity_subject: "quanta.default.realtime.capacity".to_owned(),
        }
    }
}
