use std::time::Instant;

use quanta_core_rs::delta::encoder::{
    apply_delta_into, compute_delta_into, write_state,
};
use quanta_core_rs::schema::{compile_schema, CompileOptions, CompiledSchema};

// Same WIT generator as the bench file — duplicated because Cargo can't share
// between bench and integration test targets cleanly.
fn generate_wit(n: usize) -> String {
    let mut fields = String::new();
    for i in 0..n {
        match i % 5 {
            0 | 4 => {
                fields.push_str(&format!(
                    "    /// @quanta:quantize(0.01)\n    /// @quanta:clamp(-10000, 10000)\n    field-{i}: f32,\n"
                ));
            }
            1 => {
                fields.push_str(&format!(
                    "    /// @quanta:clamp(0, 65535)\n    field-{i}: u16,\n"
                ));
            }
            2 => {
                fields.push_str(&format!("    field-{i}: bool,\n"));
            }
            3 => {
                fields.push_str(&format!("    field-{i}: u32,\n"));
            }
            _ => unreachable!(),
        }
    }
    format!("record bench-state {{\n{fields}}}")
}

fn compile_bench_schema(n: usize) -> CompiledSchema {
    let wit = generate_wit(n);
    let opts = CompileOptions::default();
    let (schema, _) = compile_schema(&wit, "bench-state", &opts).unwrap();
    schema
}

fn generate_state_pair(schema: &CompiledSchema, change_pct: usize) -> (Vec<u8>, Vec<u8>) {
    let n = schema.fields.len();
    let n_changed = (n * change_pct).div_ceil(100);

    let old_values: Vec<u64> = vec![0; n];
    let old_state = write_state(schema, &old_values);

    let mut new_values = old_values.clone();
    for i in 0..n_changed {
        new_values[i] = 1;
    }
    let new_state = write_state(schema, &new_values);

    (old_state, new_state)
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    let idx = ((sorted.len() as f64) * p / 100.0).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

/// Published SLO: p99 < 20ms. Test ceiling: p99 < 60ms (3x).
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn slo_compile_schema_50f() {
    let wit = generate_wit(50);
    let opts = CompileOptions::default();

    // Warmup
    for _ in 0..10 {
        let _ = compile_schema(&wit, "bench-state", &opts).unwrap();
    }

    let mut latencies = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        let _ = compile_schema(&wit, "bench-state", &opts).unwrap();
        latencies.push(start.elapsed().as_micros());
    }

    latencies.sort();
    let p99 = percentile(&latencies, 99.0);
    assert!(
        p99 < 60_000,
        "compile_schema(50f) p99 = {p99}μs exceeds 60ms ceiling"
    );
}

/// Published SLO: p50 < 50μs, p99 < 200μs.
/// Test ceiling: p50 < 150μs, p99 < 600μs (3x).
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn slo_compute_delta_20f_50pct() {
    let schema = compile_bench_schema(20);
    let (old_state, new_state) = generate_state_pair(&schema, 50);
    let mut output = Vec::new();

    // Warmup
    for _ in 0..100 {
        compute_delta_into(&schema, &old_state, &new_state, None, &mut output).unwrap();
    }

    let mut latencies = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let start = Instant::now();
        compute_delta_into(&schema, &old_state, &new_state, None, &mut output).unwrap();
        latencies.push(start.elapsed().as_nanos() as u128);
    }

    latencies.sort();
    let p50 = percentile(&latencies, 50.0);
    let p99 = percentile(&latencies, 99.0);

    assert!(
        p50 < 150_000,
        "compute_delta(20f/50%) p50 = {:.1}μs exceeds 150μs ceiling",
        p50 as f64 / 1000.0
    );
    assert!(
        p99 < 600_000,
        "compute_delta(20f/50%) p99 = {:.1}μs exceeds 600μs ceiling",
        p99 as f64 / 1000.0
    );
}

/// Published SLO: p50 < 10μs, p99 < 50μs.
/// Test ceiling: p50 < 30μs, p99 < 150μs (3x).
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn slo_apply_delta_20f() {
    let schema = compile_bench_schema(20);
    let (old_state, new_state) = generate_state_pair(&schema, 50);
    let mut delta_buf = Vec::new();
    compute_delta_into(&schema, &old_state, &new_state, None, &mut delta_buf).unwrap();
    let delta = delta_buf.clone();
    let mut output = Vec::new();

    // Warmup
    for _ in 0..100 {
        apply_delta_into(&schema, &old_state, &delta, &mut output).unwrap();
    }

    let mut latencies = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let start = Instant::now();
        apply_delta_into(&schema, &old_state, &delta, &mut output).unwrap();
        latencies.push(start.elapsed().as_nanos() as u128);
    }

    latencies.sort();
    let p50 = percentile(&latencies, 50.0);
    let p99 = percentile(&latencies, 99.0);

    assert!(
        p50 < 30_000,
        "apply_delta(20f) p50 = {:.1}μs exceeds 30μs ceiling",
        p50 as f64 / 1000.0
    );
    assert!(
        p99 < 150_000,
        "apply_delta(20f) p99 = {:.1}μs exceeds 150μs ceiling",
        p99 as f64 / 1000.0
    );
}
