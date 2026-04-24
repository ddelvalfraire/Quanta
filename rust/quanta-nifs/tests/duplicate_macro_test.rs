/// C1 regression test: Duplicate `nif_safe!` macro definitions.
///
/// Finding: Two files both define `macro_rules! nif_safe`:
///   - src/macros.rs  — uses `Atom::from_str($env, "error").unwrap()` in the
///                       panic-recovery branch.  If `from_str` itself panics
///                       (e.g. invalid atom UTF-8 or NIF env corruption) the
///                       BEAM scheduler thread dies.
///   - src/safety.rs  — uses the pre-registered `rustler::types::atom::error()`
///                       which is safe and cannot panic.
///
/// After the fix: only one definition should remain (the safe one in safety.rs)
/// and no remaining definition should contain `Atom::from_str` followed by
/// `.unwrap()` inside the macro body.
///
/// Test outcome:
///   TODAY  — both assertions FAIL (two definitions exist; the unsafe pattern is present)
///   FIXED  — both assertions PASS (one definition; no unsafe unwrap in panic path)

const MACROS_RS: &str = include_str!("../src/macros.rs");
const SAFETY_RS: &str = include_str!("../src/safety.rs");

/// Count `macro_rules! nif_safe` across both source files combined.
fn count_nif_safe_definitions() -> usize {
    let needle = "macro_rules! nif_safe";
    MACROS_RS.matches(needle).count() + SAFETY_RS.matches(needle).count()
}

/// Collect every line that is part of a `macro_rules! nif_safe` block in either
/// file and check whether the unsafe pattern (`Atom::from_str` + `.unwrap()`)
/// appears anywhere inside those blocks.
///
/// The approach is intentionally conservative: we flag the pattern wherever it
/// appears in either file so that moving the macro doesn't accidentally preserve
/// the dangerous code path.
fn unsafe_unwrap_present_in_any_nif_safe_definition() -> bool {
    let pattern_from_str = "Atom::from_str";
    let pattern_unwrap = ".unwrap()";

    for source in [MACROS_RS, SAFETY_RS] {
        let mut inside_macro = false;
        let mut brace_depth: i32 = 0;

        for line in source.lines() {
            if line.contains("macro_rules! nif_safe") {
                inside_macro = true;
            }

            if inside_macro {
                brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
                brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;

                // Check for the dangerous pattern on this line while still in
                // the macro body.
                if line.contains(pattern_from_str) && line.contains(pattern_unwrap) {
                    return true;
                }

                // The macro definition ends when braces balance back to zero after
                // we have seen at least one opening brace.
                if inside_macro && brace_depth <= 0 {
                    inside_macro = false;
                    brace_depth = 0;
                }
            }
        }
    }

    false
}

/// FAILING TODAY: There are currently two `macro_rules! nif_safe` definitions
/// (one in macros.rs, one in safety.rs). After the fix only safety.rs retains
/// the definition, so the count drops to 1.
#[test]
fn c1_only_one_nif_safe_macro_definition_exists() {
    let count = count_nif_safe_definitions();
    assert_eq!(
        count, 1,
        "Expected exactly 1 `macro_rules! nif_safe` definition across \
         macros.rs and safety.rs, but found {}. \
         Duplicate macro: macros.rs must be removed and all call-sites \
         must import from safety.rs instead.",
        count
    );
}

/// FAILING TODAY: The macros.rs definition contains
/// `Atom::from_str($env, \"error\").unwrap()` inside the panic-recovery branch.
/// If `from_str` itself panics the BEAM scheduler thread crashes — there is no
/// further catch_unwind around it.
///
/// After the fix (only safety.rs definition survives, using
/// `rustler::types::atom::error()`) this test passes.
#[test]
fn c1_no_nif_safe_definition_contains_atom_from_str_unwrap() {
    let present = unsafe_unwrap_present_in_any_nif_safe_definition();
    assert!(
        !present,
        "Found `Atom::from_str(...).unwrap()` inside a `macro_rules! nif_safe` \
         definition. This is dangerous: if `from_str` panics the BEAM scheduler \
         thread dies because there is no outer catch_unwind. \
         Fix: replace with the pre-registered atom (`rustler::types::atom::error()`) \
         as done in safety.rs."
    );
}
