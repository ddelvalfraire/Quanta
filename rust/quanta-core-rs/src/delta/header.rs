use super::DeltaError;

/// Fixed 4-byte delta packet header.
///
/// Layout: `flags:u8 | schema_ver:u8 | payload_bits:u16 BE`
///
/// Trailing bytes beyond [`HEADER_SIZE`] are ignored by [`decode`](Self::decode).
#[derive(Debug, Clone, PartialEq)]
pub struct DeltaHeader {
    pub is_full_snapshot: bool,
    pub is_compressed: bool,
    /// Whether the payload includes a field bitmask (partial update).
    pub has_bitmask: bool,
    pub schema_version: u8,
    pub payload_bits: u16,
}

pub const HEADER_SIZE: usize = 4;

impl DeltaHeader {
    /// Encode into a fixed 4-byte array.
    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut flags: u8 = 0;
        if self.is_full_snapshot {
            flags |= 1;
        }
        if self.is_compressed {
            flags |= 1 << 1;
        }
        if self.has_bitmask {
            flags |= 1 << 2;
        }
        let pb = self.payload_bits.to_be_bytes();
        [flags, self.schema_version, pb[0], pb[1]]
    }

    /// Decode from a byte slice (must be at least 4 bytes).
    pub fn decode(bytes: &[u8]) -> Result<Self, DeltaError> {
        if bytes.len() < HEADER_SIZE {
            return Err(DeltaError::TruncatedHeader {
                expected: HEADER_SIZE,
                got: bytes.len(),
            });
        }
        let flags = bytes[0];
        Ok(Self {
            is_full_snapshot: flags & 1 != 0,
            is_compressed: flags & (1 << 1) != 0,
            has_bitmask: flags & (1 << 2) != 0,
            schema_version: bytes[1],
            payload_bits: u16::from_be_bytes([bytes[2], bytes[3]]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_flag_combinations() {
        for flags in 0..8u8 {
            let header = DeltaHeader {
                is_full_snapshot: flags & 1 != 0,
                is_compressed: flags & 2 != 0,
                has_bitmask: flags & 4 != 0,
                schema_version: 42,
                payload_bits: 1024,
            };
            let encoded = header.encode();
            let decoded = DeltaHeader::decode(&encoded).unwrap();
            assert_eq!(decoded, header, "flag combo {flags:#04b}");
        }
    }

    #[test]
    fn payload_bits_big_endian() {
        let header = DeltaHeader {
            is_full_snapshot: false,
            is_compressed: false,
            has_bitmask: false,
            schema_version: 0,
            payload_bits: 0x0102,
        };
        let encoded = header.encode();
        assert_eq!(encoded[2], 0x01);
        assert_eq!(encoded[3], 0x02);
    }

    #[test]
    fn max_values() {
        let header = DeltaHeader {
            is_full_snapshot: true,
            is_compressed: true,
            has_bitmask: true,
            schema_version: 255,
            payload_bits: u16::MAX,
        };
        let decoded = DeltaHeader::decode(&header.encode()).unwrap();
        assert_eq!(decoded, header);
    }

    #[test]
    fn truncated_input() {
        let err = DeltaHeader::decode(&[0, 1, 2]).unwrap_err();
        assert_eq!(
            err,
            DeltaError::TruncatedHeader {
                expected: 4,
                got: 3
            }
        );
    }

    #[test]
    fn decode_ignores_extra_bytes() {
        let header = DeltaHeader {
            is_full_snapshot: true,
            is_compressed: false,
            has_bitmask: true,
            schema_version: 7,
            payload_bits: 256,
        };
        let mut data = header.encode().to_vec();
        data.extend_from_slice(&[0xDE, 0xAD]);
        let decoded = DeltaHeader::decode(&data).unwrap();
        assert_eq!(decoded, header);
    }
}
