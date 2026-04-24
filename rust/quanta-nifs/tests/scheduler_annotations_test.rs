/// Scheduler annotation and panic-safety regression tests.
///
/// Findings covered:
///
/// * **H1** — `nats_connect` in `src/nats/mod.rs` is annotated
///   `#[rustler::nif(schedule = "DirtyCpu")]` but internally calls
///   `runtime.block_on(...)` which performs TCP/TLS I/O. The correct
///   scheduler hint is `"DirtyIo"`.
///
/// * **H2** — `encode_envelope_header` and `decode_envelope_header` in
///   `src/codec.rs` are plain `#[rustler::nif]` with no `schedule = ...`
///   argument, so they run on the normal scheduler. The work is O(n) in
///   user-controlled `metadata` pairs and should run on `"DirtyCpu"`.
///
/// * **H4** — `encode_metadata` in `src/codec.rs` has a fallback path that
///   calls `Term::map_from_pairs(env, empty).unwrap()` inside a
///   `.unwrap_or_else(...)` closure. If the primary `map_from_pairs` fails
///   and the fallback also fails, this double-panics. Additionally,
///   `encode_effect` in `src/wasm_runtime.rs` is riddled with
///   `.map_put(...).unwrap()` calls that can panic if the BEAM rejects the
///   key/value pair.
///
/// These tests are static: they inspect the source files at compile time
/// via `include_str!`, so they do not require a running BEAM.
const NATS_MOD_RS: &str = include_str!("../src/nats/mod.rs");
const CODEC_RS: &str = include_str!("../src/codec.rs");
const WASM_RUNTIME_RS: &str = include_str!("../src/wasm_runtime.rs");

/// Extract the `#[rustler::nif(...)]` (or `#[rustler::nif]`) attribute line
/// that immediately precedes the definition of `fn <name>` in `source`.
/// Returns `None` if the function is not found or is not preceded by such
/// an attribute.
fn find_nif_attr_for_fn<'a>(source: &'a str, fn_name: &str) -> Option<&'a str> {
    let lines: Vec<&str> = source.lines().collect();
    // We tolerate both `fn name<` (with lifetimes/generics) and `fn name(`.
    let fn_prefixes = [format!("fn {}<", fn_name), format!("fn {}(", fn_name)];
    let pub_fn_prefixes = [
        format!("pub fn {}<", fn_name),
        format!("pub fn {}(", fn_name),
        format!("pub(crate) fn {}<", fn_name),
        format!("pub(crate) fn {}(", fn_name),
    ];

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let matches_fn = fn_prefixes.iter().any(|p| trimmed.starts_with(p))
            || pub_fn_prefixes.iter().any(|p| trimmed.starts_with(p));
        if !matches_fn {
            continue;
        }
        // Walk back to the first non-blank, non-doc-comment line and check
        // that it contains `#[rustler::nif`.
        let mut scan = idx;
        while scan > 0 {
            scan -= 1;
            let prev = lines[scan].trim();
            if prev.is_empty() || prev.starts_with("//") || prev.starts_with("///") {
                continue;
            }
            if prev.contains("#[rustler::nif") {
                return Some(lines[scan]);
            }
            // Hit some other code before finding the attribute.
            return None;
        }
        return None;
    }
    None
}

/// Brace-match the body of `fn <name>` inside `source` and return the
/// slice spanning from the `fn` keyword through the closing brace.
///
/// Requires an exact match on `fn <name>(` or `fn <name><` so that, for
/// example, looking up `encode_effect` does not accidentally match
/// `encode_effects`.
fn extract_fn_body<'a>(source: &'a str, fn_name: &str) -> &'a str {
    let needles = [format!("fn {}<", fn_name), format!("fn {}(", fn_name)];
    let start = needles
        .iter()
        .filter_map(|n| source.find(n.as_str()))
        .min()
        .unwrap_or_else(|| panic!("fn {} not found", fn_name));
    let body_start = source[start..]
        .find('{')
        .unwrap_or_else(|| panic!("opening brace for fn {} not found", fn_name))
        + start;
    let mut depth: i32 = 0;
    for (i, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..body_start + i + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in body of fn {}", fn_name);
}

/// Count bare `.unwrap()` calls in `snippet`, excluding any occurrence
/// preceded by `.unwrap_or`, `.unwrap_or_else`, or `.unwrap_or_default`,
/// and ignoring lines starting with `//`.
fn count_bare_unwrap(snippet: &str) -> usize {
    let mut total = 0usize;
    for line in snippet.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut idx = 0;
        while let Some(pos) = line[idx..].find(".unwrap()") {
            let abs = idx + pos;
            let preceding = &line[..abs];
            let is_combinator = preceding.ends_with(".unwrap_or")
                || preceding.ends_with(".unwrap_or_else")
                || preceding.ends_with(".unwrap_or_default");
            if !is_combinator {
                total += 1;
            }
            idx = abs + ".unwrap()".len();
        }
    }
    total
}

// ---------------------------------------------------------------------------
// H1
// ---------------------------------------------------------------------------

/// FAILING TODAY: `nats_connect` is annotated `schedule = "DirtyCpu"` even
/// though it drives a tokio runtime that performs TCP/TLS connect I/O.
/// After the fix it must be `schedule = "DirtyIo"`.
#[test]
fn h1_nats_connect_uses_dirty_io() {
    let attr = find_nif_attr_for_fn(NATS_MOD_RS, "nats_connect")
        .expect("could not locate #[rustler::nif(...)] attribute above `fn nats_connect`");

    assert!(
        attr.contains("schedule = \"DirtyIo\""),
        "src/nats/mod.rs `nats_connect` must be scheduled on DirtyIo because \
         its body calls `runtime.block_on(async {{ ... .connect(...).await }})` \
         which performs blocking TCP/TLS I/O. \
         Found attribute line: `{}`.",
        attr.trim()
    );
    assert!(
        !attr.contains("schedule = \"DirtyCpu\""),
        "src/nats/mod.rs `nats_connect` is still annotated DirtyCpu. \
         Blocking network I/O must not run on a DirtyCpu scheduler. \
         Found attribute line: `{}`.",
        attr.trim()
    );
}

// ---------------------------------------------------------------------------
// H2
// ---------------------------------------------------------------------------

/// FAILING TODAY: `encode_envelope_header` has no `schedule = ...` argument
/// → it runs on the normal scheduler. `bitcode::encode` over a
/// user-controlled `Vec<(String, String)>` metadata list is O(n) work and
/// can block the scheduler thread under abusive inputs.
#[test]
fn h2_encode_envelope_header_uses_dirty_cpu() {
    let attr = find_nif_attr_for_fn(CODEC_RS, "encode_envelope_header")
        .expect("could not locate #[rustler::nif(...)] attribute above `fn encode_envelope_header`");

    assert!(
        attr.contains("schedule = \"DirtyCpu\""),
        "src/codec.rs `encode_envelope_header` must be `#[rustler::nif(schedule = \"DirtyCpu\")]` — \
         `bitcode::encode` over user-supplied metadata is O(n) CPU work. \
         Found attribute line: `{}`.",
        attr.trim()
    );
}

/// FAILING TODAY: `decode_envelope_header` has no `schedule = ...` argument
/// → same reasoning as above. `bitcode::decode` over an arbitrarily-sized
/// input binary is O(n) work and must go on DirtyCpu.
#[test]
fn h2_decode_envelope_header_uses_dirty_cpu() {
    let attr = find_nif_attr_for_fn(CODEC_RS, "decode_envelope_header")
        .expect("could not locate #[rustler::nif(...)] attribute above `fn decode_envelope_header`");

    assert!(
        attr.contains("schedule = \"DirtyCpu\""),
        "src/codec.rs `decode_envelope_header` must be `#[rustler::nif(schedule = \"DirtyCpu\")]` — \
         `bitcode::decode` over a caller-supplied binary is O(n) CPU work. \
         Found attribute line: `{}`.",
        attr.trim()
    );
}

// ---------------------------------------------------------------------------
// H4
// ---------------------------------------------------------------------------

/// FAILING TODAY: `encode_metadata` in src/codec.rs ends with
///
/// ```ignore
/// Term::map_from_pairs(env, &pairs).unwrap_or_else(|_| {
///     let empty: &[(Term, Term)] = &[];
///     Term::map_from_pairs(env, empty).unwrap()   // <- double-panic path
/// })
/// ```
///
/// The inner `.unwrap()` is the dangerous fallback: if the primary call
/// fails and the fallback also fails, the NIF panics with no recovery.
/// After the fix the fallback must not contain a bare `.unwrap()`.
#[test]
fn h4_codec_encode_metadata_fallback_has_no_unwrap() {
    let body = extract_fn_body(CODEC_RS, "encode_metadata");
    let bare = count_bare_unwrap(body);
    assert_eq!(
        bare, 0,
        "src/codec.rs `encode_metadata` still contains {} bare `.unwrap()` call(s) \
         in its fallback path (around line 164). If the primary `map_from_pairs` \
         fails and the fallback also fails, this panics. Replace with \
         `.expect(\"...\")` carrying a descriptive message, or propagate via `?`.",
        bare
    );
}

/// FAILING TODAY: `encode_effect` in src/wasm_runtime.rs constructs each
/// effect map via a series of `m.map_put(..., ...).unwrap()` calls. Every
/// one of those is a potential panic site on hostile inputs. After the
/// fix: `encode_effect` must contain zero bare `.unwrap()` calls.
#[test]
fn h4_wasm_runtime_encode_effect_has_no_bare_unwrap() {
    let body = extract_fn_body(WASM_RUNTIME_RS, "encode_effect");
    let map_put_count = body.matches(".map_put(").count();
    let bare = count_bare_unwrap(body);
    assert_eq!(
        bare, 0,
        "src/wasm_runtime.rs `encode_effect` still contains {} bare `.unwrap()` \
         call(s) (out of {} `.map_put(` sites). Every one is a potential NIF \
         panic on degenerate BEAM state. Use `.expect(\"encode_effect: ...\")` \
         with a descriptive message, or propagate errors via `NifResult`.",
        bare, map_put_count
    );
}
