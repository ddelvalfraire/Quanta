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
    replay, Divergence, IslandRecording, RecordedEffect, RecordedInput, TickRecord,
};

/// Shared test executors to avoid duplication across test modules.
pub mod test_executors {
    use crate::tick::*;
    use crate::types::EntitySlot;

    /// WASM executor that increments the first byte of state on each message.
    pub struct IncrementWasm;

    impl WasmExecutor for IncrementWasm {
        fn call_handle_message(
            &mut self,
            _entity: EntitySlot,
            state: &[u8],
            _message: &TickMessage,
        ) -> Result<HandleResult, WasmTrap> {
            let mut new_state = state.to_vec();
            new_state[0] = new_state[0].wrapping_add(1);
            Ok(HandleResult {
                state: new_state,
                effects: vec![],
            })
        }
    }
}
