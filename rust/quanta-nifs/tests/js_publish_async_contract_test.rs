/// H3 regression test: `js_publish_async` in `src/nats/publish.rs` has an
/// asymmetric dual-return contract that must be documented.
///
/// Finding: `js_publish_async` may return the result in one of two ways:
///
///   (a) Synchronous return value — when the in-flight semaphore is
///       exhausted, the NIF returns `{:error, :nats_backpressure}` directly
///       from the call (see the `try_acquire_owned()` branch at the top of
///       the function).
///
///   (b) Asynchronous BEAM mailbox reply — on the happy path the NIF spawns
///       a tokio task that later sends `{:ok, ref, %{stream, seq}}` or an
///       error tuple to `caller_pid`.
///
/// A caller that treats the return value as "always async" will silently
/// drop backpressure errors; a caller that treats it as "always sync" will
/// deadlock waiting for a reply that arrives via mailbox. The contract must
/// therefore be documented on the NIF so every caller can implement both
/// paths correctly.
///
/// Test shape:
///   Static check via `include_str!` of `src/nats/publish.rs`. Locate the
///   `///` doc comment block immediately above `fn js_publish_async` and
///   assert it mentions BOTH the synchronous backpressure error path AND
///   the asynchronous mailbox reply path.
///
///   We also assert that an Elixir caller example is present so the docs
///   are actionable, not just a wall of prose.
///
/// Test outcome:
///   TODAY  — FAILS. `fn js_publish_async` has no `///` doc comment at all.
///   FIXED  — PASSES once the doc comment block is added with the required
///            keywords.
const PUBLISH_RS: &str = include_str!("../src/nats/publish.rs");

/// Extract the doc comment block that immediately precedes `fn js_publish_async`.
///
/// Walks the file backwards from the function signature and collects
/// consecutive `///` lines (ignoring `#[...]` attributes and blank lines
/// between the doc block and the fn). Returns the collected doc text in
/// original top-to-bottom order, lowercased for case-insensitive matching.
fn doc_comment_above_js_publish_async(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let fn_idx = lines
        .iter()
        .position(|line| line.trim_start().starts_with("fn js_publish_async"))
        .expect("fn js_publish_async should exist in src/nats/publish.rs");

    let mut doc_lines: Vec<&str> = Vec::new();
    let mut idx = fn_idx;
    while idx > 0 {
        idx -= 1;
        let trimmed = lines[idx].trim_start();
        if trimmed.starts_with("///") {
            doc_lines.push(lines[idx]);
            continue;
        }
        // Skip attribute macros (e.g. `#[rustler::nif]`) and blank spacer lines —
        // they don't break a doc block's association with the fn.
        if trimmed.starts_with("#[") || trimmed.is_empty() {
            continue;
        }
        break;
    }

    doc_lines.reverse();
    doc_lines.join("\n").to_lowercase()
}

/// FAILING TODAY: `fn js_publish_async` has no doc comment.
///
/// After the fix, the doc comment must explicitly describe BOTH return paths.
#[test]
fn h3_js_publish_async_doc_mentions_both_return_paths() {
    let docs = doc_comment_above_js_publish_async(PUBLISH_RS);

    assert!(
        !docs.is_empty(),
        "fn js_publish_async has no /// doc comment at all. The dual-return \
         contract (sync backpressure error vs. async mailbox reply) is \
         completely undocumented, leaving every Elixir caller to reverse-engineer \
         it from the NIF source. Add a /// block above the function."
    );

    // Synchronous error path keywords.
    let mentions_backpressure = docs.contains("backpressure") || docs.contains("nats_backpressure");
    let mentions_sync_path = docs.contains("synchronous")
        || docs.contains("immediately")
        || docs.contains("directly")
        || docs.contains("sync return");

    // Asynchronous reply path keywords.
    let mentions_async_path = docs.contains("async")
        || docs.contains("mailbox")
        || docs.contains("caller_pid")
        || docs.contains("message");

    assert!(
        mentions_backpressure,
        "fn js_publish_async's doc comment does not mention `backpressure` or \
         `nats_backpressure`. The synchronous `{{:error, :nats_backpressure}}` \
         return path must be documented. Current docs:\n---\n{docs}\n---"
    );

    assert!(
        mentions_sync_path,
        "fn js_publish_async's doc comment does not describe the SYNCHRONOUS \
         return path (expected one of: `synchronous`, `immediately`, `directly`, \
         `sync return`). Callers must know the backpressure error is returned \
         inline rather than via the BEAM mailbox. Current docs:\n---\n{docs}\n---"
    );

    assert!(
        mentions_async_path,
        "fn js_publish_async's doc comment does not describe the ASYNCHRONOUS \
         reply path (expected one of: `async`, `mailbox`, `caller_pid`, \
         `message`). Callers must know the success / publish-error tuples are \
         delivered via `send_and_clear` to the caller's mailbox. Current docs:\n---\n{docs}\n---"
    );
}

/// FAILING TODAY: no caller example in the doc comment.
///
/// The docs must include a short Elixir snippet showing a caller that
/// handles both return paths so operators don't need to invent one.
#[test]
fn h3_js_publish_async_doc_includes_caller_example() {
    let docs = doc_comment_above_js_publish_async(PUBLISH_RS);

    // Look for a code fence (```elixir / ```text / ```) that demonstrates the
    // dual-path handling, or the `receive do` pattern that handles the mailbox
    // reply.
    let has_code_fence = docs.contains("```");
    let has_receive_block = docs.contains("receive do") || docs.contains("receive");
    let has_case_on_return = docs.contains("case") || docs.contains("match");

    assert!(
        has_code_fence,
        "fn js_publish_async's doc comment does not include a fenced code block \
         (```). Add an Elixir example showing a caller that handles both the \
         sync backpressure error and the async mailbox reply. Current docs:\n---\n{docs}\n---"
    );

    assert!(
        has_receive_block,
        "fn js_publish_async's doc comment does not show a `receive` block. \
         The Elixir example must demonstrate how to await the async mailbox \
         reply. Current docs:\n---\n{docs}\n---"
    );

    assert!(
        has_case_on_return,
        "fn js_publish_async's doc comment does not show pattern-matching on \
         the NIF's synchronous return value (expected `case` or `match`). \
         The example must demonstrate branching on the backpressure error vs. \
         the async path. Current docs:\n---\n{docs}\n---"
    );
}
