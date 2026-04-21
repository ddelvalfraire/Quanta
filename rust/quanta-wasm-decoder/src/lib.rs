use wasm_bindgen::prelude::*;

use quanta_core_rs::delta::encoder::{
    self as core_encoder, dequantize, field_is_active, quantize_field, read_state, sign_extend,
    write_state,
};
use quanta_core_rs::schema::evolution::import_schema;
use quanta_core_rs::schema::{CompiledSchema, FieldMeta, FieldType};

#[wasm_bindgen]
#[derive(Debug)]
pub struct SchemaHandle {
    schema: CompiledSchema,
}

#[wasm_bindgen]
impl SchemaHandle {
    #[wasm_bindgen(getter)]
    pub fn field_count(&self) -> usize {
        self.schema.fields.len()
    }

    #[wasm_bindgen(getter)]
    pub fn version(&self) -> u8 {
        self.schema.version
    }

    #[wasm_bindgen(getter)]
    pub fn total_bits(&self) -> u32 {
        self.schema.total_bits
    }
}

#[wasm_bindgen]
pub fn create_schema(bytes: &[u8]) -> Result<SchemaHandle, JsError> {
    let schema = import_schema(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(SchemaHandle { schema })
}

/// Apply a binary delta to the current state, returning the new state bytes.
#[wasm_bindgen]
pub fn apply_delta(handle: &SchemaHandle, state: &[u8], delta: &[u8]) -> Result<Vec<u8>, JsError> {
    core_encoder::apply_delta(&handle.schema, state, delta)
        .map_err(|e| JsError::new(&e.to_string()))
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
        if !field_is_active(field) {
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
        if !field_is_active(field) {
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

fn decode_value(field: &FieldMeta, value: u64) -> JsValue {
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

fn expect_f64(field: &FieldMeta, js_val: &JsValue) -> Result<f64, JsError> {
    js_val
        .as_f64()
        .ok_or_else(|| JsError::new(&format!("expected number for field '{}'", field.name)))
}

fn encode_value(field: &FieldMeta, js_val: &JsValue) -> Result<u64, JsError> {
    if let Some(ref params) = field.quantization {
        let f = js_val.as_f64().ok_or_else(|| {
            JsError::new(&format!(
                "expected number for quantized field '{}'",
                field.name
            ))
        })?;
        return quantize_field(f, params, &field.name).map_err(|e| JsError::new(&e.to_string()));
    }

    match field.field_type {
        FieldType::Bool => {
            let b = js_val.as_bool().ok_or_else(|| {
                JsError::new(&format!("expected boolean for field '{}'", field.name))
            })?;
            Ok(b as u64)
        }
        FieldType::F32 => {
            let f = expect_f64(field, js_val)?;
            Ok((f as f32).to_bits() as u64)
        }
        FieldType::F64 => {
            let f = expect_f64(field, js_val)?;
            Ok(f.to_bits())
        }
        _ => {
            let f = expect_f64(field, js_val)?;
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

// Mirrors quanta-realtime-server auth.rs / sync.rs structs.
// Field order must match exactly for bitcode compatibility.

#[derive(bitcode::Encode)]
struct AuthRequest {
    token: String,
    client_version: String,
    session_token: Option<u64>,
    transfer_token: Option<Vec<u8>>,
}

#[derive(bitcode::Encode, bitcode::Decode)]
struct AuthResponse {
    session_id: u64,
    accepted: bool,
    reason: String,
}

#[derive(bitcode::Encode)]
struct BaselineAck {
    baseline_tick: u64,
}

#[wasm_bindgen]
pub fn encode_auth_request(
    token: &str,
    client_version: &str,
    session_token: Option<u64>,
    transfer_token: Option<Vec<u8>>,
) -> Vec<u8> {
    let req = AuthRequest {
        token: token.to_string(),
        client_version: client_version.to_string(),
        session_token,
        transfer_token,
    };
    let payload = bitcode::encode(&req);
    length_prefix(&payload)
}

#[wasm_bindgen]
pub fn decode_auth_response(bytes: &[u8]) -> Result<JsValue, JsError> {
    let payload = strip_length_prefix(bytes)?;
    let resp: AuthResponse =
        bitcode::decode(payload).map_err(|e| JsError::new(&format!("decode AuthResponse: {e}")))?;

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"sessionId".into(), &JsValue::from(resp.session_id))
        .map_err(|_| JsError::new("failed to set sessionId"))?;
    js_sys::Reflect::set(&obj, &"accepted".into(), &JsValue::from_bool(resp.accepted))
        .map_err(|_| JsError::new("failed to set accepted"))?;
    js_sys::Reflect::set(&obj, &"reason".into(), &JsValue::from_str(&resp.reason))
        .map_err(|_| JsError::new("failed to set reason"))?;

    Ok(obj.into())
}

#[wasm_bindgen]
pub fn encode_auth_response(session_id: u64, accepted: bool, reason: &str) -> Vec<u8> {
    let resp = AuthResponse {
        session_id,
        accepted,
        reason: reason.to_string(),
    };
    let payload = bitcode::encode(&resp);
    length_prefix(&payload)
}

/// Encode a BaselineAck as `[4-byte BE length][bitcode payload]`.
#[wasm_bindgen]
pub fn encode_baseline_ack(baseline_tick: u64) -> Vec<u8> {
    let ack = BaselineAck { baseline_tick };
    let payload = bitcode::encode(&ack);
    length_prefix(&payload)
}

// ---------------------------------------------------------------------------
// Particle input datagram (msg_type = 0x02) — 25-byte wire format.
// MUST stay byte-for-byte identical with
// `quanta-particle-demo/src/input.rs`; see `client_input_cross_language_golden_bytes`.
// ---------------------------------------------------------------------------

const INPUT_MSG_TYPE: u8 = 0x02;
const INPUT_DATAGRAM_LEN: usize = 25;

#[wasm_bindgen]
pub fn encode_client_input(
    entity_slot: u32,
    input_seq: u32,
    dir_x: f32,
    dir_z: f32,
    actions: u32,
    dt_ms: u16,
) -> Vec<u8> {
    let mut buf = vec![0u8; INPUT_DATAGRAM_LEN];
    buf[0] = INPUT_MSG_TYPE;
    buf[1..5].copy_from_slice(&entity_slot.to_be_bytes());
    buf[5..9].copy_from_slice(&input_seq.to_be_bytes());
    buf[9..13].copy_from_slice(&dir_x.to_be_bytes());
    buf[13..17].copy_from_slice(&dir_z.to_be_bytes());
    buf[17..21].copy_from_slice(&actions.to_be_bytes());
    buf[21..23].copy_from_slice(&dt_ms.to_be_bytes());
    // buf[23..25] reserved, already zero.
    buf
}

// ---------------------------------------------------------------------------
// Delta datagram envelope — 13-byte header.
// MUST stay in sync with `quanta-realtime-server/src/delta_envelope.rs`.
// ---------------------------------------------------------------------------

const DELTA_HEADER_LEN: usize = 13;

#[wasm_bindgen]
pub fn decode_delta_datagram(bytes: &[u8]) -> Result<JsValue, JsError> {
    if bytes.len() < DELTA_HEADER_LEN {
        return Err(JsError::new(&format!(
            "delta datagram too short: need {DELTA_HEADER_LEN}, got {}",
            bytes.len()
        )));
    }
    // Length check above guarantees the slice indexes below are in-bounds.
    let flags = bytes[0];
    let entity_slot = u32::from_be_bytes(bytes[1..5].try_into().unwrap());
    let tick = u64::from_be_bytes(bytes[5..13].try_into().unwrap());
    let payload = &bytes[DELTA_HEADER_LEN..];

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"flags".into(), &JsValue::from(flags))
        .map_err(|_| JsError::new("set flags"))?;
    js_sys::Reflect::set(&obj, &"entitySlot".into(), &JsValue::from(entity_slot))
        .map_err(|_| JsError::new("set entitySlot"))?;
    js_sys::Reflect::set(&obj, &"tick".into(), &JsValue::from(tick))
        .map_err(|_| JsError::new("set tick"))?;
    js_sys::Reflect::set(
        &obj,
        &"payload".into(),
        &js_sys::Uint8Array::from(payload).into(),
    )
    .map_err(|_| JsError::new("set payload"))?;
    Ok(obj.into())
}

// ---------------------------------------------------------------------------
// InitialStateMessage — see `quanta-realtime-server/src/sync.rs` for the
// authoritative wire format. Duplicated here (rather than importing the
// server crate) so wasm-decoder can compile to `wasm32-unknown-unknown`
// without pulling quinn/tokio.
// ---------------------------------------------------------------------------

const INITIAL_STATE_HEADER: usize = 8 + 1 + 1 + 4;
const FLAG_INCLUDES_SCHEMA: u8 = 0x01;

#[wasm_bindgen]
pub fn decode_initial_state(bytes: &[u8]) -> Result<JsValue, JsError> {
    if bytes.len() < INITIAL_STATE_HEADER {
        return Err(JsError::new(&format!(
            "initial state truncated: need {INITIAL_STATE_HEADER}, got {}",
            bytes.len()
        )));
    }
    let mut pos = 0;
    let baseline_tick = u64::from_be_bytes(bytes[pos..pos + 8].try_into().unwrap());
    pos += 8;
    let flags = bytes[pos];
    pos += 1;
    let schema_version = bytes[pos];
    pos += 1;

    let compiled_schema = if flags & FLAG_INCLUDES_SCHEMA != 0 {
        if pos + 4 > bytes.len() {
            return Err(JsError::new("truncated schema header"));
        }
        let schema_len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + schema_len > bytes.len() {
            return Err(JsError::new("truncated schema bytes"));
        }
        let s = bytes[pos..pos + schema_len].to_vec();
        pos += schema_len;
        Some(s)
    } else {
        None
    };

    if pos + 4 > bytes.len() {
        return Err(JsError::new("truncated entity count"));
    }
    let entity_count = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap());
    pos += 4;

    let entities = js_sys::Array::new();
    for _ in 0..entity_count {
        if pos + 8 > bytes.len() {
            return Err(JsError::new("truncated entity header"));
        }
        let slot = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap());
        pos += 4;
        let state_len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + state_len > bytes.len() {
            return Err(JsError::new("truncated entity state"));
        }
        let state = &bytes[pos..pos + state_len];
        pos += state_len;
        let entry = js_sys::Object::new();
        js_sys::Reflect::set(&entry, &"entitySlot".into(), &JsValue::from(slot))
            .map_err(|_| JsError::new("set entitySlot"))?;
        js_sys::Reflect::set(
            &entry,
            &"state".into(),
            &js_sys::Uint8Array::from(state).into(),
        )
        .map_err(|_| JsError::new("set state"))?;
        entities.push(&entry);
    }

    let out = js_sys::Object::new();
    js_sys::Reflect::set(&out, &"baselineTick".into(), &JsValue::from(baseline_tick))
        .map_err(|_| JsError::new("set baselineTick"))?;
    js_sys::Reflect::set(&out, &"flags".into(), &JsValue::from(flags))
        .map_err(|_| JsError::new("set flags"))?;
    js_sys::Reflect::set(
        &out,
        &"schemaVersion".into(),
        &JsValue::from(schema_version),
    )
    .map_err(|_| JsError::new("set schemaVersion"))?;
    if let Some(s) = compiled_schema {
        js_sys::Reflect::set(
            &out,
            &"compiledSchema".into(),
            &js_sys::Uint8Array::from(s.as_slice()).into(),
        )
        .map_err(|_| JsError::new("set compiledSchema"))?;
    }
    js_sys::Reflect::set(&out, &"entities".into(), &entities)
        .map_err(|_| JsError::new("set entities"))?;
    Ok(out.into())
}

fn length_prefix(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

fn strip_length_prefix(bytes: &[u8]) -> Result<&[u8], JsError> {
    if bytes.len() < 4 {
        return Err(JsError::new("message too short: need at least 4 bytes"));
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < 4 + len {
        return Err(JsError::new(&format!(
            "message truncated: expected {} bytes, got {}",
            4 + len,
            bytes.len()
        )));
    }
    Ok(&bytes[4..4 + len])
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

        let result = apply_delta(&handle, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn apply_empty_delta() {
        let schema = two_field_schema();
        let handle = roundtrip_schema(&schema);
        let state = write_state(&schema, &[1, 100]);

        let result = apply_delta(&handle, &state, &[]).unwrap();
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

        let result = apply_delta(&handle, &old, &delta).unwrap();
        assert_eq!(result, new);
    }

    // JsError::new() requires wasm target — error paths tested in tests/web.rs

    // -----------------------------------------------------------------------
    // Protocol encoding tests — verify bitcode compatibility
    // -----------------------------------------------------------------------

    // AuthRequest needs a Decode-capable mirror since the production struct
    // only derives Encode (client never decodes requests).
    #[derive(bitcode::Decode)]
    struct TestAuthRequest {
        token: String,
        client_version: String,
        session_token: Option<u64>,
        transfer_token: Option<Vec<u8>>,
    }

    #[derive(bitcode::Decode)]
    struct TestBaselineAck {
        baseline_tick: u64,
    }

    #[test]
    fn encode_auth_request_decodable_by_server() {
        let bytes = encode_auth_request("tok_abc", "0.1.0", None, None);
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let decoded: TestAuthRequest = bitcode::decode(&bytes[4..4 + len]).unwrap();
        assert_eq!(decoded.token, "tok_abc");
        assert_eq!(decoded.client_version, "0.1.0");
        assert_eq!(decoded.session_token, None);
        assert_eq!(decoded.transfer_token, None);
    }

    #[test]
    fn encode_auth_request_with_session_token() {
        let bytes = encode_auth_request("tok", "0.2.0", Some(42), None);
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let decoded: TestAuthRequest = bitcode::decode(&bytes[4..4 + len]).unwrap();
        assert_eq!(decoded.session_token, Some(42));
    }

    #[test]
    fn auth_response_encode_decode_roundtrip() {
        let bytes = encode_auth_response(99, true, "");
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let decoded: AuthResponse = bitcode::decode(&bytes[4..4 + len]).unwrap();
        assert_eq!(decoded.session_id, 99);
        assert!(decoded.accepted);
        assert!(decoded.reason.is_empty());
    }

    #[test]
    fn auth_response_rejected() {
        let bytes = encode_auth_response(0, false, "invalid_token");
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let decoded: AuthResponse = bitcode::decode(&bytes[4..4 + len]).unwrap();
        assert!(!decoded.accepted);
        assert_eq!(decoded.reason, "invalid_token");
    }

    // -----------------------------------------------------------------------
    // Cross-language wire-format parity — these byte sequences MUST match
    // the golden-bytes tests in the server / demo crates. If either side
    // changes, both tests must be updated together.
    // -----------------------------------------------------------------------

    #[test]
    fn client_input_cross_language_golden_bytes() {
        // Matches `quanta-particle-demo/src/input.rs::tests::golden_bytes`.
        let bytes = encode_client_input(1, 2, 1.0, 0.0, 0, 50);
        let expected: [u8; INPUT_DATAGRAM_LEN] = [
            0x02, 0, 0, 0, 1, 0, 0, 0, 2, 0x3F, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0, 0, 0,
            0, 0x00, 0x32, 0x00, 0x00,
        ];
        assert_eq!(bytes.as_slice(), expected);
    }

    #[test]
    fn encode_baseline_ack_decodable_by_server() {
        let bytes = encode_baseline_ack(42000);
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let decoded: TestBaselineAck = bitcode::decode(&bytes[4..4 + len]).unwrap();
        assert_eq!(decoded.baseline_tick, 42000);
    }
}
