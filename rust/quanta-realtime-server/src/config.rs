use std::sync::Arc;
use std::time::Duration;

use crate::zone_transfer::ZoneTransferConfig;
use quinn::congestion::BbrConfig;
use quinn::{IdleTimeout, TransportConfig, VarInt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub idle_timeout: Duration,
    pub keep_alive_interval: Duration,
    pub initial_rtt: Duration,
    pub datagram_receive_buffer_size: usize,
    pub datagram_send_buffer_size: usize,
    pub max_concurrent_bidi_streams: u32,
    pub max_concurrent_uni_streams: u32,
    pub rate_limit_per_sec: u32,
    pub auth_timeout: Duration,
    pub ws_port: Option<u16>,
    /// How long to retain disconnected sessions for fast reconnect (default 30s).
    pub session_retain_duration: Duration,
    /// Maximum number of retained sessions before LRU eviction (default 1000).
    pub max_retained_sessions: usize,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(30),
            keep_alive_interval: Duration::from_secs(5),
            initial_rtt: Duration::from_millis(50),
            datagram_receive_buffer_size: 65_536,
            datagram_send_buffer_size: 65_536,
            max_concurrent_bidi_streams: 8,
            max_concurrent_uni_streams: 4,
            rate_limit_per_sec: 100,
            auth_timeout: Duration::from_secs(5),
            ws_port: None,
            session_retain_duration: Duration::from_secs(30),
            max_retained_sessions: 1000,
        }
    }
}

impl EndpointConfig {
    pub fn build_transport_config(&self) -> TransportConfig {
        let mut transport = TransportConfig::default();

        transport.max_idle_timeout(Some(
            IdleTimeout::try_from(self.idle_timeout).expect("idle timeout out of range"),
        ));
        transport.keep_alive_interval(Some(self.keep_alive_interval));
        transport.initial_rtt(self.initial_rtt);
        transport.datagram_receive_buffer_size(Some(self.datagram_receive_buffer_size));
        transport.datagram_send_buffer_size(self.datagram_send_buffer_size);
        transport.max_concurrent_bidi_streams(VarInt::from_u32(self.max_concurrent_bidi_streams));
        transport.max_concurrent_uni_streams(VarInt::from_u32(self.max_concurrent_uni_streams));
        transport.congestion_controller_factory(Arc::new(BbrConfig::default()));

        transport
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// NATS broker URL. `None` disables capacity publishing and any future
    /// NATS-backed bridge wiring — the realtime server runs standalone.
    pub nats_url: Option<String>,
    pub max_islands: u32,
    pub entity_threshold: u32,
    pub capacity_interval_secs: u64,
    pub capacity_subject: String,
    /// Seconds with 0 players before an island passivates (default 30).
    pub idle_timeout_secs: u64,
    /// Grace period seconds after last player leaves before passivation starts (default 10).
    pub grace_period_secs: u64,
    /// Zone transfer configuration. `None` disables zone transfers.
    #[serde(skip)]
    pub zone_transfer: Option<ZoneTransferConfig>,
    /// Tick-engine wall-clock rate in Hz. Determines how often the island
    /// drains inputs, calls the executor, and emits snapshots. MUST match
    /// the `tick_rate_hz` passed to the executor factory — otherwise the
    /// executor's `tick_dt_secs` won't match the engine's cadence and
    /// physics will advance faster or slower than wall-clock, diverging
    /// from any client-side predictor.
    #[serde(default = "default_tick_rate_hz")]
    pub tick_rate_hz: u8,
}

fn default_tick_rate_hz() -> u8 {
    20
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            nats_url: None,
            max_islands: 200,
            entity_threshold: 100,
            capacity_interval_secs: 5,
            capacity_subject: "quanta.default.realtime.capacity".to_owned(),
            idle_timeout_secs: 30,
            grace_period_secs: 10,
            zone_transfer: None,
            tick_rate_hz: default_tick_rate_hz(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_transport_config_does_not_panic() {
        let cfg = EndpointConfig::default();
        let _transport = cfg.build_transport_config();
    }
}
