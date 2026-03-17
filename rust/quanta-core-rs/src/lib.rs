//! Shared Rust types for the Quanta platform.
//!
//! Provides binary-compatible type definitions used by both
//! the NIF crate and the standalone realtime server.

use std::fmt;

/// KV snapshot header size in bytes: js_seq(8) + state_version(2) + nonce(8).
pub const SNAPSHOT_HEADER_SIZE: usize = 18;

/// Encode the three header fields into a fixed 18-byte big-endian array.
pub fn encode_snapshot_header(js_seq: u64, state_version: u16, nonce: u64) -> [u8; 18] {
    let mut buf = [0u8; 18];
    buf[0..8].copy_from_slice(&js_seq.to_be_bytes());
    buf[8..10].copy_from_slice(&state_version.to_be_bytes());
    buf[10..18].copy_from_slice(&nonce.to_be_bytes());
    buf
}

/// Decode a snapshot header from bytes, returning (js_seq, state_version, nonce, state_data).
pub fn decode_snapshot_header(bytes: &[u8]) -> Result<(u64, u16, u64, &[u8]), CodecError> {
    if bytes.len() < SNAPSHOT_HEADER_SIZE {
        return Err(CodecError::TruncatedInput {
            expected: SNAPSHOT_HEADER_SIZE,
            got: bytes.len(),
        });
    }
    let js_seq = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
    let state_version = u16::from_be_bytes(bytes[8..10].try_into().unwrap());
    let nonce = u64::from_be_bytes(bytes[10..18].try_into().unwrap());
    let state_data = &bytes[18..];
    Ok((js_seq, state_version, nonce, state_data))
}

/// Wire format version.
pub const WIRE_VERSION: u8 = 0x01;

/// NATS wire envelope header, bitcode-encoded.
#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone, PartialEq)]
pub struct EnvelopeHeader {
    pub message_id: String,
    pub wall_us: u64,
    pub logical: u16,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub sender: SenderWire,
    pub metadata: Vec<(String, String)>,
}

/// Sender discriminant for wire encoding.
#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone, PartialEq)]
pub enum SenderWire {
    Actor {
        namespace: String,
        typ: String,
        id: String,
    },
    Client(String),
    System,
    None,
}

/// Encode a wire frame: `[version:1][header_len:4 BE][header][payload]`.
pub fn encode_wire_frame(header: &EnvelopeHeader, payload: &[u8]) -> Vec<u8> {
    let header_bytes = bitcode::encode(header);
    let header_len = header_bytes.len() as u32;
    let mut frame = Vec::with_capacity(1 + 4 + header_bytes.len() + payload.len());
    frame.push(WIRE_VERSION);
    frame.extend_from_slice(&header_len.to_be_bytes());
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(payload);
    frame
}

/// Decode a wire frame, returning the header and payload slice.
pub fn decode_wire_frame(frame: &[u8]) -> Result<(EnvelopeHeader, &[u8]), CodecError> {
    if frame.len() < 5 {
        return Err(CodecError::TruncatedInput {
            expected: 5,
            got: frame.len(),
        });
    }

    let version = frame[0];
    if version != WIRE_VERSION {
        return Err(CodecError::UnsupportedVersion {
            expected: WIRE_VERSION,
            got: version,
        });
    }

    let header_len = u32::from_be_bytes(frame[1..5].try_into().unwrap()) as usize;
    if frame.len() < 5 + header_len {
        return Err(CodecError::TruncatedInput {
            expected: 5 + header_len,
            got: frame.len(),
        });
    }

    let header_bytes = &frame[5..5 + header_len];
    let payload = &frame[5 + header_len..];

    let header =
        bitcode::decode(header_bytes).map_err(|e| CodecError::InvalidHeader(e.to_string()))?;

    Ok((header, payload))
}

#[derive(Debug, PartialEq)]
pub enum CodecError {
    TruncatedInput { expected: usize, got: usize },
    UnsupportedVersion { expected: u8, got: u8 },
    InvalidHeader(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedInput { expected, got } => {
                write!(f, "truncated input: expected at least {expected} bytes, got {got}")
            }
            Self::UnsupportedVersion { expected, got } => {
                write!(f, "unsupported wire version: expected {expected:#04x}, got {got:#04x}")
            }
            Self::InvalidHeader(msg) => write!(f, "invalid header: {msg}"),
        }
    }
}

impl std::error::Error for CodecError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip() {
        let header = encode_snapshot_header(42, 1, 99);
        let mut data = header.to_vec();
        data.extend_from_slice(b"hello");

        let (js_seq, version, nonce, state_data) = decode_snapshot_header(&data).unwrap();
        assert_eq!(js_seq, 42);
        assert_eq!(version, 1);
        assert_eq!(nonce, 99);
        assert_eq!(state_data, b"hello");
    }

    #[test]
    fn snapshot_header_is_18_bytes() {
        let header = encode_snapshot_header(0, 0, 0);
        assert_eq!(header.len(), SNAPSHOT_HEADER_SIZE);
    }

    #[test]
    fn snapshot_max_values() {
        let header = encode_snapshot_header(u64::MAX, u16::MAX, u64::MAX);
        let (js_seq, version, nonce, rest) = decode_snapshot_header(&header).unwrap();
        assert_eq!(js_seq, u64::MAX);
        assert_eq!(version, u16::MAX);
        assert_eq!(nonce, u64::MAX);
        assert!(rest.is_empty());
    }

    #[test]
    fn snapshot_truncated_input() {
        let err = decode_snapshot_header(&[0u8; 17]).unwrap_err();
        assert_eq!(
            err,
            CodecError::TruncatedInput {
                expected: 18,
                got: 17
            }
        );
    }

    /// Golden-byte test: this exact sequence is also asserted in the Elixir test suite.
    /// If either side changes encoding, the cross-language test catches the drift.
    #[test]
    fn snapshot_cross_language_golden_bytes() {
        let expected: [u8; 18] = [
            0, 0, 0, 0, 0, 0, 0, 42, // js_seq = 42
            0, 1, // state_version = 1
            0, 0, 0, 0, 0, 0, 0, 99, // nonce = 99
        ];
        assert_eq!(encode_snapshot_header(42, 1, 99), expected);

        let mut input = expected.to_vec();
        input.extend_from_slice(b"hello");
        let (js_seq, ver, nonce, data) = decode_snapshot_header(&input).unwrap();
        assert_eq!(js_seq, 42);
        assert_eq!(ver, 1);
        assert_eq!(nonce, 99);
        assert_eq!(data, b"hello");
    }

    #[test]
    fn snapshot_empty_state_data() {
        let header = encode_snapshot_header(1, 2, 3);
        let (_, _, _, state_data) = decode_snapshot_header(&header).unwrap();
        assert!(state_data.is_empty());
    }

    #[test]
    fn wire_roundtrip() {
        let header = EnvelopeHeader {
            message_id: "msg-1".into(),
            wall_us: 1_000_000,
            logical: 42,
            correlation_id: Some("corr-1".into()),
            causation_id: None,
            sender: SenderWire::System,
            metadata: vec![("k".into(), "v".into())],
        };

        let frame = encode_wire_frame(&header, b"payload");
        let (decoded_header, payload) = decode_wire_frame(&frame).unwrap();
        assert_eq!(decoded_header, header);
        assert_eq!(payload, b"payload");
    }

    #[test]
    fn wire_all_sender_types() {
        for sender in [
            SenderWire::Actor {
                namespace: "ns".into(),
                typ: "t".into(),
                id: "i".into(),
            },
            SenderWire::Client("c1".into()),
            SenderWire::System,
            SenderWire::None,
        ] {
            let header = EnvelopeHeader {
                message_id: "m".into(),
                wall_us: 0,
                logical: 0,
                correlation_id: None,
                causation_id: None,
                sender,
                metadata: vec![],
            };
            let frame = encode_wire_frame(&header, b"");
            let (decoded, _) = decode_wire_frame(&frame).unwrap();
            assert_eq!(decoded, header);
        }
    }

    #[test]
    fn wire_empty_payload() {
        let header = EnvelopeHeader {
            message_id: "m".into(),
            wall_us: 0,
            logical: 0,
            correlation_id: None,
            causation_id: None,
            sender: SenderWire::None,
            metadata: vec![],
        };
        let frame = encode_wire_frame(&header, b"");
        let (_, payload) = decode_wire_frame(&frame).unwrap();
        assert!(payload.is_empty());
    }

    #[test]
    fn wire_unsupported_version() {
        let mut frame = encode_wire_frame(
            &EnvelopeHeader {
                message_id: "m".into(),
                wall_us: 0,
                logical: 0,
                correlation_id: None,
                causation_id: None,
                sender: SenderWire::None,
                metadata: vec![],
            },
            b"",
        );
        frame[0] = 0x02;
        let err = decode_wire_frame(&frame).unwrap_err();
        assert_eq!(
            err,
            CodecError::UnsupportedVersion {
                expected: 0x01,
                got: 0x02
            }
        );
    }

    #[test]
    fn wire_truncated_frame() {
        let err = decode_wire_frame(&[0x01, 0, 0, 0]).unwrap_err();
        assert!(matches!(err, CodecError::TruncatedInput { .. }));
    }
}
