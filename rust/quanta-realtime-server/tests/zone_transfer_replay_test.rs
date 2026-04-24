//! H-4 regression test: zone-transfer replay dedup lost on server restart.
//!
//! ## Finding
//! `ZoneTransferManager` holds its dedup set (`dedup_set`) in memory only.
//! When the server restarts, a new `ZoneTransferManager` is constructed with
//! an empty `dedup_set`.  A token that was accepted and deduplicated before
//! the restart can therefore be replayed against the new instance within its
//! TTL window — bypassing the dedup protection entirely.
//!
//! The module-level doc comment in `zone_transfer.rs` explicitly acknowledges
//! this limitation:
//!   > "Replay on restart: The dedup set is in-memory. On server restart,
//!     tokens accepted before the restart can be replayed within their TTL."
//!
//! ## Test shape
//! 1. Create `manager_a` (the "old server").
//! 2. Sign a token and accept it through `manager_a` — dedup entry recorded.
//! 3. Verify a second accept on `manager_a` is rejected (`DuplicateToken`).
//! 4. Create `manager_b` with the **same config** (same HMAC secret, same TTL)
//!    — models a server restart.
//! 5. Replay the same token against `manager_b`.
//!    Expected (correct behaviour): `DuplicateToken`.
//!    Actual   (current bug):       `Ok(TransferredPlayer)` — ACCEPTED.
//!
//! The test at step 5 asserts the expected-correct result, so it FAILS today.
//!
//! ## No production code changes required
//! The entire test is expressed through the existing public API:
//! `ZoneTransferManager`, `ZoneTransferConfig::for_testing()`, and
//! `accept_transfer_at` (which takes an explicit `now_ms` for determinism).

mod common;

use quanta_realtime_server::types::IslandId;
use quanta_realtime_server::zone_transfer::{
    TransferError, ZoneTransferConfig, ZoneTransferManager,
};

fn zone(s: &str) -> IslandId {
    IslandId::from(s)
}

/// Config shared by both the "old server" and the "restarted server".
/// Same HMAC secret means a token signed before restart passes HMAC
/// verification on the new instance — only the dedup_set would stop it.
fn shared_config() -> ZoneTransferConfig {
    ZoneTransferConfig {
        // Generous TTL so the token is still valid well after the simulated
        // restart (ts + 300 ms is far inside the 30-second window).
        token_ttl_ms: 30_000,
        // dedup_retention outlasts the TTL so a correct persistent-dedup fix
        // would keep the entry alive across the restart window.
        dedup_retention: std::time::Duration::from_secs(60),
        ..ZoneTransferConfig::for_testing()
    }
}

// ---------------------------------------------------------------------------
// Test 1 — FAILING TODAY (proves H-4)
//
// Replays an already-accepted token against a freshly-constructed manager
// that shares the same HMAC secret.  The replay succeeds because the new
// instance has an empty dedup_set.
// ---------------------------------------------------------------------------
#[tokio::test(start_paused = true)]
async fn restart_clears_dedup_allowing_token_replay() {
    // --- Step 1: "Old server" ---
    let mut manager_a = ZoneTransferManager::new(shared_config());

    // --- Step 2: sign and accept a token ---
    let token = manager_a
        .prepare_transfer(
            "player-42".into(),
            zone("zone-alpha"),
            zone("zone-beta"),
            [10.0, 0.0, 5.0],
            [1.0, 0.0, -1.0],
            vec![],
        )
        .unwrap();

    let ts = token.timestamp;

    let first_accept = manager_a.accept_transfer_at(&token, &zone("zone-beta"), ts + 100);
    assert!(
        first_accept.is_ok(),
        "first accept on manager_a must succeed, got: {:?}",
        first_accept
    );

    // --- Step 3: second accept on old server is rejected by dedup ---
    assert_eq!(
        manager_a.accept_transfer_at(&token, &zone("zone-beta"), ts + 200),
        Err(TransferError::DuplicateToken {
            player_id: "player-42".into()
        }),
        "second accept on same manager must be rejected by dedup"
    );

    assert_eq!(
        manager_a.dedup_count(),
        1,
        "manager_a dedup_set must hold 1 entry"
    );

    // --- Step 4: "Server restart" — new manager, same config ---
    // All in-memory state (including dedup_set) is lost.
    let mut manager_b = ZoneTransferManager::new(shared_config());

    assert_eq!(
        manager_b.dedup_count(),
        0,
        "fresh manager_b must start with empty dedup_set"
    );

    // --- Step 5: replay the token against the restarted server ---
    // HMAC is valid (same secret), token is not expired (ts+300 < ts+30_000),
    // and manager_b has no dedup entry — so the replay goes through.
    let replay_result = manager_b.accept_transfer_at(&token, &zone("zone-beta"), ts + 300);

    // FAILING ASSERTION — proves H-4.
    //
    // A correct implementation (persistent dedup — e.g. Redis, or a
    // timestamp-bound nonce checked against a monotonic wall-clock floor)
    // would reject this replay.  Today it returns Ok because manager_b has
    // a fresh empty dedup_set.
    assert_eq!(
        replay_result,
        Err(TransferError::DuplicateToken {
            player_id: "player-42".into()
        }),
        "BUG H-4: replayed token was ACCEPTED by the restarted manager \
         because the in-memory dedup_set was cleared on restart. \
         A persistent dedup store is required to reject replays across \
         server restarts within the token TTL."
    );
}

// ---------------------------------------------------------------------------
// Test 2 — PASSING TODAY (control: same-instance dedup works)
//
// Confirms dedup functions correctly within a single manager lifetime.
// If this test fails, the dedup logic itself is broken — independent of H-4.
// ---------------------------------------------------------------------------
#[tokio::test(start_paused = true)]
async fn same_instance_dedup_rejects_replay() {
    let mut mgr = ZoneTransferManager::new(shared_config());

    let token = mgr
        .prepare_transfer(
            "player-99".into(),
            zone("zone-x"),
            zone("zone-y"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();

    let ts = token.timestamp;

    assert!(
        mgr.accept_transfer_at(&token, &zone("zone-y"), ts + 50)
            .is_ok(),
        "first accept must succeed"
    );

    assert_eq!(
        mgr.accept_transfer_at(&token, &zone("zone-y"), ts + 100),
        Err(TransferError::DuplicateToken {
            player_id: "player-99".into()
        }),
        "same-instance replay must be rejected by dedup"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — PASSING TODAY (control: expired token rejected regardless)
//
// A token replayed after its TTL has elapsed is rejected by the expiry check
// even on a fresh instance — TTL and dedup are independent rejection paths.
// This confirms the TTL check is not accidentally satisfying the H-4 scenario.
// ---------------------------------------------------------------------------
#[tokio::test(start_paused = true)]
async fn expired_token_rejected_after_restart_regardless_of_dedup() {
    let config = ZoneTransferConfig {
        token_ttl_ms: 1_000, // 1-second TTL
        ..ZoneTransferConfig::for_testing()
    };

    let mut manager_a = ZoneTransferManager::new(config.clone());

    let token = manager_a
        .prepare_transfer(
            "player-exp".into(),
            zone("zone-p"),
            zone("zone-q"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();

    let ts = token.timestamp;

    // Accept while still valid.
    assert!(
        manager_a
            .accept_transfer_at(&token, &zone("zone-q"), ts + 500)
            .is_ok(),
        "accept within TTL must succeed"
    );

    // Restart.
    let mut manager_b = ZoneTransferManager::new(config);

    // Replay after TTL elapsed — rejected by expiry, not dedup.
    assert_eq!(
        manager_b.accept_transfer_at(&token, &zone("zone-q"), ts + 2_000),
        Err(TransferError::TokenExpired),
        "replay after TTL must be rejected by expiry check, not dedup"
    );
}
