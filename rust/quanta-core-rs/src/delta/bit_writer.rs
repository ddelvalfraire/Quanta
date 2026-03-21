/// Accumulates bits into a byte buffer.
///
/// Bits are written MSB-first into a 64-bit scratch register.
/// Full bytes are flushed to the output buffer as they accumulate.
pub struct BitWriter {
    buf: Vec<u8>,
    scratch: u64,
    bits_in_scratch: u8,
}

impl BitWriter {
    /// Create a writer pre-allocated for `capacity_bits` of output.
    pub fn new(capacity_bits: u32) -> Self {
        let bytes = (capacity_bits as usize).div_ceil(8);
        Self {
            buf: Vec::with_capacity(bytes),
            scratch: 0,
            bits_in_scratch: 0,
        }
    }

    /// Write the low `bits` of `value` (1..=64).
    pub fn write_bits(&mut self, value: u64, bits: u8) {
        debug_assert!((1..=64).contains(&bits), "bits must be 1..=64, got {bits}");

        let masked = if bits == 64 {
            value
        } else {
            value & ((1u64 << bits) - 1)
        };

        let space = 64 - self.bits_in_scratch;
        if bits <= space {
            self.scratch |= masked << (space - bits);
            self.bits_in_scratch += bits;
        } else {
            self.scratch |= masked >> (bits - space);
            self.bits_in_scratch = 64;
            self.flush_full_bytes();
            let remaining = bits - space;
            self.scratch = masked << (64 - remaining);
            self.bits_in_scratch = remaining;
        }

        self.flush_full_bytes();
    }

    /// Convenience: write a single bit.
    pub fn write_bool(&mut self, value: bool) {
        self.write_bits(value as u64, 1);
    }

    /// Flush remaining bits (zero-padded to byte boundary) and return the buffer.
    pub fn finish(mut self) -> Vec<u8> {
        if self.bits_in_scratch > 0 {
            self.buf.push((self.scratch >> 56) as u8);
        }
        self.buf
    }

    fn flush_full_bytes(&mut self) {
        while self.bits_in_scratch >= 8 {
            self.buf.push((self.scratch >> 56) as u8);
            self.scratch <<= 8;
            self.bits_in_scratch -= 8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta::BitReader;

    #[test]
    fn roundtrip_various_widths() {
        let cases: &[(u64, u8)] = &[
            (1, 1),
            (0, 1),
            (0x5A, 7),
            (0xFF, 8),
            (0x1FFF, 13),
            (0x1FFFFF, 21),
            (0xDEADBEEF, 32),
            (u64::MAX, 64),
        ];

        let total_bits: u32 = cases.iter().map(|&(_, b)| b as u32).sum();
        let mut w = BitWriter::new(total_bits);
        for &(val, bits) in cases {
            w.write_bits(val, bits);
        }
        let data = w.finish();

        let mut r = BitReader::new(&data, total_bits);
        for &(val, bits) in cases {
            let expected = if bits == 64 {
                val
            } else {
                val & ((1u64 << bits) - 1)
            };
            assert_eq!(r.read_bits(bits).unwrap(), expected, "bits={bits}");
        }
        assert_eq!(r.bits_remaining(), 0);
    }

    #[test]
    fn single_bit() {
        let mut w = BitWriter::new(1);
        w.write_bool(true);
        let data = w.finish();
        assert_eq!(data, vec![0x80]); // 1 bit set, padded with 7 zeros

        let mut r = BitReader::new(&data, 1);
        assert!(r.read_bool().unwrap());
    }

    #[test]
    fn empty_writer() {
        let w = BitWriter::new(0);
        let data = w.finish();
        assert!(data.is_empty());
    }

    #[test]
    fn exactly_64_bits() {
        let mut w = BitWriter::new(64);
        w.write_bits(0xCAFEBABEDEADC0DE, 64);
        let data = w.finish();
        assert_eq!(data.len(), 8);

        let mut r = BitReader::new(&data, 64);
        assert_eq!(r.read_bits(64).unwrap(), 0xCAFEBABEDEADC0DE);
    }

    #[test]
    fn write_64_bits_into_nonempty_scratch() {
        let mut w = BitWriter::new(3 + 64);
        w.write_bits(0b101, 3);
        w.write_bits(0xCAFEBABEDEADC0DE, 64);
        let data = w.finish();

        let mut r = BitReader::new(&data, 3 + 64);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        assert_eq!(r.read_bits(64).unwrap(), 0xCAFEBABEDEADC0DE);
    }

    #[test]
    fn cross_byte_boundary() {
        let mut w = BitWriter::new(10);
        w.write_bits(0b10110, 5);
        w.write_bits(0b01101, 5);
        let data = w.finish();

        let mut r = BitReader::new(&data, 10);
        assert_eq!(r.read_bits(5).unwrap(), 0b10110);
        assert_eq!(r.read_bits(5).unwrap(), 0b01101);
    }

    #[test]
    fn write_bool_sequence() {
        let mut w = BitWriter::new(8);
        for b in [true, false, true, true, false, false, true, false] {
            w.write_bool(b);
        }
        let data = w.finish();
        assert_eq!(data, vec![0b10110010]);
    }
}
