//! C1 regression test: `nif_safe!` must be defined exactly once, in `safety.rs`,
//! and must not use the unsafe `Atom::from_str(...).unwrap()` pattern in its
//! panic-recovery branch.
//!
//! History: a second definition once lived in `src/macros.rs` that used
//! `Atom::from_str($env, "error").unwrap()` in the panic-recovery branch. If
//! `from_str` itself panicked (e.g. invalid atom UTF-8 or NIF env corruption)
//! the BEAM scheduler thread died because there was no outer `catch_unwind`.
//! That duplicate file has since been deleted — `safety.rs` is now the sole
//! home for the macro and uses the pre-registered
//! `rustler::types::atom::error()` which cannot panic.
//!
//! This test guards against regressions on either axis:
//!   1. Re-introducing a duplicate `macro_rules! nif_safe` anywhere under the
//!      crate's `src/` tree, not just in the old `macros.rs` location.
//!   2. Re-introducing the `Atom::from_str(...).unwrap()` anti-pattern inside
//!      the surviving `safety.rs` definition.
//!
//! Test outcome:
//!   FIXED  — both assertions PASS (one definition; no unsafe unwrap in panic path)

const SAFETY_RS: &str = include_str!("../src/safety.rs");

/// Count `macro_rules! nif_safe` across every `.rs` file under `src/`.
///
/// Scanning the tree (instead of a fixed pair of `include_str!`s) means the
/// test catches a duplicate being reintroduced in any new module, not only the
/// original `macros.rs` location.
fn count_nif_safe_definitions() -> usize {
    let needle = "macro_rules! nif_safe";
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut count = 0;
    walk_rs_files(&src_dir, &mut |path| {
        if let Ok(contents) = std::fs::read_to_string(path) {
            count += contents.matches(needle).count();
        }
    });
    count
}

fn walk_rs_files(dir: &std::path::Path, visit: &mut dyn FnMut(&std::path::Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, visit);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            visit(&path);
        }
    }
}

/// Scan the surviving `macro_rules! nif_safe` block in `safety.rs` and check
/// whether the unsafe pattern (`Atom::from_str` + `.unwrap()`) appears anywhere
/// inside it.
fn unsafe_unwrap_present_in_any_nif_safe_definition() -> bool {
    let pattern_from_str = "Atom::from_str";
    let pattern_unwrap = ".unwrap()";

    let mut inside_macro = false;
    let mut brace_depth: i32 = 0;

    for line in SAFETY_RS.lines() {
        if line.contains("macro_rules! nif_safe") {
            inside_macro = true;
        }

        if inside_macro {
            brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
            brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;

            if line.contains(pattern_from_str) && line.contains(pattern_unwrap) {
                return true;
            }

            if brace_depth <= 0 {
                inside_macro = false;
                brace_depth = 0;
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
