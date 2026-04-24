//! Bridge protocol types for realtime-server <-> Elixir runtime communication.
//!
//! Wire format: `[version:1][header_len:4 BE][header_bitcode][payload]`
//! Reuses the same framing pattern as the actor wire codec.

use crate::{decode_frame, encode_frame, CodecError};

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
    Request = 8,
    Response = 9,
    FireAndForget = 10,
    SagaFailed = 11,
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

/// Encode multiple bridge frames into a single batch envelope.
///
/// Format: `[batch_count:u16 BE][len1:u32 BE][frame_1][len2:u32 BE][frame_2]...`
///
/// Used for tick-aligned r2d batching: one NATS publish per target actor type per tick.
pub fn encode_batch_envelope(frames: &[&[u8]]) -> Vec<u8> {
    let total: usize = 2 + frames.iter().map(|f| 4 + f.len()).sum::<usize>();
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&(frames.len() as u16).to_be_bytes());
    for frame in frames {
        buf.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        buf.extend_from_slice(frame);
    }
    buf
}

/// Decode a batch envelope into individual frame slices.
pub fn decode_batch_envelope(batch: &[u8]) -> Result<Vec<&[u8]>, CodecError> {
    if batch.len() < 2 {
        return Err(CodecError::TruncatedInput {
            expected: 2,
            got: batch.len(),
        });
    }
    let count = u16::from_be_bytes([batch[0], batch[1]]) as usize;
    // Each frame needs at least 4 bytes for its length prefix.
    let max_possible = (batch.len() - 2) / 4;
    if count > max_possible {
        return Err(CodecError::TruncatedInput {
            expected: 2 + count * 4,
            got: batch.len(),
        });
    }
    let mut offset = 2;
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 4 > batch.len() {
            return Err(CodecError::TruncatedInput {
                expected: offset + 4,
                got: batch.len(),
            });
        }
        let len = u32::from_be_bytes([
            batch[offset],
            batch[offset + 1],
            batch[offset + 2],
            batch[offset + 3],
        ]) as usize;
        offset += 4;
        if offset + len > batch.len() {
            return Err(CodecError::TruncatedInput {
                expected: offset + len,
                got: batch.len(),
            });
        }
        frames.push(&batch[offset..offset + len]);
        offset += len;
    }
    Ok(frames)
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
            BridgeMsgType::Request,
            BridgeMsgType::Response,
            BridgeMsgType::FireAndForget,
            BridgeMsgType::SagaFailed,
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

    #[test]
    fn batch_roundtrip() {
        let frames: Vec<Vec<u8>> = (0..5)
            .map(|i| {
                let header = BridgeHeader {
                    msg_type: BridgeMsgType::EntityCommand,
                    sequence: i,
                    timestamp: 1000 + i,
                    correlation_id: None,
                };
                encode_bridge_frame(&header, &[i as u8; 4])
            })
            .collect();

        let refs: Vec<&[u8]> = frames.iter().map(|f| f.as_slice()).collect();
        let batch = encode_batch_envelope(&refs);
        let decoded = decode_batch_envelope(&batch).unwrap();

        assert_eq!(decoded.len(), 5);
        for (i, frame) in decoded.iter().enumerate() {
            let (header, payload) = decode_bridge_frame(frame).unwrap();
            assert_eq!(header.sequence, i as u64);
            assert_eq!(payload, &[i as u8; 4]);
        }
    }

    #[test]
    fn batch_empty() {
        let batch = encode_batch_envelope(&[]);
        let decoded = decode_batch_envelope(&batch).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn batch_single_frame() {
        let frame = encode_bridge_frame(
            &BridgeHeader {
                msg_type: BridgeMsgType::Request,
                sequence: 1,
                timestamp: 100,
                correlation_id: Some([0xAB; 16]),
            },
            b"request-payload",
        );
        let batch = encode_batch_envelope(&[&frame]);
        let decoded = decode_batch_envelope(&batch).unwrap();
        assert_eq!(decoded.len(), 1);
        let (header, payload) = decode_bridge_frame(decoded[0]).unwrap();
        assert_eq!(header.msg_type, BridgeMsgType::Request);
        assert_eq!(payload, b"request-payload");
    }

    #[test]
    fn batch_truncated() {
        let err = decode_batch_envelope(&[0]).unwrap_err();
        assert!(matches!(err, CodecError::TruncatedInput { .. }));

        // Valid count but truncated frame length
        let err = decode_batch_envelope(&[0, 1, 0, 0]).unwrap_err();
        assert!(matches!(err, CodecError::TruncatedInput { .. }));

        // Valid count + length but truncated frame data
        let err = decode_batch_envelope(&[0, 1, 0, 0, 0, 10, 0xFF]).unwrap_err();
        assert!(matches!(err, CodecError::TruncatedInput { .. }));
    }
}
