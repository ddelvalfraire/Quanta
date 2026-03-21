use std::cell::RefCell;

use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};

use crate::resources::CompiledSchemaResource;
use quanta_core_rs::delta::encoder::{
    self, dequantize, quantize_field, read_state, sign_extend, write_state,
};
use quanta_core_rs::delta::{DeltaError, FieldBitmask};
use quanta_core_rs::schema::FieldType;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        schema_version_mismatch,
    }
}

thread_local! {
    static COMPUTE_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static APPLY_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ENCODE_BUF: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

#[rustler::nif(schedule = "DirtyCpu")]
fn delta_compute<'a>(
    env: Env<'a>,
    schema_arc: ResourceArc<CompiledSchemaResource>,
    old_binary: Binary,
    new_binary: Binary,
    group_mask_or_nil: Term<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let schema = &schema_arc.0;
        let old = old_binary.as_slice();
        let new = new_binary.as_slice();

        let mask = if group_mask_or_nil.is_binary() {
            let mask_bin: Binary = match group_mask_or_nil.decode() {
                Ok(b) => b,
                Err(_) => {
                    return (atoms::error(), "invalid group mask binary".to_string()).encode(env)
                }
            };
            let n_fields = schema.fields.len() as u16;
            match FieldBitmask::from_bytes(mask_bin.as_slice(), n_fields) {
                Ok(m) => Some(m),
                Err(e) => return (atoms::error(), e.to_string()).encode(env),
            }
        } else {
            None
        };

        COMPUTE_BUF.with(|buf| {
            let mut buf = buf.borrow_mut();
            match encoder::compute_delta_into(schema, old, new, mask.as_ref(), &mut buf) {
                Ok(()) => {
                    let mut bin = NewBinary::new(env, buf.len());
                    bin.as_mut_slice().copy_from_slice(&buf);
                    (atoms::ok(), Binary::from(bin)).encode(env)
                }
                Err(e) => (atoms::error(), e.to_string()).encode(env),
            }
        })
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn delta_apply<'a>(
    env: Env<'a>,
    schema_arc: ResourceArc<CompiledSchemaResource>,
    current_binary: Binary,
    delta_binary: Binary,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let schema = &schema_arc.0;
        let current = current_binary.as_slice();
        let delta = delta_binary.as_slice();

        APPLY_BUF.with(|buf| {
            let mut buf = buf.borrow_mut();
            match encoder::apply_delta_into(schema, current, delta, &mut buf) {
                Ok(()) => {
                    let mut bin = NewBinary::new(env, buf.len());
                    bin.as_mut_slice().copy_from_slice(&buf);
                    (atoms::ok(), Binary::from(bin)).encode(env)
                }
                Err(DeltaError::SchemaVersionMismatch { .. }) => {
                    (atoms::error(), atoms::schema_version_mismatch()).encode(env)
                }
                Err(e) => (atoms::error(), e.to_string()).encode(env),
            }
        })
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn delta_decode_state<'a>(
    env: Env<'a>,
    schema_arc: ResourceArc<CompiledSchemaResource>,
    state_binary: Binary,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let schema = &schema_arc.0;
        let state = state_binary.as_slice();

        let values = match read_state(schema, state) {
            Ok(v) => v,
            Err(e) => return (atoms::error(), e.to_string()).encode(env),
        };

        let pairs: Vec<(Term, Term)> = schema
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.skip_delta && f.bit_width > 0)
            .map(|(i, field)| {
                let key = field.name.as_str().encode(env);
                let val = decode_value_to_term(env, field, values[i]);
                (key, val)
            })
            .collect();

        let map = Term::map_from_pairs(env, &pairs).unwrap();
        (atoms::ok(), map).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn delta_encode_state<'a>(
    env: Env<'a>,
    schema_arc: ResourceArc<CompiledSchemaResource>,
    values_list: Term<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let schema = &schema_arc.0;

        let elixir_values: Vec<Term> = match values_list.decode() {
            Ok(v) => v,
            Err(_) => {
                return (atoms::error(), "expected a list of values".to_string()).encode(env)
            }
        };

        let active_fields: Vec<_> = schema
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.skip_delta && f.bit_width > 0)
            .collect();

        if elixir_values.len() != active_fields.len() {
            return (
                atoms::error(),
                DeltaError::FieldCountMismatch {
                    expected: active_fields.len(),
                    got: elixir_values.len(),
                }
                .to_string(),
            )
                .encode(env);
        }

        ENCODE_BUF.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.clear();
            buf.resize(schema.fields.len(), 0u64);

            for (list_idx, &(field_idx, field)) in active_fields.iter().enumerate() {
                let term = elixir_values[list_idx];
                match encode_term_to_value(field, term) {
                    Ok(val) => buf[field_idx] = val,
                    Err(e) => return (atoms::error(), e).encode(env),
                }
            }

            let state = write_state(schema, &buf);
            let mut bin = NewBinary::new(env, state.len());
            bin.as_mut_slice().copy_from_slice(&state);
            (atoms::ok(), Binary::from(bin)).encode(env)
        })
    })
}

fn decode_value_to_term<'a>(
    env: Env<'a>,
    field: &quanta_core_rs::schema::FieldMeta,
    value: u64,
) -> Term<'a> {
    use rustler::types::atom;

    if let Some(ref params) = field.quantization {
        return dequantize(value, params).encode(env);
    }

    match field.field_type {
        FieldType::Bool => {
            if value != 0 {
                atom::true_().encode(env)
            } else {
                atom::false_().encode(env)
            }
        }
        FieldType::F32 => {
            let f = f32::from_bits(value as u32);
            (f as f64).encode(env)
        }
        FieldType::F64 => {
            let f = f64::from_bits(value);
            f.encode(env)
        }
        FieldType::S8 | FieldType::S16 | FieldType::S32 | FieldType::S64 => {
            sign_extend(value, field.bit_width).encode(env)
        }
        _ => value.encode(env),
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn delta_changed_fields<'a>(
    env: Env<'a>,
    schema_arc: ResourceArc<CompiledSchemaResource>,
    delta_binary: Binary,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let schema = &schema_arc.0;
        let delta = delta_binary.as_slice();

        if delta.is_empty() {
            return (atoms::ok(), Vec::<String>::new()).encode(env);
        }

        let header = match quanta_core_rs::delta::DeltaHeader::decode(delta) {
            Ok(h) => h,
            Err(e) => return (atoms::error(), e.to_string()).encode(env),
        };

        if !header.has_bitmask {
            return (atoms::error(), "delta has no bitmask".to_string()).encode(env);
        }

        let n_fields = schema.fields.len() as u16;
        let bitmask_bytes = (n_fields as usize).div_ceil(8);
        let bitmask_start = quanta_core_rs::delta::HEADER_SIZE;
        let bitmask_end = bitmask_start + bitmask_bytes;

        if delta.len() < bitmask_end {
            return (atoms::error(), "truncated delta".to_string()).encode(env);
        }

        let changed = match FieldBitmask::from_bytes(&delta[bitmask_start..bitmask_end], n_fields) {
            Ok(m) => m,
            Err(e) => return (atoms::error(), e.to_string()).encode(env),
        };

        let names: Vec<&str> = changed
            .iter_set()
            .filter(|&i| {
                let f = &schema.fields[i as usize];
                !f.skip_delta && f.bit_width > 0
            })
            .map(|i| schema.fields[i as usize].name.as_str())
            .collect();

        (atoms::ok(), names).encode(env)
    })
}

fn encode_term_to_value(
    field: &quanta_core_rs::schema::FieldMeta,
    term: Term,
) -> Result<u64, String> {
    if let Some(ref params) = field.quantization {
        let float_val: f64 = term
            .decode()
            .map_err(|_| format!("expected float for quantized field '{}'", field.name))?;
        return quantize_field(float_val, params, &field.name).map_err(|e| e.to_string());
    }

    match field.field_type {
        FieldType::Bool => {
            if let Ok(b) = term.decode::<bool>() {
                Ok(b as u64)
            } else if let Ok(i) = term.decode::<i64>() {
                Ok(if i != 0 { 1 } else { 0 })
            } else {
                Err(format!("expected boolean for field '{}'", field.name))
            }
        }
        FieldType::F32 => {
            let f: f64 = term
                .decode()
                .map_err(|_| format!("expected float for field '{}'", field.name))?;
            Ok((f as f32).to_bits() as u64)
        }
        FieldType::F64 => {
            let f: f64 = term
                .decode()
                .map_err(|_| format!("expected float for field '{}'", field.name))?;
            Ok(f.to_bits())
        }
        // Two's complement encoding is identical for signed and unsigned at the bit level
        _ => {
            let i: i64 = term
                .decode()
                .map_err(|_| format!("expected integer for field '{}'", field.name))?;
            let mask = if field.bit_width >= 64 {
                u64::MAX
            } else {
                (1u64 << field.bit_width) - 1
            };
            Ok((i as u64) & mask)
        }
    }
}
