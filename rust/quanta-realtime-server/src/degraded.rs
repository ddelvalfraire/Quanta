/// Degraded-mode limits for WebSocket transport.
///
/// These constants define the reduced capabilities applied when a client
/// connects over WebSocket instead of QUIC/WebTransport. Enforcement is
/// in the game loop / tick broadcaster, not the transport layer itself.

/// Maximum tick rate for WebSocket clients (Hz).
pub const WS_TICK_RATE: u32 = 10;

/// Maximum number of entities replicated to a WebSocket client.
pub const WS_MAX_ENTITIES: u32 = 100;

/// Maximum outbound bytes per second to a WebSocket client.
pub const WS_MAX_BYTES_PER_SEC: u32 = 10_240;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degraded_constants_are_sane() {
        assert!(WS_TICK_RATE > 0 && WS_TICK_RATE <= 60);
        assert!(WS_MAX_ENTITIES > 0);
        assert!(WS_MAX_BYTES_PER_SEC > 0);
    }
}
