/// C2 regression test: `bitcode::decode(&bytes).unwrap()` in codec.rs.
///
/// Finding: codec.rs contains `bitcode::decode(&bytes).unwrap()` at lines 216
/// and 242 (inside the test module). The concern is that any code path reaching
/// `bitcode::decode` without error-propagation will panic on malformed input,
/// and the dual-macro situation (C1) means the panic-recovery path in
/// macros.rs itself has an unwrap — compounding the risk.
///
/// Static-grep approach is used here because the NIF entry points
/// (`encode_envelope_header`, `decode_envelope_header`) require a live BEAM
/// environment and cannot be invoked from a plain `cargo test` binary.
/// Runtime testing of the production `do_decode` function is not possible
/// without adding test-seams (pub(crate) visibility) that would modify
/// production source — which is prohibited.
///
/// The static approach directly verifies the source-level property: after the
/// fix, no call to `bitcode::decode(` in any non-test context should be
/// followed by `.unwrap()`.  The stricter form asserts zero occurrences of the
/// pattern anywhere in the file (including test code), as test code with
/// `.unwrap()` on decode operations also contributes to the finding's stated
/// line numbers.
///
/// Test outcome:
///   TODAY  — assertion FAILS (2 occurrences of the pattern exist in the file,
///             at lines 216 and 242 inside the #[cfg(test)] block)
///   FIXED  — assertion PASSES (all bitcode::decode calls use `?` or `.map_err`)
///
/// Runtime complement (informational, always passes):
///   A direct `bitcode::decode::<quanta_core_rs::EnvelopeHeader>` call on
///   all-zero bytes is also exercised via `std::panic::catch_unwind` to
///   confirm whether the crate panics or returns Err on malformed input.
///   This is a read-only probe that does NOT modify production code.
use quanta_core_rs::EnvelopeHeader;

const CODEC_RS: &str = include_str!("../src/codec.rs");

/// Count occurrences of `bitcode::decode(` + `.unwrap()` on the same source
/// line.  The pattern is simple and intentionally broad: any `.unwrap()` on
/// a `bitcode::decode` call, whether in production or test code.
fn count_bitcode_decode_unwrap_occurrences() -> usize {
    CODEC_RS
        .lines()
        .filter(|line| line.contains("bitcode::decode(") && line.contains(".unwrap()"))
        .count()
}

/// FAILING TODAY: codec.rs currently has 2 lines that call
/// `bitcode::decode(...).unwrap()` (lines 216 and 242, both inside the
/// `#[cfg(test)]` module).  The finding flags these as the evidence that the
/// pattern exists in the file.  After the fix, every bitcode::decode call
/// should propagate errors with `?` or `.map_err(...)`, making the count 0.
#[test]
fn c2_no_bitcode_decode_unwrap_in_codec_source() {
    let count = count_bitcode_decode_unwrap_occurrences();
    assert_eq!(
        count,
        0,
        "Found {} occurrence(s) of `bitcode::decode(...).unwrap()` in codec.rs. \
         All bitcode::decode calls must use error-propagation (`?` or `.map_err`) \
         so malformed bytes return an Err instead of panicking. \
         Lines with the pattern:\n{}",
        count,
        CODEC_RS
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("bitcode::decode(") && line.contains(".unwrap()"))
            .map(|(i, line)| format!("  line {}: {}", i + 1, line.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Runtime probe (informational): directly call bitcode::decode on malformed
/// bytes via catch_unwind to characterise the actual failure mode.
///
/// This test ALWAYS PASSES — it documents the behaviour rather than gating it.
/// It complements the static test above: even if the static test passes (no
/// `.unwrap()` in source), this probe confirms the decode returns Err rather
/// than panicking on garbage input.
#[test]
fn c2_bitcode_decode_on_malformed_bytes_does_not_panic() {
    let malformed_inputs: &[(&str, &[u8])] = &[
        ("all-zeros 16 bytes", &[0u8; 16]),
        ("all-0xFF 16 bytes", &[0xFFu8; 16]),
        ("empty slice", &[]),
        ("single byte 0x42", &[0x42]),
        ("truncated 4 bytes", &[0x01, 0x02, 0x03, 0x04]),
    ];

    for (label, bytes) in malformed_inputs {
        let bytes_owned = bytes.to_vec();
        let result =
            std::panic::catch_unwind(move || bitcode::decode::<EnvelopeHeader>(&bytes_owned));

        // The result of catch_unwind is Ok(decode_result).
        // decode_result should be Err(...) for malformed input.
        // If catch_unwind itself returns Err, the decode panicked — a bug.
        assert!(
            result.is_ok(),
            "bitcode::decode panicked on malformed input '{}' — \
             this means the caller must never use .unwrap() on decode results; \
             the panic would escape into the NIF scheduler thread.",
            label
        );

        let decode_result = result.unwrap();
        assert!(
            decode_result.is_err(),
            "bitcode::decode returned Ok on malformed input '{}' — \
             expected Err for garbage bytes.",
            label
        );
    }
}
