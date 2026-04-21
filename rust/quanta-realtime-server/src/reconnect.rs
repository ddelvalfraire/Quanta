use std::sync::Arc;

use crate::session::Session;
use crate::session_store::RetainedSession;

/// Classification of how a client connected.
///
/// Tier 1 (QUIC connection migration) is handled transparently by Quinn
/// and is invisible to application code — it doesn't appear here.
#[derive(Debug)]
pub enum ReconnectTier {
    /// Tier 2: Client reconnected within the retention window.
    /// The retained session provides `baseline_tick` and `visible_entities`
    /// so the caller can compute and send deltas from baseline to current.
    Fast { retained: RetainedSession },
    /// Tier 3: Cold connect (first time or session expired).
    /// Full authentication and initial state sync required.
    Cold,
}

/// A fully authenticated client connection ready for the application layer.
pub struct ConnectedClient {
    pub session: Arc<dyn Session>,
    pub session_id: u64,
    /// The raw QUIC connection handle for bulk transfer (None for WebSocket).
    pub quic_connection: Option<quinn::Connection>,
    pub reconnect_tier: ReconnectTier,
}
