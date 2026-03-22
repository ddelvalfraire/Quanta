/// Maximum tick rate for WebSocket clients (Hz).
pub const WS_TICK_RATE: u32 = 10;

/// Maximum number of entities replicated to a WebSocket client.
pub const WS_MAX_ENTITIES: u32 = 100;

/// Maximum outbound bytes per second to a WebSocket client.
pub const WS_MAX_BYTES_PER_SEC: u32 = 10_240;
