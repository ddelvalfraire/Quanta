//! Failing tests that prove three security findings from the code review.
//!
//! Finding C-3 + C-2 (a): No startup guard prevents the hardcoded dev token
//!   from being used when the server binds on a non-loopback address.
//!
//! Finding C-3 + C-2 (b): The WebTransport accept path in connection.rs has no
//!   origin check — any `Origin` header is silently accepted.
//!
//! Finding C-1: DevTokenValidator hands out sequential session IDs starting at
//!   1, making session IDs trivially predictable.
//!
//! All tests are expected to FAIL against the current (unfixed) implementation.
//! Do not fix the underlying code.

// ── Finding C-3 + C-2 (a): Startup guard ─────────────────────────────────────
//
// Today: `run_server` with the hardcoded DEFAULT_DEV_TOKEN and a non-loopback
// bind address starts successfully — no guard exists.
//
// Expected after fix: the server should refuse to start (return an Err) when the
// dev token is the well-known default string AND the bind address is not loopback.
//
// The test below calls `run_server` with:
//   - validator built from the hardcoded DEFAULT_DEV_TOKEN (same as main.rs:38)
//   - quic_addr bound to 0.0.0.0:0  (non-loopback, same as main.rs:25 default)
//
// Today the assertion `result.is_err()` FAILS because startup succeeds.
// After the fix it should pass.

use std::sync::Arc;
use tokio::sync::watch;

use quanta_realtime_server::auth::DevTokenValidator;
use quanta_realtime_server::config::{EndpointConfig, ServerConfig};
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::{run_server, AuthValidator, RunServerArgs};

/// The same constant that ships in `main.rs`.
const DEFAULT_DEV_TOKEN: &str = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

#[tokio::test]
async fn startup_guard_rejects_dev_token_on_non_loopback_addr() {
    // Arrange: use the hardcoded dev token on a wildcard (non-loopback) address.
    // This is precisely the configuration main.rs uses when QUANTA_DEV_TOKEN is
    // not set and QUANTA_QUIC_ADDR defaults to 0.0.0.0:4443.
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let validator = DevTokenValidator::new(DEFAULT_DEV_TOKEN) as Arc<_>;

    let result = run_server(RunServerArgs {
        server_config: ServerConfig::default(),
        endpoint_config: EndpointConfig::default(),
        // Non-loopback wildcard address — same as the default in main.rs
        quic_addr: "0.0.0.0:0".parse().unwrap(),
        ws_addr: None,
        tls: TlsConfig::SelfSigned,
        validator,
        shutdown_rx,
        server_id: "srv-test-security".into(),
        executor_factory: None,
        fanout_factory: None,
        default_island_id: None,
        metrics_addr: None,
    })
    .await;

    // EXPECTED (after fix): Err(...) with a message like "dev token must not be
    //   used on a non-loopback address".
    // ACTUAL TODAY: Ok(...) — no such guard exists.
    // This assertion FAILS today, proving the finding.
    assert!(
        result.is_err(),
        "BUG (C-3/C-2): run_server must refuse to start when the hardcoded dev \
         token is used on a non-loopback bind address (got Ok — no guard exists)"
    );
}

// ── Finding C-3 + C-2 (b): No origin validation on WebTransport accept ────────
//
// connection.rs handle_h3_webtransport() calls:
//
//   let request = web_transport_quinn::Request::accept(connection.clone()).await?;
//   let session = request.ok().await?;    // <- no origin check here
//
// Any `Origin` header, including "https://evil.test", is silently accepted.
// A hardened implementation would call `request.bad()` or return an error when
// the origin does not match an allowlist.
//
// Because exercising the full WebTransport (HTTP/3) handshake in a unit test
// requires standing up a real QUIC endpoint with h3 ALPN negotiation, which
// would make the test slow and brittle, we instead write a narrow structural
// proof: we assert that `connection.rs` does NOT contain any origin validation
// code. This documents the absence of the guard so that, after the fix, the
// test must be updated to exercise the real check.
//
// When the fix is implemented, this test should be REPLACED with a runtime test
// that verifies cross-origin requests are rejected.

#[test]
fn webtransport_accept_has_no_origin_check() {
    // Read the production source of connection.rs at compile time.
    // We search for identifiers that would indicate an origin validation guard.
    let connection_src = include_str!("../src/connection.rs");

    // These are the code patterns one would write to enforce origin checking
    // with `web_transport_quinn`. Their absence is the bug.
    let origin_guard_patterns = [
        "origin",
        "bad()", // web_transport_quinn::Request::bad() — rejects the session
        "allowlist",
        "allowed_origins",
    ];

    let has_any_origin_guard = origin_guard_patterns
        .iter()
        .any(|pat| connection_src.contains(pat));

    // EXPECTED (after fix): at least one origin-related guard is present.
    // ACTUAL TODAY: none of the patterns exist — test FAILS, proving the finding.
    assert!(
        has_any_origin_guard,
        "BUG (C-3/C-2): connection.rs handle_h3_webtransport() contains no origin \
         validation. The WebTransport session is accepted for ANY Origin header, \
         including cross-origin requests from untrusted domains. \
         Patterns searched: {origin_guard_patterns:?}"
    );
}

// ── Finding C-1: Predictable sequential session IDs ───────────────────────────
//
// DevTokenValidator uses `AtomicU64::fetch_add(1, Relaxed)` starting at 1.
// Two consecutive authentications produce session IDs 1 and 2: trivially
// enumerable. An attacker who observes their own session ID can predict all
// other active session IDs.
//
// Expected after fix: session IDs should be generated from a CSPRNG (e.g.
// `rand::thread_rng().gen::<u64>()`) so the difference between consecutive
// IDs is not 1 and the IDs are not sequential integers.
//
// Today: id_b - id_a == 1, so the assertion `id_b - id_a != 1` FAILS.

#[test]
fn session_ids_are_not_sequential_integers() {
    // Arrange: two auth requests using the same DevTokenValidator instance,
    // which is what the server does for every connection on a single process.
    let validator = DevTokenValidator::new("qk_rw_dev_xxx");

    let make_req = || quanta_realtime_server::auth::AuthRequest {
        token: "qk_rw_dev_xxx".into(),
        client_version: "0.1.0".into(),
        session_token: None,
        transfer_token: None,
    };

    // Act: authenticate two clients back-to-back (no concurrent access needed
    // to reproduce — sequential use is sufficient to show the pattern).
    let resp_a = validator
        .validate(&make_req())
        .expect("first auth should succeed");
    let resp_b = validator
        .validate(&make_req())
        .expect("second auth should succeed");

    assert!(resp_a.accepted, "first client must be accepted");
    assert!(resp_b.accepted, "second client must be accepted");

    let id_a = resp_a.session_id;
    let id_b = resp_b.session_id;

    // Sanity: both IDs must be non-zero.
    assert!(id_a > 0, "session_id must be non-zero");
    assert!(id_b > 0, "session_id must be non-zero");

    // The finding: the difference between consecutive IDs is exactly 1.
    //
    // EXPECTED (after fix): difference != 1 (IDs drawn from CSPRNG).
    // ACTUAL TODAY: id_b - id_a == 1 — test FAILS here, proving the finding.
    assert_ne!(
        id_b.wrapping_sub(id_a),
        1,
        "BUG (C-1): session IDs are sequential (id_a={id_a}, id_b={id_b}, \
         diff={}). An observer who knows their own session ID can enumerate all \
         other active sessions. IDs must be drawn from a CSPRNG.",
        id_b.wrapping_sub(id_a)
    );

    // Secondary assertion: IDs should not be small integers (1, 2, 3, ...).
    // A CSPRNG-based ID has an astronomically low probability of landing below 2^32.
    assert!(
        id_a > u32::MAX as u64,
        "BUG (C-1): session_id {id_a} looks like a sequential counter, not a \
         random 64-bit value. Use a CSPRNG."
    );
}
