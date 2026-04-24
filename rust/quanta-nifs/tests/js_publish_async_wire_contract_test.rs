//! H-1 regression test: the docstring on `js_publish_async` must describe the
//! actual wire shape produced by the NIF, not a made-up one.
//!
//! Finding: the docstring added in commit 8516591 claims the asynchronous
//! publish-failure mailbox message is `{:error, ref, {:publish_failed, reason}}`,
//! but `NifError::Other(msg)` is encoded in `src/nats/mod.rs` as
//! `msg.encode(env)` — i.e. the third element of the tuple is a bare string,
//! not a `{:publish_failed, reason}` tuple. Any Elixir caller that pattern-
//! matches on `{:error, ref, {:publish_failed, reason}}` will silently drop
//! every non-`:wrong_last_sequence` JetStream error.
//!
//! Test shape:
//!   Static assertion via `include_str!` of both `src/nats/publish.rs`
//!   (docstring source) and `src/nats/mod.rs` (NifError::encode_term source).
//!   If the docstring promises a `{:publish_failed, ...}` tuple, the NifError
//!   encoder must actually produce such a tuple. Otherwise the docstring is
//!   lying and the test fails.
//!
//! Test outcome:
//!   TODAY  — FAILS. Docstring references `{:publish_failed, reason}` but
//!            NifError::Other encodes as a bare string.
//!   FIXED  — PASSES once the docstring is corrected to match the actual
//!            bare-string wire shape (or the encoder is changed to emit the
//!            documented tuple).

const PUBLISH_RS: &str = include_str!("../src/nats/publish.rs");
const NATS_MOD_RS: &str = include_str!("../src/nats/mod.rs");

/// Returns true if the docstring above `fn js_publish_async` promises that the
/// async mailbox reply uses a `{:publish_failed, reason}` tuple for the
/// general publish-failure path.
fn docstring_promises_publish_failed_tuple(src: &str) -> bool {
    src.contains(":publish_failed")
}

/// Returns true if `NifError::Other`'s encoding path actually produces a
/// tuple shape prefixed with a `:publish_failed` atom. We look for either an
/// explicit atom registration in the `atoms!` block or a literal
/// `publish_failed` identifier anywhere in the encoder source.
fn encoder_emits_publish_failed_tuple(nats_mod_src: &str) -> bool {
    let atom_registered = nats_mod_src
        .lines()
        .any(|line| line.trim().trim_end_matches(',').trim() == "publish_failed");
    let literal_in_tuple = nats_mod_src.contains("publish_failed");
    atom_registered || literal_in_tuple
}

/// FAILING TODAY: the docstring advertises a `{:publish_failed, reason}`
/// tuple but the encoder emits a bare string. Either the docstring or the
/// encoder must change so the two agree.
#[test]
fn h1_publish_async_docstring_matches_actual_wire_shape() {
    let docstring_claims_tuple = docstring_promises_publish_failed_tuple(PUBLISH_RS);
    let encoder_emits_tuple = encoder_emits_publish_failed_tuple(NATS_MOD_RS);

    assert_eq!(
        docstring_claims_tuple, encoder_emits_tuple,
        "docstring/encoder mismatch: docstring references `:publish_failed` \
         tuple = {docstring_claims_tuple}, but NifError encoder actually emits \
         such a tuple = {encoder_emits_tuple}. Either rewrite the docstring to \
         describe the bare-string reason that ships today, or change the \
         encoder to produce the documented `{{:publish_failed, reason}}` tuple."
    );
}
