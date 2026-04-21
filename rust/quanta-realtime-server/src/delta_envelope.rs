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
}
