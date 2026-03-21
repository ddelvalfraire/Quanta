pub mod annotations;
pub mod bitwidth;
pub mod export;
pub mod layout;
pub mod parser;
pub mod types;
pub mod validation;

pub use types::*;

use layout::LayoutResult;

/// Compile a WIT source into a CompiledSchema.
///
/// Returns (CompiledSchema, Vec<SchemaWarning>) on success.
pub fn compile_schema(
    wit_source: &str,
    type_name: &str,
) -> Result<(CompiledSchema, Vec<SchemaWarning>), SchemaError> {
    let parsed_fields = parser::parse_wit_record(wit_source, type_name)?;

    let LayoutResult {
        fields,
        groups,
        total_bits,
        bitmask_byte_count,
        warnings,
    } = layout::compute_layout(&parsed_fields)?;

    let schema = CompiledSchema {
        version: 1,
        fields,
        field_groups: groups,
        total_bits,
        bitmask_byte_count,
    };

    Ok((schema, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_minimal() {
        let wit = "record my-state {\n    alive: bool,\n}";
        let (schema, warnings) = compile_schema(wit, "my-state").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(schema.fields.len(), 1);
        assert_eq!(schema.fields[0].name, "alive");
        assert_eq!(schema.fields[0].bit_width, 1);
        assert_eq!(schema.total_bits, 1);
        assert_eq!(schema.bitmask_byte_count, 1);
    }

    #[test]
    fn compile_error_on_missing_type() {
        let wit = "record other {\n    x: u32,\n}";
        let err = compile_schema(wit, "player-state").unwrap_err();
        assert!(matches!(err, SchemaError::TypeNotFound(_)));
    }

    #[test]
    fn compile_error_on_string_without_skip_delta() {
        let wit = "record my-state {\n    name: string,\n}";
        let err = compile_schema(wit, "my-state").unwrap_err();
        assert!(matches!(err, SchemaError::StringWithoutSkipDelta { .. }));
    }

    #[test]
    fn compile_warning_on_quantize_without_clamp() {
        let wit = "record my-state {\n    /// @quanta:quantize(0.01)\n    x: f32,\n}";
        let (_, warnings) = compile_schema(wit, "my-state").unwrap();
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SchemaWarning::QuantizeWithoutClamp { .. })));
    }

    #[test]
    fn compile_full_20_field_schema() {
        let wit = r#"
enum player-class {
    warrior,
    mage,
    ranger,
    healer,
}

flags abilities {
    fly,
    swim,
    climb,
    dash,
    stealth,
    shield,
}

record player-state {
    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    pos-x: f32,

    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    pos-y: f32,

    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-10000, 10000)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    pos-z: f32,

    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-3.15, 3.15)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    yaw: f32,

    /// @quanta:quantize(0.01)
    /// @quanta:clamp(-1.58, 1.58)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:interpolate(linear)
    pitch: f32,

    /// @quanta:quantize(0.1)
    /// @quanta:clamp(-100, 100)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    vel-x: f32,

    /// @quanta:quantize(0.1)
    /// @quanta:clamp(-100, 100)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    vel-y: f32,

    /// @quanta:quantize(0.1)
    /// @quanta:clamp(-100, 100)
    /// @quanta:field_group(movement)
    /// @quanta:priority(critical)
    /// @quanta:predict(input_replay)
    /// @quanta:interpolate(linear)
    vel-z: f32,

    /// @quanta:clamp(0, 100)
    /// @quanta:field_group(stats)
    /// @quanta:priority(high)
    health: u16,

    /// @quanta:clamp(0, 100)
    /// @quanta:field_group(stats)
    /// @quanta:priority(high)
    mana: u16,

    /// @quanta:field_group(stats)
    /// @quanta:priority(high)
    level: u8,

    /// @quanta:field_group(stats)
    /// @quanta:priority(high)
    xp: u32,

    is-alive: bool,

    is-crouching: bool,

    is-sprinting: bool,

    class: player-class,

    active-abilities: abilities,

    /// @quanta:skip_delta
    /// @quanta:priority(low)
    display-name: string,

    /// @quanta:field_group(cosmetic)
    /// @quanta:priority(low)
    /// @quanta:tick_rate(10)
    hat-color: u8,

    /// @quanta:field_group(cosmetic)
    /// @quanta:priority(low)
    /// @quanta:tick_rate(10)
    cape-style: u8,
}
"#;

        let (schema, warnings) = compile_schema(wit, "player-state").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(schema.fields.len(), 20);

        // Verify specific bit widths
        let field = |name: &str| schema.fields.iter().find(|f| f.name == name).unwrap();

        // Movement fields: quantize(0.01) + clamp(-10000, 10000) -> 21 bits
        assert_eq!(field("pos-x").bit_width, 21);
        assert_eq!(field("pos-y").bit_width, 21);
        assert_eq!(field("pos-z").bit_width, 21);
        assert!(field("pos-x").quantization.is_some());
        assert_eq!(field("pos-x").prediction, PredictionMode::InputReplay);
        assert_eq!(field("pos-x").interpolation, InterpolationMode::Linear);

        // yaw: quantize(0.01) + clamp(-3.15, 3.15) -> 10 bits
        assert_eq!(field("yaw").bit_width, 10);

        // pitch: quantize(0.01) + clamp(-1.58, 1.58) -> 9 bits
        assert_eq!(field("pitch").bit_width, 9);

        // velocity: quantize(0.1) + clamp(-100, 100) -> 11 bits
        assert_eq!(field("vel-x").bit_width, 11);
        assert_eq!(field("vel-y").bit_width, 11);
        assert_eq!(field("vel-z").bit_width, 11);

        // health/mana: clamp(0, 100) -> 7 bits
        assert_eq!(field("health").bit_width, 7);
        assert_eq!(field("mana").bit_width, 7);

        // Unquantized fields: native width
        assert_eq!(field("level").bit_width, 8);
        assert_eq!(field("xp").bit_width, 32);
        assert_eq!(field("is-alive").bit_width, 1);
        assert_eq!(field("is-crouching").bit_width, 1);
        assert_eq!(field("is-sprinting").bit_width, 1);

        // Enum(4) -> 2 bits
        assert_eq!(field("class").bit_width, 2);

        // Flags(6) -> 6 bits
        assert_eq!(field("active-abilities").bit_width, 6);

        // String -> 0 bits, skip_delta
        assert_eq!(field("display-name").bit_width, 0);
        assert!(field("display-name").skip_delta);

        // Groups sorted by priority
        assert_eq!(schema.field_groups.len(), 4);
        assert_eq!(schema.field_groups[0].name, "movement");
        assert_eq!(schema.field_groups[0].priority, Priority::Critical);
        assert_eq!(schema.field_groups[1].name, "stats");
        assert_eq!(schema.field_groups[1].priority, Priority::High);
        assert_eq!(schema.field_groups[2].name, "default");
        assert_eq!(schema.field_groups[2].priority, Priority::Medium);
        assert_eq!(schema.field_groups[3].name, "cosmetic");
        assert_eq!(schema.field_groups[3].priority, Priority::Low);

        // Cosmetic group has tick_rate 10
        assert_eq!(schema.field_groups[3].max_tick_rate, 10);

        // Bitmask ranges contiguous
        assert_eq!(schema.field_groups[0].bitmask_range, (0, 8)); // 8 movement fields
        assert_eq!(schema.field_groups[1].bitmask_range, (8, 12)); // 4 stats fields
        assert_eq!(schema.field_groups[2].bitmask_range, (12, 18)); // 6 default fields
        assert_eq!(schema.field_groups[3].bitmask_range, (18, 20)); // 2 cosmetic fields

        // Total bits: 21*3 + 10 + 9 + 11*3 + 7*2 + 8 + 32 + 1*3 + 2 + 6 + 0 + 8*2
        // = 63 + 10 + 9 + 33 + 14 + 8 + 32 + 3 + 2 + 6 + 0 + 16 = 196
        assert_eq!(schema.total_bits, 196);

        // Bitmask byte count: ceil(20/8) = 3
        assert_eq!(schema.bitmask_byte_count, 3);

        // Export is deterministic
        let bytes1 = export::export_schema(&schema);
        let bytes2 = export::export_schema(&schema);
        assert_eq!(bytes1, bytes2);
        assert_eq!(&bytes1[0..4], b"QSCH");
    }

    #[test]
    fn compile_three_group_bitmask_ranges() {
        let wit = r#"
record entity-state {
    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    pos-x: f32,

    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    pos-y: f32,

    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    pos-z: f32,

    /// @quanta:field_group(combat)
    /// @quanta:priority(high)
    health: u16,

    /// @quanta:field_group(combat)
    /// @quanta:priority(high)
    mana: u16,

    /// @quanta:field_group(combat)
    /// @quanta:priority(high)
    damage: u8,

    /// @quanta:field_group(combat)
    /// @quanta:priority(high)
    armor: u8,

    /// @quanta:field_group(inventory)
    /// @quanta:priority(low)
    slot1: u8,

    /// @quanta:field_group(inventory)
    /// @quanta:priority(low)
    slot2: u8,

    /// @quanta:field_group(inventory)
    /// @quanta:priority(low)
    slot3: u8,
}
"#;

        let (schema, warnings) = compile_schema(wit, "entity-state").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(schema.fields.len(), 10);

        // Groups sorted: critical → high → low
        assert_eq!(schema.field_groups.len(), 3);
        assert_eq!(schema.field_groups[0].name, "spatial");
        assert_eq!(schema.field_groups[0].priority, Priority::Critical);
        assert_eq!(schema.field_groups[1].name, "combat");
        assert_eq!(schema.field_groups[1].priority, Priority::High);
        assert_eq!(schema.field_groups[2].name, "inventory");
        assert_eq!(schema.field_groups[2].priority, Priority::Low);

        // Bitmask ranges: 3 spatial, 4 combat, 3 inventory
        assert_eq!(schema.field_groups[0].bitmask_range, (0, 3));
        assert_eq!(schema.field_groups[1].bitmask_range, (3, 7));
        assert_eq!(schema.field_groups[2].bitmask_range, (7, 10));

        // Field bit_offsets respect group ordering (spatial fields come first)
        let field = |name: &str| schema.fields.iter().find(|f| f.name == name).unwrap();
        assert!(field("pos-x").bit_offset < field("health").bit_offset);
        assert!(field("health").bit_offset < field("slot1").bit_offset);
    }

    #[test]
    fn compile_ungrouped_fields_in_default_group() {
        let wit = r#"
record entity-state {
    /// @quanta:field_group(alpha)
    /// @quanta:priority(high)
    f1: u8,

    /// @quanta:field_group(alpha)
    /// @quanta:priority(high)
    f2: u8,

    f3: bool,

    f4: u16,

    f5: u8,

    /// @quanta:field_group(zeta)
    /// @quanta:priority(low)
    f6: u8,
}
"#;

        let (schema, _) = compile_schema(wit, "entity-state").unwrap();

        // Sorted: alpha(high) → default(medium) → zeta(low)
        assert_eq!(schema.field_groups.len(), 3);
        assert_eq!(schema.field_groups[0].name, "alpha");
        assert_eq!(schema.field_groups[0].priority, Priority::High);
        assert_eq!(schema.field_groups[1].name, "default");
        assert_eq!(schema.field_groups[1].priority, Priority::Medium);
        assert_eq!(schema.field_groups[2].name, "zeta");
        assert_eq!(schema.field_groups[2].priority, Priority::Low);

        // Bitmask ranges: 2 alpha, 3 default, 1 zeta
        assert_eq!(schema.field_groups[0].bitmask_range, (0, 2));
        assert_eq!(schema.field_groups[1].bitmask_range, (2, 5));
        assert_eq!(schema.field_groups[2].bitmask_range, (5, 6));

        // Ungrouped fields have group_index pointing to "default"
        let field = |name: &str| schema.fields.iter().find(|f| f.name == name).unwrap();
        assert_eq!(field("f3").group_index, 1);
        assert_eq!(field("f4").group_index, 1);
        assert_eq!(field("f5").group_index, 1);
    }

    #[test]
    fn compile_group_priority_from_highest_field() {
        let wit = r#"
record entity-state {
    /// @quanta:field_group(mixed)
    /// @quanta:priority(low)
    a: u8,

    /// @quanta:field_group(mixed)
    /// @quanta:priority(high)
    b: u8,

    /// @quanta:field_group(mixed)
    c: u8,
}
"#;

        let (schema, _) = compile_schema(wit, "entity-state").unwrap();

        // Group adopts highest priority (High) from field "b"
        assert_eq!(schema.field_groups.len(), 1);
        assert_eq!(schema.field_groups[0].name, "mixed");
        assert_eq!(schema.field_groups[0].priority, Priority::High);
    }
}
