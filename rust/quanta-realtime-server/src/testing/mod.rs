mod bot;
mod certs;
pub mod endpoint_helpers;
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

pub mod test_executors {
    use crate::tick::*;
    use crate::types::EntitySlot;

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
