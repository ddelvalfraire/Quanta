use super::export::{
    FLAG_HAS_QUANTIZATION, FLAG_HAS_SMOOTHING, FLAG_SKIP_DELTA, FORMAT_VERSION, MAGIC,
    MAX_FIELD_COUNT,
};
use super::types::*;

#[derive(Debug, Clone, PartialEq)]
pub enum CompatibilityResult {
    Identical,
    Compatible { details: String },
    Incompatible { details: String },
}

/// Cursor over a byte slice for incremental parsing.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, SchemaError> {
        if self.pos >= self.data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u16(&mut self) -> Result<u16, SchemaError> {
        if self.pos + 2 > self.data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, SchemaError> {
        if self.pos + 4 > self.data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = u32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_f64(&mut self) -> Result<f64, SchemaError> {
        if self.pos + 8 > self.data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = f64::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(v)
    }

    fn read_string(&mut self) -> Result<String, SchemaError> {
        let len = self.read_u16()? as usize;
        if len > MAX_STRING_LEN {
            return Err(SchemaError::ParseError(format!(
                "string length {} exceeds maximum {}",
                len, MAX_STRING_LEN
            )));
        }
        if self.pos + len > self.data.len() {
            return Err(SchemaError::ParseError(
                "unexpected end of data reading string".into(),
            ));
        }
        let s = std::str::from_utf8(&self.data[self.pos..self.pos + len])
            .map_err(|e| SchemaError::ParseError(format!("invalid UTF-8: {e}")))?
            .to_string();
        self.pos += len;
        Ok(s)
    }
}

const MAX_STRING_LEN: usize = 4096;

/// Import a `CompiledSchema` from the QSCH binary format produced by `export_schema`.
pub fn import_schema(bytes: &[u8]) -> Result<CompiledSchema, SchemaError> {
    if bytes.len() < 4 {
        return Err(SchemaError::ParseError("data too short for magic bytes".into()));
    }
    if &bytes[0..4] != MAGIC {
        return Err(SchemaError::ParseError(format!(
            "invalid magic bytes: expected QSCH, got {:?}",
            &bytes[0..4]
        )));
    }

    let mut r = Reader::new(bytes);
    r.pos = 4;

    let fmt_ver = r.read_u8()?;
    if fmt_ver != FORMAT_VERSION {
        return Err(SchemaError::ParseError(format!(
            "unsupported format version: expected {}, got {}",
            FORMAT_VERSION, fmt_ver
        )));
    }

    let version = r.read_u8()?;
    let field_count = r.read_u16()?;
    if field_count > MAX_FIELD_COUNT {
        return Err(SchemaError::ParseError(format!(
            "field count {} exceeds maximum {}",
            field_count, MAX_FIELD_COUNT
        )));
    }
    let group_count = r.read_u8()?;
    let total_bits = r.read_u32()?;
    let bitmask_byte_count = r.read_u8()?;

    let mut fields = Vec::with_capacity(field_count as usize);
    for _ in 0..field_count {
        fields.push(parse_field(&mut r)?);
    }

    let mut field_groups = Vec::with_capacity(group_count as usize);
    for _ in 0..group_count {
        field_groups.push(parse_group(&mut r)?);
    }

    Ok(CompiledSchema {
        version,
        fields,
        field_groups,
        total_bits,
        bitmask_byte_count,
    })
}

fn parse_field(r: &mut Reader) -> Result<FieldMeta, SchemaError> {
    let name = r.read_string()?;
    let type_byte = r.read_u8()?;
    let bit_width = r.read_u8()?;
    let bit_offset = r.read_u32()?;
    let group_index = r.read_u8()?;
    let flags = r.read_u8()?;
    let prediction_byte = r.read_u8()?;
    let interpolation_byte = r.read_u8()?;

    let skip_delta = (flags & FLAG_SKIP_DELTA) != 0;
    let has_quantization = (flags & FLAG_HAS_QUANTIZATION) != 0;
    let has_smoothing = (flags & FLAG_HAS_SMOOTHING) != 0;

    let quantization = if has_quantization {
        let min = r.read_f64()?;
        let max = r.read_f64()?;
        let precision = r.read_f64()?;

        let (num_values, bits) = quantize_bits(precision, min, max).ok_or_else(|| {
            SchemaError::ParseError(format!(
                "invalid quantization range for field {name}: min={min}, max={max}, precision={precision}"
            ))
        })?;
        let mask = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
        Some(QuantizationParams {
            min,
            max,
            precision,
            num_values,
            mask,
        })
    } else {
        None
    };

    let smoothing = if has_smoothing {
        let mode_byte = r.read_u8()?;
        let param_count = r.read_u8()?;
        let mut params = Vec::with_capacity(param_count as usize);
        for _ in 0..param_count {
            params.push(r.read_f64()?);
        }
        let mode = SmoothingMode::from_byte(mode_byte).ok_or_else(|| {
            SchemaError::ParseError(format!("invalid smoothing mode byte: {mode_byte}"))
        })?;
        Some(SmoothingParams { mode, params })
    } else {
        None
    };

    // Enum/Flags variant count is lossy: we reconstruct the upper bound from bit_width.
    // Enum(3) with bit_width=2 imports as Enum(4). This does not affect compatibility
    // checks which compare the full FieldType (including variant count).
    let variant_count = match type_byte {
        12 /* Enum */ => {
            if bit_width == 0 { 1 } else { 1u16 << bit_width }
        }
        13 /* Flags */ => bit_width as u16,
        _ => 0,
    };

    let field_type = FieldType::from_byte(type_byte, variant_count).ok_or_else(|| {
        SchemaError::ParseError(format!("invalid field type byte: {type_byte}"))
    })?;

    let prediction = PredictionMode::from_byte(prediction_byte).ok_or_else(|| {
        SchemaError::ParseError(format!("invalid prediction mode byte: {prediction_byte}"))
    })?;

    let interpolation = InterpolationMode::from_byte(interpolation_byte).ok_or_else(|| {
        SchemaError::ParseError(format!(
            "invalid interpolation mode byte: {interpolation_byte}"
        ))
    })?;

    Ok(FieldMeta {
        name,
        field_type,
        bit_width,
        bit_offset,
        group_index,
        quantization,
        prediction,
        smoothing,
        interpolation,
        skip_delta,
    })
}

fn parse_group(r: &mut Reader) -> Result<FieldGroup, SchemaError> {
    let name = r.read_string()?;
    let priority_byte = r.read_u8()?;
    let priority = Priority::from_byte(priority_byte).ok_or_else(|| {
        SchemaError::ParseError(format!("invalid priority byte: {priority_byte}"))
    })?;
    let max_tick_rate = r.read_u16()?;
    let range_start = r.read_u16()?;
    let range_end = r.read_u16()?;

    Ok(FieldGroup {
        name,
        priority,
        max_tick_rate,
        bitmask_range: (range_start, range_end),
    })
}

/// Compare two compiled schemas for append-only compatibility.
///
/// Checks name and type at each position. Layout-altering changes (quantization,
/// prediction, smoothing, bit_width) on existing fields are reported as
/// `Compatible`, not `Identical`, since they change the wire encoding.
pub fn check_schema_compatibility(
    old: &CompiledSchema,
    new: &CompiledSchema,
) -> CompatibilityResult {
    let min_len = old.fields.len().min(new.fields.len());
    let mut has_layout_changes = false;

    for i in 0..min_len {
        let old_f = &old.fields[i];
        let new_f = &new.fields[i];

        if old_f.name != new_f.name {
            return CompatibilityResult::Incompatible {
                details: format!(
                    "field {} name changed: {:?} -> {:?}",
                    i, old_f.name, new_f.name
                ),
            };
        }

        if old_f.field_type != new_f.field_type {
            return CompatibilityResult::Incompatible {
                details: format!(
                    "field {} ({}) type changed: {:?} -> {:?}",
                    i, old_f.name, old_f.field_type, new_f.field_type
                ),
            };
        }

        if old_f != new_f {
            has_layout_changes = true;
        }
    }

    if new.fields.len() < old.fields.len() {
        return CompatibilityResult::Incompatible {
            details: format!(
                "fields removed: old has {} fields, new has {}",
                old.fields.len(),
                new.fields.len()
            ),
        };
    }

    if new.fields.len() > old.fields.len() {
        let added: Vec<&str> = new.fields[old.fields.len()..]
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        return CompatibilityResult::Compatible {
            details: format!("appended {} field(s): {}", added.len(), added.join(", ")),
        };
    }

    if has_layout_changes {
        CompatibilityResult::Compatible {
            details: "field layout changed (quantization, prediction, or bit_width)".into(),
        }
    } else {
        CompatibilityResult::Identical
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::export::export_schema;
    use crate::schema::types::test_fixtures::*;

    // --- import_schema tests ---

    #[test]
    fn import_export_roundtrip_minimal() {
        let original = minimal_schema();
        let bytes = export_schema(&original);
        let imported = import_schema(&bytes).unwrap();
        assert_eq!(original, imported);
    }

    #[test]
    fn import_export_roundtrip_with_quantization_and_smoothing() {
        let original = schema_with_quantization_and_smoothing();
        let bytes = export_schema(&original);
        let imported = import_schema(&bytes).unwrap();
        assert_eq!(original, imported);
    }

    #[test]
    fn import_bad_magic() {
        let mut bytes = export_schema(&minimal_schema());
        bytes[0] = b'X';
        let err = import_schema(&bytes).unwrap_err();
        assert!(err.to_string().contains("magic"));
    }

    #[test]
    fn import_wrong_version() {
        let mut bytes = export_schema(&minimal_schema());
        bytes[4] = 99;
        let err = import_schema(&bytes).unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn import_truncated() {
        let bytes = export_schema(&minimal_schema());
        let err = import_schema(&bytes[..6]).unwrap_err();
        assert!(err.to_string().contains("unexpected end"));
    }

    #[test]
    fn import_empty() {
        let err = import_schema(&[]).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    // --- check_schema_compatibility tests ---

    #[test]
    fn identical_schemas() {
        let schema = minimal_schema();
        assert_eq!(
            check_schema_compatibility(&schema, &schema),
            CompatibilityResult::Identical
        );
    }

    #[test]
    fn append_field_compatible() {
        let old = minimal_schema();
        let new = two_field_schema();
        match check_schema_compatibility(&old, &new) {
            CompatibilityResult::Compatible { details } => {
                assert!(details.contains("health"));
            }
            other => panic!("expected Compatible, got {other:?}"),
        }
    }

    #[test]
    fn remove_field_incompatible() {
        let old = two_field_schema();
        let new = minimal_schema();
        match check_schema_compatibility(&old, &new) {
            CompatibilityResult::Incompatible { details } => {
                assert!(details.contains("removed"));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn change_field_type_incompatible() {
        let old = minimal_schema();
        let mut new = minimal_schema();
        new.fields[0].field_type = FieldType::U8;
        match check_schema_compatibility(&old, &new) {
            CompatibilityResult::Incompatible { details } => {
                assert!(details.contains("type changed"));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn reorder_fields_incompatible() {
        let old = two_field_schema();
        let mut new = two_field_schema();
        new.fields.swap(0, 1);
        match check_schema_compatibility(&old, &new) {
            CompatibilityResult::Incompatible { details } => {
                assert!(details.contains("name changed"));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_then_compatibility() {
        let original = schema_with_quantization_and_smoothing();
        let bytes = export_schema(&original);
        let imported = import_schema(&bytes).unwrap();
        assert_eq!(
            check_schema_compatibility(&original, &imported),
            CompatibilityResult::Identical
        );
    }

    #[test]
    fn layout_change_is_compatible_not_identical() {
        let old = schema_with_quantization_and_smoothing();
        let mut new = old.clone();
        // Change quantization params (same name+type, different bit_width)
        new.fields[0].quantization = Some(QuantizationParams {
            min: -5000.0,
            max: 5000.0,
            precision: 0.01,
            num_values: 1_000_001,
            mask: (1u64 << 20) - 1,
        });
        new.fields[0].bit_width = 20;
        match check_schema_compatibility(&old, &new) {
            CompatibilityResult::Compatible { details } => {
                assert!(details.contains("layout changed"));
            }
            other => panic!("expected Compatible, got {other:?}"),
        }
    }

    #[test]
    fn import_rejects_field_count_over_max() {
        let mut bytes = export_schema(&minimal_schema());
        // Overwrite field_count (bytes 6-7) with MAX_FIELD_COUNT + 1
        let too_many = (super::MAX_FIELD_COUNT + 1).to_be_bytes();
        bytes[6] = too_many[0];
        bytes[7] = too_many[1];
        let err = import_schema(&bytes).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn import_rejects_zero_precision_quantization() {
        let mut schema = schema_with_quantization_and_smoothing();
        schema.fields[0].quantization.as_mut().unwrap().precision = 0.0;
        let bytes = export_schema(&schema);
        let err = import_schema(&bytes).unwrap_err();
        assert!(err.to_string().contains("invalid quantization range"));
    }

    #[test]
    fn enum_variant_count_lossy_on_roundtrip() {
        // Enum(3) has bit_width=2, which imports back as Enum(4).
        // This documents the known lossy behavior.
        let mut schema = minimal_schema();
        schema.fields[0].field_type = FieldType::Enum(3);
        schema.fields[0].bit_width = 2;
        let bytes = export_schema(&schema);
        let imported = import_schema(&bytes).unwrap();
        assert_eq!(imported.fields[0].field_type, FieldType::Enum(4));
    }
}
