use super::{BitReader, BitWriter, DeltaError, DeltaHeader, FieldBitmask, HEADER_SIZE};
use crate::schema::{CompiledSchema, FieldMeta, FieldType, QuantizationParams};

pub fn field_is_active(field: &FieldMeta) -> bool {
    !field.skip_delta && field.bit_width > 0
}

/// Clamps to [min, max], then maps to [0, num_values-1].
pub fn quantize(value: f64, params: &QuantizationParams) -> Result<u64, DeltaError> {
    if value.is_nan() || value.is_infinite() {
        return Err(DeltaError::NaNOrInfinity {
            field: String::new(),
        });
    }
    let clamped = value.clamp(params.min, params.max);
    let normalized = (clamped - params.min) / params.precision;
    let packed = (normalized.round() as u64).min(params.num_values.saturating_sub(1));
    Ok(packed & params.mask)
}

pub fn quantize_field(
    value: f64,
    params: &QuantizationParams,
    field_name: &str,
) -> Result<u64, DeltaError> {
    quantize(value, params).map_err(|e| match e {
        DeltaError::NaNOrInfinity { .. } => DeltaError::NaNOrInfinity {
            field: field_name.to_string(),
        },
        other => other,
    })
}

pub fn dequantize(packed: u64, params: &QuantizationParams) -> f64 {
    let masked = packed & params.mask;
    params.min + (masked as f64) * params.precision
}

pub fn read_state(schema: &CompiledSchema, state: &[u8]) -> Result<Vec<u64>, DeltaError> {
    let expected_bytes = (schema.total_bits as usize).div_ceil(8);
    if state.len() < expected_bytes {
        return Err(DeltaError::StateTooShort {
            expected: expected_bytes,
            got: state.len(),
        });
    }

    let mut values = Vec::with_capacity(schema.fields.len());
    let mut reader = BitReader::new(state, schema.total_bits);

    for field in &schema.fields {
        if !field_is_active(field) {
            values.push(0);
            continue;
        }
        let val = reader.read_bits(field.bit_width)?;
        values.push(val);
    }

    Ok(values)
}

pub fn write_state(schema: &CompiledSchema, values: &[u64]) -> Vec<u8> {
    let mut writer = BitWriter::new(schema.total_bits);

    for (i, field) in schema.fields.iter().enumerate() {
        if !field_is_active(field) {
            continue;
        }
        let val = values.get(i).copied().unwrap_or(0);
        writer.write_bits(val, field.bit_width);
    }

    writer.finish()
}

/// Returns empty Vec when no fields changed. `mask` filters which fields are compared.
pub fn compute_delta(
    schema: &CompiledSchema,
    old: &[u8],
    new: &[u8],
    mask: Option<&FieldBitmask>,
) -> Result<Vec<u8>, DeltaError> {
    let mut output = Vec::new();
    compute_delta_into(schema, old, new, mask, &mut output)?;
    Ok(output)
}

pub fn compute_delta_into(
    schema: &CompiledSchema,
    old: &[u8],
    new: &[u8],
    mask: Option<&FieldBitmask>,
    output: &mut Vec<u8>,
) -> Result<(), DeltaError> {
    let expected_bytes = (schema.total_bits as usize).div_ceil(8);
    if old.len() < expected_bytes {
        return Err(DeltaError::StateTooShort {
            expected: expected_bytes,
            got: old.len(),
        });
    }
    if new.len() < expected_bytes {
        return Err(DeltaError::StateTooShort {
            expected: expected_bytes,
            got: new.len(),
        });
    }

    output.clear();

    let n_fields = schema.fields.len() as u16;
    let old_values = read_state(schema, old)?;
    let new_values = read_state(schema, new)?;

    let mut changed = FieldBitmask::new(n_fields);
    for (i, field) in schema.fields.iter().enumerate() {
        if !field_is_active(field) {
            continue;
        }
        if let Some(m) = mask {
            if !m.test(i as u16) {
                continue;
            }
        }
        if old_values[i] != new_values[i] {
            changed.set(i as u16);
        }
    }

    if changed.count_set() == 0 {
        return Ok(());
    }

    let payload_bits: u32 = changed
        .iter_set()
        .map(|i| schema.fields[i as usize].bit_width as u32)
        .sum();

    let header = DeltaHeader {
        is_full_snapshot: false,
        is_compressed: false,
        has_bitmask: true,
        schema_version: schema.version,
        payload_bits: payload_bits as u16,
    };
    output.extend_from_slice(&header.encode());
    output.extend_from_slice(changed.as_bytes());

    let mut writer = BitWriter::new(payload_bits);
    for idx in changed.iter_set() {
        let field = &schema.fields[idx as usize];
        writer.write_bits(new_values[idx as usize], field.bit_width);
    }
    output.extend_from_slice(&writer.finish());

    Ok(())
}

/// Empty delta returns current state unchanged.
pub fn apply_delta(
    schema: &CompiledSchema,
    current: &[u8],
    delta: &[u8],
) -> Result<Vec<u8>, DeltaError> {
    let mut output = Vec::new();
    apply_delta_into(schema, current, delta, &mut output)?;
    Ok(output)
}

pub fn apply_delta_into(
    schema: &CompiledSchema,
    current: &[u8],
    delta: &[u8],
    output: &mut Vec<u8>,
) -> Result<(), DeltaError> {
    let expected_bytes = (schema.total_bits as usize).div_ceil(8);
    if current.len() < expected_bytes {
        return Err(DeltaError::StateTooShort {
            expected: expected_bytes,
            got: current.len(),
        });
    }

    output.clear();

    if delta.is_empty() {
        output.extend_from_slice(&current[..expected_bytes]);
        return Ok(());
    }

    let header = DeltaHeader::decode(delta).map_err(|_| DeltaError::TruncatedDelta)?;

    if header.schema_version != schema.version {
        return Err(DeltaError::SchemaVersionMismatch {
            expected: schema.version,
            got: header.schema_version,
        });
    }

    if !header.has_bitmask {
        return Err(DeltaError::UnsupportedDeltaFormat);
    }

    let mut values = read_state(schema, current)?;

    let n_fields = schema.fields.len() as u16;
    let bitmask_bytes = (n_fields as usize).div_ceil(8);

    let bitmask_start = HEADER_SIZE;
    let bitmask_end = bitmask_start + bitmask_bytes;
    if delta.len() < bitmask_end {
        return Err(DeltaError::TruncatedDelta);
    }
    let changed = FieldBitmask::from_bytes(&delta[bitmask_start..bitmask_end], n_fields)?;

    let expected_payload_bits: u32 = changed
        .iter_set()
        .filter(|&i| field_is_active(&schema.fields[i as usize]))
        .map(|i| schema.fields[i as usize].bit_width as u32)
        .sum();

    if header.payload_bits as u32 != expected_payload_bits {
        return Err(DeltaError::PayloadBitsMismatch {
            expected: expected_payload_bits,
            got: header.payload_bits,
        });
    }

    let payload_start = bitmask_end;
    let payload_data = &delta[payload_start..];
    let mut reader = BitReader::new(payload_data, header.payload_bits as u32);

    for idx in changed.iter_set() {
        let field = &schema.fields[idx as usize];
        if !field_is_active(field) {
            continue;
        }
        let val = reader
            .read_bits(field.bit_width)
            .map_err(|_| DeltaError::TruncatedDelta)?;
        values[idx as usize] = val;
    }

    let result = write_state(schema, &values);
    output.extend_from_slice(&result);

    Ok(())
}

pub fn is_signed_int(ft: &FieldType) -> bool {
    matches!(
        ft,
        FieldType::S8 | FieldType::S16 | FieldType::S32 | FieldType::S64
    )
}

pub fn sign_extend(value: u64, bit_width: u8) -> i64 {
    if bit_width == 0 || bit_width >= 64 {
        return value as i64;
    }
    let shift = 64 - bit_width;
    ((value << shift) as i64) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::test_fixtures::*;

    #[test]
    fn quantize_dequantize_roundtrip() {
        let params = QuantizationParams {
            min: -10000.0,
            max: 10000.0,
            precision: 0.01,
            num_values: 2_000_001,
            mask: (1u64 << 21) - 1,
        };

        for value in [0.0, 42.42, -9999.99, 10000.0, -10000.0, 5000.5] {
            let packed = quantize(value, &params).unwrap();
            let restored = dequantize(packed, &params);
            assert!(
                (value - restored).abs() < params.precision,
                "value={value}, restored={restored}"
            );
        }
    }

    #[test]
    fn quantize_clamps_out_of_range() {
        let params = QuantizationParams {
            min: 0.0,
            max: 100.0,
            precision: 1.0,
            num_values: 101,
            mask: (1u64 << 7) - 1,
        };

        let packed = quantize(200.0, &params).unwrap();
        let restored = dequantize(packed, &params);
        assert!((restored - 100.0).abs() < 1.0);

        let packed = quantize(-50.0, &params).unwrap();
        let restored = dequantize(packed, &params);
        assert!((restored - 0.0).abs() < 1.0);
    }

    #[test]
    fn quantize_nan_rejected() {
        let params = QuantizationParams {
            min: 0.0,
            max: 1.0,
            precision: 0.01,
            num_values: 101,
            mask: (1u64 << 7) - 1,
        };
        assert!(matches!(
            quantize(f64::NAN, &params),
            Err(DeltaError::NaNOrInfinity { .. })
        ));
    }

    #[test]
    fn quantize_infinity_rejected() {
        let params = QuantizationParams {
            min: 0.0,
            max: 1.0,
            precision: 0.01,
            num_values: 101,
            mask: (1u64 << 7) - 1,
        };
        assert!(matches!(
            quantize(f64::INFINITY, &params),
            Err(DeltaError::NaNOrInfinity { .. })
        ));
        assert!(matches!(
            quantize(f64::NEG_INFINITY, &params),
            Err(DeltaError::NaNOrInfinity { .. })
        ));
    }

    #[test]
    fn quantize_field_populates_name() {
        let params = QuantizationParams {
            min: 0.0,
            max: 1.0,
            precision: 0.01,
            num_values: 101,
            mask: (1u64 << 7) - 1,
        };
        let err = quantize_field(f64::NAN, &params, "pos-x").unwrap_err();
        assert_eq!(
            err,
            DeltaError::NaNOrInfinity {
                field: "pos-x".to_string()
            }
        );
    }

    #[test]
    fn identity_delta_returns_empty() {
        let schema = two_field_schema();
        let state = write_state(&schema, &[1, 100]);
        let delta = compute_delta(&schema, &state, &state, None).unwrap();
        assert!(delta.is_empty());
    }

    #[test]
    fn apply_empty_delta_returns_current() {
        let schema = two_field_schema();
        let state = write_state(&schema, &[1, 100]);
        let result = apply_delta(&schema, &state, &[]).unwrap();
        assert_eq!(result, state);
    }

    #[test]
    fn roundtrip_single_field_changed() {
        let schema = two_field_schema();
        let old = write_state(&schema, &[1, 100]);
        let new = write_state(&schema, &[1, 200]);

        let delta = compute_delta(&schema, &old, &new, None).unwrap();
        assert!(!delta.is_empty());

        let header = DeltaHeader::decode(&delta).unwrap();
        assert!(header.has_bitmask);
        let bitmask = FieldBitmask::from_bytes(&delta[HEADER_SIZE..HEADER_SIZE + 1], 2).unwrap();
        assert!(!bitmask.test(0));
        assert!(bitmask.test(1));

        let result = apply_delta(&schema, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn roundtrip_all_fields_changed() {
        let schema = two_field_schema();
        let old = write_state(&schema, &[0, 0]);
        let new = write_state(&schema, &[1, 65535]);

        let delta = compute_delta(&schema, &old, &new, None).unwrap();
        let result = apply_delta(&schema, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn roundtrip_minimal_schema() {
        let schema = minimal_schema();
        let old = write_state(&schema, &[0]);
        let new = write_state(&schema, &[1]);

        let delta = compute_delta(&schema, &old, &new, None).unwrap();
        let result = apply_delta(&schema, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn roundtrip_quantized_schema() {
        let schema = schema_with_quantization_and_smoothing();
        let params = schema.fields[0].quantization.as_ref().unwrap();
        let x_packed = quantize(5000.0, params).unwrap();
        let old = write_state(&schema, &[x_packed, 1]);

        let x_new = quantize(-3000.0, params).unwrap();
        let new = write_state(&schema, &[x_new, 0]);

        let delta = compute_delta(&schema, &old, &new, None).unwrap();
        let result = apply_delta(&schema, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn boundary_values() {
        let schema = two_field_schema();

        for alive in [0u64, 1] {
            for health in [0u64, 65535] {
                let state = write_state(&schema, &[alive, health]);
                let values = read_state(&schema, &state).unwrap();
                assert_eq!(values[0], alive);
                assert_eq!(values[1], health);
            }
        }
    }

    #[test]
    fn schema_version_mismatch_error() {
        let mut schema = two_field_schema();
        schema.version = 5;
        let state = write_state(&schema, &[1, 100]);
        let delta =
            compute_delta(&schema, &state, &write_state(&schema, &[0, 100]), None).unwrap();

        schema.version = 6;
        let err = apply_delta(&schema, &state, &delta).unwrap_err();
        assert!(matches!(
            err,
            DeltaError::SchemaVersionMismatch {
                expected: 6,
                got: 5
            }
        ));
    }

    #[test]
    fn truncated_delta_error() {
        let schema = two_field_schema();
        let state = write_state(&schema, &[1, 100]);

        let err = apply_delta(&schema, &state, &[0x04, 0x01]).unwrap_err();
        assert!(matches!(err, DeltaError::TruncatedDelta));
    }

    #[test]
    fn no_bitmask_returns_unsupported() {
        let schema = two_field_schema();
        let state = write_state(&schema, &[1, 100]);
        let delta = [0x00, 0x01, 0x00, 0x00]; // has_bitmask=false
        let err = apply_delta(&schema, &state, &delta).unwrap_err();
        assert!(matches!(err, DeltaError::UnsupportedDeltaFormat));
    }

    #[test]
    fn payload_bits_mismatch_error() {
        let schema = two_field_schema();
        let old = write_state(&schema, &[1, 100]);
        let new = write_state(&schema, &[1, 200]);

        let mut delta = compute_delta(&schema, &old, &new, None).unwrap();
        assert!(!delta.is_empty());

        delta[2] = 0x00;
        delta[3] = 0xFF;

        let err = apply_delta(&schema, &old, &delta).unwrap_err();
        assert!(matches!(err, DeltaError::PayloadBitsMismatch { .. }));
    }

    #[test]
    fn state_too_short_error() {
        let schema = two_field_schema();
        let err = read_state(&schema, &[]).unwrap_err();
        assert!(matches!(err, DeltaError::StateTooShort { .. }));
    }

    #[test]
    fn field_group_mask_filtering() {
        let schema = two_field_schema();
        let old = write_state(&schema, &[0, 100]);
        let new = write_state(&schema, &[1, 200]);

        let mut mask = FieldBitmask::new(2);
        mask.set(0);

        let delta = compute_delta(&schema, &old, &new, Some(&mask)).unwrap();
        let result = apply_delta(&schema, &old, &delta).unwrap();

        let values = read_state(&schema, &result).unwrap();
        assert_eq!(values[0], 1);
        assert_eq!(values[1], 100);
    }

    #[test]
    fn sign_extend_works() {
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0xFFFF, 16), -1);
        assert_eq!(sign_extend(0xFFFFFFFF, 32), -1);
    }

    #[test]
    fn read_write_state_roundtrip() {
        let schema = two_field_schema();
        let values = vec![1u64, 12345];
        let state = write_state(&schema, &values);
        let read_back = read_state(&schema, &state).unwrap();
        assert_eq!(read_back, values);
    }
}
