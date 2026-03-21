use super::types::*;

/// Parsed annotations for a single field.
#[derive(Debug, Clone, Default)]
pub struct FieldAnnotations {
    pub quantize_precision: Option<f64>,
    pub clamp: Option<(f64, f64)>,
    pub priority: Option<Priority>,
    pub skip_delta: bool,
    pub tick_rate: Option<u16>,
    pub interpolation: Option<InterpolationMode>,
    pub field_group: Option<String>,
    pub prediction: Option<PredictionMode>,
    pub smoothing: Option<SmoothingParams>,
    pub warnings: Vec<SchemaWarning>,
}

/// Parse `@quanta:` directives from `///` doc comment lines.
pub fn parse_annotations(doc_lines: &[&str], field_name: &str) -> FieldAnnotations {
    let mut ann = FieldAnnotations::default();

    for line in doc_lines {
        let trimmed = line.trim();
        let content = match trimmed.strip_prefix("///") {
            Some(rest) => rest.trim(),
            None => continue,
        };

        let mut remaining = content;
        while let Some(idx) = remaining.find("@quanta:") {
            let after = &remaining[idx + "@quanta:".len()..];
            let (directive, rest) = extract_directive(after);
            parse_directive(&directive, field_name, &mut ann);
            remaining = rest;
        }
    }

    ann
}

fn extract_directive(s: &str) -> (String, &str) {
    let mut i = 0;
    let bytes = s.as_bytes();

    // Read directive name (alphanumeric + underscore)
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }

    let name = &s[..i];

    if i < bytes.len() && bytes[i] == b'(' {
        let start = i + 1;
        let mut depth = 1;
        let mut j = start;
        while j < bytes.len() && depth > 0 {
            if bytes[j] == b'(' {
                depth += 1;
            }
            if bytes[j] == b')' {
                depth -= 1;
            }
            j += 1;
        }
        if depth == 0 {
            let args = &s[start..j - 1];
            let rest = &s[j..];
            (format!("{}({})", name, args), rest)
        } else {
            // Malformed — no closing paren, consume everything
            (name.to_string(), "")
        }
    } else {
        (name.to_string(), &s[i..])
    }
}

fn malformed(field_name: &str, directive: &str) -> SchemaWarning {
    SchemaWarning::MalformedAnnotation {
        field: field_name.to_string(),
        directive: directive.to_string(),
    }
}

fn parse_directive(directive: &str, field_name: &str, ann: &mut FieldAnnotations) {
    if let Some(args) = strip_func("quantize", directive) {
        match args.trim().parse::<f64>() {
            Ok(precision) => ann.quantize_precision = Some(precision),
            Err(_) => ann.warnings.push(malformed(field_name, "quantize")),
        }
    } else if let Some(args) = strip_func("clamp", directive) {
        let parts: Vec<&str> = args.split(',').collect();
        if parts.len() == 2 {
            if let (Ok(min), Ok(max)) = (
                parts[0].trim().parse::<f64>(),
                parts[1].trim().parse::<f64>(),
            ) {
                ann.clamp = Some((min, max));
            } else {
                ann.warnings.push(malformed(field_name, "clamp"));
            }
        } else {
            ann.warnings.push(malformed(field_name, "clamp"));
        }
    } else if let Some(args) = strip_func("priority", directive) {
        match args.trim() {
            "critical" => ann.priority = Some(Priority::Critical),
            "high" => ann.priority = Some(Priority::High),
            "medium" => ann.priority = Some(Priority::Medium),
            "low" => ann.priority = Some(Priority::Low),
            _ => ann.warnings.push(malformed(field_name, "priority")),
        }
    } else if directive == "skip_delta" {
        ann.skip_delta = true;
    } else if let Some(args) = strip_func("tick_rate", directive) {
        match args.trim().parse::<u16>() {
            Ok(hz) => ann.tick_rate = Some(hz),
            Err(_) => ann.warnings.push(malformed(field_name, "tick_rate")),
        }
    } else if let Some(args) = strip_func("interpolate", directive) {
        match args.trim() {
            "linear" => ann.interpolation = Some(InterpolationMode::Linear),
            "hermite" => ann.interpolation = Some(InterpolationMode::Hermite),
            "none" => ann.interpolation = Some(InterpolationMode::None),
            _ => ann.warnings.push(malformed(field_name, "interpolate")),
        }
    } else if let Some(args) = strip_func("field_group", directive) {
        ann.field_group = Some(args.trim().to_string());
    } else if let Some(args) = strip_func("predict", directive) {
        match args.trim() {
            "input_replay" => ann.prediction = Some(PredictionMode::InputReplay),
            "cosmetic" => ann.prediction = Some(PredictionMode::Cosmetic),
            "none" => ann.prediction = Some(PredictionMode::None),
            _ => ann.warnings.push(malformed(field_name, "predict")),
        }
    } else if let Some(args) = strip_func("smooth", directive) {
        let parts: Vec<&str> = args.split(',').collect();
        let raw_params: Vec<&str> = parts[1..]
            .iter()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();
        match parts[0].trim() {
            "lerp" => {
                if raw_params.len() == 1 {
                    if let Ok(d) = raw_params[0].parse::<u32>() {
                        ann.smoothing = Some(SmoothingParams {
                            mode: SmoothingMode::Lerp,
                            duration_ms: d,
                            max_distance: 0.0,
                        });
                    } else {
                        ann.warnings.push(malformed(field_name, "smooth"));
                    }
                } else {
                    ann.warnings.push(malformed(field_name, "smooth"));
                }
            }
            "snap" => {
                if raw_params.is_empty() {
                    ann.smoothing = Some(SmoothingParams {
                        mode: SmoothingMode::Snap,
                        duration_ms: 0,
                        max_distance: 0.0,
                    });
                } else {
                    ann.warnings.push(malformed(field_name, "smooth"));
                }
            }
            "snap_lerp" => {
                if raw_params.len() == 2 {
                    if let (Ok(d), Ok(md)) = (
                        raw_params[0].parse::<u32>(),
                        raw_params[1].parse::<f64>(),
                    ) {
                        ann.smoothing = Some(SmoothingParams {
                            mode: SmoothingMode::SnapLerp,
                            duration_ms: d,
                            max_distance: md,
                        });
                    } else {
                        ann.warnings.push(malformed(field_name, "smooth"));
                    }
                } else {
                    ann.warnings.push(malformed(field_name, "smooth"));
                }
            }
            _ => ann.warnings.push(malformed(field_name, "smooth")),
        }
    } else {
        let name = directive.split('(').next().unwrap_or(directive);
        ann.warnings.push(SchemaWarning::UnknownAnnotation {
            field: field_name.to_string(),
            annotation: name.to_string(),
        });
    }
}

fn strip_func<'a>(name: &str, directive: &'a str) -> Option<&'a str> {
    directive
        .strip_prefix(name)
        .and_then(|s| s.strip_prefix('('))
        .and_then(|s| s.strip_suffix(')'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quantize() {
        let ann = parse_annotations(&["/// @quanta:quantize(0.01)"], "pos-x");
        assert_eq!(ann.quantize_precision, Some(0.01));
    }

    #[test]
    fn parse_clamp() {
        let ann = parse_annotations(&["/// @quanta:clamp(-10000, 10000)"], "pos-x");
        assert_eq!(ann.clamp, Some((-10000.0, 10000.0)));
    }

    #[test]
    fn parse_priority_variants() {
        for (input, expected) in [
            ("critical", Priority::Critical),
            ("high", Priority::High),
            ("medium", Priority::Medium),
            ("low", Priority::Low),
        ] {
            let line = format!("/// @quanta:priority({})", input);
            let ann = parse_annotations(&[line.as_str()], "f");
            assert_eq!(ann.priority, Some(expected));
        }
    }

    #[test]
    fn parse_skip_delta() {
        let ann = parse_annotations(&["/// @quanta:skip_delta"], "name");
        assert!(ann.skip_delta);
    }

    #[test]
    fn parse_tick_rate() {
        let ann = parse_annotations(&["/// @quanta:tick_rate(20)"], "f");
        assert_eq!(ann.tick_rate, Some(20));
    }

    #[test]
    fn parse_interpolate() {
        let ann = parse_annotations(&["/// @quanta:interpolate(linear)"], "f");
        assert_eq!(ann.interpolation, Some(InterpolationMode::Linear));
    }

    #[test]
    fn parse_interpolate_hermite() {
        let ann = parse_annotations(&["/// @quanta:interpolate(hermite)"], "f");
        assert_eq!(ann.interpolation, Some(InterpolationMode::Hermite));
    }

    #[test]
    fn parse_field_group() {
        let ann = parse_annotations(&["/// @quanta:field_group(movement)"], "f");
        assert_eq!(ann.field_group, Some("movement".to_string()));
    }

    #[test]
    fn parse_predict() {
        let ann = parse_annotations(&["/// @quanta:predict(input_replay)"], "f");
        assert_eq!(ann.prediction, Some(PredictionMode::InputReplay));
    }

    #[test]
    fn parse_predict_cosmetic() {
        let ann = parse_annotations(&["/// @quanta:predict(cosmetic)"], "f");
        assert_eq!(ann.prediction, Some(PredictionMode::Cosmetic));
    }

    #[test]
    fn parse_smooth_lerp() {
        let ann = parse_annotations(&["/// @quanta:smooth(lerp, 100)"], "f");
        let smooth = ann.smoothing.unwrap();
        assert_eq!(smooth.mode, SmoothingMode::Lerp);
        assert_eq!(smooth.duration_ms, 100);
        assert_eq!(smooth.max_distance, 0.0);
    }

    #[test]
    fn parse_smooth_snap() {
        let ann = parse_annotations(&["/// @quanta:smooth(snap)"], "f");
        let smooth = ann.smoothing.unwrap();
        assert_eq!(smooth.mode, SmoothingMode::Snap);
        assert_eq!(smooth.duration_ms, 0);
        assert_eq!(smooth.max_distance, 0.0);
    }

    #[test]
    fn parse_smooth_snap_lerp() {
        let ann = parse_annotations(&["/// @quanta:smooth(snap_lerp, 150, 5.0)"], "f");
        let smooth = ann.smoothing.unwrap();
        assert_eq!(smooth.mode, SmoothingMode::SnapLerp);
        assert_eq!(smooth.duration_ms, 150);
        assert_eq!(smooth.max_distance, 5.0);
    }

    #[test]
    fn smooth_lerp_wrong_param_count() {
        let ann = parse_annotations(&["/// @quanta:smooth(lerp)"], "f");
        assert!(ann.smoothing.is_none());
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(&ann.warnings[0], SchemaWarning::MalformedAnnotation { .. }));
    }

    #[test]
    fn smooth_snap_with_params_is_malformed() {
        let ann = parse_annotations(&["/// @quanta:smooth(snap, 100)"], "f");
        assert!(ann.smoothing.is_none());
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(&ann.warnings[0], SchemaWarning::MalformedAnnotation { .. }));
    }

    #[test]
    fn smooth_snap_lerp_wrong_param_count() {
        let ann = parse_annotations(&["/// @quanta:smooth(snap_lerp, 100)"], "f");
        assert!(ann.smoothing.is_none());
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(&ann.warnings[0], SchemaWarning::MalformedAnnotation { .. }));
    }

    #[test]
    fn unknown_annotation_produces_warning() {
        let ann = parse_annotations(&["/// @quanta:frobnicate(42)"], "f");
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(
            &ann.warnings[0],
            SchemaWarning::UnknownAnnotation { annotation, .. } if annotation == "frobnicate"
        ));
    }

    #[test]
    fn multiple_annotations_on_separate_lines() {
        let lines = &[
            "/// @quanta:quantize(0.01)",
            "/// @quanta:clamp(-100, 100)",
            "/// @quanta:field_group(movement)",
        ];
        let ann = parse_annotations(lines, "pos-x");
        assert_eq!(ann.quantize_precision, Some(0.01));
        assert_eq!(ann.clamp, Some((-100.0, 100.0)));
        assert_eq!(ann.field_group, Some("movement".to_string()));
    }

    #[test]
    fn malformed_annotation_no_crash() {
        let ann = parse_annotations(&["/// @quanta:quantize("], "f");
        assert_eq!(ann.quantize_precision, None);
    }

    #[test]
    fn malformed_quantize_args_produce_warning() {
        let ann = parse_annotations(&["/// @quanta:quantize(abc)"], "f");
        assert_eq!(ann.quantize_precision, None);
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(
            &ann.warnings[0],
            SchemaWarning::MalformedAnnotation { directive, .. } if directive == "quantize"
        ));
    }

    #[test]
    fn invalid_priority_value_produces_warning() {
        let ann = parse_annotations(&["/// @quanta:priority(ultra)"], "f");
        assert_eq!(ann.priority, None);
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(
            &ann.warnings[0],
            SchemaWarning::MalformedAnnotation { directive, .. } if directive == "priority"
        ));
    }

    #[test]
    fn invalid_interpolate_value_produces_warning() {
        let ann = parse_annotations(&["/// @quanta:interpolate(cubic)"], "f");
        assert_eq!(ann.interpolation, None);
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(
            &ann.warnings[0],
            SchemaWarning::MalformedAnnotation { directive, .. } if directive == "interpolate"
        ));
    }

    #[test]
    fn invalid_clamp_args_produce_warning() {
        let ann = parse_annotations(&["/// @quanta:clamp(-10000 10000)"], "f");
        assert_eq!(ann.clamp, None);
        assert_eq!(ann.warnings.len(), 1);
        assert!(matches!(&ann.warnings[0], SchemaWarning::MalformedAnnotation { .. }));
    }

    #[test]
    fn non_doc_comment_lines_ignored() {
        let ann = parse_annotations(&["// @quanta:skip_delta", "not a comment"], "f");
        assert!(!ann.skip_delta);
    }

    #[test]
    fn empty_doc_comment_line() {
        let ann = parse_annotations(&["///"], "f");
        assert!(!ann.skip_delta);
        assert!(ann.warnings.is_empty());
    }
}
