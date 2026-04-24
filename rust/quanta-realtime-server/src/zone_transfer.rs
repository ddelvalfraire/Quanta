//! Zone Transition Protocol
//!
//! Two-phase protocol for player transfer between simulation islands.
//! Phase 1 (Prepare): old zone creates a signed token, marks player as transferring.
//! Phase 2 (Execute): new zone validates token, creates player entity, acknowledges.
//!
//! ## Limitations
//!
//! - **Replay on restart:** The dedup set is in-memory. On server restart, tokens
//!   accepted before the restart can be replayed within their TTL. Persistent dedup
//!   (e.g. Redis) should be added when the NATS orchestration layer is built.
//!
//! - **Canonicality:** HMAC validation re-encodes the decoded token fields via bitcode.
//!   This assumes `bitcode::encode(bitcode::decode(bytes)) == bytes`. A future
//!   improvement can sign/verify over raw wire bytes to eliminate this assumption.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::types::IslandId;

type HmacSha256 = Hmac<Sha256>;

const MAX_PLAYER_ID_LEN: usize = 256;
const MAX_TOKEN_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct BuffState {
    pub buff_id: String,
    pub remaining_ms: u32,
    pub data: Vec<u8>,
}

/// Fields included in the HMAC signature (all token fields except the signature).
#[derive(bitcode::Encode)]
struct TokenPayload {
    player_id: String,
    source_zone: IslandId,
    target_zone: IslandId,
    position: [f32; 3],
    velocity: [f32; 3],
    buffs: Vec<BuffState>,
    timestamp: u64,
    ttl_ms: u32,
}

/// Signed zone transfer token (~200 bytes typical).
///
/// Created by the old zone during Phase 1, validated by the new zone in Phase 2.
/// The HMAC-SHA256 covers all fields except `hmac` itself.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct ZoneTransferToken {
    pub player_id: String,
    pub source_zone: IslandId,
    pub target_zone: IslandId,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub buffs: Vec<BuffState>,
    pub timestamp: u64,
    pub ttl_ms: u32,
    pub hmac: [u8; 32],
}

impl ZoneTransferToken {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms > self.timestamp + self.ttl_ms as u64
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bitcode::encode(self)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, TransferError> {
        if data.len() > MAX_TOKEN_BYTES {
            return Err(TransferError::InvalidToken);
        }
        bitcode::decode(data).map_err(|_| TransferError::InvalidToken)
    }
}

pub struct TokenSigner {
    key: Vec<u8>,
    default_ttl_ms: u32,
}

impl TokenSigner {
    pub fn new(secret: &[u8], default_ttl_ms: u32) -> Self {
        Self {
            key: secret.to_vec(),
            default_ttl_ms,
        }
    }

    pub fn sign(
        &self,
        player_id: String,
        source_zone: IslandId,
        target_zone: IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        buffs: Vec<BuffState>,
    ) -> ZoneTransferToken {
        self.sign_at(
            player_id,
            source_zone,
            target_zone,
            position,
            velocity,
            buffs,
            now_ms(),
        )
    }

    pub fn sign_at(
        &self,
        player_id: String,
        source_zone: IslandId,
        target_zone: IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        buffs: Vec<BuffState>,
        timestamp: u64,
    ) -> ZoneTransferToken {
        let payload = TokenPayload {
            player_id,
            source_zone,
            target_zone,
            position,
            velocity,
            buffs,
            timestamp,
            ttl_ms: self.default_ttl_ms,
        };
        let payload_bytes = bitcode::encode(&payload);
        let hmac = self.compute_hmac(&payload_bytes);

        let TokenPayload {
            player_id,
            source_zone,
            target_zone,
            position,
            velocity,
            buffs,
            timestamp,
            ttl_ms,
        } = payload;

        ZoneTransferToken {
            player_id,
            source_zone,
            target_zone,
            position,
            velocity,
            buffs,
            timestamp,
            ttl_ms,
            hmac,
        }
    }

    pub fn validate(
        &self,
        token: &ZoneTransferToken,
        expected_target: &IslandId,
    ) -> Result<(), TransferError> {
        self.validate_at(token, expected_target, now_ms())
    }

    /// Validate token. Verifies HMAC first, then checks authenticated fields.
    pub fn validate_at(
        &self,
        token: &ZoneTransferToken,
        expected_target: &IslandId,
        now_ms: u64,
    ) -> Result<(), TransferError> {
        let payload_bytes = bitcode::encode(&TokenPayload {
            player_id: token.player_id.clone(),
            source_zone: token.source_zone.clone(),
            target_zone: token.target_zone.clone(),
            position: token.position,
            velocity: token.velocity,
            buffs: token.buffs.clone(),
            timestamp: token.timestamp,
            ttl_ms: token.ttl_ms,
        });
        if !self.verify_hmac(&payload_bytes, &token.hmac) {
            return Err(TransferError::InvalidHmac);
        }

        if token.target_zone != *expected_target {
            return Err(TransferError::ZoneMismatch);
        }
        if token.is_expired(now_ms) {
            return Err(TransferError::TokenExpired);
        }

        Ok(())
    }

    fn compute_hmac(&self, data: &[u8]) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(data);
        let mut out = [0u8; 32];
        out.copy_from_slice(&mac.finalize().into_bytes());
        out
    }

    fn verify_hmac(&self, data: &[u8], expected: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(data);
        mac.verify_slice(expected).is_ok()
    }
}

pub struct InFlightTransfer {
    pub token: ZoneTransferToken,
    pub started_at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransferredPlayer {
    pub player_id: String,
    pub source_zone: IslandId,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub buffs: Vec<BuffState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferError {
    InvalidHmac,
    TokenExpired,
    /// Target zone does not match. Zone names omitted to avoid topology leakage.
    ZoneMismatch,
    DuplicateToken {
        player_id: String,
    },
    PlayerNotTransferring {
        player_id: String,
    },
    InvalidToken,
    InvalidPlayerId,
    AtCapacity,
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHmac => write!(f, "transfer token signature invalid"),
            Self::TokenExpired => write!(f, "transfer token expired"),
            Self::ZoneMismatch => write!(f, "transfer token target mismatch"),
            Self::DuplicateToken { .. } => write!(f, "transfer token already used"),
            Self::PlayerNotTransferring { .. } => write!(f, "player not in active transfer"),
            Self::InvalidToken => write!(f, "transfer token decode failed"),
            Self::InvalidPlayerId => write!(f, "player id exceeds maximum length"),
            Self::AtCapacity => write!(f, "transfer capacity exceeded"),
        }
    }
}

impl std::error::Error for TransferError {}

#[derive(Debug, Clone)]
pub struct ZoneTransferConfig {
    pub hmac_secret: Vec<u8>,
    pub token_ttl_ms: u32,
    pub ack_timeout: Duration,
    pub dedup_retention: Duration,
    pub max_in_flight: usize,
    pub max_dedup_entries: usize,
    /// Optional path to a binary file that persists accepted-token HMACs
    /// across manager restarts.  `None` keeps the behaviour purely
    /// in-memory (fine for fresh dev servers, unsafe in prod: see H-4).
    /// When `Some`, each `accept_transfer_at` refreshes its in-memory view
    /// from the file before checking dedup, and writes the new entry back
    /// atomically so a peer manager sharing the same path observes it.
    pub dedup_store_path: Option<PathBuf>,
}

#[cfg(any(test, feature = "test-utils"))]
impl ZoneTransferConfig {
    pub fn for_testing() -> Self {
        let secret = b"test-only-secret-do-not-use-prod".to_vec();
        Self {
            // Default to a deterministic tempdir path derived from the
            // hmac_secret so two test managers built from the same config
            // share dedup state — the canonical "restart" scenario.
            // Unique secrets (tests that tweak `hmac_secret`) route to
            // unique files, so parallel tests don't collide.
            dedup_store_path: Some(default_dedup_store_path(&secret)),
            hmac_secret: secret,
            token_ttl_ms: 10_000,
            ack_timeout: Duration::from_secs(5),
            dedup_retention: Duration::from_secs(30),
            max_in_flight: 1000,
            max_dedup_entries: 10_000,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
fn default_dedup_store_path(hmac_secret: &[u8]) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(hmac_secret);
    // Include the pid and current thread name in the hash so parallel
    // tests don't collide on the same file — `cargo test` shards tests
    // across threads, and each `#[tokio::test]` runs on its own thread
    // named after the test function. Two managers within the same test
    // (the H-4 "restart" scenario) stay on the same thread and therefore
    // resolve to the same file, which is exactly what we want.
    hasher.update(std::process::id().to_le_bytes());
    if let Some(name) = std::thread::current().name() {
        hasher.update(name.as_bytes());
    }
    let digest = hasher.finalize();
    // 16 hex chars is plenty to avoid collisions in a tempdir; using the
    // hash (not the secret itself) avoids leaking the raw bytes into a
    // user-readable filename.
    let name = format!(
        "quanta-zone-dedup-{:016x}.bin",
        u64::from_be_bytes(digest[..8].try_into().expect("sha256 yields 32 bytes"))
    );
    std::env::temp_dir().join(name)
}

/// Coordinates zone transfers on a single server.
///
/// Thread safety is the caller's responsibility — wrap in `Arc<Mutex<_>>`
/// (same pattern as `SessionStore`).
pub struct ZoneTransferManager {
    signer: TokenSigner,
    config: ZoneTransferConfig,
    in_flight: FxHashMap<String, InFlightTransfer>,
    /// Keyed by token HMAC (unique per token) so the same player can transfer
    /// again immediately after a completed transfer (e.g. A→B then B→C).
    dedup_set: FxHashMap<[u8; 32], Instant>,
}

impl ZoneTransferManager {
    pub fn new(config: ZoneTransferConfig) -> Self {
        let signer = TokenSigner::new(&config.hmac_secret, config.token_ttl_ms);
        Self {
            signer,
            config,
            in_flight: FxHashMap::default(),
            dedup_set: FxHashMap::default(),
        }
    }

    /// Check the on-disk dedup store for a specific token HMAC.
    /// Returns `true` iff the store contains the HMAC with an expiry in the
    /// future. The in-memory `dedup_set` is NOT populated by this check —
    /// `dedup_count()` continues to reflect only entries this manager
    /// instance has accepted, which keeps the "fresh manager starts empty"
    /// contract (H-4 regression test relies on it).
    fn persistent_dedup_contains(&self, hmac: &[u8; 32]) -> bool {
        let Some(path) = self.config.dedup_store_path.as_ref() else {
            return false;
        };
        let entries = match load_dedup_file(path) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "zone-transfer dedup reload failed — falling back to in-memory only"
                );
                return false;
            }
        };
        let now_unix = now_ms();
        entries
            .into_iter()
            .any(|(h, exp)| &h == hmac && exp > now_unix)
    }

    /// Append the given entry to the persistent store.
    /// Best-effort: a write failure is logged and does not fail the accept
    /// (the entry still lives in the in-memory `dedup_set`).
    fn persist_dedup_entry(&self, hmac: &[u8; 32], expiry_unix_ms: u64) {
        let Some(path) = self.config.dedup_store_path.as_ref() else {
            return;
        };
        if let Err(e) = append_dedup_file(path, hmac, expiry_unix_ms) {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "zone-transfer dedup persist failed — restart replay window is open"
            );
        }
    }

    /// Phase 1: Create a signed token and mark the player as transferring.
    ///
    /// The caller should stop processing inputs for this player, publish the
    /// token to NATS, and send a reliable message to the client.
    pub fn prepare_transfer(
        &mut self,
        player_id: String,
        source_zone: IslandId,
        target_zone: IslandId,
        position: [f32; 3],
        velocity: [f32; 3],
        buffs: Vec<BuffState>,
    ) -> Result<ZoneTransferToken, TransferError> {
        if player_id.len() > MAX_PLAYER_ID_LEN {
            return Err(TransferError::InvalidPlayerId);
        }
        if self.in_flight.len() >= self.config.max_in_flight {
            return Err(TransferError::AtCapacity);
        }

        let token = self.signer.sign(
            player_id.clone(),
            source_zone,
            target_zone,
            position,
            velocity,
            buffs,
        );

        self.in_flight.insert(
            player_id,
            InFlightTransfer {
                token: token.clone(),
                started_at: Instant::now(),
            },
        );

        Ok(token)
    }

    /// Phase 2: Accept a transfer token on the receiving zone.
    ///
    /// Validates the token (HMAC → zone → TTL), checks dedup, and returns
    /// the transferred player state for entity creation.
    pub fn accept_transfer(
        &mut self,
        token: &ZoneTransferToken,
        this_zone: &IslandId,
    ) -> Result<TransferredPlayer, TransferError> {
        self.accept_transfer_at(token, this_zone, now_ms())
    }

    pub fn accept_transfer_at(
        &mut self,
        token: &ZoneTransferToken,
        this_zone: &IslandId,
        now: u64,
    ) -> Result<TransferredPlayer, TransferError> {
        if token.player_id.len() > MAX_PLAYER_ID_LEN {
            return Err(TransferError::InvalidPlayerId);
        }

        self.signer.validate_at(token, this_zone, now)?;

        if let Some(expiry) = self.dedup_set.get(&token.hmac) {
            if Instant::now() < *expiry {
                return Err(TransferError::DuplicateToken {
                    player_id: token.player_id.clone(),
                });
            }
            self.dedup_set.remove(&token.hmac);
        }

        // Consult the on-disk store BEFORE recording a new entry so a
        // sibling manager (or a prior incarnation of this one — the H-4
        // restart scenario) that already accepted this token is observed.
        if self.persistent_dedup_contains(&token.hmac) {
            return Err(TransferError::DuplicateToken {
                player_id: token.player_id.clone(),
            });
        }

        if self.dedup_set.len() >= self.config.max_dedup_entries {
            self.purge_expired_dedup();
            if self.dedup_set.len() >= self.config.max_dedup_entries {
                return Err(TransferError::AtCapacity);
            }
        }

        let expiry_unix_ms =
            now_ms().saturating_add(self.config.dedup_retention.as_millis() as u64);
        self.dedup_set
            .insert(token.hmac, Instant::now() + self.config.dedup_retention);
        self.persist_dedup_entry(&token.hmac, expiry_unix_ms);

        Ok(TransferredPlayer {
            player_id: token.player_id.clone(),
            source_zone: token.source_zone.clone(),
            position: token.position,
            velocity: token.velocity,
            buffs: token.buffs.clone(),
        })
    }

    pub fn acknowledge_transfer(&mut self, player_id: &str) -> Result<(), TransferError> {
        self.remove_in_flight(player_id)
    }

    pub fn rollback_transfer(&mut self, player_id: &str) -> Result<(), TransferError> {
        self.remove_in_flight(player_id)
    }

    fn remove_in_flight(&mut self, player_id: &str) -> Result<(), TransferError> {
        self.in_flight
            .remove(player_id)
            .ok_or_else(|| TransferError::PlayerNotTransferring {
                player_id: player_id.to_owned(),
            })?;
        Ok(())
    }

    pub fn check_timeouts(&mut self) -> Vec<String> {
        let now = Instant::now();
        let timeout = self.config.ack_timeout;
        let mut rolled_back = Vec::new();

        self.in_flight.retain(|player_id, transfer| {
            if now.duration_since(transfer.started_at) >= timeout {
                rolled_back.push(player_id.clone());
                false
            } else {
                true
            }
        });

        rolled_back
    }

    pub fn is_transferring(&self, player_id: &str) -> bool {
        self.in_flight.contains_key(player_id)
    }

    pub fn purge_expired_dedup(&mut self) {
        let now = Instant::now();
        self.dedup_set.retain(|_, expiry| now < *expiry);
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    pub fn dedup_count(&self) -> usize {
        self.dedup_set.len()
    }

    pub fn signer(&self) -> &TokenSigner {
        &self.signer
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Persistent dedup store (H-4)
//
// File format: repeated fixed-size 40-byte records:
//   bytes[0..32]  -- token HMAC (SHA-256 output)
//   bytes[32..40] -- expiry timestamp as Unix milliseconds, little-endian u64
//
// The format is intentionally trivial: append-only writes, whole-file reads.
// Records older than `now_ms()` are filtered on load, so the file converges
// back to its steady-state size naturally.
// ---------------------------------------------------------------------------

const DEDUP_RECORD_BYTES: usize = 40;

fn load_dedup_file(path: &Path) -> std::io::Result<Vec<([u8; 32], u64)>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(parse_dedup_records(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

fn parse_dedup_records(bytes: &[u8]) -> Vec<([u8; 32], u64)> {
    let n = bytes.len() / DEDUP_RECORD_BYTES;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let start = i * DEDUP_RECORD_BYTES;
        let mut hmac = [0u8; 32];
        hmac.copy_from_slice(&bytes[start..start + 32]);
        let mut ms_bytes = [0u8; 8];
        ms_bytes.copy_from_slice(&bytes[start + 32..start + 40]);
        out.push((hmac, u64::from_le_bytes(ms_bytes)));
    }
    out
}

fn append_dedup_file(path: &Path, hmac: &[u8; 32], expiry_unix_ms: u64) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut buf = [0u8; DEDUP_RECORD_BYTES];
    buf[..32].copy_from_slice(hmac);
    buf[32..].copy_from_slice(&expiry_unix_ms.to_le_bytes());
    f.write_all(&buf)?;
    // Flush so a peer manager's immediate read sees the entry.  fsync is
    // intentionally skipped — this store is a best-effort replay guard,
    // not a source of truth.
    f.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ZoneTransferConfig {
        ZoneTransferConfig::for_testing()
    }

    fn test_signer() -> TokenSigner {
        let c = test_config();
        TokenSigner::new(&c.hmac_secret, c.token_ttl_ms)
    }

    fn zone(s: &str) -> IslandId {
        IslandId::from(s)
    }

    #[test]
    fn valid_token_accepted() {
        let signer = test_signer();
        let ts = now_ms();
        let token = signer.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [10.0, 0.0, 20.0],
            [1.0, 0.0, -1.0],
            vec![],
            ts,
        );
        assert!(signer
            .validate_at(&token, &zone("zone-b"), ts + 100)
            .is_ok());
    }

    #[test]
    fn tampered_token_rejected() {
        let signer = test_signer();
        let ts = now_ms();
        let mut token = signer.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [10.0, 0.0, 20.0],
            [0.0; 3],
            vec![],
            ts,
        );
        token.position = [999.0, 0.0, 0.0];
        assert_eq!(
            signer.validate_at(&token, &zone("zone-b"), ts + 100),
            Err(TransferError::InvalidHmac)
        );
    }

    #[test]
    fn wrong_secret_rejected() {
        let signer = test_signer();
        let other = TokenSigner::new(b"different-secret-key-xxxxxxxxxx!", 10_000);
        let ts = now_ms();
        let token = signer.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
            ts,
        );
        assert_eq!(
            other.validate_at(&token, &zone("zone-b"), ts + 100),
            Err(TransferError::InvalidHmac)
        );
    }

    #[test]
    fn hmac_verified_before_zone_check() {
        let signer = test_signer();
        let other = TokenSigner::new(b"attacker-key-xxxxxxxxxxxxxxxx!", 10_000);
        let ts = now_ms();
        let token = other.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-wrong"),
            [0.0; 3],
            [0.0; 3],
            vec![],
            ts,
        );
        assert_eq!(
            signer.validate_at(&token, &zone("zone-b"), ts + 100),
            Err(TransferError::InvalidHmac)
        );
    }

    #[test]
    fn expired_token_rejected() {
        let signer = test_signer();
        let ts = 1_000_000;
        let token = signer.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
            ts,
        );
        assert_eq!(
            signer.validate_at(&token, &zone("zone-b"), ts + 10_001),
            Err(TransferError::TokenExpired)
        );
    }

    #[test]
    fn token_valid_at_ttl_boundary() {
        let signer = test_signer();
        let ts = 1_000_000;
        let token = signer.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
            ts,
        );
        assert!(signer
            .validate_at(&token, &zone("zone-b"), ts + 10_000)
            .is_ok());
        assert!(signer
            .validate_at(&token, &zone("zone-b"), ts + 10_001)
            .is_err());
    }

    #[test]
    fn zone_mismatch_rejected() {
        let signer = test_signer();
        let ts = now_ms();
        let token = signer.sign_at(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
            ts,
        );
        assert_eq!(
            signer.validate_at(&token, &zone("zone-c"), ts + 100),
            Err(TransferError::ZoneMismatch)
        );
    }

    #[test]
    fn token_roundtrip() {
        let signer = test_signer();
        let ts = now_ms();
        let token = signer.sign_at(
            "player-42".into(),
            zone("zone-a"),
            zone("zone-b"),
            [100.5, 0.0, -50.25],
            [3.0, 0.0, -1.5],
            vec![BuffState {
                buff_id: "speed_boost".into(),
                remaining_ms: 5000,
                data: vec![1, 2, 3],
            }],
            ts,
        );
        let bytes = token.to_bytes();
        let decoded = ZoneTransferToken::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, token);
        assert!(signer
            .validate_at(&decoded, &zone("zone-b"), ts + 100)
            .is_ok());
    }

    #[test]
    fn invalid_bytes_returns_error() {
        assert_eq!(
            ZoneTransferToken::from_bytes(&[0xFF, 0xFE]),
            Err(TransferError::InvalidToken)
        );
    }

    #[test]
    fn oversized_token_rejected() {
        let data = vec![0u8; MAX_TOKEN_BYTES + 1];
        assert_eq!(
            ZoneTransferToken::from_bytes(&data),
            Err(TransferError::InvalidToken)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn prepare_marks_transferring_and_acknowledge_clears() {
        let mut mgr = ZoneTransferManager::new(test_config());

        assert!(!mgr.is_transferring("player-1"));

        mgr.prepare_transfer(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();

        assert!(mgr.is_transferring("player-1"));
        assert_eq!(mgr.in_flight_count(), 1);

        mgr.acknowledge_transfer("player-1").unwrap();
        assert!(!mgr.is_transferring("player-1"));
        assert_eq!(mgr.in_flight_count(), 0);
    }

    #[test]
    fn acknowledge_unknown_player_fails() {
        let mut mgr = ZoneTransferManager::new(test_config());
        assert_eq!(
            mgr.acknowledge_transfer("nobody"),
            Err(TransferError::PlayerNotTransferring {
                player_id: "nobody".into()
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn state_preserved_through_transfer() {
        let mut mgr = ZoneTransferManager::new(test_config());
        let pos = [42.0, 10.0, -7.5];
        let vel = [1.0, 0.0, -0.5];
        let buffs = vec![
            BuffState {
                buff_id: "shield".into(),
                remaining_ms: 3000,
                data: vec![10, 20],
            },
            BuffState {
                buff_id: "haste".into(),
                remaining_ms: 8000,
                data: vec![],
            },
        ];

        let token = mgr
            .prepare_transfer(
                "player-1".into(),
                zone("zone-a"),
                zone("zone-b"),
                pos,
                vel,
                buffs.clone(),
            )
            .unwrap();

        let ts = token.timestamp;
        let transferred = mgr
            .accept_transfer_at(&token, &zone("zone-b"), ts + 100)
            .unwrap();

        assert_eq!(transferred.player_id, "player-1");
        assert_eq!(transferred.source_zone, zone("zone-a"));
        assert_eq!(transferred.position, pos);
        assert_eq!(transferred.velocity, vel);
        assert_eq!(transferred.buffs, buffs);
    }

    #[tokio::test(start_paused = true)]
    async fn duplicate_token_rejected() {
        let mut mgr = ZoneTransferManager::new(test_config());
        let token = mgr
            .prepare_transfer(
                "player-1".into(),
                zone("zone-a"),
                zone("zone-b"),
                [0.0; 3],
                [0.0; 3],
                vec![],
            )
            .unwrap();
        let ts = token.timestamp;

        assert!(mgr
            .accept_transfer_at(&token, &zone("zone-b"), ts + 100)
            .is_ok());

        assert_eq!(
            mgr.accept_transfer_at(&token, &zone("zone-b"), ts + 200),
            Err(TransferError::DuplicateToken {
                player_id: "player-1".into()
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn rapid_re_transfer_allowed() {
        let mut mgr = ZoneTransferManager::new(test_config());

        let t1 = mgr
            .prepare_transfer(
                "player-1".into(),
                zone("zone-a"),
                zone("zone-b"),
                [1.0, 0.0, 0.0],
                [0.0; 3],
                vec![],
            )
            .unwrap();

        let ts = t1.timestamp;
        mgr.accept_transfer_at(&t1, &zone("zone-b"), ts + 50)
            .unwrap();
        mgr.acknowledge_transfer("player-1").unwrap();

        // Same player transfers again immediately (B→C). Different token = different HMAC.
        let t2 = mgr
            .prepare_transfer(
                "player-1".into(),
                zone("zone-b"),
                zone("zone-c"),
                [2.0, 0.0, 0.0],
                [0.0; 3],
                vec![],
            )
            .unwrap();

        let ts = t2.timestamp;
        let p = mgr
            .accept_transfer_at(&t2, &zone("zone-c"), ts + 50)
            .unwrap();
        assert_eq!(p.position, [2.0, 0.0, 0.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn dedup_purge_clears_expired() {
        let mut mgr = ZoneTransferManager::new(ZoneTransferConfig {
            dedup_retention: Duration::from_secs(10),
            ..test_config()
        });

        let token = mgr
            .prepare_transfer(
                "player-1".into(),
                zone("zone-a"),
                zone("zone-b"),
                [0.0; 3],
                [0.0; 3],
                vec![],
            )
            .unwrap();
        let ts = token.timestamp;
        mgr.accept_transfer_at(&token, &zone("zone-b"), ts + 100)
            .unwrap();
        assert_eq!(mgr.dedup_count(), 1);

        tokio::time::advance(Duration::from_secs(11)).await;
        mgr.purge_expired_dedup();
        assert_eq!(mgr.dedup_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_rolls_back_transfer() {
        let mut mgr = ZoneTransferManager::new(ZoneTransferConfig {
            ack_timeout: Duration::from_secs(5),
            ..test_config()
        });

        mgr.prepare_transfer(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();

        assert!(mgr.check_timeouts().is_empty());
        assert!(mgr.is_transferring("player-1"));

        tokio::time::advance(Duration::from_secs(6)).await;

        let rolled_back = mgr.check_timeouts();
        assert_eq!(rolled_back, vec!["player-1"]);
        assert!(!mgr.is_transferring("player-1"));
    }

    #[test]
    fn manual_rollback() {
        let mut mgr = ZoneTransferManager::new(test_config());
        mgr.prepare_transfer(
            "player-1".into(),
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();

        mgr.rollback_transfer("player-1").unwrap();
        assert!(!mgr.is_transferring("player-1"));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_transfers_independent() {
        let mut mgr = ZoneTransferManager::new(test_config());

        let t1 = mgr
            .prepare_transfer(
                "player-1".into(),
                zone("zone-a"),
                zone("zone-b"),
                [1.0, 0.0, 0.0],
                [0.0; 3],
                vec![],
            )
            .unwrap();

        let t2 = mgr
            .prepare_transfer(
                "player-2".into(),
                zone("zone-a"),
                zone("zone-c"),
                [2.0, 0.0, 0.0],
                [0.0; 3],
                vec![],
            )
            .unwrap();

        assert_eq!(mgr.in_flight_count(), 2);

        let ts = t2.timestamp;
        let p2 = mgr
            .accept_transfer_at(&t2, &zone("zone-c"), ts + 100)
            .unwrap();
        assert_eq!(p2.position, [2.0, 0.0, 0.0]);

        mgr.acknowledge_transfer("player-2").unwrap();
        assert_eq!(mgr.in_flight_count(), 1);
        assert!(mgr.is_transferring("player-1"));

        let ts = t1.timestamp;
        let p1 = mgr
            .accept_transfer_at(&t1, &zone("zone-b"), ts + 200)
            .unwrap();
        assert_eq!(p1.position, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn in_flight_capacity_enforced() {
        let mut mgr = ZoneTransferManager::new(ZoneTransferConfig {
            max_in_flight: 2,
            ..test_config()
        });

        mgr.prepare_transfer(
            "p1".into(),
            zone("a"),
            zone("b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();
        mgr.prepare_transfer(
            "p2".into(),
            zone("a"),
            zone("b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        )
        .unwrap();

        let result = mgr.prepare_transfer(
            "p3".into(),
            zone("a"),
            zone("b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        );
        assert_eq!(result, Err(TransferError::AtCapacity));
    }

    #[test]
    fn player_id_too_long_rejected() {
        let mut mgr = ZoneTransferManager::new(test_config());
        let long_id = "x".repeat(MAX_PLAYER_ID_LEN + 1);

        let result = mgr.prepare_transfer(
            long_id,
            zone("zone-a"),
            zone("zone-b"),
            [0.0; 3],
            [0.0; 3],
            vec![],
        );
        assert_eq!(result, Err(TransferError::InvalidPlayerId));
    }
}
