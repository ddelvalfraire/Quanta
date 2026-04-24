use super::{CompiledSchema, QuantizationParams};
use crate::delta::encoder::dequantize;

/// Cached field slot for direct bit extraction without iteration.
#[derive(Debug, Clone)]
pub struct FieldSlot {
    pub bit_offset: u32,
    pub bit_width: u8,
    pub quantization: Option<QuantizationParams>,
}

/// Cached layout for fast position extraction from bit-packed entity state.
///
/// Built once from a `CompiledSchema` by scanning for fields named
/// `pos-x`, `pos-y`, `pos-z`. Extracts positions via direct byte math
/// at known offsets — no `BitReader` iteration required.
#[derive(Debug, Clone)]
pub struct PositionLayout {
    pub x: FieldSlot,
    pub y: FieldSlot,
    pub z: FieldSlot,
}

impl PositionLayout {
    /// Returns `None` if any position field is missing.
    pub fn from_schema(schema: &CompiledSchema) -> Option<Self> {
        let mut x = None;
        let mut y = None;
        let mut z = None;

        for field in &schema.fields {
            let slot = || FieldSlot {
                bit_offset: field.bit_offset,
                bit_width: field.bit_width,
                quantization: field.quantization.clone(),
            };
            match field.name.as_str() {
                "pos-x" => x = Some(slot()),
                "pos-y" => y = Some(slot()),
                "pos-z" => z = Some(slot()),
                _ => {}
            }
        }

        Some(Self {
            x: x?,
            y: y?,
            z: z?,
        })
    }

    /// Extract `[x, y, z]` from bit-packed state using cached offsets.
    pub fn extract(&self, state: &[u8]) -> [f32; 3] {
        [
            Self::read_field(&self.x, state),
            Self::read_field(&self.y, state),
            Self::read_field(&self.z, state),
        ]
    }

    fn read_field(slot: &FieldSlot, state: &[u8]) -> f32 {
        let raw = read_bits_at(state, slot.bit_offset, slot.bit_width);
        match &slot.quantization {
            Some(params) => dequantize(raw, params) as f32,
            None => f32::from_bits(raw as u32),
        }
    }
}

/// Read `bit_width` bits starting at `bit_offset` from `data` (MSB-first).
///
/// Returns 0 if the requested range extends past the end of `data`.
pub fn read_bits_at(data: &[u8], bit_offset: u32, bit_width: u8) -> u64 {
    if bit_width == 0 || bit_width > 64 {
        return 0;
    }

    let end_bit = bit_offset as u64 + bit_width as u64;
    let needed_bytes = end_bit.div_ceil(8) as usize;
    if data.len() < needed_bytes {
        return 0;
    }

    let mut value: u64 = 0;
    let mut bits_left = bit_width;
    let mut pos = bit_offset;

    while bits_left > 0 {
        let byte_idx = (pos / 8) as usize;
        let bit_off = (pos % 8) as u8;
        let available = 8 - bit_off;
        let take = bits_left.min(available);

        let shifted = (data[byte_idx] >> (available - take)) as u64;
        let mask = (1u64 << take) - 1;
        value = (value << take) | (shifted & mask);

        pos += take as u32;
        bits_left -= take;
    }

    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta::encoder::{dequantize, read_state, write_state};
    use crate::schema::{compile_schema, CompileOptions};

    fn quantized_pos_schema() -> CompiledSchema {
        let wit = r#"
record entity-state {
    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    pos-x: f32,

    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    pos-y: f32,

    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    pos-z: f32,

    health: u16,
}
"#;
        let (schema, _) = compile_schema(wit, "entity-state", &CompileOptions::default()).unwrap();
        schema
    }

    fn unquantized_pos_schema() -> CompiledSchema {
        let wit = r#"
record entity-state {
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    pos-x: f32,

    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    pos-y: f32,

    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    pos-z: f32,

    health: u16,
}
"#;
        let (schema, _) = compile_schema(wit, "entity-state", &CompileOptions::default()).unwrap();
        schema
    }

    #[test]
    fn extract_quantized_matches_full_decode() {
        let schema = quantized_pos_schema();
        let layout = PositionLayout::from_schema(&schema).unwrap();

        let px = &schema.fields.iter().find(|f| f.name == "pos-x").unwrap();
        let py = &schema.fields.iter().find(|f| f.name == "pos-y").unwrap();
        let pz = &schema.fields.iter().find(|f| f.name == "pos-z").unwrap();
        let qx = px.quantization.as_ref().unwrap();
        let qy = py.quantization.as_ref().unwrap();
        let qz = pz.quantization.as_ref().unwrap();

        let vx = crate::delta::encoder::quantize(1234.56, qx).unwrap();
        let vy = crate::delta::encoder::quantize(-500.0, qy).unwrap();
        let vz = crate::delta::encoder::quantize(9999.99, qz).unwrap();

        let mut values = vec![0u64; schema.fields.len()];
        for (i, field) in schema.fields.iter().enumerate() {
            match field.name.as_str() {
                "pos-x" => values[i] = vx,
                "pos-y" => values[i] = vy,
                "pos-z" => values[i] = vz,
                "health" => values[i] = 100,
                _ => {}
            }
        }

        let state = write_state(&schema, &values);
        let [ex, ey, ez] = layout.extract(&state);

        let full = read_state(&schema, &state).unwrap();
        let fx_idx = schema
            .fields
            .iter()
            .position(|f| f.name == "pos-x")
            .unwrap();
        let fy_idx = schema
            .fields
            .iter()
            .position(|f| f.name == "pos-y")
            .unwrap();
        let fz_idx = schema
            .fields
            .iter()
            .position(|f| f.name == "pos-z")
            .unwrap();

        let full_x = dequantize(full[fx_idx], qx) as f32;
        let full_y = dequantize(full[fy_idx], qy) as f32;
        let full_z = dequantize(full[fz_idx], qz) as f32;

        assert!(
            (ex - full_x).abs() < 0.001,
            "x mismatch: extract={ex}, full={full_x}"
        );
        assert!(
            (ey - full_y).abs() < 0.001,
            "y mismatch: extract={ey}, full={full_y}"
        );
        assert!(
            (ez - full_z).abs() < 0.001,
            "z mismatch: extract={ez}, full={full_z}"
        );
    }

    #[test]
    fn extract_unquantized_f32_fields() {
        let schema = unquantized_pos_schema();
        let layout = PositionLayout::from_schema(&schema).unwrap();

        let x_val: f32 = 42.5;
        let y_val: f32 = -100.25;
        let z_val: f32 = 0.0;

        let mut values = vec![0u64; schema.fields.len()];
        for (i, field) in schema.fields.iter().enumerate() {
            match field.name.as_str() {
                "pos-x" => values[i] = x_val.to_bits() as u64,
                "pos-y" => values[i] = y_val.to_bits() as u64,
                "pos-z" => values[i] = z_val.to_bits() as u64,
                "health" => values[i] = 50,
                _ => {}
            }
        }

        let state = write_state(&schema, &values);
        let [ex, ey, ez] = layout.extract(&state);

        assert_eq!(ex, x_val);
        assert_eq!(ey, y_val);
        assert_eq!(ez, z_val);
    }

    #[test]
    fn from_schema_returns_none_when_missing() {
        let wit = r#"
record entity-state {
    health: u16,
    mana: u16,
}
"#;
        let (schema, _) = compile_schema(wit, "entity-state", &CompileOptions::default()).unwrap();
        assert!(PositionLayout::from_schema(&schema).is_none());
    }

    #[test]
    fn from_schema_returns_none_partial_fields() {
        let wit = r#"
record entity-state {
    pos-x: f32,
    pos-y: f32,
    health: u16,
}
"#;
        let (schema, _) = compile_schema(wit, "entity-state", &CompileOptions::default()).unwrap();
        assert!(PositionLayout::from_schema(&schema).is_none());
    }

    #[test]
    fn read_bits_at_various_offsets() {
        let data = [0b10110011, 0b01010101, 0b11110000];

        assert_eq!(read_bits_at(&data, 0, 4), 0b1011);
        assert_eq!(read_bits_at(&data, 4, 4), 0b0011);
        assert_eq!(read_bits_at(&data, 6, 8), 0b11010101);
        assert_eq!(read_bits_at(&data, 0, 1), 1);
        assert_eq!(read_bits_at(&data, 1, 1), 0);
        assert_eq!(read_bits_at(&data, 0, 16), 0b10110011_01010101);
    }

    #[test]
    fn read_bits_at_matches_sequential_read() {
        use crate::delta::BitReader;

        let data = [0xDE, 0xAD, 0xBE, 0xEF, 0x42];
        let widths = [5, 3, 8, 12, 7, 5]; // total = 40 bits = 5 bytes

        let mut reader = BitReader::new(&data, 40);
        let mut offset = 0u32;

        for &width in &widths {
            let sequential = reader.read_bits(width).unwrap();
            let direct = read_bits_at(&data, offset, width);
            assert_eq!(
                sequential, direct,
                "mismatch at offset={offset}, width={width}: sequential={sequential}, direct={direct}"
            );
            offset += width as u32;
        }
    }

    #[test]
    fn read_bits_at_short_buffer_returns_zero() {
        let data = [0xFF];
        // Requesting bits past the end of the buffer
        assert_eq!(read_bits_at(&data, 0, 16), 0);
        assert_eq!(read_bits_at(&data, 8, 1), 0);
        // Empty data
        assert_eq!(read_bits_at(&[], 0, 8), 0);
    }

    #[test]
    fn read_bits_at_zero_width_returns_zero() {
        let data = [0xFF];
        assert_eq!(read_bits_at(&data, 0, 0), 0);
    }

    #[test]
    fn extract_from_short_state_does_not_panic() {
        let schema = quantized_pos_schema();
        let layout = PositionLayout::from_schema(&schema).unwrap();
        // Truncated state — must not panic (returns fallback values)
        let _ = layout.extract(&[0u8; 2]);
        let _ = layout.extract(&[]);
    }
}
