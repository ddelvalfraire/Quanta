use std::fmt;

/// Checkpoint header size: js_seq(8) + state_ver(2) + nonce(8) + checkpoint_tick(8) = 26 bytes.
pub const CHECKPOINT_HEADER_SIZE: usize = 26;

/// Current checkpoint state version. Bumped when the payload layout changes.
pub const CHECKPOINT_STATE_VERSION: u16 = 1;

/// Errors during checkpoint encoding/decoding.
#[derive(Debug, PartialEq)]
pub enum CheckpointCodecError {
    TruncatedInput { expected: usize, got: usize },
    UnsupportedVersion { expected: u16, got: u16 },
    InvalidPayload(String),
}

impl fmt::Display for CheckpointCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedInput { expected, got } => {
                write!(
                    f,
                    "truncated checkpoint: expected at least {expected} bytes, got {got}"
                )
            }
            Self::UnsupportedVersion { expected, got } => {
                write!(
                    f,
                    "unsupported checkpoint version: expected {expected}, got {got}"
                )
            }
            Self::InvalidPayload(msg) => write!(f, "invalid checkpoint payload: {msg}"),
        }
    }
}

impl std::error::Error for CheckpointCodecError {}

/// A single entity's state within a checkpoint.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct CheckpointEntity {
    pub slot: u32,
    pub state: Vec<u8>,
    pub owner_session: Option<String>,
}

/// Bitcode-encoded checkpoint payload containing all entity states.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct CheckpointPayload {
    pub entities: Vec<CheckpointEntity>,
}

/// Encode checkpoint header: `[js_seq:8][state_ver:2][nonce:8][checkpoint_tick:8]`.
pub fn encode_checkpoint_header(
    js_seq: u64,
    state_version: u16,
    nonce: u64,
    checkpoint_tick: u64,
) -> [u8; CHECKPOINT_HEADER_SIZE] {
    let mut buf = [0u8; CHECKPOINT_HEADER_SIZE];
    buf[0..8].copy_from_slice(&js_seq.to_be_bytes());
    buf[8..10].copy_from_slice(&state_version.to_be_bytes());
    buf[10..18].copy_from_slice(&nonce.to_be_bytes());
    buf[18..26].copy_from_slice(&checkpoint_tick.to_be_bytes());
    buf
}

/// Decode checkpoint header, returning `(js_seq, state_version, nonce, checkpoint_tick, data)`.
pub fn decode_checkpoint_header(
    bytes: &[u8],
) -> Result<(u64, u16, u64, u64, &[u8]), CheckpointCodecError> {
    if bytes.len() < CHECKPOINT_HEADER_SIZE {
        return Err(CheckpointCodecError::TruncatedInput {
            expected: CHECKPOINT_HEADER_SIZE,
            got: bytes.len(),
        });
    }
    let js_seq = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
    let state_version = u16::from_be_bytes(bytes[8..10].try_into().unwrap());
    let nonce = u64::from_be_bytes(bytes[10..18].try_into().unwrap());
    let checkpoint_tick = u64::from_be_bytes(bytes[18..26].try_into().unwrap());
    let data = &bytes[26..];
    Ok((js_seq, state_version, nonce, checkpoint_tick, data))
}

/// Encode a full checkpoint: header + bitcode payload.
/// Uses `js_seq=0` for realtime checkpoints (no JetStream event stream).
pub fn encode_checkpoint(tick: u64, payload: &CheckpointPayload) -> Vec<u8> {
    let nonce = nonce_from_timestamp();
    let header = encode_checkpoint_header(0, CHECKPOINT_STATE_VERSION, nonce, tick);
    let body = bitcode::encode(payload);
    let mut buf = Vec::with_capacity(CHECKPOINT_HEADER_SIZE + body.len());
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&body);
    buf
}

/// Decode a full checkpoint, returning `(tick, payload)`.
pub fn decode_checkpoint(
    bytes: &[u8],
) -> Result<(u64, CheckpointPayload), CheckpointCodecError> {
    let (_js_seq, state_ver, _nonce, tick, data_bytes) = decode_checkpoint_header(bytes)?;
    if state_ver != CHECKPOINT_STATE_VERSION {
        return Err(CheckpointCodecError::UnsupportedVersion {
            expected: CHECKPOINT_STATE_VERSION,
            got: state_ver,
        });
    }
    let payload: CheckpointPayload = bitcode::decode(data_bytes)
        .map_err(|e| CheckpointCodecError::InvalidPayload(e.to_string()))?;
    Ok((tick, payload))
}

fn nonce_from_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let header = encode_checkpoint_header(42, 1, 99, 1000);
        assert_eq!(header.len(), CHECKPOINT_HEADER_SIZE);

        let (js_seq, ver, nonce, tick, rest) = decode_checkpoint_header(&header).unwrap();
        assert_eq!(js_seq, 42);
        assert_eq!(ver, 1);
        assert_eq!(nonce, 99);
        assert_eq!(tick, 1000);
        assert!(rest.is_empty());
    }

    #[test]
    fn header_golden_bytes() {
        let expected: [u8; 26] = [
            0, 0, 0, 0, 0, 0, 0, 42, // js_seq = 42
            0, 1, // state_version = 1
            0, 0, 0, 0, 0, 0, 0, 99, // nonce = 99
            0, 0, 0, 0, 0, 0, 3, 232, // checkpoint_tick = 1000
        ];
        assert_eq!(encode_checkpoint_header(42, 1, 99, 1000), expected);
    }

    #[test]
    fn header_truncated() {
        let err = decode_checkpoint_header(&[0u8; 25]).unwrap_err();
        assert_eq!(
            err,
            CheckpointCodecError::TruncatedInput {
                expected: 26,
                got: 25,
            }
        );
    }

    #[test]
    fn full_checkpoint_roundtrip() {
        let payload = CheckpointPayload {
            entities: vec![
                CheckpointEntity {
                    slot: 0,
                    state: vec![1, 2, 3],
                    owner_session: Some("player1".into()),
                },
                CheckpointEntity {
                    slot: 5,
                    state: vec![10, 20],
                    owner_session: None,
                },
            ],
        };

        let encoded = encode_checkpoint(42, &payload);
        let (tick, decoded) = decode_checkpoint(&encoded).unwrap();
        assert_eq!(tick, 42);
        assert_eq!(decoded, payload);
    }
}
