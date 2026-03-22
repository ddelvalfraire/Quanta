//! Bridge request-reply, tick-aligned batching, and d2r coalescing.
//!
//! Sits between the tick engine and NATS transport:
//! - **RequestReplyMap**: tracks pending r2d requests by correlation ID
//! - **R2dBatcher**: collects r2d messages during a tick, flushes as batch envelopes
//! - **D2rCoalescer**: optional coalescing of state-sync d2r messages within a time window
//!
//! This module handles the data-plane bridge RPC (request-reply, batching, coalescing).
//! The control-plane bridge (island lifecycle events like stop/passivation) is defined
//! separately in [`crate::traits::Bridge`].

use crate::tick::types::CorrelationId;
use crate::types::EntitySlot;
use quanta_core_rs::bridge::{
    encode_batch_envelope, encode_bridge_frame, BridgeHeader, BridgeMsgType,
};
use rustc_hash::FxHashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

fn epoch_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone)]
pub struct BridgeRpcConfig {
    pub d2r_timeout: Duration,
    pub r2d_timeout: Duration,
    pub coalesce_d2r: Option<Duration>,
    pub timeout_overrides: FxHashMap<u8, Duration>,
    pub max_pending: usize,
}

impl Default for BridgeRpcConfig {
    fn default() -> Self {
        Self {
            d2r_timeout: Duration::from_millis(500),
            r2d_timeout: Duration::from_millis(200),
            coalesce_d2r: None,
            timeout_overrides: FxHashMap::default(),
            max_pending: 1024,
        }
    }
}

impl BridgeRpcConfig {
    /// Look up the r2d timeout for a specific message type, falling back to the default.
    pub fn r2d_timeout_for(&self, msg_type: BridgeMsgType) -> Duration {
        self.timeout_overrides
            .get(&(msg_type as u8))
            .copied()
            .unwrap_or(self.r2d_timeout)
    }

    /// Look up the d2r timeout for a specific message type, falling back to the default.
    pub fn d2r_timeout_for(&self, msg_type: BridgeMsgType) -> Duration {
        self.timeout_overrides
            .get(&(msg_type as u8))
            .copied()
            .unwrap_or(self.d2r_timeout)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RequestError {
    Timeout,
    NoResponders,
    AtCapacity,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "request timed out"),
            Self::NoResponders => write!(f, "no responders on target subject"),
            Self::AtCapacity => write!(f, "pending request map at capacity"),
        }
    }
}

impl std::error::Error for RequestError {}

/// Tracks pending r2d requests by correlation ID with deadline-based expiry.
pub struct RequestReplyMap {
    pending: FxHashMap<CorrelationId, PendingRequest>,
    counter: u64,
    max_pending: usize,
}

struct PendingRequest {
    sender: oneshot::Sender<Vec<u8>>,
    source_entity: EntitySlot,
    deadline: Instant,
}

impl RequestReplyMap {
    pub fn new(max_pending: usize) -> Self {
        Self {
            pending: FxHashMap::default(),
            counter: 0,
            max_pending,
        }
    }

    /// Generate a new correlation ID.
    ///
    /// Layout (ULID-inspired): 6 bytes ms timestamp + 2 bytes counter + 8 bytes random.
    /// The random component (via RandomState) prevents collisions across concurrent islands.
    pub fn next_correlation_id(&mut self) -> CorrelationId {
        self.counter = self.counter.wrapping_add(1);
        let ts = epoch_millis();

        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(ts);
        hasher.write_u64(self.counter);
        let random = hasher.finish();

        let mut id = [0u8; 16];
        id[..6].copy_from_slice(&ts.to_be_bytes()[2..]);
        id[6..8].copy_from_slice(&(self.counter as u16).to_be_bytes());
        id[8..].copy_from_slice(&random.to_ne_bytes());
        id
    }

    pub fn insert(
        &mut self,
        correlation_id: CorrelationId,
        source_entity: EntitySlot,
        timeout: Duration,
    ) -> Result<oneshot::Receiver<Vec<u8>>, RequestError> {
        if self.pending.len() >= self.max_pending {
            return Err(RequestError::AtCapacity);
        }
        let (tx, rx) = oneshot::channel();
        self.pending.insert(
            correlation_id,
            PendingRequest {
                sender: tx,
                source_entity,
                deadline: Instant::now() + timeout,
            },
        );
        Ok(rx)
    }

    pub fn resolve(
        &mut self,
        correlation_id: &CorrelationId,
        payload: Vec<u8>,
    ) -> Option<EntitySlot> {
        if let Some(req) = self.pending.remove(correlation_id) {
            let entity = req.source_entity;
            let _ = req.sender.send(payload);
            Some(entity)
        } else {
            None
        }
    }

    pub fn remove_expired(&mut self) -> Vec<(CorrelationId, EntitySlot)> {
        let now = Instant::now();
        let mut expired = Vec::new();
        self.pending.retain(|cid, req| {
            if now >= req.deadline {
                expired.push((*cid, req.source_entity));
                false
            } else {
                true
            }
        });
        expired
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// Collects r2d messages during a tick, flushes as one batch envelope per target type.
pub struct R2dBatcher {
    batches: FxHashMap<String, Vec<Vec<u8>>>,
    sequence: u64,
}

impl R2dBatcher {
    pub fn new() -> Self {
        Self {
            batches: FxHashMap::default(),
            sequence: 0,
        }
    }

    pub fn push(
        &mut self,
        target: String,
        msg_type: BridgeMsgType,
        payload: &[u8],
        correlation_id: Option<CorrelationId>,
    ) {
        self.sequence = self.sequence.wrapping_add(1);
        let header = BridgeHeader {
            msg_type,
            sequence: self.sequence,
            timestamp: epoch_millis(),
            correlation_id,
        };
        let frame = encode_bridge_frame(&header, payload);
        self.batches.entry(target).or_default().push(frame);
    }

    pub fn flush(&mut self) -> Vec<(String, Vec<u8>)> {
        let batches = std::mem::take(&mut self.batches);
        batches
            .into_iter()
            .map(|(target, frames)| {
                let refs: Vec<&[u8]> = frames.iter().map(|f| f.as_slice()).collect();
                let envelope = encode_batch_envelope(&refs);
                (target, envelope)
            })
            .collect()
    }

    pub fn target_count(&self) -> usize {
        self.batches.len()
    }

    pub fn message_count(&self) -> usize {
        self.batches.values().map(|v| v.len()).sum()
    }
}

/// Coalesces d2r state-sync messages within a time window (latest wins per key).
pub struct D2rCoalescer {
    window: Duration,
    pending: FxHashMap<(String, EntitySlot), CoalescedEntry>,
}

struct CoalescedEntry {
    payload: Vec<u8>,
    received_at: Instant,
}

impl D2rCoalescer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: FxHashMap::default(),
        }
    }

    pub fn submit(
        &mut self,
        source_island: String,
        target_entity: EntitySlot,
        payload: Vec<u8>,
    ) -> bool {
        let key = (source_island, target_entity);
        let is_new = !self.pending.contains_key(&key);
        self.pending.insert(
            key,
            CoalescedEntry {
                payload,
                received_at: Instant::now(),
            },
        );
        is_new
    }

    pub fn drain_ready(&mut self) -> Vec<(String, EntitySlot, Vec<u8>)> {
        let now = Instant::now();
        let mut ready = Vec::new();
        self.pending.retain(|key, entry| {
            if now.duration_since(entry.received_at) >= self.window {
                ready.push((key.0.clone(), key.1, std::mem::take(&mut entry.payload)));
                false
            } else {
                true
            }
        });
        ready
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_reply_insert_and_resolve() {
        let mut map = RequestReplyMap::new(1024);
        let cid = map.next_correlation_id();
        let mut rx = map.insert(cid, EntitySlot(1), Duration::from_millis(200)).unwrap();

        assert_eq!(map.pending_count(), 1);
        let entity = map.resolve(&cid, b"response".to_vec());
        assert_eq!(entity, Some(EntitySlot(1)));
        assert_eq!(map.pending_count(), 0);

        assert_eq!(rx.try_recv().unwrap(), b"response");
    }

    #[test]
    fn request_reply_resolve_unknown_returns_none() {
        let mut map = RequestReplyMap::new(1024);
        let entity = map.resolve(&[0; 16], b"data".to_vec());
        assert_eq!(entity, None);
    }

    #[test]
    fn request_reply_expired_cleanup() {
        let mut map = RequestReplyMap::new(1024);
        let cid = [42u8; 16];
        let _rx = map.insert(cid, EntitySlot(5), Duration::from_millis(0)).unwrap();

        std::thread::sleep(Duration::from_millis(1));
        let expired = map.remove_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], (cid, EntitySlot(5)));
        assert_eq!(map.pending_count(), 0);
    }

    #[test]
    fn correlation_id_has_random_component() {
        let mut map = RequestReplyMap::new(1024);
        let id1 = map.next_correlation_id();
        let id2 = map.next_correlation_id();
        assert_ne!(id1, id2);
        // Random bytes (last 8) should differ even within the same ms
        assert_ne!(id1[8..], id2[8..]);
    }

    #[test]
    fn request_reply_at_capacity() {
        let mut map = RequestReplyMap::new(2);
        let cid1 = map.next_correlation_id();
        let cid2 = map.next_correlation_id();
        let cid3 = map.next_correlation_id();

        let _rx1 = map.insert(cid1, EntitySlot(1), Duration::from_secs(10)).unwrap();
        let _rx2 = map.insert(cid2, EntitySlot(2), Duration::from_secs(10)).unwrap();

        let err = map.insert(cid3, EntitySlot(3), Duration::from_secs(10));
        assert_eq!(err.unwrap_err(), RequestError::AtCapacity);
    }

    #[test]
    fn per_message_type_timeout_override() {
        let mut config = BridgeRpcConfig::default();
        config.timeout_overrides.insert(
            BridgeMsgType::EntityCommand as u8,
            Duration::from_millis(100),
        );

        assert_eq!(
            config.r2d_timeout_for(BridgeMsgType::EntityCommand),
            Duration::from_millis(100),
        );
        assert_eq!(
            config.r2d_timeout_for(BridgeMsgType::StateSync),
            Duration::from_millis(200),
        );
    }

    #[tokio::test]
    async fn request_reply_timeout_via_tokio() {
        let mut map = RequestReplyMap::new(1024);
        let cid = map.next_correlation_id();
        let rx = map.insert(cid, EntitySlot(1), Duration::from_millis(50)).unwrap();

        // Don't resolve — let it timeout
        let result = tokio::time::timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_err(), "should timeout waiting for unresolved response");
    }

    #[test]
    fn batcher_groups_by_target() {
        let mut batcher = R2dBatcher::new();
        batcher.push("type_a".into(), BridgeMsgType::EntityCommand, b"p1", None);
        batcher.push("type_a".into(), BridgeMsgType::EntityCommand, b"p2", None);
        batcher.push("type_b".into(), BridgeMsgType::StateSync, b"p3", None);

        assert_eq!(batcher.target_count(), 2);
        assert_eq!(batcher.message_count(), 3);

        let batches = batcher.flush();
        assert_eq!(batches.len(), 2);
        assert_eq!(batcher.message_count(), 0);
    }

    #[test]
    fn batcher_five_messages_one_publish() {
        use quanta_core_rs::bridge::decode_batch_envelope;

        let mut batcher = R2dBatcher::new();
        for i in 0..5u8 {
            batcher.push("target".into(), BridgeMsgType::EntityCommand, &[i], None);
        }

        let batches = batcher.flush();
        assert_eq!(batches.len(), 1, "5 messages to same target → 1 batch");

        let (target, envelope) = &batches[0];
        assert_eq!(target, "target");

        let frames = decode_batch_envelope(envelope).unwrap();
        assert_eq!(frames.len(), 5, "batch contains 5 frames");
    }

    #[test]
    fn batcher_flush_clears_state() {
        let mut batcher = R2dBatcher::new();
        batcher.push("t".into(), BridgeMsgType::Heartbeat, b"", None);
        assert_eq!(batcher.flush().len(), 1);
        assert!(batcher.flush().is_empty());
    }

    #[test]
    fn coalescer_three_messages_within_10ms_merged_to_one() {
        let mut coalescer = D2rCoalescer::new(Duration::from_millis(10));
        coalescer.submit("island_a".into(), EntitySlot(1), b"v1".to_vec());
        coalescer.submit("island_a".into(), EntitySlot(1), b"v2".to_vec());
        coalescer.submit("island_a".into(), EntitySlot(1), b"v3".to_vec());

        assert_eq!(coalescer.pending_count(), 1, "3 messages to same key coalesced");

        std::thread::sleep(Duration::from_millis(15));
        let ready = coalescer.drain_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].2, b"v3", "latest payload wins");
    }

    #[test]
    fn coalescer_distinct_keys_kept_separate() {
        let mut coalescer = D2rCoalescer::new(Duration::from_millis(5));
        coalescer.submit("island_a".into(), EntitySlot(1), b"a1".to_vec());
        coalescer.submit("island_a".into(), EntitySlot(2), b"a2".to_vec());
        coalescer.submit("island_b".into(), EntitySlot(1), b"b1".to_vec());

        assert_eq!(coalescer.pending_count(), 3);

        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(coalescer.drain_ready().len(), 3);
    }

    #[test]
    fn coalescer_respects_window() {
        let mut coalescer = D2rCoalescer::new(Duration::from_millis(100));
        coalescer.submit("island".into(), EntitySlot(1), b"data".to_vec());
        assert!(coalescer.drain_ready().is_empty());
        assert_eq!(coalescer.pending_count(), 1);
    }
}
