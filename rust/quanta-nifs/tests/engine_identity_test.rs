/// C3 regression test: Component::deserialize with a mismatched Engine config.
///
/// Finding: `unsafe { Component::deserialize(&engine_arc.0, serialized) }` at
/// wasm_runtime.rs:413 carries no engine-identity check in the application
/// code itself. The HMAC only verifies that the bytes were produced by *this*
/// process, not that they were produced by an Engine with the *same* Config.
///
/// This test proves whether wasmtime 40.0.4 internally catches a meaningful
/// Config mismatch (fuel enabled vs disabled) when `Component::deserialize`
/// is called, or whether it silently succeeds (UB / wrong behaviour).
///
/// Expected result shape:
/// - If wasmtime catches it → deserialize returns Err → test PASSES
///   → finding severity should be downgraded (wasmtime self-enforces)
/// - If wasmtime does NOT catch it → deserialize returns Ok → test FAILS
///   → finding reproduces: no engine-identity guard exists
///
/// The test mirrors the exact code path in wasm_runtime.rs:
///   Component::new(&engine_a, wasm)  →  component.serialize()  →
///   unsafe { Component::deserialize(&engine_b, serialized) }
use wasmtime::{Config, Engine};
use wasmtime::component::Component;

/// Minimal valid WebAssembly component binary.
///
/// The empty component `(component)` is the smallest legal component-model
/// object.  Encoding:
///   00 61 73 6d  — magic "\0asm"
///   0d 00 01 00  — version 13 (component model), layer 1, encoding 0
const MINIMAL_COMPONENT_BYTES: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x0d, 0x00, 0x01, 0x00, // component-model header
];

fn make_engine_with_fuel(consume_fuel: bool) -> Engine {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(consume_fuel);
    Engine::new(&config).expect("Engine::new should succeed")
}

#[test]
fn cross_engine_deserialize_with_different_fuel_config_returns_err() {
    // --- Arrange ---
    // Engine A: fuel ENABLED (matches the production engine_new_inner() config)
    let engine_a = make_engine_with_fuel(true);

    // Engine B: fuel DISABLED — a different tunable from Engine A
    let engine_b = make_engine_with_fuel(false);

    // Compile and serialize a minimal component with Engine A
    let component_a = Component::new(&engine_a, MINIMAL_COMPONENT_BYTES)
        .expect("Component::new with engine_a should succeed");

    let serialized = component_a
        .serialize()
        .expect("Component::serialize should succeed");

    // --- Act ---
    // Attempt to deserialize Engine-A bytes using Engine B, mirroring line 413:
    //   unsafe { Component::deserialize(&engine_arc.0, serialized) }
    //
    // SAFETY NOTE (for the test): we are intentionally passing bytes from a
    // different engine config to prove the presence or absence of a runtime
    // identity check.  This is the unsafe operation under scrutiny.
    let result = unsafe { Component::deserialize(&engine_b, &serialized) };

    // --- Assert ---
    // The finding claims no identity check exists, so deserialize silently
    // "succeeds" (UB territory).  If wasmtime 40.x internally enforces config
    // parity, result will be Err and this assertion will pass, which means the
    // finding's severity should be downgraded.
    //
    // If result is Ok the assertion fails → finding reproduces.
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!(
            "Expected Component::deserialize to return Err when Engine configs differ \
             (Engine A: consume_fuel=true, Engine B: consume_fuel=false), but it \
             returned Ok.  This means wasmtime 40.0.4 does NOT enforce engine-config \
             parity on deserialization — finding C3 reproduces."
        ),
    };

    // Confirm the error message identifies the mismatch
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("fuel") || err_msg.contains("compatible") || err_msg.contains("compiled"),
        "Error message should reference fuel/compatible/compiled mismatch, got: {err_msg}"
    );
}
