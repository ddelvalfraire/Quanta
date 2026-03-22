use std::collections::VecDeque;
use std::time::Duration;

use rustc_hash::FxHashMap;
use tokio::time::Instant;

use crate::types::EntitySlot;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Retained state for a disconnected client, enabling fast reconnect.
#[derive(Debug)]
pub struct RetainedSession {
    /// The tick at which this client last received a full/delta state update.
    pub baseline_tick: u64,
    /// Snapshot of entity slots visible to the client at disconnect.
    pub visible_entities: Vec<EntitySlot>,
    /// Last acknowledged input sequence number.
    pub input_seq: u32,
    /// The session_id from auth, used as reconnect token.
    pub session_token: u64,
    /// Client capabilities retained for fast reconnect.
    pub client_capabilities: FxHashMap<String, String>,
    /// When this retention entry was created.
    pub created_at: Instant,
}

/// Bounded session store with per-session TTL expiry and LRU eviction.
///
/// This is a synchronous data structure — the caller is responsible for
/// periodically calling [`purge_expired`] (e.g. on a 1-second tokio interval).
pub struct SessionStore {
    sessions: FxHashMap<u64, RetainedSession>,
    /// Tracks insertion order for LRU eviction. Oldest is at front.
    insertion_order: VecDeque<u64>,
    expiry_duration: Duration,
    max_sessions: usize,
}

impl SessionStore {
    pub fn new(expiry_duration: Duration, max_sessions: usize) -> Self {
        Self {
            sessions: FxHashMap::default(),
            insertion_order: VecDeque::new(),
            expiry_duration,
            max_sessions,
        }
    }

    /// Insert a retained session. Evicts LRU if at capacity.
    /// Returns the evicted session_id if one was removed due to capacity.
    pub fn insert(&mut self, session_id: u64, session: RetainedSession) -> Option<u64> {
        // If this session_id already exists, remove the old entry first.
        if self.sessions.contains_key(&session_id) {
            self.sessions.remove(&session_id);
            self.insertion_order.retain(|&id| id != session_id);
        }

        let mut evicted = None;

        // Evict LRU if at capacity.
        if self.sessions.len() >= self.max_sessions {
            if let Some(oldest_id) = self.insertion_order.pop_front() {
                self.sessions.remove(&oldest_id);
                evicted = Some(oldest_id);
            }
        }

        self.sessions.insert(session_id, session);
        self.insertion_order.push_back(session_id);
        evicted
    }

    /// Look up and remove a retained session for fast reconnect.
    /// Returns `None` if `session_id` is not found or the session has expired.
    pub fn take(&mut self, session_id: u64) -> Option<RetainedSession> {
        let session = self.sessions.remove(&session_id)?;
        self.insertion_order.retain(|&id| id != session_id);

        if session.created_at.elapsed() > self.expiry_duration {
            return None;
        }

        Some(session)
    }

    /// Remove all expired sessions. Returns the number of sessions purged.
    pub fn purge_expired(&mut self) -> usize {
        let expiry = self.expiry_duration;
        let before = self.sessions.len();

        self.sessions
            .retain(|_, session| session.created_at.elapsed() <= expiry);

        let removed = before - self.sessions.len();
        if removed > 0 {
            self.insertion_order
                .retain(|id| self.sessions.contains_key(id));
        }
        removed
    }

    /// Current number of retained sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(token: u64) -> RetainedSession {
        RetainedSession {
            baseline_tick: 100,
            visible_entities: vec![EntitySlot(0), EntitySlot(1)],
            input_seq: 5,
            session_token: token,
            client_capabilities: FxHashMap::default(),
            created_at: Instant::now(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn insert_and_take() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);
        store.insert(42, make_session(42));

        let session = store.take(42).unwrap();
        assert_eq!(session.session_token, 42);
        assert_eq!(session.baseline_tick, 100);

        // Second take returns None (already removed).
        assert!(store.take(42).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn take_returns_none_after_expiry() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);
        store.insert(1, make_session(1));

        // Advance past the 30s expiry.
        tokio::time::advance(Duration::from_secs(31)).await;

        assert!(store.take(1).is_none());
        assert!(store.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn purge_removes_expired() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);
        store.insert(1, make_session(1));
        store.insert(2, make_session(2));
        store.insert(3, make_session(3));

        tokio::time::advance(Duration::from_secs(31)).await;

        let purged = store.purge_expired();
        assert_eq!(purged, 3);
        assert!(store.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn purge_keeps_unexpired() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);
        store.insert(1, make_session(1));

        tokio::time::advance(Duration::from_secs(10)).await;
        store.insert(2, make_session(2));

        tokio::time::advance(Duration::from_secs(21)).await;
        // Session 1 is 31s old (expired), session 2 is 21s old (not expired).

        let purged = store.purge_expired();
        assert_eq!(purged, 1);
        assert_eq!(store.len(), 1);
        assert!(store.take(2).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn lru_eviction_at_capacity() {
        let mut store = SessionStore::new(Duration::from_secs(30), 3);
        store.insert(1, make_session(1));
        store.insert(2, make_session(2));
        store.insert(3, make_session(3));

        // Inserting a 4th should evict the oldest (1).
        let evicted = store.insert(4, make_session(4));
        assert_eq!(evicted, Some(1));
        assert_eq!(store.len(), 3);
        assert!(store.take(1).is_none());
        assert!(store.take(4).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn eviction_at_1001() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);

        for i in 0..1000 {
            store.insert(i, make_session(i));
        }
        assert_eq!(store.len(), 1000);

        // 1001st session should evict session 0 (the oldest).
        let evicted = store.insert(1000, make_session(1000));
        assert_eq!(evicted, Some(0));
        assert_eq!(store.len(), 1000);
    }

    #[tokio::test(start_paused = true)]
    async fn take_nonexistent_returns_none() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);
        assert!(store.take(999).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn insert_duplicate_replaces() {
        let mut store = SessionStore::new(Duration::from_secs(30), 1000);
        store.insert(1, RetainedSession {
            baseline_tick: 10,
            visible_entities: vec![],
            input_seq: 1,
            session_token: 1,
            client_capabilities: FxHashMap::default(),
            created_at: Instant::now(),
        });
        store.insert(1, RetainedSession {
            baseline_tick: 20,
            visible_entities: vec![],
            input_seq: 2,
            session_token: 1,
            client_capabilities: FxHashMap::default(),
            created_at: Instant::now(),
        });

        assert_eq!(store.len(), 1);
        let session = store.take(1).unwrap();
        assert_eq!(session.baseline_tick, 20);
    }
}
