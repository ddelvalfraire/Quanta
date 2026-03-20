use super::export::{MAGIC, FORMAT_VERSION};
use super::types::*;

#[derive(Debug, Clone, PartialEq)]
pub enum CompatibilityResult {
    Identical,
    Compatible { details: String },
    Incompatible { details: String },
}

/// Import a `CompiledSchema` from the QSCH binary format produced by `export_schema`.
pub fn import_schema(bytes: &[u8]) -> Result<CompiledSchema, SchemaError> {
    let read_u8 = |pos: &mut usize, data: &[u8]| -> Result<u8, SchemaError> {
        if *pos >= data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = data[*pos];
        *pos += 1;
        Ok(v)
    };

    let read_u16 = |pos: &mut usize, data: &[u8]| -> Result<u16, SchemaError> {
        if *pos + 2 > data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
        *pos += 2;
        Ok(v)
    };

    let read_u32 = |pos: &mut usize, data: &[u8]| -> Result<u32, SchemaError> {
        if *pos + 4 > data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
        *pos += 4;
        Ok(v)
    };

    let read_f64 = |pos: &mut usize, data: &[u8]| -> Result<f64, SchemaError> {
        if *pos + 8 > data.len() {
            return Err(SchemaError::ParseError("unexpected end of data".into()));
        }
        let v = f64::from_be_bytes([
            data[*pos],
            data[*pos + 1],
            data[*pos + 2],
            data[*pos + 3],
            data[*pos + 4],
            data[*pos + 5],
            data[*pos + 6],
            data[*pos + 7],
        ]);
        *pos += 8;
        Ok(v)
    };

    // Header: magic
    if bytes.len() < 4 {
        return Err(SchemaError::ParseError("data too short for magic bytes".into()));
    }
    if &bytes[0..4] != MAGIC {
        return Err(SchemaError::ParseError(format!(
            "invalid magic bytes: expected QSCH, got {:?}",
            &bytes[0..4]
        )));
    }
    let mut pos = 4;

    // format version
    let fmt_ver = read_u8(&mut pos, bytes)?;
    if fmt_ver != FORMAT_VERSION {
        return Err(SchemaError::ParseError(format!(
            "unsupported format version: expected {}, got {}",
            FORMAT_VERSION, fmt_ver
        )));
    }

    let version = read_u8(&mut pos, bytes)?;
    let field_count = read_u16(&mut pos, bytes)?;
    let group_count = read_u8(&mut pos, bytes)?;
    let total_bits = read_u32(&mut pos, bytes)?;
    let bitmask_byte_count = read_u8(&mut pos, bytes)?;

    // Fields
    let mut fields = Vec::with_capacity(field_count as usize);
    for _ in 0..field_count {
        let name_len = read_u16(&mut pos, bytes)?;
        if pos + name_len as usize > bytes.len() {
            return Err(SchemaError::ParseError("unexpected end of data reading field name".into()));
        }
        let name = std::str::from_utf8(&bytes[pos..pos + name_len as usize])
            .map_err(|e| SchemaError::ParseError(format!("invalid UTF-8 in field name: {e}")))?
            .to_string();
        pos += name_len as usize;

        let type_byte = read_u8(&mut pos, bytes)?;
        let bit_width = read_u8(&mut pos, bytes)?;
        let bit_offset = read_u32(&mut pos, bytes)?;
        let group_index = read_u8(&mut pos, bytes)?;
        let flags = read_u8(&mut pos, bytes)?;
        let prediction_byte = read_u8(&mut pos, bytes)?;
        let interpolation_byte = read_u8(&mut pos, bytes)?;

        let skip_delta = (flags & 0x01) != 0;
        let has_quantization = (flags & 0x02) != 0;
        let has_smoothing = (flags & 0x04) != 0;

        let quantization = if has_quantization {
            let min = read_f64(&mut pos, bytes)?;
            let max = read_f64(&mut pos, bytes)?;
            let precision = read_f64(&mut pos, bytes)?;
            // Reconstruct num_values and mask from min/max/precision
            let num_values = ((max - min) / precision).floor() as u64 + 1;
            let bits = ceil_log2_u64(num_values);
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
            let mode_byte = read_u8(&mut pos, bytes)?;
            let param_count = read_u8(&mut pos, bytes)?;
            let mut params = Vec::with_capacity(param_count as usize);
            for _ in 0..param_count {
                params.push(read_f64(&mut pos, bytes)?);
            }
            let mode = SmoothingMode::from_byte(mode_byte).ok_or_else(|| {
                SchemaError::ParseError(format!("invalid smoothing mode byte: {mode_byte}"))
            })?;
            Some(SmoothingParams { mode, params })
        } else {
            None
        };

        // For Enum/Flags we need the variant count — derive it from bit_width
        let variant_count = match type_byte {
            12 => {
                // Enum: bit_width = ceil_log2(variant_count), so variant_count = 2^bit_width
                // but that's an upper bound. We store what we can reconstruct.
                if bit_width == 0 { 1 } else { 1u16 << bit_width }
            }
            13 => {
                // Flags: bit_width == flag_count
                bit_width as u16
            }
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

        fields.push(FieldMeta {
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
        });
    }

    // Groups
    let mut field_groups = Vec::with_capacity(group_count as usize);
    for _ in 0..group_count {
        let name_len = read_u16(&mut pos, bytes)?;
        if pos + name_len as usize > bytes.len() {
            return Err(SchemaError::ParseError(
                "unexpected end of data reading group name".into(),
            ));
        }
        let name = std::str::from_utf8(&bytes[pos..pos + name_len as usize])
            .map_err(|e| SchemaError::ParseError(format!("invalid UTF-8 in group name: {e}")))?
            .to_string();
        pos += name_len as usize;

        let priority_byte = read_u8(&mut pos, bytes)?;
        let priority = Priority::from_byte(priority_byte).ok_or_else(|| {
            SchemaError::ParseError(format!("invalid priority byte: {priority_byte}"))
        })?;

        let max_tick_rate = read_u16(&mut pos, bytes)?;
        let range_start = read_u16(&mut pos, bytes)?;
        let range_end = read_u16(&mut pos, bytes)?;

        field_groups.push(FieldGroup {
            name,
            priority,
            max_tick_rate,
            bitmask_range: (range_start, range_end),
        });
    }

    Ok(CompiledSchema {
        version,
        fields,
        field_groups,
        total_bits,
        bitmask_byte_count,
    })
}

/// Compare two compiled schemas for append-only compatibility.
pub fn check_schema_compatibility(
    old: &CompiledSchema,
    new: &CompiledSchema,
) -> CompatibilityResult {
    let min_len = old.fields.len().min(new.fields.len());

    // Check existing fields match position-by-position
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

    if new.fields.len() == old.fields.len() {
        CompatibilityResult::Identical
    } else {
        let added: Vec<&str> = new.fields[old.fields.len()..]
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        CompatibilityResult::Compatible {
            details: format!("appended {} field(s): {}", added.len(), added.join(", ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::export::export_schema;

    fn minimal_schema() -> CompiledSchema {
        CompiledSchema {
            version: 1,
            fields: vec![FieldMeta {
                name: "alive".to_string(),
                field_type: FieldType::Bool,
                bit_width: 1,
                bit_offset: 0,
                group_index: 0,
                quantization: None,
                prediction: PredictionMode::None,
                smoothing: None,
                interpolation: InterpolationMode::None,
                skip_delta: false,
            }],
            field_groups: vec![FieldGroup {
                name: "default".to_string(),
                priority: Priority::Medium,
                max_tick_rate: 0,
                bitmask_range: (0, 1),
            }],
            total_bits: 1,
            bitmask_byte_count: 1,
        }
    }

    fn two_field_schema() -> CompiledSchema {
        CompiledSchema {
            version: 1,
            fields: vec![
                FieldMeta {
                    name: "alive".to_string(),
                    field_type: FieldType::Bool,
                    bit_width: 1,
                    bit_offset: 0,
                    group_index: 0,
                    quantization: None,
                    prediction: PredictionMode::None,
                    smoothing: None,
                    interpolation: InterpolationMode::None,
                    skip_delta: false,
                },
                FieldMeta {
                    name: "health".to_string(),
                    field_type: FieldType::U16,
                    bit_width: 16,
                    bit_offset: 1,
                    group_index: 0,
                    quantization: None,
                    prediction: PredictionMode::None,
                    smoothing: None,
                    interpolation: InterpolationMode::None,
                    skip_delta: false,
                },
            ],
            field_groups: vec![FieldGroup {
                name: "default".to_string(),
                priority: Priority::Medium,
                max_tick_rate: 0,
                bitmask_range: (0, 2),
            }],
            total_bits: 17,
            bitmask_byte_count: 1,
        }
    }

    fn schema_with_quantization_and_smoothing() -> CompiledSchema {
        CompiledSchema {
            version: 1,
            fields: vec![
                FieldMeta {
                    name: "x".to_string(),
                    field_type: FieldType::F32,
                    bit_width: 21,
                    bit_offset: 0,
                    group_index: 0,
                    quantization: Some(QuantizationParams {
                        min: -10000.0,
                        max: 10000.0,
                        precision: 0.01,
                        num_values: 2_000_001,
                        mask: (1u64 << 21) - 1,
                    }),
                    prediction: PredictionMode::InputReplay,
                    smoothing: Some(SmoothingParams {
                        mode: SmoothingMode::Lerp,
                        params: vec![0.1],
                    }),
                    interpolation: InterpolationMode::Linear,
                    skip_delta: false,
                },
                FieldMeta {
                    name: "alive".to_string(),
                    field_type: FieldType::Bool,
                    bit_width: 1,
                    bit_offset: 21,
                    group_index: 0,
                    quantization: None,
                    prediction: PredictionMode::None,
                    smoothing: None,
                    interpolation: InterpolationMode::None,
                    skip_delta: false,
                },
            ],
            field_groups: vec![FieldGroup {
                name: "default".to_string(),
                priority: Priority::Medium,
                max_tick_rate: 0,
                bitmask_range: (0, 2),
            }],
            total_bits: 22,
            bitmask_byte_count: 1,
        }
    }

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
}
