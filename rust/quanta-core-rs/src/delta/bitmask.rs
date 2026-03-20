use super::DeltaError;

/// Fixed-size bitmap indicating which fields are present in a delta packet.
///
/// Little-endian bit ordering within each byte: field 0 is bit 0 of byte 0,
/// field 8 is bit 0 of byte 1, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldBitmask {
    bytes: Vec<u8>,
    n_fields: u16,
}

impl FieldBitmask {
    /// Allocate a zeroed bitmask for `n_fields` fields.
    pub fn new(n_fields: u16) -> Self {
        let byte_len = Self::byte_len(n_fields);
        Self {
            bytes: vec![0u8; byte_len],
            n_fields,
        }
    }

    /// Set the bit for field `index`.
    pub fn set(&mut self, index: u16) {
        debug_assert!(index < self.n_fields, "index {index} >= n_fields {}", self.n_fields);
        let byte = (index / 8) as usize;
        let bit = index % 8;
        self.bytes[byte] |= 1 << bit;
    }

    /// Test whether field `index` is set.
    pub fn test(&self, index: u16) -> bool {
        debug_assert!(index < self.n_fields, "index {index} >= n_fields {}", self.n_fields);
        let byte = (index / 8) as usize;
        let bit = index % 8;
        self.bytes[byte] & (1 << bit) != 0
    }

    /// Iterate over the indices of set bits, in ascending order.
    pub fn iter_set(&self) -> impl Iterator<Item = u16> + '_ {
        (0..self.n_fields).filter(|&i| self.test(i))
    }

    /// View the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Number of fields this bitmask covers.
    pub fn n_fields(&self) -> u16 {
        self.n_fields
    }

    /// Number of set bits.
    pub fn count_set(&self) -> u16 {
        self.bytes.iter().map(|b| b.count_ones() as u16).sum()
    }

    /// Construct from raw bytes, validating length.
    pub fn from_bytes(bytes: &[u8], n_fields: u16) -> Result<Self, DeltaError> {
        let expected = Self::byte_len(n_fields);
        if bytes.len() != expected {
            return Err(DeltaError::InvalidBitmaskLength {
                expected,
                got: bytes.len(),
            });
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            n_fields,
        })
    }

    fn byte_len(n_fields: u16) -> usize {
        (n_fields as usize).div_ceil(8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bitmask() {
        let bm = FieldBitmask::new(0);
        assert_eq!(bm.as_bytes(), &[] as &[u8]);
        assert_eq!(bm.count_set(), 0);
        assert_eq!(bm.iter_set().collect::<Vec<_>>(), Vec::<u16>::new());
    }

    #[test]
    fn single_field() {
        let mut bm = FieldBitmask::new(1);
        assert!(!bm.test(0));
        bm.set(0);
        assert!(bm.test(0));
        assert_eq!(bm.count_set(), 1);
        assert_eq!(bm.as_bytes(), &[0x01]);
    }

    #[test]
    fn all_set() {
        let n = 16;
        let mut bm = FieldBitmask::new(n);
        for i in 0..n {
            bm.set(i);
        }
        assert_eq!(bm.count_set(), n);
        let indices: Vec<u16> = bm.iter_set().collect();
        assert_eq!(indices, (0..n).collect::<Vec<_>>());
        assert_eq!(bm.as_bytes(), &[0xFF, 0xFF]);
    }

    #[test]
    fn scattered_bits() {
        let mut bm = FieldBitmask::new(20);
        bm.set(0);
        bm.set(5);
        bm.set(10);
        bm.set(19);
        let indices: Vec<u16> = bm.iter_set().collect();
        assert_eq!(indices, vec![0, 5, 10, 19]);
        assert_eq!(bm.count_set(), 4);
    }

    #[test]
    fn from_bytes_roundtrip() {
        let mut bm = FieldBitmask::new(12);
        bm.set(3);
        bm.set(11);
        let bytes = bm.as_bytes().to_vec();
        let bm2 = FieldBitmask::from_bytes(&bytes, 12).unwrap();
        assert_eq!(bm, bm2);
    }

    #[test]
    fn from_bytes_wrong_length() {
        let err = FieldBitmask::from_bytes(&[0, 0, 0], 10).unwrap_err();
        assert_eq!(
            err,
            DeltaError::InvalidBitmaskLength {
                expected: 2,
                got: 3
            }
        );
    }

    #[test]
    fn byte_boundary_alignment() {
        // 8 fields should be exactly 1 byte; 9 fields should be 2.
        assert_eq!(FieldBitmask::new(8).as_bytes().len(), 1);
        assert_eq!(FieldBitmask::new(9).as_bytes().len(), 2);
    }
}
