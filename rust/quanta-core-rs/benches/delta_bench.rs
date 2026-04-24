use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use quanta_core_rs::delta::encoder::{
    apply_delta_into, compute_delta_into, read_state, write_state,
};
use quanta_core_rs::schema::{compile_schema, CompileOptions, CompiledSchema};
use std::hint::black_box;

// Generates a WIT record with `n` fields using a repeating pattern:
//   0: quantized f32, 1: clamped u16, 2: bool, 3: raw u32, 4: quantized f32, ...
// This gives ~40% quantized f32, ~20% clamped u16, ~20% bool, ~20% raw u32.
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
    let (schema, _warnings) = compile_schema(&wit, "bench-state", &opts).unwrap();
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

fn bench_compile_schema(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile_schema");
    for &n in &[5, 10, 20, 50, 100] {
        let wit = generate_wit(n);
        let opts = CompileOptions::default();
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                black_box(compile_schema(black_box(&wit), "bench-state", &opts).unwrap());
            });
        });
    }
    group.finish();
}

fn bench_compute_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_delta");
    let mut output = Vec::new();

    for &n in &[5, 20, 50, 100] {
        let schema = compile_bench_schema(n);
        for &pct in &[0, 10, 50, 100] {
            let (old_state, new_state) = generate_state_pair(&schema, pct);
            let id = BenchmarkId::new(format!("{n}f"), format!("{pct}pct"));
            group.bench_with_input(id, &(&schema, &old_state, &new_state), |b, _| {
                b.iter(|| {
                    compute_delta_into(
                        black_box(&schema),
                        black_box(&old_state),
                        black_box(&new_state),
                        None,
                        &mut output,
                    )
                    .unwrap();
                    black_box(&output);
                });
            });
        }
    }
    group.finish();
}

fn bench_apply_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_delta");
    let mut output = Vec::new();
    let mut delta_buf = Vec::new();

    for &n in &[5, 20, 50, 100] {
        let schema = compile_bench_schema(n);
        for &pct in &[10, 50, 100] {
            let (old_state, new_state) = generate_state_pair(&schema, pct);
            compute_delta_into(&schema, &old_state, &new_state, None, &mut delta_buf).unwrap();
            let delta = delta_buf.clone();

            let id = BenchmarkId::new(format!("{n}f"), format!("{pct}pct"));
            group.bench_with_input(id, &(&schema, &old_state, &delta), |b, _| {
                b.iter(|| {
                    apply_delta_into(
                        black_box(&schema),
                        black_box(&old_state),
                        black_box(&delta),
                        &mut output,
                    )
                    .unwrap();
                    black_box(&output);
                });
            });
        }
    }
    group.finish();
}

fn bench_decode_state(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_state");

    for &n in &[5, 20, 50, 100] {
        let schema = compile_bench_schema(n);
        let values: Vec<u64> = (0..n as u64).map(|i| i % 2).collect();
        let state = write_state(&schema, &values);

        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &(&schema, &state),
            |b, _| {
                b.iter(|| {
                    black_box(read_state(black_box(&schema), black_box(&state)).unwrap());
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_compile_schema,
    bench_compute_delta,
    bench_apply_delta,
    bench_decode_state
);
criterion_main!(benches);
