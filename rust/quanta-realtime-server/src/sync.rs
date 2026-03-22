use std::fmt;
use std::time::Duration;

/// Flag: compiled schema bytes are included after the schema_version field.
pub const FLAG_INCLUDES_SCHEMA: u8 = 0x01;

/// Maximum entity count in a single InitialStateMessage.
const MAX_INITIAL_ENTITIES: u32 = 10_000;

/// Minimum header size: baseline_tick(8) + flags(1) + schema_version(1) + entity_count(4).
const HEADER_SIZE: usize = 8 + 1 + 1 + 4;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Full entity state snapshot sent to a client over a reliable QUIC stream
/// immediately after authentication.
///
/// Wire format:
/// ```text
/// [baseline_tick:u64][flags:u8][schema_version:u8]
/// [optional: schema_len:u32 + compiled_schema_bytes]
/// [entity_count:u32]
/// [repeated: entity_slot:u32 + state_len:u32 + state_bytes]
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct InitialStateMessage {
    pub baseline_tick: u64,
    pub flags: u8,
    pub schema_version: u8,
    pub compiled_schema: Option<Vec<u8>>,
    pub entities: Vec<EntityPayload>,
}

/// A single entity's slot index and serialized state within an InitialStateMessage.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityPayload {
    pub entity_slot: u32,
    pub state: Vec<u8>,
}

/// Client acknowledgment that the baseline snapshot was received and applied.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct BaselineAck {
    pub baseline_tick: u64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum SyncError {
    TruncatedInput { expected: usize, got: usize },
    InvalidPayload(String),
    EntityCountExceeded { count: u32, max: u32 },
    StreamError(String),
    Timeout,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedInput { expected, got } => {
                write!(f, "truncated input: expected {expected} bytes, got {got}")
            }
            Self::InvalidPayload(msg) => write!(f, "invalid payload: {msg}"),
            Self::EntityCountExceeded { count, max } => {
                write!(f, "entity count {count} exceeds maximum {max}")
            }
            Self::StreamError(msg) => write!(f, "stream error: {msg}"),
            Self::Timeout => write!(f, "sync timed out"),
        }
    }
}

impl std::error::Error for SyncError {}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encode an `InitialStateMessage` into the binary wire format.
pub fn encode_initial_state(msg: &InitialStateMessage) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_SIZE + msg.entities.len() * 40);

    buf.extend_from_slice(&msg.baseline_tick.to_be_bytes());
    buf.push(msg.flags);
    buf.push(msg.schema_version);

    if let Some(schema) = &msg.compiled_schema {
        buf.extend_from_slice(&(schema.len() as u32).to_be_bytes());
        buf.extend_from_slice(schema);
    }

    buf.extend_from_slice(&(msg.entities.len() as u32).to_be_bytes());
    for entity in &msg.entities {
        buf.extend_from_slice(&entity.entity_slot.to_be_bytes());
        buf.extend_from_slice(&(entity.state.len() as u32).to_be_bytes());
        buf.extend_from_slice(&entity.state);
    }

    buf
}

/// Decode an `InitialStateMessage` from the binary wire format.
pub fn decode_initial_state(bytes: &[u8]) -> Result<InitialStateMessage, SyncError> {
    if bytes.len() < HEADER_SIZE {
        return Err(SyncError::TruncatedInput {
            expected: HEADER_SIZE,
            got: bytes.len(),
        });
    }

    let mut pos = 0;

    let baseline_tick = u64::from_be_bytes(read_array::<8>(bytes, &mut pos)?);
    let flags = bytes[pos];
    pos += 1;
    let schema_version = bytes[pos];
    pos += 1;

    let compiled_schema = if flags & FLAG_INCLUDES_SCHEMA != 0 {
        let schema_len = u32::from_be_bytes(read_array::<4>(bytes, &mut pos)?) as usize;
        if pos + schema_len > bytes.len() {
            return Err(SyncError::TruncatedInput {
                expected: pos + schema_len,
                got: bytes.len(),
            });
        }
        let schema = bytes[pos..pos + schema_len].to_vec();
        pos += schema_len;
        Some(schema)
    } else {
        None
    };

    let entity_count = u32::from_be_bytes(read_array::<4>(bytes, &mut pos)?);
    if entity_count > MAX_INITIAL_ENTITIES {
        return Err(SyncError::EntityCountExceeded {
            count: entity_count,
            max: MAX_INITIAL_ENTITIES,
        });
    }

    let mut entities = Vec::with_capacity(entity_count as usize);
    for _ in 0..entity_count {
        let entity_slot = u32::from_be_bytes(read_array::<4>(bytes, &mut pos)?);
        let state_len = u32::from_be_bytes(read_array::<4>(bytes, &mut pos)?) as usize;
        if pos + state_len > bytes.len() {
            return Err(SyncError::TruncatedInput {
                expected: pos + state_len,
                got: bytes.len(),
            });
        }
        let state = bytes[pos..pos + state_len].to_vec();
        pos += state_len;
        entities.push(EntityPayload { entity_slot, state });
    }

    Ok(InitialStateMessage {
        baseline_tick,
        flags,
        schema_version,
        compiled_schema,
        entities,
    })
}

/// Encode a `BaselineAck` with a 4-byte big-endian length prefix (bitcode payload).
pub fn encode_baseline_ack(ack: &BaselineAck) -> Vec<u8> {
    let payload = bitcode::encode(ack);
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&payload);
    buf
}

/// Decode a `BaselineAck` from a length-prefixed bitcode buffer.
pub fn decode_baseline_ack(bytes: &[u8]) -> Result<BaselineAck, SyncError> {
    if bytes.len() < 4 {
        return Err(SyncError::TruncatedInput {
            expected: 4,
            got: bytes.len(),
        });
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < 4 + len {
        return Err(SyncError::TruncatedInput {
            expected: 4 + len,
            got: bytes.len(),
        });
    }
    bitcode::decode(&bytes[4..4 + len])
        .map_err(|e| SyncError::InvalidPayload(format!("decode BaselineAck: {e}")))
}

// ---------------------------------------------------------------------------
// Bulk transfer over QUIC streams
// ---------------------------------------------------------------------------

/// Server-side: open a bidirectional QUIC stream, send the `InitialStateMessage`,
/// and wait for the client's `BaselineAck`.
pub async fn send_initial_state_stream(
    connection: &quinn::Connection,
    msg: &InitialStateMessage,
    timeout: Duration,
) -> Result<BaselineAck, SyncError> {
    let result = tokio::time::timeout(timeout, async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;

        // Write length-prefixed encoded message.
        let encoded = encode_initial_state(msg);
        let len = (encoded.len() as u32).to_be_bytes();
        send.write_all(&len)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;
        send.write_all(&encoded)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;
        send.finish()
            .map_err(|e| SyncError::StreamError(e.to_string()))?;

        // Read length-prefixed BaselineAck from client.
        let mut ack_len_buf = [0u8; 4];
        recv.read_exact(&mut ack_len_buf)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;
        let ack_len = u32::from_be_bytes(ack_len_buf) as usize;
        let mut ack_buf = vec![0u8; ack_len];
        recv.read_exact(&mut ack_buf)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;

        decode_baseline_ack(&encode_baseline_ack_raw(&ack_buf))
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(SyncError::Timeout),
    }
}

/// Client-side: accept a bidirectional QUIC stream from the server, read the
/// `InitialStateMessage`, and return it along with the send half for acking.
pub async fn recv_initial_state_stream(
    connection: &quinn::Connection,
    timeout: Duration,
) -> Result<InitialStateMessage, SyncError> {
    let result = tokio::time::timeout(timeout, async {
        let (send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;

        // Read length-prefixed InitialStateMessage.
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;

        let msg = decode_initial_state(&buf)?;

        // Send BaselineAck back to server.
        let ack = BaselineAck {
            baseline_tick: msg.baseline_tick,
        };
        let ack_bytes = bitcode::encode(&ack);
        let ack_len = (ack_bytes.len() as u32).to_be_bytes();
        let mut send = send;
        send.write_all(&ack_len)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;
        send.write_all(&ack_bytes)
            .await
            .map_err(|e| SyncError::StreamError(e.to_string()))?;
        send.finish()
            .map_err(|e| SyncError::StreamError(e.to_string()))?;

        Ok(msg)
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(SyncError::Timeout),
    }
}

/// Re-wrap raw ack bytes into the length-prefixed format expected by decode_baseline_ack.
fn encode_baseline_ack_raw(ack_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + ack_bytes.len());
    buf.extend_from_slice(&(ack_bytes.len() as u32).to_be_bytes());
    buf.extend_from_slice(ack_bytes);
    buf
}

// ---------------------------------------------------------------------------
// Bandwidth ramping
// ---------------------------------------------------------------------------

/// Configuration for bandwidth ramping during initial sync.
#[derive(Debug, Clone)]
pub struct BandwidthRampConfig {
    /// Duration of the ramp-up period after connect (default 500ms).
    pub ramp_duration: Duration,
    /// Multiplier applied to normal bandwidth budget during the ramp (default 2.0).
    pub ramp_multiplier: f64,
    /// Maximum entities to send immediately in the initial burst (default 200).
    pub immediate_entity_limit: usize,
}

impl Default for BandwidthRampConfig {
    fn default() -> Self {
        Self {
            ramp_duration: Duration::from_millis(500),
            ramp_multiplier: 2.0,
            immediate_entity_limit: 200,
        }
    }
}

/// Split entities into an immediate batch and a deferred batch for bandwidth ramping.
///
/// If there are ≤ `config.immediate_entity_limit` entities, all go in the immediate
/// batch. Otherwise, the first `immediate_entity_limit` go immediately and the rest
/// are deferred to be trickled via the priority queue.
pub fn split_for_ramping(
    entities: Vec<EntityPayload>,
    config: &BandwidthRampConfig,
) -> (Vec<EntityPayload>, Vec<EntityPayload>) {
    if entities.len() <= config.immediate_entity_limit {
        (entities, Vec::new())
    } else {
        let mut entities = entities;
        let deferred = entities.split_off(config.immediate_entity_limit);
        (entities, deferred)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a fixed-size array from `bytes` at `pos`, advancing `pos`.
fn read_array<const N: usize>(bytes: &[u8], pos: &mut usize) -> Result<[u8; N], SyncError> {
    if *pos + N > bytes.len() {
        return Err(SyncError::TruncatedInput {
            expected: *pos + N,
            got: bytes.len(),
        });
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes[*pos..*pos + N]);
    *pos += N;
    Ok(arr)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entities(count: usize) -> Vec<EntityPayload> {
        (0..count)
            .map(|i| EntityPayload {
                entity_slot: i as u32,
                state: vec![(i & 0xFF) as u8; 8],
            })
            .collect()
    }

    #[test]
    fn roundtrip_200_entities_no_schema() {
        let msg = InitialStateMessage {
            baseline_tick: 12345,
            flags: 0,
            schema_version: 3,
            compiled_schema: None,
            entities: make_entities(200),
        };
        let bytes = encode_initial_state(&msg);
        let decoded = decode_initial_state(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_with_schema() {
        let schema = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02];
        let msg = InitialStateMessage {
            baseline_tick: 99,
            flags: FLAG_INCLUDES_SCHEMA,
            schema_version: 1,
            compiled_schema: Some(schema),
            entities: make_entities(5),
        };
        let bytes = encode_initial_state(&msg);
        let decoded = decode_initial_state(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_empty_entities() {
        let msg = InitialStateMessage {
            baseline_tick: 0,
            flags: 0,
            schema_version: 0,
            compiled_schema: None,
            entities: vec![],
        };
        let bytes = encode_initial_state(&msg);
        let decoded = decode_initial_state(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn truncated_header_returns_error() {
        let err = decode_initial_state(&[0u8; 5]).unwrap_err();
        assert_eq!(
            err,
            SyncError::TruncatedInput {
                expected: HEADER_SIZE,
                got: 5,
            }
        );
    }

    #[test]
    fn truncated_entity_state_returns_error() {
        let msg = InitialStateMessage {
            baseline_tick: 1,
            flags: 0,
            schema_version: 1,
            compiled_schema: None,
            entities: make_entities(1),
        };
        let mut bytes = encode_initial_state(&msg);
        bytes.truncate(bytes.len() - 2); // chop off part of the entity state
        let err = decode_initial_state(&bytes).unwrap_err();
        assert!(matches!(err, SyncError::TruncatedInput { .. }));
    }

    #[test]
    fn baseline_ack_roundtrip() {
        let ack = BaselineAck {
            baseline_tick: 42_000,
        };
        let bytes = encode_baseline_ack(&ack);
        let decoded = decode_baseline_ack(&bytes).unwrap();
        assert_eq!(ack, decoded);
    }

    #[test]
    fn baseline_ack_truncated_returns_error() {
        let err = decode_baseline_ack(&[0u8; 2]).unwrap_err();
        assert!(matches!(err, SyncError::TruncatedInput { .. }));
    }

    #[test]
    fn split_for_ramping_all_immediate() {
        let entities = make_entities(150);
        let config = BandwidthRampConfig::default();
        let (immediate, deferred) = split_for_ramping(entities, &config);
        assert_eq!(immediate.len(), 150);
        assert!(deferred.is_empty());
    }

    #[test]
    fn split_for_ramping_splits_at_limit() {
        let entities = make_entities(300);
        let config = BandwidthRampConfig::default(); // limit = 200
        let (immediate, deferred) = split_for_ramping(entities, &config);
        assert_eq!(immediate.len(), 200);
        assert_eq!(deferred.len(), 100);
        // Verify ordering: immediate has slots 0..200, deferred has 200..300
        assert_eq!(immediate[0].entity_slot, 0);
        assert_eq!(immediate[199].entity_slot, 199);
        assert_eq!(deferred[0].entity_slot, 200);
        assert_eq!(deferred[99].entity_slot, 299);
    }

    #[test]
    fn split_for_ramping_exact_limit() {
        let entities = make_entities(200);
        let config = BandwidthRampConfig::default();
        let (immediate, deferred) = split_for_ramping(entities, &config);
        assert_eq!(immediate.len(), 200);
        assert!(deferred.is_empty());
    }

    #[test]
    fn bandwidth_ramp_config_defaults() {
        let config = BandwidthRampConfig::default();
        assert_eq!(config.ramp_duration, Duration::from_millis(500));
        assert_eq!(config.ramp_multiplier, 2.0);
        assert_eq!(config.immediate_entity_limit, 200);
    }

    #[test]
    fn typical_200_entity_message_size() {
        // Verify ~20KB for 200 entities with 8-byte states (per ticket spec estimate)
        let msg = InitialStateMessage {
            baseline_tick: 100,
            flags: 0,
            schema_version: 1,
            compiled_schema: None,
            entities: make_entities(200),
        };
        let bytes = encode_initial_state(&msg);
        // Header(14) + 200 * (slot:4 + len:4 + state:8) = 14 + 3200 = 3214
        // With realistic 80-byte states: ~18KB. Our 8-byte states are smaller.
        assert!(bytes.len() < 65_536, "message should fit in a single burst");
    }
}
