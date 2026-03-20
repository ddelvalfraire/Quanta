use super::annotations::FieldAnnotations;
use super::types::*;

/// Compute the bit width for a field based on its type and annotations.
/// Returns (bit_width, Option<QuantizationParams>, warnings).
pub fn compute_bit_width(
    field_type: FieldType,
    annotations: &FieldAnnotations,
    field_name: &str,
) -> (u8, Option<QuantizationParams>, Vec<SchemaWarning>) {
    let mut warnings = Vec::new();

    if field_type == FieldType::String {
        return (0, None, warnings);
    }

    // Quantized + clamped
    if let (Some(precision), Some((min, max))) = (annotations.quantize_precision, annotations.clamp)
    {
        let (num_values, bits) = match quantize_bits(precision, min, max) {
            Some(result) => result,
            None => return (field_type.native_bits().unwrap_or(0), None, warnings),
        };
        let mask = if bits >= 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };

        if let Some(native) = field_type.native_bits() {
            if bits >= native {
                warnings.push(SchemaWarning::BitsGeNativeWidth {
                    field: field_name.to_string(),
                    computed: bits,
                    native,
                });
            }
        }

        let qp = QuantizationParams {
            min,
            max,
            precision,
            num_values,
            mask,
        };

        return (bits, Some(qp), warnings);
    }

    // Integer with clamp (no quantize)
    if let Some((min, max)) = annotations.clamp {
        if field_type.is_numeric() && !field_type.is_float() {
            let raw = max - min;
            if !raw.is_finite() || raw < 0.0 || raw >= u64::MAX as f64 {
                return (field_type.native_bits().unwrap_or(0), None, warnings);
            }
            let range = (raw as u64).saturating_add(1);
            let bits = ceil_log2_u64(range);

            if let Some(native) = field_type.native_bits() {
                if bits >= native {
                    warnings.push(SchemaWarning::BitsGeNativeWidth {
                        field: field_name.to_string(),
                        computed: bits,
                        native,
                    });
                }
            }

            return (bits, None, warnings);
        }
    }

    // Default: native width
    let bits = field_type.native_bits().unwrap_or(0);
    (bits, None, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_is_1_bit() {
        let ann = FieldAnnotations::default();
        let (bits, qp, warnings) = compute_bit_width(FieldType::Bool, &ann, "f");
        assert_eq!(bits, 1);
        assert!(qp.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn u8_is_8_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::U8, &ann, "f");
        assert_eq!(bits, 8);
    }

    #[test]
    fn u16_is_16_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::U16, &ann, "f");
        assert_eq!(bits, 16);
    }

    #[test]
    fn u32_is_32_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::U32, &ann, "f");
        assert_eq!(bits, 32);
    }

    #[test]
    fn u64_is_64_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::U64, &ann, "f");
        assert_eq!(bits, 64);
    }

    #[test]
    fn f32_is_32_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::F32, &ann, "f");
        assert_eq!(bits, 32);
    }

    #[test]
    fn f64_is_64_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::F64, &ann, "f");
        assert_eq!(bits, 64);
    }

    #[test]
    fn string_is_0_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::String, &ann, "f");
        assert_eq!(bits, 0);
    }

    #[test]
    fn enum_3_variants_is_2_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::Enum(3), &ann, "f");
        assert_eq!(bits, 2);
    }

    #[test]
    fn enum_4_variants_is_2_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::Enum(4), &ann, "f");
        assert_eq!(bits, 2);
    }

    #[test]
    fn enum_1_variant_is_0_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::Enum(1), &ann, "f");
        assert_eq!(bits, 0);
    }

    #[test]
    fn flags_4_is_4_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::Flags(4), &ann, "f");
        assert_eq!(bits, 4);
    }

    #[test]
    fn flags_6_is_6_bits() {
        let ann = FieldAnnotations::default();
        let (bits, _, _) = compute_bit_width(FieldType::Flags(6), &ann, "f");
        assert_eq!(bits, 6);
    }

    /// Spec example: quantize(0.01) + clamp(-10000, 10000) -> 21 bits
    #[test]
    fn quantize_clamp_f32_21_bits() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        ann.clamp = Some((-10000.0, 10000.0));
        let (bits, qp, warnings) = compute_bit_width(FieldType::F32, &ann, "pos-x");
        assert_eq!(bits, 21);
        let qp = qp.unwrap();
        assert_eq!(qp.num_values, 2_000_001);
        assert_eq!(qp.mask, (1u64 << 21) - 1);
        assert!(warnings.is_empty());
    }

    /// quantize(0.01) + clamp(-3.15, 3.15) -> 10 bits (yaw)
    #[test]
    fn quantize_clamp_yaw_10_bits() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        ann.clamp = Some((-3.15, 3.15));
        let (bits, qp, _) = compute_bit_width(FieldType::F32, &ann, "yaw");
        assert_eq!(bits, 10);
        assert_eq!(qp.unwrap().num_values, 631);
    }

    /// quantize(0.01) + clamp(-1.58, 1.58) -> 9 bits (pitch)
    #[test]
    fn quantize_clamp_pitch_9_bits() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        ann.clamp = Some((-1.58, 1.58));
        let (bits, qp, _) = compute_bit_width(FieldType::F32, &ann, "pitch");
        assert_eq!(bits, 9);
        assert_eq!(qp.unwrap().num_values, 317);
    }

    /// quantize(0.1) + clamp(-100, 100) -> 11 bits (velocity)
    #[test]
    fn quantize_clamp_velocity_11_bits() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.1);
        ann.clamp = Some((-100.0, 100.0));
        let (bits, qp, _) = compute_bit_width(FieldType::F32, &ann, "vel-x");
        assert_eq!(bits, 11);
        assert_eq!(qp.unwrap().num_values, 2001);
    }

    #[test]
    fn integer_clamp_0_255_is_8_bits() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((0.0, 255.0));
        let (bits, qp, _) = compute_bit_width(FieldType::U16, &ann, "f");
        assert_eq!(bits, 8);
        assert!(qp.is_none()); // integer clamp doesn't produce QuantizationParams
    }

    #[test]
    fn integer_clamp_0_100_is_7_bits() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((0.0, 100.0));
        let (bits, _, _) = compute_bit_width(FieldType::U16, &ann, "health");
        assert_eq!(bits, 7);
    }

    #[test]
    fn bits_ge_native_warns() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.00001);
        ann.clamp = Some((-100000.0, 100000.0));
        let (bits, _, warnings) = compute_bit_width(FieldType::F32, &ann, "x");
        assert!(bits >= 32);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings[0],
            SchemaWarning::BitsGeNativeWidth { .. }
        ));
    }

    #[test]
    fn quantize_without_clamp_uses_native() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        // No clamp — falls through to native width
        let (bits, qp, _) = compute_bit_width(FieldType::F32, &ann, "x");
        assert_eq!(bits, 32);
        assert!(qp.is_none());
    }

    #[test]
    fn float_with_clamp_no_quantize_uses_native() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((-100.0, 100.0));
        let (bits, _, _) = compute_bit_width(FieldType::F32, &ann, "x");
        assert_eq!(bits, 32);
    }
}
