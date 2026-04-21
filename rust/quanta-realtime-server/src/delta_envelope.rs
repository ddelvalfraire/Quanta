//! Delta datagram wire format — a generic 13-byte header used by fanout
//! tasks to send per-entity state updates to clients.
//!
//! Layout (big-endian):
//!   [0]        flags: u8 (bit 0 = FULL_STATE)
//!   [1..5]     entity_slot: u32
//!   [5..13]    tick: u64
//!   [13..]     delta_bytes

pub const DELTA_HEADER_LEN: usize = 13;
pub const FLAG_FULL_STATE: u8 = 0x01;
/// Server-to-client identity message, sent once immediately after a client
/// is registered to an island. The datagram's `entity_slot` field is the
/// slot assigned to this client. `delta_bytes` is always empty. This lets
/// the browser reliably identify "which slot is me" without guessing from
/// the normal fanout stream (where NPCs routinely outrank an idle self in
/// the interest priority order).
pub const FLAG_WELCOME: u8 = 0x02;
/// Server→client acknowledgement of the last client input processed by the
/// authoritative simulation for the receiving client's own entity. When
/// this bit is set, the first 4 bytes of `delta_bytes` are a big-endian
/// `u32` `last_processed_input_seq`; the real delta payload follows.
/// This is the piece the client needs to run canonical server
/// reconciliation: rewind to server state, drop acknowledged inputs, and
/// replay the tail of its input buffer on top. Without it, any naive
/// reconciliation pulls the predictor backward by the network-latency
/// offset every snapshot — i.e. rubber-band.
pub const FLAG_HAS_SEQ_ACK: u8 = 0x04;
/// Size in bytes of the seq-ack prefix inside `delta_bytes` when
/// `FLAG_HAS_SEQ_ACK` is set.
pub const SEQ_ACK_PREFIX_LEN: usize = 4;

pub fn encode_delta_datagram(
    flags: u8,
    entity_slot: u32,
    tick: u64,
    delta_bytes: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(DELTA_HEADER_LEN + delta_bytes.len());
    buf.push(flags);
    buf.extend_from_slice(&entity_slot.to_be_bytes());
    buf.extend_from_slice(&tick.to_be_bytes());
    buf.extend_from_slice(delta_bytes);
    buf
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeltaParseError {
    Truncated { expected: usize, got: usize },
}

impl std::fmt::Display for DeltaParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { expected, got } => {
                write!(
                    f,
                    "truncated delta datagram: expected ≥{expected} bytes, got {got}"
                )
            }
        }
    }
}

impl std::error::Error for DeltaParseError {}

/// Encode a delta datagram with a prepended 4-byte `last_processed_input_seq`
/// inside `delta_bytes`. Automatically ORs `FLAG_HAS_SEQ_ACK` into `flags`.
/// Used by the particle-world fanout when shipping state to a client for
/// its OWN entity; ignored for other entities.
pub fn encode_delta_datagram_with_seq_ack(
    flags: u8,
    entity_slot: u32,
    tick: u64,
    last_processed_input_seq: u32,
    delta_bytes: &[u8],
) -> Vec<u8> {
    let mut buf =
        Vec::with_capacity(DELTA_HEADER_LEN + SEQ_ACK_PREFIX_LEN + delta_bytes.len());
    buf.push(flags | FLAG_HAS_SEQ_ACK);
    buf.extend_from_slice(&entity_slot.to_be_bytes());
    buf.extend_from_slice(&tick.to_be_bytes());
    buf.extend_from_slice(&last_processed_input_seq.to_be_bytes());
    buf.extend_from_slice(delta_bytes);
    buf
}

pub fn parse_delta_datagram(bytes: &[u8]) -> Result<(u8, u32, u64, &[u8]), DeltaParseError> {
    if bytes.len() < DELTA_HEADER_LEN {
        return Err(DeltaParseError::Truncated {
            expected: DELTA_HEADER_LEN,
            got: bytes.len(),
        });
    }
    let flags = bytes[0];
    // The length check above guarantees bytes.len() >= 13, so both slices
    // are statically 4 and 8 bytes — the try_into never fails.
    let slot = u32::from_be_bytes(bytes[1..5].try_into().unwrap());
    let tick = u64::from_be_bytes(bytes[5..13].try_into().unwrap());
    Ok((flags, slot, tick, &bytes[13..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_bytes() {
        let bytes = encode_delta_datagram(FLAG_FULL_STATE, 1, 42, &[0xDE, 0xAD]);
        let expected: Vec<u8> = vec![
            0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A, 0xDE,
            0xAD,
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn roundtrip() {
        let bytes = encode_delta_datagram(0, 7, 100, &[1, 2, 3, 4]);
        let (f, s, t, d) = parse_delta_datagram(&bytes).unwrap();
        assert_eq!((f, s, t, d), (0, 7, 100, &[1, 2, 3, 4][..]));
    }

    #[test]
    fn rejects_truncated() {
        assert_eq!(
            parse_delta_datagram(&[0u8; 12]).unwrap_err(),
            DeltaParseError::Truncated {
                expected: 13,
                got: 12
            }
        );
    }

    #[test]
    fn zero_delta_bytes_is_valid() {
        let bytes = encode_delta_datagram(0, 0, 0, &[]);
        assert_eq!(bytes.len(), DELTA_HEADER_LEN);
        let (_, _, _, d) = parse_delta_datagram(&bytes).unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn welcome_flag_is_distinct_and_orthogonal() {
        assert_eq!(FLAG_WELCOME, 0x02);
        // WELCOME must not collide with FULL_STATE — they're independent
        // bits, and a client should be able to distinguish them.
        assert_eq!(FLAG_WELCOME & FLAG_FULL_STATE, 0);
    }

    #[test]
    fn seq_ack_flag_is_distinct() {
        assert_eq!(FLAG_HAS_SEQ_ACK, 0x04);
        assert_eq!(FLAG_HAS_SEQ_ACK & FLAG_FULL_STATE, 0);
        assert_eq!(FLAG_HAS_SEQ_ACK & FLAG_WELCOME, 0);
    }

    #[test]
    fn encode_with_seq_ack_prepends_u32_and_sets_flag() {
        let delta = [0xAA, 0xBB];
        let bytes = encode_delta_datagram_with_seq_ack(
            FLAG_FULL_STATE,
            7,
            123,
            0xDEAD_BEEF,
            &delta,
        );
        let (flags, slot, tick, payload) = parse_delta_datagram(&bytes).unwrap();
        assert_eq!(flags & FLAG_HAS_SEQ_ACK, FLAG_HAS_SEQ_ACK);
        assert_eq!(flags & FLAG_FULL_STATE, FLAG_FULL_STATE);
        assert_eq!(slot, 7);
        assert_eq!(tick, 123);
        assert_eq!(payload.len(), SEQ_ACK_PREFIX_LEN + delta.len());
        // First 4 bytes of payload are seq_ack, big-endian.
        let seq = u32::from_be_bytes(payload[..4].try_into().unwrap());
        assert_eq!(seq, 0xDEAD_BEEF);
        assert_eq!(&payload[4..], &delta);
    }

    #[test]
    fn encode_without_seq_ack_leaves_payload_untouched() {
        let delta = [1u8, 2, 3];
        let bytes = encode_delta_datagram(0, 5, 50, &delta);
        let (flags, _, _, payload) = parse_delta_datagram(&bytes).unwrap();
        assert_eq!(flags & FLAG_HAS_SEQ_ACK, 0);
        assert_eq!(payload, &delta);
    }

    #[test]
    fn welcome_datagram_roundtrip_carries_slot_without_body() {
        let slot_for_client = 42u32;
        let bytes = encode_delta_datagram(FLAG_WELCOME, slot_for_client, 0, &[]);
        assert_eq!(bytes.len(), DELTA_HEADER_LEN);
        let (flags, slot, tick, delta) = parse_delta_datagram(&bytes).unwrap();
        assert_eq!(flags & FLAG_WELCOME, FLAG_WELCOME);
        assert_eq!(flags & FLAG_FULL_STATE, 0);
        assert_eq!(slot, slot_for_client);
        assert_eq!(tick, 0);
        assert!(
            delta.is_empty(),
            "welcome datagrams must not carry a state payload"
        );
    }
}
