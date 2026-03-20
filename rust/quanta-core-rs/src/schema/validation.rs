use super::parser::ParsedField;
use super::types::*;

pub fn validate_field(
    field: &ParsedField,
    warnings: &mut Vec<SchemaWarning>,
) -> Result<(), SchemaError> {
    let ann = &field.annotations;
    let name = &field.name;
    let ft = field.field_type;

    if ann.quantize_precision.is_some() && !ft.is_numeric() {
        return Err(SchemaError::QuantizeOnNonNumeric {
            field: name.clone(),
        });
    }

    if let Some(interp) = ann.interpolation {
        if interp != InterpolationMode::None && !ft.is_numeric() {
            return Err(SchemaError::InterpolateOnNonNumeric {
                field: name.clone(),
            });
        }
    }

    if let Some(p) = ann.quantize_precision {
        if !p.is_finite() || p <= 0.0 {
            return Err(SchemaError::PrecisionNotPositive {
                field: name.clone(),
            });
        }
    }

    if let Some((min, max)) = ann.clamp {
        if !min.is_finite() || !max.is_finite() || min >= max {
            return Err(SchemaError::ClampMinGeMax {
                field: name.clone(),
                min,
                max,
            });
        }
    }

    if let (Some(precision), Some((min, max))) = (ann.quantize_precision, ann.clamp) {
        match quantize_bits(precision, min, max) {
            Some((_, bits)) if bits > 64 => {
                return Err(SchemaError::QuantizeClampBitsTooLarge {
                    field: name.clone(),
                    bits: bits as u32,
                });
            }
            None => {
                return Err(SchemaError::QuantizeClampBitsTooLarge {
                    field: name.clone(),
                    bits: 65,
                });
            }
            _ => {}
        }
    }

    if ft == FieldType::String && !ann.skip_delta {
        return Err(SchemaError::StringWithoutSkipDelta {
            field: name.clone(),
        });
    }

    if let FieldType::Flags(n) = ft {
        if n > 255 {
            return Err(SchemaError::ParseError(format!(
                "flags type for field '{}' has {} members (max 255)",
                name, n
            )));
        }
    }

    // --- Warnings ---

    if ann.quantize_precision.is_some() && ann.clamp.is_none() {
        warnings.push(SchemaWarning::QuantizeWithoutClamp {
            field: name.clone(),
        });
    }

    if ann.clamp.is_some() && ann.quantize_precision.is_none() && ft.is_float() {
        warnings.push(SchemaWarning::RedundantClamp {
            field: name.clone(),
        });
    }

    for w in &ann.warnings {
        warnings.push(w.clone());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::annotations::FieldAnnotations;
    use crate::schema::parser::ParsedField;

    fn make_field(name: &str, ft: FieldType, ann: FieldAnnotations) -> ParsedField {
        ParsedField {
            name: name.to_string(),
            field_type: ft,
            annotations: ann,
            declaration_order: 0,
        }
    }

    #[test]
    fn quantize_on_non_numeric_is_error() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        let field = make_field("flag", FieldType::Bool, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::QuantizeOnNonNumeric { .. }));
    }

    #[test]
    fn interpolate_on_non_numeric_is_error() {
        let mut ann = FieldAnnotations::default();
        ann.interpolation = Some(InterpolationMode::Linear);
        let field = make_field("flag", FieldType::Bool, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::InterpolateOnNonNumeric { .. }));
    }

    #[test]
    fn interpolate_none_on_non_numeric_ok() {
        let mut ann = FieldAnnotations::default();
        ann.interpolation = Some(InterpolationMode::None);
        let field = make_field("flag", FieldType::Bool, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
    }

    #[test]
    fn precision_not_positive_is_error() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.0);
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::PrecisionNotPositive { .. }));
    }

    #[test]
    fn negative_precision_is_error() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(-0.5);
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::PrecisionNotPositive { .. }));
    }

    #[test]
    fn clamp_min_ge_max_is_error() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((10.0, 5.0));
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::ClampMinGeMax { .. }));
    }

    #[test]
    fn clamp_min_eq_max_is_error() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((5.0, 5.0));
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::ClampMinGeMax { .. }));
    }

    #[test]
    fn string_without_skip_delta_is_error() {
        let ann = FieldAnnotations::default();
        let field = make_field("name", FieldType::String, ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::StringWithoutSkipDelta { .. }));
    }

    #[test]
    fn string_with_skip_delta_is_ok() {
        let mut ann = FieldAnnotations::default();
        ann.skip_delta = true;
        let field = make_field("name", FieldType::String, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
    }

    #[test]
    fn quantize_without_clamp_is_warning() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings[0],
            SchemaWarning::QuantizeWithoutClamp { .. }
        ));
    }

    #[test]
    fn redundant_clamp_on_float_is_warning() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((-100.0, 100.0));
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings[0],
            SchemaWarning::RedundantClamp { .. }
        ));
    }

    #[test]
    fn clamp_on_integer_not_redundant() {
        let mut ann = FieldAnnotations::default();
        ann.clamp = Some((0.0, 100.0));
        let field = make_field("hp", FieldType::U16, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_annotation_passed_through() {
        let mut ann = FieldAnnotations::default();
        ann.warnings.push(SchemaWarning::UnknownAnnotation {
            field: "x".into(),
            annotation: "frobnicate".into(),
        });
        let field = make_field("x", FieldType::U32, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings[0],
            SchemaWarning::UnknownAnnotation { .. }
        ));
    }

    #[test]
    fn flags_over_255_is_error() {
        let ann = FieldAnnotations::default();
        let field = make_field("bits", FieldType::Flags(256), ann);
        let mut warnings = Vec::new();
        let err = validate_field(&field, &mut warnings).unwrap_err();
        assert!(matches!(err, SchemaError::ParseError(_)));
    }

    #[test]
    fn valid_quantize_clamp_passes() {
        let mut ann = FieldAnnotations::default();
        ann.quantize_precision = Some(0.01);
        ann.clamp = Some((-10000.0, 10000.0));
        let field = make_field("x", FieldType::F32, ann);
        let mut warnings = Vec::new();
        validate_field(&field, &mut warnings).unwrap();
        assert!(warnings.is_empty());
    }
}
