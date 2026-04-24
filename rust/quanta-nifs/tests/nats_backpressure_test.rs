/// H5 regression test: `nats_connect` accepts `max_in_flight = 0`, creating a
/// `Semaphore::new(0)` that permanently blocks every publish/consume operation.
///
/// Finding: rust/quanta-nifs/src/nats/mod.rs line 81 reads
/// `term_map_get::<usize>(&opts, "max_in_flight")` and passes the result
/// directly to `Semaphore::new(max_in_flight)` at line 105.  There is no guard
/// that rejects or clamps a caller-supplied value of 0.
///
/// A `Semaphore::new(0)` has zero available permits.  Every `semaphore.acquire()`
/// call (used by `nats_backpressure` in publish/consume paths) will block
/// indefinitely — a deadlock that silently stalls all NATS operations on the
/// connection without any error being surfaced to the caller.
///
/// Runtime test shape:
///   The Semaphore construction itself is pure Rust (tokio::sync::Semaphore)
///   and does NOT require a live NATS server or a BEAM environment.  We can
///   therefore test the runtime deadlock behaviour directly:
///
///     let sem = Arc::new(Semaphore::new(0));
///     sem.try_acquire() → Err(TryAcquireError::NoPermits)   // blocked immediately
///
///   This directly proves the impact without any external dependency.
///
/// Static test shape (belt-and-suspenders):
///   Assert that src/nats/mod.rs contains a validation guard that rejects
///   `max_in_flight == 0`.  Today no such guard exists, so the static assertion
///   FAILS.
///
/// Test outcome:
///   TODAY  — h5_nats_mod_contains_max_in_flight_zero_guard FAILS;
///             runtime probes pass (they document behaviour, not gate it)
///   FIXED  — all tests pass: guard present in source, Semaphore::new(0) rejected
use std::sync::Arc;
use tokio::sync::{Semaphore, TryAcquireError};

const NATS_MOD_RS: &str = include_str!("../src/nats/mod.rs");

/// FAILING TODAY: nats/mod.rs does not contain any guard that rejects
/// max_in_flight == 0 before passing it to Semaphore::new().
///
/// After the fix a validation block such as:
///   if max_in_flight == 0 { return (atoms::error(), "max_in_flight must be >= 1").encode(env); }
/// or equivalent must be present.  We check for the literal `max_in_flight == 0`
/// or `max_in_flight < 1` or `max_in_flight >= 1` or `max_in_flight > 0` guard patterns.
#[test]
fn h5_nats_mod_contains_max_in_flight_zero_guard() {
    let has_guard = NATS_MOD_RS.contains("max_in_flight == 0")
        || NATS_MOD_RS.contains("max_in_flight < 1")
        || NATS_MOD_RS.contains("max_in_flight >= 1")
        || NATS_MOD_RS.contains("max_in_flight > 0");

    assert!(
        has_guard,
        "src/nats/mod.rs does not contain a guard that rejects max_in_flight == 0. \
         Passing 0 to Semaphore::new(0) creates a semaphore with no permits, \
         permanently blocking every acquire() call in the publish/consume paths. \
         Add a validation check before `Arc::new(Semaphore::new(max_in_flight))` \
         that returns an error term when max_in_flight is 0."
    );
}

/// Runtime probe: directly demonstrates that Semaphore::new(0) deadlocks
/// all non-blocking acquire attempts.
///
/// This test ALWAYS PASSES (try_acquire never parks the thread), but the
/// assertions confirm the exact failure mode that callers of the NIF would hit.
#[test]
fn h5_semaphore_with_zero_permits_blocks_all_acquires() {
    // This is exactly what nats_connect produces when max_in_flight = 0.
    let sem = Arc::new(Semaphore::new(0));

    // try_acquire() is the non-blocking form; it returns Err immediately rather
    // than parking the thread.  The result proves the semaphore is unusable.
    let result = sem.try_acquire();

    assert!(
        matches!(result, Err(TryAcquireError::NoPermits)),
        "Expected Semaphore::new(0).try_acquire() to return \
         Err(TryAcquireError::NoPermits), but got a successful permit. \
         A blocking acquire() on this semaphore would deadlock the caller.",
    );

    // Confirm available_permits() is 0 so the deadlock is fully explicit.
    assert_eq!(
        sem.available_permits(),
        0,
        "Semaphore::new(0) should report 0 available permits"
    );
}

/// Runtime probe: confirm that a semaphore constructed with the DEFAULT value
/// (10_000 permits, as used by DEFAULT_MAX_IN_FLIGHT in nats/mod.rs) does NOT
/// block — so the fix only needs to guard against 0, not change the defaults.
#[test]
fn h5_semaphore_with_default_permits_is_usable() {
    const DEFAULT_MAX_IN_FLIGHT: usize = 10_000;
    let sem = Arc::new(Semaphore::new(DEFAULT_MAX_IN_FLIGHT));

    let result = sem.try_acquire();
    assert!(
        result.is_ok(),
        "Semaphore::new(DEFAULT_MAX_IN_FLIGHT) should immediately grant a permit, \
         but try_acquire() returned an error."
    );
}
