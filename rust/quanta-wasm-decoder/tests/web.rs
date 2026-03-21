use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use quanta_core_rs::delta::encoder::{compute_delta, quantize, write_state};
use quanta_core_rs::schema::export::export_schema;
use quanta_core_rs::schema::types::test_fixtures::*;

use quanta_wasm_decoder::{create_schema, decode_state, encode_state, wasm_apply_delta};

fn schema_bytes_two_field() -> Vec<u8> {
    export_schema(&two_field_schema())
}

fn schema_bytes_quantized() -> Vec<u8> {
    export_schema(&schema_with_quantization_and_smoothing())
}

#[wasm_bindgen_test]
fn create_schema_parses_valid_bytes() {
    let handle = create_schema(&schema_bytes_two_field()).unwrap();
    assert_eq!(handle.field_count(), 2);
    assert_eq!(handle.version(), 1);
}

#[wasm_bindgen_test]
fn create_schema_rejects_invalid() {
    assert!(create_schema(&[]).is_err());
    assert!(create_schema(b"XXXX").is_err());
}

#[wasm_bindgen_test]
fn apply_delta_two_field_roundtrip() {
    let schema = two_field_schema();
    let handle = create_schema(&schema_bytes_two_field()).unwrap();

    let old = write_state(&schema, &[1, 100]);
    let new = write_state(&schema, &[0, 65535]);
    let delta = compute_delta(&schema, &old, &new, None).unwrap();

    let result = wasm_apply_delta(&handle, &old, &delta).unwrap();
    assert_eq!(result, new);
}

#[wasm_bindgen_test]
fn apply_empty_delta_returns_current() {
    let schema = two_field_schema();
    let handle = create_schema(&schema_bytes_two_field()).unwrap();
    let state = write_state(&schema, &[1, 42]);

    let result = wasm_apply_delta(&handle, &state, &[]).unwrap();
    assert_eq!(result, state);
}

#[wasm_bindgen_test]
fn apply_delta_quantized_roundtrip() {
    let schema = schema_with_quantization_and_smoothing();
    let handle = create_schema(&schema_bytes_quantized()).unwrap();

    let params = schema.fields[0].quantization.as_ref().unwrap();
    let old_x = quantize(1234.56, params).unwrap();
    let new_x = quantize(-5678.9, params).unwrap();

    let old = write_state(&schema, &[old_x, 1]);
    let new = write_state(&schema, &[new_x, 0]);
    let delta = compute_delta(&schema, &old, &new, None).unwrap();

    let result = wasm_apply_delta(&handle, &old, &delta).unwrap();
    assert_eq!(result, new);
}

#[wasm_bindgen_test]
fn decode_state_two_field() {
    let schema = two_field_schema();
    let handle = create_schema(&schema_bytes_two_field()).unwrap();
    let state = write_state(&schema, &[1, 12345]);

    let obj = decode_state(&handle, &state).unwrap();

    let alive = js_sys::Reflect::get(&obj, &"alive".into()).unwrap();
    assert_eq!(alive.as_bool(), Some(true));

    let health = js_sys::Reflect::get(&obj, &"health".into()).unwrap();
    assert_eq!(health.as_f64(), Some(12345.0));
}

#[wasm_bindgen_test]
fn decode_state_quantized() {
    let schema = schema_with_quantization_and_smoothing();
    let handle = create_schema(&schema_bytes_quantized()).unwrap();

    let params = schema.fields[0].quantization.as_ref().unwrap();
    let packed = quantize(5000.0, params).unwrap();
    let state = write_state(&schema, &[packed, 0]);

    let obj = decode_state(&handle, &state).unwrap();

    let x = js_sys::Reflect::get(&obj, &"x".into())
        .unwrap()
        .as_f64()
        .unwrap();
    assert!((x - 5000.0).abs() < params.precision);

    let alive = js_sys::Reflect::get(&obj, &"alive".into()).unwrap();
    assert_eq!(alive.as_bool(), Some(false));
}

#[wasm_bindgen_test]
fn encode_decode_roundtrip() {
    let schema = two_field_schema();
    let handle = create_schema(&schema_bytes_two_field()).unwrap();
    let state = write_state(&schema, &[1, 500]);

    // decode -> encode -> decode should be identical
    let decoded = decode_state(&handle, &state).unwrap();
    let re_encoded = encode_state(&handle, decoded.clone()).unwrap();
    let re_decoded = decode_state(&handle, &re_encoded).unwrap();

    let alive1 = js_sys::Reflect::get(&decoded, &"alive".into()).unwrap();
    let alive2 = js_sys::Reflect::get(&re_decoded, &"alive".into()).unwrap();
    assert_eq!(alive1.as_bool(), alive2.as_bool());

    let health1 = js_sys::Reflect::get(&decoded, &"health".into()).unwrap();
    let health2 = js_sys::Reflect::get(&re_decoded, &"health".into()).unwrap();
    assert_eq!(health1.as_f64(), health2.as_f64());
}

#[wasm_bindgen_test]
fn encode_decode_quantized_roundtrip() {
    let schema = schema_with_quantization_and_smoothing();
    let handle = create_schema(&schema_bytes_quantized()).unwrap();

    let params = schema.fields[0].quantization.as_ref().unwrap();
    let packed = quantize(42.42, params).unwrap();
    let state = write_state(&schema, &[packed, 1]);

    let decoded = decode_state(&handle, &state).unwrap();
    let re_encoded = encode_state(&handle, decoded.clone()).unwrap();
    let re_decoded = decode_state(&handle, &re_encoded).unwrap();

    let x1 = js_sys::Reflect::get(&decoded, &"x".into())
        .unwrap()
        .as_f64()
        .unwrap();
    let x2 = js_sys::Reflect::get(&re_decoded, &"x".into())
        .unwrap()
        .as_f64()
        .unwrap();
    assert!((x1 - x2).abs() < params.precision);
}
