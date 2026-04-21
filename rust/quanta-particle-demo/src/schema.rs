//! Particle-state schema definition.
//!
//! The WIT source is compiled once at first access. Callers receive a
//! static reference to the [`CompiledSchema`] plus cached field indices so
//! the executor can read/write entity state without re-searching fields on
//! every tick.

use std::sync::OnceLock;

use quanta_core_rs::delta::encoder::{quantize_field, write_state};
use quanta_core_rs::schema::{compile_schema, CompileOptions, CompiledSchema};

const PARTICLE_WIT: &str = r#"
record particle-state {
    /// @quanta:quantize(0.2)
    /// @quanta:clamp(-5000, 5000)
    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    pos-x: f32,

    /// @quanta:quantize(0.2)
    /// @quanta:clamp(-5000, 5000)
    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    pos-z: f32,

    /// @quanta:quantize(0.1)
    /// @quanta:clamp(-250, 250)
    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    vel-x: f32,

    /// @quanta:quantize(0.1)
    /// @quanta:clamp(-250, 250)
    /// @quanta:field_group(spatial)
    /// @quanta:priority(critical)
    vel-z: f32,
}
"#;

static PARTICLE_SCHEMA: OnceLock<CompiledSchema> = OnceLock::new();

/// Absolute bound of the 2D world on each axis; matches `@quanta:clamp(-5000, 5000)`.
pub const WORLD_BOUND: f32 = 5000.0;

/// Maximum entity velocity magnitude in units/sec; matches `@quanta:clamp(-250, 250)`
/// per-axis (so the magnitude bound is conservative — actual max is 250 * sqrt(2)).
/// Tuned for smooth motion at 30 Hz: at 250 u/s an entity advances ~8 units
/// per tick, small enough that interpolation between ticks reads as fluid
/// movement rather than discrete steps. Player still feels snappy because
/// client-side prediction runs at 60 fps.
pub const MAX_VELOCITY: f32 = 250.0;

/// Cached indices into `CompiledSchema.fields` for the particle fields.
#[derive(Debug, Clone, Copy)]
pub struct ParticleFieldIndices {
    pub pos_x: usize,
    pub pos_z: usize,
    pub vel_x: usize,
    pub vel_z: usize,
}

pub fn particle_schema() -> &'static CompiledSchema {
    PARTICLE_SCHEMA.get_or_init(|| {
        let (schema, _warnings) =
            compile_schema(PARTICLE_WIT, "particle-state", &CompileOptions::default())
                .expect("PARTICLE_WIT is hardcoded and valid");
        // Guard against WIT-vs-constant drift: if someone edits the WIT
        // clamp values without updating WORLD_BOUND / MAX_VELOCITY (or
        // vice versa), surface the mismatch immediately at schema init.
        let find = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("particle schema missing field `{name}`"))
        };
        let pos_q = find("pos-x")
            .quantization
            .as_ref()
            .expect("pos-x quantized");
        assert!(
            (pos_q.max as f32 - WORLD_BOUND).abs() < f32::EPSILON
                && (pos_q.min as f32 + WORLD_BOUND).abs() < f32::EPSILON,
            "WORLD_BOUND ({WORLD_BOUND}) must equal pos-x clamp bounds (\
             min={}, max={}). Edit PARTICLE_WIT or WORLD_BOUND together.",
            pos_q.min,
            pos_q.max
        );
        let vel_q = find("vel-x")
            .quantization
            .as_ref()
            .expect("vel-x quantized");
        assert!(
            (vel_q.max as f32 - MAX_VELOCITY).abs() < f32::EPSILON
                && (vel_q.min as f32 + MAX_VELOCITY).abs() < f32::EPSILON,
            "MAX_VELOCITY ({MAX_VELOCITY}) must equal vel-x clamp bounds (\
             min={}, max={}). Edit PARTICLE_WIT or MAX_VELOCITY together.",
            vel_q.min,
            vel_q.max
        );
        schema
    })
}

pub fn particle_field_indices() -> ParticleFieldIndices {
    let schema = particle_schema();
    let find = |name: &str| {
        schema
            .fields
            .iter()
            .position(|f| f.name == name)
            .unwrap_or_else(|| panic!("particle schema missing field `{name}`"))
    };
    ParticleFieldIndices {
        pos_x: find("pos-x"),
        pos_z: find("pos-z"),
        vel_x: find("vel-x"),
        vel_z: find("vel-z"),
    }
}

/// Produces a state byte buffer where all fields are quantized from 0.0.
pub fn initial_state() -> Vec<u8> {
    let schema = particle_schema();
    let ix = particle_field_indices();
    let mut values = vec![0u64; schema.fields.len()];
    for &idx in &[ix.pos_x, ix.pos_z, ix.vel_x, ix.vel_z] {
        let params = schema.fields[idx]
            .quantization
            .as_ref()
            .expect("all particle fields quantized");
        values[idx] =
            quantize_field(0.0, params, &schema.fields[idx].name).expect("0 is within clamp range");
    }
    write_state(schema, &values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quanta_core_rs::delta::encoder::{dequantize, read_state};

    #[test]
    fn schema_compiles_with_four_fields() {
        let s = particle_schema();
        assert_eq!(s.fields.len(), 4);
        assert!(s.fields.iter().all(|f| f.quantization.is_some()));
    }

    #[test]
    fn indices_are_cached_correctly() {
        let ix = particle_field_indices();
        let schema = particle_schema();
        assert_eq!(schema.fields[ix.pos_x].name, "pos-x");
        assert_eq!(schema.fields[ix.pos_z].name, "pos-z");
        assert_eq!(schema.fields[ix.vel_x].name, "vel-x");
        assert_eq!(schema.fields[ix.vel_z].name, "vel-z");
    }

    #[test]
    fn initial_state_roundtrips_to_zero() {
        let bytes = initial_state();
        let schema = particle_schema();
        let values = read_state(schema, &bytes).expect("decode");
        let ix = particle_field_indices();
        for &idx in &[ix.pos_x, ix.pos_z, ix.vel_x, ix.vel_z] {
            let params = schema.fields[idx].quantization.as_ref().unwrap();
            let f = dequantize(values[idx], params);
            assert!(
                f.abs() < 0.25,
                "field {} should be ~0, got {}",
                schema.fields[idx].name,
                f
            );
        }
    }
}
