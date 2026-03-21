use wasm_bindgen::prelude::*;

use quanta_core_rs::delta::encoder::{
    apply_delta, dequantize, quantize_field, read_state, sign_extend, write_state,
};
use quanta_core_rs::schema::evolution::import_schema;
use quanta_core_rs::schema::{CompiledSchema, FieldType};

/// Opaque handle wrapping a parsed CompiledSchema.
#[wasm_bindgen]
#[derive(Debug)]
pub struct SchemaHandle {
    schema: CompiledSchema,
}

#[wasm_bindgen]
impl SchemaHandle {
    /// Number of fields in the schema.
    #[wasm_bindgen(getter)]
    pub fn field_count(&self) -> usize {
        self.schema.fields.len()
    }

    /// Schema version byte.
    #[wasm_bindgen(getter)]
    pub fn version(&self) -> u8 {
        self.schema.version
    }

    /// Total state size in bits.
    #[wasm_bindgen(getter)]
    pub fn total_bits(&self) -> u32 {
        self.schema.total_bits
    }
}

/// Parse QSCH binary bytes into a reusable SchemaHandle.
#[wasm_bindgen]
pub fn create_schema(bytes: &[u8]) -> Result<SchemaHandle, JsError> {
    let schema = import_schema(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(SchemaHandle { schema })
}

/// Apply a binary delta to the current state, returning the new state bytes.
#[wasm_bindgen]
pub fn wasm_apply_delta(
    handle: &SchemaHandle,
    state: &[u8],
    delta: &[u8],
) -> Result<Vec<u8>, JsError> {
    apply_delta(&handle.schema, state, delta).map_err(|e| JsError::new(&e.to_string()))
}

/// Decode packed state bytes into a JS object `{ fieldName: value, ... }`.
///
/// Value conversion mirrors the NIF's `decode_value_to_term`:
/// - Quantized fields -> dequantized f64
/// - Bool -> JS boolean
/// - F32/F64 -> JS number (via from_bits)
/// - Signed ints -> sign_extend() then JS number
/// - Unsigned/Enum/Flags -> JS number
#[wasm_bindgen]
pub fn decode_state(handle: &SchemaHandle, state: &[u8]) -> Result<JsValue, JsError> {
    let schema = &handle.schema;
    let values = read_state(schema, state).map_err(|e| JsError::new(&e.to_string()))?;

    let obj = js_sys::Object::new();

    for (i, field) in schema.fields.iter().enumerate() {
        if field.skip_delta || field.bit_width == 0 {
            continue;
        }

        let key = JsValue::from_str(&field.name);
        let val = decode_value(field, values[i]);

        js_sys::Reflect::set(&obj, &key, &val)
            .map_err(|_| JsError::new("failed to set property on JS object"))?;
    }

    Ok(obj.into())
}

/// Encode a JS object `{ fieldName: value, ... }` into packed state bytes.
///
/// Value conversion mirrors the NIF's `encode_term_to_value`:
/// - Quantized fields -> quantize_field()
/// - Bool -> 0 or 1
/// - F32/F64 -> to_bits()
/// - Signed/unsigned ints -> masked u64
#[wasm_bindgen]
pub fn encode_state(handle: &SchemaHandle, state_obj: JsValue) -> Result<Vec<u8>, JsError> {
    let schema = &handle.schema;
    let mut values = vec![0u64; schema.fields.len()];

    for (i, field) in schema.fields.iter().enumerate() {
        if field.skip_delta || field.bit_width == 0 {
            continue;
        }

        let key = JsValue::from_str(&field.name);
        let js_val = js_sys::Reflect::get(&state_obj, &key)
            .map_err(|_| JsError::new(&format!("failed to read field '{}'", field.name)))?;

        if js_val.is_undefined() {
            continue; // leave as zero default
        }

        values[i] = encode_value(field, &js_val)?;
    }

    Ok(write_state(schema, &values))
}

/// Convert a raw u64 value to JS, matching the NIF's decode_value_to_term.
fn decode_value(field: &quanta_core_rs::schema::FieldMeta, value: u64) -> JsValue {
    if let Some(ref params) = field.quantization {
        return JsValue::from_f64(dequantize(value, params));
    }

    match field.field_type {
        FieldType::Bool => JsValue::from_bool(value != 0),
        FieldType::F32 => {
            let f = f32::from_bits(value as u32);
            JsValue::from_f64(f as f64)
        }
        FieldType::F64 => {
            let f = f64::from_bits(value);
            JsValue::from_f64(f)
        }
        FieldType::S8 | FieldType::S16 | FieldType::S32 | FieldType::S64 => {
            JsValue::from_f64(sign_extend(value, field.bit_width) as f64)
        }
        _ => JsValue::from_f64(value as f64),
    }
}

/// Convert a JS value to raw u64, matching the NIF's encode_term_to_value.
fn encode_value(
    field: &quanta_core_rs::schema::FieldMeta,
    js_val: &JsValue,
) -> Result<u64, JsError> {
    if let Some(ref params) = field.quantization {
        let f = js_val
            .as_f64()
            .ok_or_else(|| JsError::new(&format!("expected number for quantized field '{}'", field.name)))?;
        return quantize_field(f, params, &field.name).map_err(|e| JsError::new(&e.to_string()));
    }

    match field.field_type {
        FieldType::Bool => {
            let b = js_val.as_bool().unwrap_or_else(|| {
                // Fallback: treat truthy numbers as true
                js_val.as_f64().map(|n| n != 0.0).unwrap_or(false)
            });
            Ok(b as u64)
        }
        FieldType::F32 => {
            let f = js_val
                .as_f64()
                .ok_or_else(|| JsError::new(&format!("expected number for field '{}'", field.name)))?;
            Ok((f as f32).to_bits() as u64)
        }
        FieldType::F64 => {
            let f = js_val
                .as_f64()
                .ok_or_else(|| JsError::new(&format!("expected number for field '{}'", field.name)))?;
            Ok(f.to_bits())
        }
        _ => {
            let f = js_val
                .as_f64()
                .ok_or_else(|| JsError::new(&format!("expected number for field '{}'", field.name)))?;
            let i = f as i64;
            let mask = if field.bit_width >= 64 {
                u64::MAX
            } else {
                (1u64 << field.bit_width) - 1
            };
            Ok((i as u64) & mask)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quanta_core_rs::delta::encoder::{compute_delta, quantize};
    use quanta_core_rs::schema::export::export_schema;
    use quanta_core_rs::schema::types::test_fixtures::*;

    fn roundtrip_schema(schema: &CompiledSchema) -> SchemaHandle {
        let bytes = export_schema(schema);
        create_schema(&bytes).expect("schema parse failed")
    }

    #[test]
    fn create_schema_from_exported_bytes() {
        let schema = two_field_schema();
        let handle = roundtrip_schema(&schema);
        assert_eq!(handle.field_count(), 2);
        assert_eq!(handle.version(), 1);
        assert_eq!(handle.total_bits(), 17);
    }

    #[test]
    fn apply_delta_roundtrip() {
        let schema = two_field_schema();
        let handle = roundtrip_schema(&schema);

        let old = write_state(&schema, &[1, 100]);
        let new = write_state(&schema, &[1, 200]);
        let delta = compute_delta(&schema, &old, &new, None).unwrap();

        let result = wasm_apply_delta(&handle, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn apply_empty_delta() {
        let schema = two_field_schema();
        let handle = roundtrip_schema(&schema);
        let state = write_state(&schema, &[1, 100]);

        let result = wasm_apply_delta(&handle, &state, &[]).unwrap();
        assert_eq!(result, state);
    }

    #[test]
    fn apply_delta_quantized() {
        let schema = schema_with_quantization_and_smoothing();
        let handle = roundtrip_schema(&schema);

        let params = schema.fields[0].quantization.as_ref().unwrap();
        let old_x = quantize(5000.0, params).unwrap();
        let new_x = quantize(-3000.0, params).unwrap();

        let old = write_state(&schema, &[old_x, 1]);
        let new = write_state(&schema, &[new_x, 0]);
        let delta = compute_delta(&schema, &old, &new, None).unwrap();

        let result = wasm_apply_delta(&handle, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    // JsError::new() requires wasm target — error paths tested in tests/web.rs
}
