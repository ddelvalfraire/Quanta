use super::types::*;

/// Binary export magic bytes.
pub const MAGIC: &[u8; 4] = b"QSCH";

/// Current format version.
pub const FORMAT_VERSION: u8 = 1;

/// Per-field flag bits (shared between export and import).
pub const FLAG_SKIP_DELTA: u8 = 0x01;
pub const FLAG_HAS_QUANTIZATION: u8 = 0x02;
pub const FLAG_HAS_SMOOTHING: u8 = 0x04;

/// Maximum field count we accept on import (defense-in-depth).
pub const MAX_FIELD_COUNT: u16 = 4096;

/// Export a CompiledSchema to deterministic big-endian binary format.
///
/// Layout:
/// ```text
/// [magic: "QSCH"][format_ver: u8][schema_ver: u8][field_count: u16][group_count: u8]
/// [total_bits: u32][bitmask_byte_count: u8]
/// per field: [name_len:u16][name][type:u8][bit_width:u8][bit_offset:u32][group_idx:u8]
///            [flags:u8][prediction:u8][interpolation:u8][quantization?][smoothing?]
/// per group: [name_len:u16][name][priority:u8][tick_rate:u16][range_start:u16][range_end:u16]
/// ```
pub fn export_schema(schema: &CompiledSchema) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(MAGIC);
    buf.push(FORMAT_VERSION);
    buf.push(schema.version);
    buf.extend_from_slice(&(schema.fields.len() as u16).to_be_bytes());
    buf.push(schema.field_groups.len() as u8);
    buf.extend_from_slice(&schema.total_bits.to_be_bytes());
    buf.push(schema.bitmask_byte_count);

    // Fields
    for field in &schema.fields {
        let name_bytes = field.name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
        buf.extend_from_slice(name_bytes);

        buf.push(field.field_type.type_byte());
        buf.push(field.bit_width);
        buf.extend_from_slice(&field.bit_offset.to_be_bytes());
        buf.push(field.group_index);

        let mut flags: u8 = 0;
        if field.skip_delta {
            flags |= FLAG_SKIP_DELTA;
        }
        if field.quantization.is_some() {
            flags |= FLAG_HAS_QUANTIZATION;
        }
        if field.smoothing.is_some() {
            flags |= FLAG_HAS_SMOOTHING;
        }
        buf.push(flags);

        buf.push(field.prediction.as_byte());
        buf.push(field.interpolation.as_byte());

        // Optional quantization params
        if let Some(ref qp) = field.quantization {
            buf.extend_from_slice(&qp.min.to_be_bytes());
            buf.extend_from_slice(&qp.max.to_be_bytes());
            buf.extend_from_slice(&qp.precision.to_be_bytes());
        }

        // Optional smoothing params
        if let Some(ref sp) = field.smoothing {
            buf.push(sp.mode.as_byte());
            buf.push(sp.params.len() as u8);
            for p in &sp.params {
                buf.extend_from_slice(&p.to_be_bytes());
            }
        }
    }

    // Groups
    for group in &schema.field_groups {
        let name_bytes = group.name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
        buf.extend_from_slice(name_bytes);
        buf.push(group.priority.as_byte());
        buf.extend_from_slice(&group.max_tick_rate.to_be_bytes());
        buf.extend_from_slice(&group.bitmask_range.0.to_be_bytes());
        buf.extend_from_slice(&group.bitmask_range.1.to_be_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::test_fixtures::minimal_schema;

    #[test]
    fn magic_bytes() {
        let bytes = export_schema(&minimal_schema());
        assert_eq!(&bytes[0..4], b"QSCH");
    }

    #[test]
    fn format_version() {
        let bytes = export_schema(&minimal_schema());
        assert_eq!(bytes[4], FORMAT_VERSION);
    }

    #[test]
    fn schema_version() {
        let bytes = export_schema(&minimal_schema());
        assert_eq!(bytes[5], 1);
    }

    #[test]
    fn field_count_encoded() {
        let bytes = export_schema(&minimal_schema());
        let field_count = u16::from_be_bytes([bytes[6], bytes[7]]);
        assert_eq!(field_count, 1);
    }

    #[test]
    fn group_count_encoded() {
        let bytes = export_schema(&minimal_schema());
        assert_eq!(bytes[8], 1);
    }

    #[test]
    fn total_bits_encoded() {
        let bytes = export_schema(&minimal_schema());
        let total_bits = u32::from_be_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
        assert_eq!(total_bits, 1);
    }

    #[test]
    fn bitmask_byte_count_encoded() {
        let bytes = export_schema(&minimal_schema());
        assert_eq!(bytes[13], 1);
    }

    #[test]
    fn deterministic_export() {
        let schema = minimal_schema();
        let bytes1 = export_schema(&schema);
        let bytes2 = export_schema(&schema);
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn roundtrip_determinism_with_quantization_and_smoothing() {
        use crate::schema::types::test_fixtures::schema_with_quantization_and_smoothing;
        let schema = schema_with_quantization_and_smoothing();
        let bytes1 = export_schema(&schema);
        let bytes2 = export_schema(&schema);
        assert_eq!(bytes1, bytes2);
        assert_eq!(&bytes1[0..4], b"QSCH");
    }
}
