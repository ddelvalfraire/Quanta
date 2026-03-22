use std::sync::Arc;
use std::time::Duration;

use quinn::congestion::BbrConfig;
use quinn::{IdleTimeout, TransportConfig, VarInt};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_transport_config_does_not_panic() {
        let cfg = EndpointConfig::default();
        let _transport = cfg.build_transport_config();
    }
}
