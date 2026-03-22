//! Testing utilities for quanta-realtime-server.
//!
//! Gated on `#[cfg(any(test, feature = "test-utils"))]` at the module level in lib.rs.

mod bot;
mod certs;
mod harness;
mod mock_session;
mod recording;

pub use bot::{BotAction, BotBehavior, BotHarness, BotMetrics, IdleBot, RandomWalkBot, StressBot};
pub use certs::generate_test_certs;
pub use harness::{TestHarness, TestHarnessBuilder};
pub use mock_session::MockSession;
pub use recording::{
    replay, Divergence, IslandRecording, RecordedEffect, RecordedInput, ReplayResult, TickRecord,
};
