use super::bitwidth;
use super::parser::ParsedField;
use super::types::*;
use super::validation;

pub struct LayoutResult {
    pub fields: Vec<FieldMeta>,
    pub groups: Vec<FieldGroup>,
    pub total_bits: u32,
    pub bitmask_byte_count: u8,
    pub warnings: Vec<SchemaWarning>,
}

pub fn compute_layout(
    parsed_fields: &[ParsedField],
    opts: &CompileOptions,
) -> Result<LayoutResult, SchemaError> {
    let mut warnings = Vec::new();

    for field in parsed_fields {
        validation::validate_field(field, opts, &mut warnings)?;
    }

    // Group fields by field_group annotation.
    // Each entry: (group_name, group_priority, fields_in_group)
    let mut group_map: Vec<(String, Priority, Vec<&ParsedField>)> = Vec::new();

    for field in parsed_fields {
        let group_name = field
            .annotations
            .field_group
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let priority = field.annotations.priority.unwrap_or(Priority::Medium);

        if let Some(entry) = group_map.iter_mut().find(|(name, _, _)| *name == group_name) {
            // Use highest priority (lowest enum value) in the group
            if priority < entry.1 {
                entry.1 = priority;
            }
            entry.2.push(field);
        } else {
            group_map.push((group_name, priority, vec![field]));
        }
    }

    group_map.sort_by_key(|(_, priority, _)| *priority);

    let mut fields = Vec::new();
    let mut groups = Vec::new();
    let mut current_bit_offset: u32 = 0;
    let mut field_index: u16 = 0;

    for (group_idx, (group_name, priority, group_fields)) in group_map.iter().enumerate() {
        let bitmask_start = field_index;
        let mut max_tick_rate: u16 = 0;

        for pf in group_fields {
            let (bit_width, quantization, bw_warnings) =
                bitwidth::compute_bit_width(pf.field_type, &pf.annotations, &pf.name);
            warnings.extend(bw_warnings);

            if let Some(tr) = pf.annotations.tick_rate {
                if tr > max_tick_rate {
                    max_tick_rate = tr;
                }
            }

            let fm = FieldMeta {
                name: pf.name.clone(),
                field_type: pf.field_type,
                bit_width,
                bit_offset: current_bit_offset,
                group_index: group_idx as u8,
                quantization,
                prediction: pf
                    .annotations
                    .prediction
                    .unwrap_or(PredictionMode::None),
                smoothing: pf.annotations.smoothing.clone().unwrap_or(SmoothingParams {
                    mode: SmoothingMode::Snap,
                    duration_ms: 0,
                    max_distance: 0.0,
                }),
                interpolation: pf
                    .annotations
                    .interpolation
                    .unwrap_or(InterpolationMode::None),
                skip_delta: pf.annotations.skip_delta,
            };

            current_bit_offset += bit_width as u32;
            field_index += 1;
            fields.push(fm);
        }

        let bitmask_end = field_index;

        groups.push(FieldGroup {
            name: group_name.clone(),
            priority: *priority,
            max_tick_rate,
            bitmask_range: (bitmask_start, bitmask_end),
        });
    }

    let total_bits = current_bit_offset;
    let bitmask_byte_count = ((fields.len() + 7) / 8) as u8;

    Ok(LayoutResult {
        fields,
        groups,
        total_bits,
        bitmask_byte_count,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::annotations::FieldAnnotations;
    use crate::schema::parser::ParsedField;

    fn make_field(name: &str, ft: FieldType, ann: FieldAnnotations, order: usize) -> ParsedField {
        ParsedField {
            name: name.to_string(),
            field_type: ft,
            annotations: ann,
            declaration_order: order,
        }
    }

    #[test]
    fn single_field_layout() {
        let fields = vec![make_field(
            "alive",
            FieldType::Bool,
            FieldAnnotations::default(),
            0,
        )];
        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.fields.len(), 1);
        assert_eq!(result.fields[0].bit_width, 1);
        assert_eq!(result.fields[0].bit_offset, 0);
        assert_eq!(result.total_bits, 1);
        assert_eq!(result.bitmask_byte_count, 1);
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].name, "default");
    }

    #[test]
    fn priority_ordering() {
        let mut ann_low = FieldAnnotations::default();
        ann_low.field_group = Some("cosmetic".into());
        ann_low.priority = Some(Priority::Low);

        let mut ann_critical = FieldAnnotations::default();
        ann_critical.field_group = Some("movement".into());
        ann_critical.priority = Some(Priority::Critical);

        // cosmetic declared first, but movement has higher priority
        let fields = vec![
            make_field("color", FieldType::U8, ann_low, 0),
            make_field("pos-x", FieldType::U16, ann_critical, 1),
        ];

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();

        assert_eq!(result.groups[0].name, "movement");
        assert_eq!(result.groups[0].priority, Priority::Critical);
        assert_eq!(result.groups[1].name, "cosmetic");
        assert_eq!(result.groups[1].priority, Priority::Low);

        // pos-x should have lower bit_offset than color
        let pos = result.fields.iter().find(|f| f.name == "pos-x").unwrap();
        let color = result.fields.iter().find(|f| f.name == "color").unwrap();
        assert!(pos.bit_offset < color.bit_offset);
    }

    #[test]
    fn contiguous_bitmask_ranges() {
        let mut ann_a = FieldAnnotations::default();
        ann_a.field_group = Some("a".into());
        ann_a.priority = Some(Priority::High);

        let mut ann_b = FieldAnnotations::default();
        ann_b.field_group = Some("b".into());
        ann_b.priority = Some(Priority::Low);

        let fields = vec![
            make_field("f1", FieldType::Bool, ann_a.clone(), 0),
            make_field("f2", FieldType::Bool, ann_a, 1),
            make_field("f3", FieldType::Bool, ann_b.clone(), 2),
            make_field("f4", FieldType::Bool, ann_b, 3),
        ];

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.groups[0].bitmask_range, (0, 2));
        assert_eq!(result.groups[1].bitmask_range, (2, 4));
    }

    #[test]
    fn bit_offsets_accumulate() {
        let fields = vec![
            make_field("a", FieldType::U8, FieldAnnotations::default(), 0),
            make_field("b", FieldType::U16, FieldAnnotations::default(), 1),
            make_field("c", FieldType::Bool, FieldAnnotations::default(), 2),
        ];

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.fields[0].bit_offset, 0);
        assert_eq!(result.fields[0].bit_width, 8);
        assert_eq!(result.fields[1].bit_offset, 8);
        assert_eq!(result.fields[1].bit_width, 16);
        assert_eq!(result.fields[2].bit_offset, 24);
        assert_eq!(result.fields[2].bit_width, 1);
        assert_eq!(result.total_bits, 25);
    }

    #[test]
    fn max_tick_rate_in_group() {
        let mut ann1 = FieldAnnotations::default();
        ann1.field_group = Some("movement".into());
        ann1.tick_rate = Some(20);

        let mut ann2 = FieldAnnotations::default();
        ann2.field_group = Some("movement".into());
        ann2.tick_rate = Some(60);

        let fields = vec![
            make_field("x", FieldType::F32, ann1, 0),
            make_field("y", FieldType::F32, ann2, 1),
        ];

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.groups[0].max_tick_rate, 60);
    }

    #[test]
    fn default_tick_rate_is_zero() {
        let fields = vec![make_field(
            "x",
            FieldType::F32,
            FieldAnnotations::default(),
            0,
        )];
        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.groups[0].max_tick_rate, 0);
    }

    #[test]
    fn bitmask_byte_count_9_fields() {
        let fields: Vec<_> = (0..9)
            .map(|i| {
                make_field(
                    &format!("f{}", i),
                    FieldType::Bool,
                    FieldAnnotations::default(),
                    i,
                )
            })
            .collect();

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.bitmask_byte_count, 2); // ceil(9/8) = 2
    }

    #[test]
    fn bitmask_byte_count_8_fields() {
        let fields: Vec<_> = (0..8)
            .map(|i| {
                make_field(
                    &format!("f{}", i),
                    FieldType::Bool,
                    FieldAnnotations::default(),
                    i,
                )
            })
            .collect();

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.bitmask_byte_count, 1); // ceil(8/8) = 1
    }

    #[test]
    fn empty_layout() {
        let fields: Vec<ParsedField> = vec![];
        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert!(result.fields.is_empty());
        assert!(result.groups.is_empty());
        assert_eq!(result.total_bits, 0);
        assert_eq!(result.bitmask_byte_count, 0);
    }

    #[test]
    fn group_priority_uses_highest_field() {
        let mut ann_high = FieldAnnotations::default();
        ann_high.field_group = Some("mixed".into());
        ann_high.priority = Some(Priority::High);

        let mut ann_low = FieldAnnotations::default();
        ann_low.field_group = Some("mixed".into());
        ann_low.priority = Some(Priority::Low);

        let fields = vec![
            make_field("a", FieldType::U8, ann_low, 0),
            make_field("b", FieldType::U8, ann_high, 1),
        ];

        let result = compute_layout(&fields, &CompileOptions::default()).unwrap();
        assert_eq!(result.groups[0].priority, Priority::High);
    }
}
