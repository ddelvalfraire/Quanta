//! Bridge protocol types for realtime-server <-> Elixir runtime communication.
//!
//! Wire format: `[version:1][header_len:4 BE][header_bitcode][payload]`
//! Reuses the same framing pattern as the actor wire codec.

use crate::{CodecError, decode_frame, encode_frame};

pub const BRIDGE_VERSION: u8 = 0x01;

#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BridgeMsgType {
    ActivateIsland = 0,
    DeactivateIsland = 1,
    PlayerJoin = 2,
    PlayerLeave = 3,
    EntityCommand = 4,
    StateSync = 5,
    Heartbeat = 6,
    CapacityReport = 7,
}

#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone, PartialEq)]
pub struct BridgeHeader {
    pub msg_type: BridgeMsgType,
    pub sequence: u64,
    pub timestamp: u64,
    pub correlation_id: Option<[u8; 16]>,
}

pub fn encode_bridge_frame(header: &BridgeHeader, payload: &[u8]) -> Vec<u8> {
    encode_frame(BRIDGE_VERSION, header, payload)
}

pub fn decode_bridge_frame(frame: &[u8]) -> Result<(BridgeHeader, &[u8]), CodecError> {
    decode_frame(BRIDGE_VERSION, frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_msg_types() -> Vec<BridgeMsgType> {
        vec![
            BridgeMsgType::ActivateIsland,
            BridgeMsgType::DeactivateIsland,
            BridgeMsgType::PlayerJoin,
            BridgeMsgType::PlayerLeave,
            BridgeMsgType::EntityCommand,
            BridgeMsgType::StateSync,
            BridgeMsgType::Heartbeat,
            BridgeMsgType::CapacityReport,
        ]
    }

    #[test]
    fn roundtrip_all_msg_types() {
        for msg_type in all_msg_types() {
            let header = BridgeHeader {
                msg_type,
                sequence: 42,
                timestamp: 1_000_000,
                correlation_id: None,
            };
            let frame = encode_bridge_frame(&header, b"test");
            let (decoded, payload) = decode_bridge_frame(&frame).unwrap();
            assert_eq!(decoded, header);
            assert_eq!(payload, b"test");
        }
    }

    #[test]
    fn roundtrip_with_correlation_id() {
        let cid = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let header = BridgeHeader {
            msg_type: BridgeMsgType::PlayerJoin,
            sequence: 99,
            timestamp: 500,
            correlation_id: Some(cid),
        };
        let frame = encode_bridge_frame(&header, b"payload");
        let (decoded, payload) = decode_bridge_frame(&frame).unwrap();
        assert_eq!(decoded, header);
        assert_eq!(decoded.correlation_id, Some(cid));
        assert_eq!(payload, b"payload");
    }

    #[test]
    fn roundtrip_without_correlation_id() {
        let header = BridgeHeader {
            msg_type: BridgeMsgType::Heartbeat,
            sequence: 0,
            timestamp: 0,
            correlation_id: None,
        };
        let frame = encode_bridge_frame(&header, b"");
        let (decoded, payload) = decode_bridge_frame(&frame).unwrap();
        assert_eq!(decoded, header);
        assert_eq!(decoded.correlation_id, None);
        assert!(payload.is_empty());
    }

    #[test]
    fn invalid_version() {
        let header = BridgeHeader {
            msg_type: BridgeMsgType::Heartbeat,
            sequence: 0,
            timestamp: 0,
            correlation_id: None,
        };
        let mut frame = encode_bridge_frame(&header, b"");
        frame[0] = 0xFF;
        let err = decode_bridge_frame(&frame).unwrap_err();
        assert_eq!(
            err,
            CodecError::UnsupportedVersion {
                expected: BRIDGE_VERSION,
                got: 0xFF
            }
        );
    }

    #[test]
    fn truncated_input_too_short() {
        let err = decode_bridge_frame(&[0x01, 0, 0, 0]).unwrap_err();
        assert!(matches!(err, CodecError::TruncatedInput { .. }));
    }

    #[test]
    fn truncated_input_header_exceeds_frame() {
        let frame = [0x01, 0, 0, 0, 100];
        let err = decode_bridge_frame(&frame).unwrap_err();
        assert_eq!(
            err,
            CodecError::TruncatedInput {
                expected: 105,
                got: 5
            }
        );
    }

    #[test]
    fn deterministic_encoding() {
        let header = BridgeHeader {
            msg_type: BridgeMsgType::StateSync,
            sequence: 123,
            timestamp: 456,
            correlation_id: Some([0xAA; 16]),
        };
        let frame1 = encode_bridge_frame(&header, b"data");
        let frame2 = encode_bridge_frame(&header, b"data");
        assert_eq!(frame1, frame2);
    }

    #[test]
    fn empty_payload() {
        let header = BridgeHeader {
            msg_type: BridgeMsgType::CapacityReport,
            sequence: 1,
            timestamp: 2,
            correlation_id: None,
        };
        let frame = encode_bridge_frame(&header, b"");
        let (decoded, payload) = decode_bridge_frame(&frame).unwrap();
        assert_eq!(decoded, header);
        assert!(payload.is_empty());
    }

    #[test]
    fn empty_frame() {
        let err = decode_bridge_frame(&[]).unwrap_err();
        assert_eq!(
            err,
            CodecError::TruncatedInput {
                expected: 5,
                got: 0
            }
        );
    }
}
