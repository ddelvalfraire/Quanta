//! M-1 regression test: server-encoder / wasm-decoder byte parity for the
//! `InitialStateMessage` wire format when `FLAG_INCLUDES_SCHEMA` is CLEARED.
//!
//! Finding: `wasm-decoder/src/lib.rs` declares
//! `INITIAL_STATE_HEADER = 8 + 1 + 1 + 4 = 14`, comprising
//! `tick(8) + flags(1) + schema_version(1) + schema_len(4)`. When the
//! `FLAG_INCLUDES_SCHEMA` flag is cleared, the server encoder in
//! `quanta-realtime-server/src/sync.rs::encode_initial_state` ELIDES the
//! 4-byte schema_len field entirely:
//!
//! ```text
//! if let Some(schema) = &msg.compiled_schema {
//!     buf.extend_from_slice(&(schema.len() as u32).to_be_bytes());
//!     buf.extend_from_slice(schema);
//! }
//! ```
//!
//! The wasm decoder compensates by only advancing `pos` by 4 when the flag is
//! set (`src/lib.rs` line 352), then reads `entity_count` from whatever byte
//! offset it lands at. The two must agree.
//!
//! Risk: If either side's logic drifts (e.g. encoder always writes a reserved
//! schema_len=0, decoder always skips 4 bytes), `entity_count` is read from
//! the wrong offset → silent protocol corruption.
//!
//! Test shape:
//!   1. Hand-craft the server's wire bytes for an `InitialStateMessage` with
//!      `flags = 0` (schema excluded), a known `baseline_tick`,
//!      `schema_version`, and two entities with known slots and states. The
//!      byte layout is taken directly from
//!      `quanta-realtime-server/src/sync.rs::encode_initial_state`.
//!   2. Decode with the wasm decoder's native helper
//!      (`decode_initial_state_native`) and assert every header field and
//!      entity payload matches what was encoded.
//!   3. Also cover the `FLAG_INCLUDES_SCHEMA = 1` branch so regressions on
//!      either path are caught.
//!
//! We do NOT take `quanta-realtime-server` as a dev-dependency — that would
//! pull quinn/tokio just to test a pure-bytes helper. Hand-crafted bytes
//! match the server encoder's deterministic layout exactly, and the server
//! crate already has a round-trip test (`roundtrip_with_schema`) covering
//! its own encoder/decoder for both flag states.

use quanta_wasm_decoder::decode_initial_state_native;

/// Mirror of server's `FLAG_INCLUDES_SCHEMA`. Kept as a literal so a rename
/// on either side is caught.
const FLAG_INCLUDES_SCHEMA: u8 = 0x01;

/// Hand-craft the server's wire bytes for an `InitialStateMessage` exactly
/// as `encode_initial_state` in `quanta-realtime-server/src/sync.rs` writes
/// them. When `schema` is `None`, the `schema_len` field is ELIDED (not
/// written as zero).
fn encode_like_server(
    baseline_tick: u64,
    flags: u8,
    schema_version: u8,
    schema: Option<&[u8]>,
    entities: &[(u32, &[u8])],
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&baseline_tick.to_be_bytes());
    buf.push(flags);
    buf.push(schema_version);
    if let Some(schema) = schema {
        buf.extend_from_slice(&(schema.len() as u32).to_be_bytes());
        buf.extend_from_slice(schema);
    }
    buf.extend_from_slice(&(entities.len() as u32).to_be_bytes());
    for (slot, state) in entities {
        buf.extend_from_slice(&slot.to_be_bytes());
        buf.extend_from_slice(&(state.len() as u32).to_be_bytes());
        buf.extend_from_slice(state);
    }
    buf
}

#[test]
fn m1_decoder_reads_correct_offset_when_flag_cleared() {
    let baseline_tick = 0x0123_4567_89AB_CDEFu64;
    let schema_version = 7u8;
    let entities: &[(u32, &[u8])] = &[(42, &[0xDE, 0xAD]), (99, &[0xBE, 0xEF, 0x01])];

    let bytes = encode_like_server(baseline_tick, 0, schema_version, None, entities);

    // Sanity: without the elided schema_len, the header should be exactly
    // tick(8) + flags(1) + schema_version(1) + entity_count(4) = 14 bytes
    // before the entity payloads begin.
    //   entity 0: slot(4) + len(4) + state(2) = 10
    //   entity 1: slot(4) + len(4) + state(3) = 11
    // Total = 14 + 10 + 11 = 35.
    assert_eq!(
        bytes.len(),
        35,
        "wire length mismatch — encoder may have added an unexpected field"
    );

    let decoded = decode_initial_state_native(&bytes)
        .expect("decoder must accept valid flag-cleared initial state");

    assert_eq!(decoded.baseline_tick, baseline_tick);
    assert_eq!(decoded.flags, 0);
    assert_eq!(decoded.schema_version, schema_version);
    assert!(decoded.compiled_schema.is_none());
    assert_eq!(decoded.entities.len(), 2);
    assert_eq!(decoded.entities[0].entity_slot, 42);
    assert_eq!(decoded.entities[0].state, vec![0xDE, 0xAD]);
    assert_eq!(decoded.entities[1].entity_slot, 99);
    assert_eq!(decoded.entities[1].state, vec![0xBE, 0xEF, 0x01]);
}

#[test]
fn m1_decoder_reads_correct_offset_when_flag_set() {
    let baseline_tick = 42u64;
    let schema_version = 3u8;
    let schema: &[u8] = &[0x01, 0x02, 0x03, 0x04];
    let entities: &[(u32, &[u8])] = &[(7, &[0xAA, 0xBB, 0xCC])];

    let bytes = encode_like_server(
        baseline_tick,
        FLAG_INCLUDES_SCHEMA,
        schema_version,
        Some(schema),
        entities,
    );

    let decoded = decode_initial_state_native(&bytes)
        .expect("decoder must accept valid flag-set initial state");

    assert_eq!(decoded.baseline_tick, baseline_tick);
    assert_eq!(decoded.flags, FLAG_INCLUDES_SCHEMA);
    assert_eq!(decoded.schema_version, schema_version);
    assert_eq!(decoded.compiled_schema.as_deref(), Some(schema));
    assert_eq!(decoded.entities.len(), 1);
    assert_eq!(decoded.entities[0].entity_slot, 7);
    assert_eq!(decoded.entities[0].state, vec![0xAA, 0xBB, 0xCC]);
}

#[test]
fn m1_decoder_rejects_truncated_header_when_flag_cleared() {
    // 10 bytes = tick(8) + flags(1) + schema_version(1), no entity_count.
    // With flag cleared, the decoder must still require the 4-byte
    // entity_count and fail cleanly on truncation.
    let short = vec![0u8; 10];
    let err = decode_initial_state_native(&short);
    assert!(
        err.is_err(),
        "decoder must reject a 10-byte payload that omits entity_count"
    );
}

/// H-2 regression: when `FLAG_INCLUDES_SCHEMA` is set, the minimum wire size is
/// 14 (flag-cleared header) + 4 (schema_len) + N (schema bytes) + 4
/// (entity_count). An input that is exactly 14 bytes with the flag set is
/// structurally truncated — there is no room for even the schema_len field —
/// and the decoder must reject it with an error that clearly identifies the
/// header as the site of truncation.
///
/// Today the decoder's single 14-byte guard at the top passes for this input
/// regardless of the flag. It then reads a zero schema_len, advances `pos` to
/// 14, and fails deeper in the function with `"truncated entity count"` —
/// which points operators at the wrong field. The fix is to bound-check the
/// header against the flag state before touching schema_len.
#[test]
fn h2_decoder_rejects_truncated_header_when_flag_set() {
    let mut bytes = vec![0u8; 14];
    bytes[8] = FLAG_INCLUDES_SCHEMA; // flag SET, but no schema_len/schema/entity_count follow.

    let result = decode_initial_state_native(&bytes);
    assert!(
        result.is_err(),
        "flag-set 14-byte input must be rejected — there is no room for \
         schema_len, schema bytes, or entity_count"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("header"),
        "error must identify the header as truncated so operators don't \
         chase the wrong field. Got: {err}"
    );
}
