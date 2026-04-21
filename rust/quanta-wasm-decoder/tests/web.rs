use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use quanta_core_rs::delta::encoder::{compute_delta, quantize, write_state};
use quanta_core_rs::schema::export::export_schema;
use quanta_core_rs::schema::types::test_fixtures::*;

use quanta_wasm_decoder::{
    apply_delta, create_schema, decode_delta_datagram, decode_initial_state, decode_state,
    encode_client_input, encode_state,
};
use wasm_bindgen::{JsCast, JsValue};

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

    let result = apply_delta(&handle, &old, &delta).unwrap();
    assert_eq!(result, new);
}

#[wasm_bindgen_test]
fn apply_empty_delta_returns_current() {
    let schema = two_field_schema();
    let handle = create_schema(&schema_bytes_two_field()).unwrap();
    let state = write_state(&schema, &[1, 42]);

    let result = apply_delta(&handle, &state, &[]).unwrap();
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

    let result = apply_delta(&handle, &old, &delta).unwrap();
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

// ---------------------------------------------------------------------------
// Cross-language wire-format parity — verifies the JS side of the decoder
// returns exactly what the Rust encoder produced.
// ---------------------------------------------------------------------------

fn read_u32_field(obj: &JsValue, key: &str) -> u32 {
    js_sys::Reflect::get(obj, &key.into())
        .unwrap()
        .as_f64()
        .unwrap() as u32
}

fn read_u64_field(obj: &JsValue, key: &str) -> u64 {
    let v = js_sys::Reflect::get(obj, &key.into()).unwrap();
    if let Some(bi) = v.dyn_ref::<js_sys::BigInt>() {
        u64::try_from(bi.clone()).unwrap_or(0)
    } else {
        v.as_f64().unwrap() as u64
    }
}

#[wasm_bindgen_test]
fn client_input_round_trip_matches_expected_bytes() {
    let bytes = encode_client_input(1, 2, 1.0, 0.0, 0, 50);
    let expected: [u8; 25] = [
        0x02, 0, 0, 0, 1, 0, 0, 0, 2, 0x3F, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0, 0, 0, 0,
        0x00, 0x32, 0x00, 0x00,
    ];
    assert_eq!(bytes.as_slice(), expected);
}

#[wasm_bindgen_test]
fn decode_delta_datagram_roundtrip_golden_bytes() {
    // Matches `quanta-realtime-server/src/delta_envelope.rs::tests::golden_bytes`.
    let bytes: [u8; 15] = [
        0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A, 0xDE, 0xAD,
    ];
    let obj = decode_delta_datagram(&bytes).unwrap();
    assert_eq!(read_u32_field(&obj, "flags"), 1);
    assert_eq!(read_u32_field(&obj, "entitySlot"), 1);
    assert_eq!(read_u64_field(&obj, "tick"), 42);
    let payload = js_sys::Reflect::get(&obj, &"payload".into())
        .unwrap()
        .dyn_into::<js_sys::Uint8Array>()
        .unwrap();
    let mut copy = vec![0u8; payload.length() as usize];
    payload.copy_to(&mut copy);
    assert_eq!(copy, vec![0xDE, 0xAD]);
}

#[wasm_bindgen_test]
fn decode_delta_datagram_rejects_truncated() {
    let bytes = [0u8; 12];
    assert!(decode_delta_datagram(&bytes).is_err());
}

#[wasm_bindgen_test]
fn decode_initial_state_minimal_one_entity() {
    // [baseline_tick=42 u64 BE][flags=0][schema_version=1][entity_count=1]
    // [entity_slot=7 u32 BE][state_len=3 u32 BE][state=0xDE,0xAD,0xBE]
    let bytes: Vec<u8> = vec![
        0, 0, 0, 0, 0, 0, 0, 0x2A, // baseline_tick
        0x00, // flags
        0x01, // schema_version
        0, 0, 0, 1, // entity_count
        0, 0, 0, 7, // entity_slot
        0, 0, 0, 3, // state_len
        0xDE, 0xAD, 0xBE, // state
    ];
    let obj = decode_initial_state(&bytes).unwrap();
    assert_eq!(read_u64_field(&obj, "baselineTick"), 42);
    assert_eq!(read_u32_field(&obj, "schemaVersion"), 1);
    let entities = js_sys::Reflect::get(&obj, &"entities".into())
        .unwrap()
        .dyn_into::<js_sys::Array>()
        .unwrap();
    assert_eq!(entities.length(), 1);
    let first = entities.get(0);
    assert_eq!(read_u32_field(&first, "entitySlot"), 7);
}

#[wasm_bindgen_test]
fn decode_initial_state_with_schema_flag() {
    // flags=0x01 (FLAG_INCLUDES_SCHEMA), schema bytes "SCH" then 0 entities.
    let bytes: Vec<u8> = vec![
        0, 0, 0, 0, 0, 0, 0, 0,    // baseline_tick=0
        0x01, // flags=includes_schema
        0x02, // schema_version=2
        0, 0, 0, 3, // schema_len=3
        b'S', b'C', b'H', // schema bytes
        0, 0, 0, 0, // entity_count=0
    ];
    let obj = decode_initial_state(&bytes).unwrap();
    let schema = js_sys::Reflect::get(&obj, &"compiledSchema".into())
        .unwrap()
        .dyn_into::<js_sys::Uint8Array>()
        .unwrap();
    let mut copy = vec![0u8; schema.length() as usize];
    schema.copy_to(&mut copy);
    assert_eq!(copy, b"SCH");
}

#[wasm_bindgen_test]
fn decode_initial_state_rejects_truncated() {
    assert!(decode_initial_state(&[0u8; 5]).is_err());
}
