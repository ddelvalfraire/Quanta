//! Particle-world input datagram wire format.
//!
//! 25-byte layout (all multi-byte fields big-endian):
//!
//! ```text
//!   [0]       msg_type = 0x02
//!   [1..5]    entity_slot: u32 BE
//!   [5..9]    input_seq:   u32 BE
//!   [9..13]   dir_x:       f32 BE (IEEE-754 bit pattern)
//!   [13..17]  dir_z:       f32 BE
//!   [17..21]  actions:     u32 BE (reserved in Phase 2)
//!   [21..23]  dt_ms:       u16 BE
//!   [23..25]  _reserved:   u16 BE (future flags)
//! ```
//!
//! The PRD spec originally listed `f16 dt`; we use `u16 dt_ms`
//! (milliseconds as unsigned integer) to avoid pulling the `half` crate for
//! a single field. Total length stays 25 bytes. Cross-language parity with
//! the browser is guaranteed by `tests::golden_bytes` here plus
//! `tests::client_input_cross_language_golden_bytes` in `quanta-wasm-decoder`.

use std::fmt;

pub const INPUT_MSG_TYPE: u8 = 0x02;
pub const INPUT_DATAGRAM_LEN: usize = 25;

#[derive(Debug, Clone, PartialEq)]
pub struct ParticleInputPayload {
    pub entity_slot: u32,
    pub input_seq: u32,
    pub dir_x: f32,
    pub dir_z: f32,
    pub actions: u32,
    pub dt_ms: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InputParseError {
    WrongLength { expected: usize, got: usize },
    WrongMsgType { got: u8 },
}

impl fmt::Display for InputParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength { expected, got } => {
                write!(
                    f,
                    "input datagram wrong length: expected {expected}, got {got}"
                )
            }
            Self::WrongMsgType { got } => write!(f, "input datagram wrong msg_type: {got:#04x}"),
        }
    }
}

impl std::error::Error for InputParseError {}

pub fn parse_datagram(bytes: &[u8]) -> Result<ParticleInputPayload, InputParseError> {
    if bytes.len() != INPUT_DATAGRAM_LEN {
        return Err(InputParseError::WrongLength {
            expected: INPUT_DATAGRAM_LEN,
            got: bytes.len(),
        });
    }
    if bytes[0] != INPUT_MSG_TYPE {
        return Err(InputParseError::WrongMsgType { got: bytes[0] });
    }
    Ok(ParticleInputPayload {
        entity_slot: u32::from_be_bytes(bytes[1..5].try_into().unwrap()),
        input_seq: u32::from_be_bytes(bytes[5..9].try_into().unwrap()),
        dir_x: f32::from_be_bytes(bytes[9..13].try_into().unwrap()),
        dir_z: f32::from_be_bytes(bytes[13..17].try_into().unwrap()),
        actions: u32::from_be_bytes(bytes[17..21].try_into().unwrap()),
        dt_ms: u16::from_be_bytes(bytes[21..23].try_into().unwrap()),
    })
}

pub fn encode_datagram(payload: &ParticleInputPayload) -> [u8; INPUT_DATAGRAM_LEN] {
    let mut buf = [0u8; INPUT_DATAGRAM_LEN];
    buf[0] = INPUT_MSG_TYPE;
    buf[1..5].copy_from_slice(&payload.entity_slot.to_be_bytes());
    buf[5..9].copy_from_slice(&payload.input_seq.to_be_bytes());
    buf[9..13].copy_from_slice(&payload.dir_x.to_be_bytes());
    buf[13..17].copy_from_slice(&payload.dir_z.to_be_bytes());
    buf[17..21].copy_from_slice(&payload.actions.to_be_bytes());
    buf[21..23].copy_from_slice(&payload.dt_ms.to_be_bytes());
    // buf[23..25] stays zero (reserved).
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = ParticleInputPayload {
            entity_slot: 42,
            input_seq: 7,
            dir_x: 1.0,
            dir_z: -0.5,
            actions: 0b0010,
            dt_ms: 50,
        };
        assert_eq!(parse_datagram(&encode_datagram(&p)).unwrap(), p);
    }

    #[test]
    fn golden_bytes() {
        // MUST match `client_input_cross_language_golden_bytes` in the
        // wasm-decoder crate byte-for-byte. Changing this breaks the browser
        // client; both tests must be updated together.
        let p = ParticleInputPayload {
            entity_slot: 1,
            input_seq: 2,
            dir_x: 1.0,
            dir_z: 0.0,
            actions: 0,
            dt_ms: 50,
        };
        let expected: [u8; INPUT_DATAGRAM_LEN] = [
            0x02, 0, 0, 0, 1, // entity_slot = 1
            0, 0, 0, 2, // input_seq = 2
            0x3F, 0x80, 0x00, 0x00, // dir_x = 1.0
            0x00, 0x00, 0x00, 0x00, // dir_z = 0.0
            0, 0, 0, 0, // actions = 0
            0x00, 0x32, // dt_ms = 50
            0x00, 0x00, // reserved
        ];
        assert_eq!(encode_datagram(&p), expected);
        assert_eq!(parse_datagram(&expected).unwrap(), p);
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(
            parse_datagram(&[0x02; 24]).unwrap_err(),
            InputParseError::WrongLength {
                expected: 25,
                got: 24
            }
        );
    }

    #[test]
    fn rejects_wrong_msg_type() {
        let mut bytes = [0u8; 25];
        bytes[0] = 0x99;
        assert_eq!(
            parse_datagram(&bytes).unwrap_err(),
            InputParseError::WrongMsgType { got: 0x99 }
        );
    }
}
