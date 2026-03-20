use std::collections::HashMap;

use super::annotations::{self, FieldAnnotations};
use super::types::*;

/// A parsed field from a WIT record.
#[derive(Debug, Clone)]
pub struct ParsedField {
    pub name: String,
    pub field_type: FieldType,
    pub annotations: FieldAnnotations,
    pub declaration_order: usize,
}

/// Parse a WIT source to extract a specific record type and its fields.
pub fn parse_wit_record(
    wit_source: &str,
    type_name: &str,
) -> Result<Vec<ParsedField>, SchemaError> {
    let type_defs = collect_type_definitions(wit_source);
    let lines: Vec<&str> = wit_source.lines().collect();
    let record_start = find_record_start(&lines, type_name)?;
    parse_record_fields(&lines, record_start, &type_defs)
}

/// Collect enum/flags definitions: name -> FieldType with variant/flag count.
fn collect_type_definitions(wit_source: &str) -> HashMap<String, FieldType> {
    let mut defs = HashMap::new();
    let lines: Vec<&str> = wit_source.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(name) = parse_type_header("enum", trimmed) {
            let count = count_members(&lines, i);
            defs.insert(name, FieldType::Enum(count as u16));
        } else if let Some(name) = parse_type_header("flags", trimmed) {
            let count = count_members(&lines, i);
            defs.insert(name, FieldType::Flags(count as u16));
        }
    }

    defs
}

fn parse_type_header(keyword: &str, line: &str) -> Option<String> {
    let prefix = format!("{} ", keyword);
    if !line.starts_with(&prefix) {
        return None;
    }
    let rest = line[prefix.len()..].trim();
    let name = rest
        .split(|c: char| c == '{' || c.is_whitespace())
        .next()?
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn count_members(lines: &[&str], header_line: usize) -> usize {
    let mut count = 0;
    let mut in_body = false;

    for line in &lines[header_line..] {
        let trimmed = line.trim();
        if !in_body {
            if trimmed.contains('{') {
                in_body = true;
                // Count members after the opening brace on the same line
                let after_brace = trimmed.split('{').nth(1).unwrap_or("");
                for token in after_brace.split(',') {
                    let t = token.trim().trim_end_matches('}').trim();
                    if !t.is_empty() && !t.starts_with("//") {
                        count += 1;
                    }
                }
                if trimmed.contains('}') {
                    return count;
                }
            }
            continue;
        }

        if trimmed.contains('}') {
            let before = trimmed.split('}').next().unwrap_or("");
            for token in before.split(',') {
                let t = token.trim();
                if !t.is_empty() && !t.starts_with("//") {
                    count += 1;
                }
            }
            return count;
        }

        let cleaned = trimmed.trim_end_matches(',').trim();
        if !cleaned.is_empty() && !cleaned.starts_with("//") {
            count += 1;
        }
    }

    count
}

fn find_record_start(lines: &[&str], type_name: &str) -> Result<usize, SchemaError> {
    let pattern = format!("record {}", type_name);
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(&pattern) {
            let rest = trimmed[pattern.len()..].trim();
            if rest.is_empty() || rest.starts_with('{') {
                return Ok(i);
            }
        }
    }
    Err(SchemaError::TypeNotFound(type_name.to_string()))
}

fn parse_record_fields(
    lines: &[&str],
    record_start: usize,
    type_defs: &HashMap<String, FieldType>,
) -> Result<Vec<ParsedField>, SchemaError> {
    let mut fields = Vec::new();
    let mut doc_comments: Vec<&str> = Vec::new();
    let mut in_body = false;
    let mut order = 0;

    for line in &lines[record_start..] {
        let trimmed = line.trim();

        if !in_body {
            if trimmed.contains('{') {
                in_body = true;
            }
            continue;
        }

        if trimmed.starts_with('}') {
            break;
        }

        if trimmed.starts_with("///") {
            doc_comments.push(trimmed);
            continue;
        }

        // Regular comments and empty lines clear accumulated doc comments
        if trimmed.starts_with("//") || trimmed.is_empty() {
            doc_comments.clear();
            continue;
        }

        if let Some(field) = parse_field_line(trimmed, &doc_comments, order, type_defs)? {
            fields.push(field);
            order += 1;
        }

        doc_comments.clear();
    }

    Ok(fields)
}

fn parse_field_line(
    line: &str,
    doc_comments: &[&str],
    order: usize,
    type_defs: &HashMap<String, FieldType>,
) -> Result<Option<ParsedField>, SchemaError> {
    let cleaned = line.trim_end_matches(',').trim();
    if cleaned.is_empty() {
        return Ok(None);
    }

    let parts: Vec<&str> = cleaned.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(SchemaError::ParseError(format!(
            "invalid field declaration: {}",
            line
        )));
    }

    let name = parts[0].trim().to_string();
    let type_str = parts[1].trim();

    let field_type = resolve_type(type_str, type_defs)?;
    let annotations = annotations::parse_annotations(doc_comments, &name);

    Ok(Some(ParsedField {
        name,
        field_type,
        annotations,
        declaration_order: order,
    }))
}

fn resolve_type(
    type_str: &str,
    type_defs: &HashMap<String, FieldType>,
) -> Result<FieldType, SchemaError> {
    match type_str {
        "bool" => Ok(FieldType::Bool),
        "u8" => Ok(FieldType::U8),
        "s8" => Ok(FieldType::S8),
        "u16" => Ok(FieldType::U16),
        "s16" => Ok(FieldType::S16),
        "u32" => Ok(FieldType::U32),
        "s32" => Ok(FieldType::S32),
        "u64" => Ok(FieldType::U64),
        "s64" => Ok(FieldType::S64),
        "f32" | "float32" => Ok(FieldType::F32),
        "f64" | "float64" => Ok(FieldType::F64),
        "string" => Ok(FieldType::String),
        other => type_defs
            .get(other)
            .copied()
            .ok_or_else(|| SchemaError::ParseError(format!("unknown type: {}", other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_record() {
        let wit = "record my-state {\n    alive: bool,\n}";
        let fields = parse_wit_record(wit, "my-state").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "alive");
        assert_eq!(fields[0].field_type, FieldType::Bool);
    }

    #[test]
    fn parse_multi_field_record() {
        let wit = r#"
record player-state {
    pos-x: f32,
    pos-y: f32,
    health: u16,
    is-alive: bool,
}
"#;
        let fields = parse_wit_record(wit, "player-state").unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].name, "pos-x");
        assert_eq!(fields[0].field_type, FieldType::F32);
        assert_eq!(fields[1].name, "pos-y");
        assert_eq!(fields[2].name, "health");
        assert_eq!(fields[2].field_type, FieldType::U16);
        assert_eq!(fields[3].name, "is-alive");
        assert_eq!(fields[3].field_type, FieldType::Bool);
    }

    #[test]
    fn parse_with_annotations() {
        let wit = r#"
record player-state {
    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    pos-x: f32,
    /// @quanta:skip_delta
    name: string,
}
"#;
        let fields = parse_wit_record(wit, "player-state").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].annotations.quantize_precision, Some(0.01));
        assert_eq!(fields[0].annotations.clamp, Some((-10000.0, 10000.0)));
        assert!(fields[1].annotations.skip_delta);
    }

    #[test]
    fn parse_with_enum_type() {
        let wit = r#"
enum player-class {
    warrior,
    mage,
    ranger,
}

record player-state {
    class: player-class,
}
"#;
        let fields = parse_wit_record(wit, "player-state").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field_type, FieldType::Enum(3));
    }

    #[test]
    fn parse_with_flags_type() {
        let wit = r#"
flags abilities {
    fly,
    swim,
    climb,
    dash,
}

record player-state {
    abilities: abilities,
}
"#;
        let fields = parse_wit_record(wit, "player-state").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field_type, FieldType::Flags(4));
    }

    #[test]
    fn missing_type_name_error() {
        let wit = "record other-state {\n    x: u32,\n}";
        let err = parse_wit_record(wit, "player-state").unwrap_err();
        assert!(matches!(err, SchemaError::TypeNotFound(_)));
    }

    #[test]
    fn empty_record() {
        let wit = "record empty-state {\n}";
        let fields = parse_wit_record(wit, "empty-state").unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn whitespace_and_trailing_comma() {
        let wit = "record my-state {\n    x : f32 ,\n    y : f32,\n}";
        let fields = parse_wit_record(wit, "my-state").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "x");
        assert_eq!(fields[0].field_type, FieldType::F32);
    }

    #[test]
    fn record_with_comments() {
        let wit = r#"
record my-state {
    // this is a regular comment
    x: f32,
    /// doc comment (no annotation)
    y: f32,
}
"#;
        let fields = parse_wit_record(wit, "my-state").unwrap();
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn enum_inline_definition() {
        let wit = "enum status { active, inactive }\nrecord s {\n    st: status,\n}";
        let fields = parse_wit_record(wit, "s").unwrap();
        assert_eq!(fields[0].field_type, FieldType::Enum(2));
    }

    #[test]
    fn declaration_order_preserved() {
        let wit = "record s {\n    a: u8,\n    b: u16,\n    c: u32,\n}";
        let fields = parse_wit_record(wit, "s").unwrap();
        assert_eq!(fields[0].declaration_order, 0);
        assert_eq!(fields[1].declaration_order, 1);
        assert_eq!(fields[2].declaration_order, 2);
    }

    #[test]
    fn unknown_field_type_error() {
        let wit = "record s {\n    x: unknown-type,\n}";
        let err = parse_wit_record(wit, "s").unwrap_err();
        assert!(matches!(err, SchemaError::ParseError(_)));
    }

    #[test]
    fn record_brace_on_next_line() {
        let wit = "record my-state\n{\n    x: f32,\n}";
        let fields = parse_wit_record(wit, "my-state").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "x");
    }

    #[test]
    fn all_primitive_types() {
        let wit = r#"
record all-types {
    a: bool,
    b: u8,
    c: s8,
    d: u16,
    e: s16,
    f: u32,
    g: s32,
    h: u64,
    i: s64,
    j: f32,
    k: f64,
    /// @quanta:skip_delta
    l: string,
}
"#;
        let fields = parse_wit_record(wit, "all-types").unwrap();
        assert_eq!(fields.len(), 12);
        assert_eq!(fields[0].field_type, FieldType::Bool);
        assert_eq!(fields[1].field_type, FieldType::U8);
        assert_eq!(fields[2].field_type, FieldType::S8);
        assert_eq!(fields[3].field_type, FieldType::U16);
        assert_eq!(fields[4].field_type, FieldType::S16);
        assert_eq!(fields[5].field_type, FieldType::U32);
        assert_eq!(fields[6].field_type, FieldType::S32);
        assert_eq!(fields[7].field_type, FieldType::U64);
        assert_eq!(fields[8].field_type, FieldType::S64);
        assert_eq!(fields[9].field_type, FieldType::F32);
        assert_eq!(fields[10].field_type, FieldType::F64);
        assert_eq!(fields[11].field_type, FieldType::String);
    }
}
