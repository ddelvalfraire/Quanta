use super::DeltaError;

/// Reads individual bits from a byte slice.
///
/// Bits are read MSB-first. The reader tracks a bit position and refuses
/// to read past `total_bits` (the logical payload size from the header).
///
/// `total_bits` is clamped to the actual slice length at construction,
/// so reads never index past the backing data.
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: u32,
    total_bits: u32,
}

impl<'a> BitReader<'a> {
    /// Create a reader over `data` limited to `total_bits`.
    ///
    /// If `total_bits` exceeds the slice capacity, it is saturated to
    /// `data.len() * 8` to prevent out-of-bounds access.
    pub fn new(data: &'a [u8], total_bits: u32) -> Self {
        let max_bits = (data.len() as u32).saturating_mul(8);
        Self {
            data,
            bit_pos: 0,
            total_bits: total_bits.min(max_bits),
        }
    }

    /// Read `bits` (1..=64) from the stream.
    pub fn read_bits(&mut self, bits: u8) -> Result<u64, DeltaError> {
        debug_assert!((1..=64).contains(&bits), "bits must be 1..=64, got {bits}");

        let remaining = self.total_bits.saturating_sub(self.bit_pos);
        if (bits as u32) > remaining {
            return Err(DeltaError::ReadPastEnd {
                requested: bits,
                remaining,
            });
        }

        let mut value: u64 = 0;
        let mut bits_left = bits;
        let mut pos = self.bit_pos;

        while bits_left > 0 {
            let byte_idx = (pos / 8) as usize;
            let bit_offset = (pos % 8) as u8;
            let available = 8 - bit_offset;
            let take = bits_left.min(available);

            let shifted = (self.data[byte_idx] >> (available - take)) as u64;
            let mask = (1u64 << take) - 1;
            value = (value << take) | (shifted & mask);

            pos += take as u32;
            bits_left -= take;
        }

        self.bit_pos = pos;
        Ok(value)
    }

    /// Read a single bit as bool.
    pub fn read_bool(&mut self) -> Result<bool, DeltaError> {
        self.read_bits(1).map(|v| v != 0)
    }

    /// Number of bits remaining before `total_bits`.
    pub fn bits_remaining(&self) -> u32 {
        self.total_bits.saturating_sub(self.bit_pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_past_end_returns_error() {
        let data = [0xFF];
        let mut r = BitReader::new(&data, 4);
        assert_eq!(r.read_bits(4).unwrap(), 0xF);
        let err = r.read_bits(1).unwrap_err();
        assert_eq!(
            err,
            DeltaError::ReadPastEnd {
                requested: 1,
                remaining: 0
            }
        );
    }

    #[test]
    fn read_past_end_partial() {
        let data = [0xFF];
        let mut r = BitReader::new(&data, 5);
        let err = r.read_bits(6).unwrap_err();
        assert_eq!(
            err,
            DeltaError::ReadPastEnd {
                requested: 6,
                remaining: 5
            }
        );
    }

    #[test]
    fn empty_reader() {
        let data = [];
        let r = BitReader::new(&data, 0);
        assert_eq!(r.bits_remaining(), 0);
    }

    #[test]
    fn total_bits_clamped_to_slice() {
        let data = [0xFF; 2]; // 16 bits of backing data
        let r = BitReader::new(&data, 100); // claim 100 bits
        assert_eq!(r.bits_remaining(), 16); // clamped to 16
    }

    #[test]
    fn read_bool_sequence() {
        let data = [0b10110010];
        let mut r = BitReader::new(&data, 8);
        let bools: Vec<bool> = (0..8).map(|_| r.read_bool().unwrap()).collect();
        assert_eq!(
            bools,
            vec![true, false, true, true, false, false, true, false]
        );
    }

    #[test]
    fn bits_remaining_decrements() {
        let data = [0xAB, 0xCD];
        let mut r = BitReader::new(&data, 16);
        assert_eq!(r.bits_remaining(), 16);
        r.read_bits(5).unwrap();
        assert_eq!(r.bits_remaining(), 11);
        r.read_bits(11).unwrap();
        assert_eq!(r.bits_remaining(), 0);
    }
}
