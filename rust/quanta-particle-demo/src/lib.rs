//! Particle World — a 2D authoritative-movement demo for Quanta's realtime
//! server. Decoupled from the platform: this crate depends on the public
//! `quanta-realtime-server` API (including the `WasmExecutor` trait) and
//! the `quanta-core-rs` schema/delta codec, and plugs itself in via the
//! `executor_factory` field on `RunServerArgs`.

pub mod executor;
pub mod input;
pub mod schema;

use std::sync::Arc;

use quanta_realtime_server::tick::WasmExecutor;
use quanta_realtime_server::ExecutorFactory;

use crate::executor::ParticleExecutor;

/// Build an `ExecutorFactory` suitable for `RunServerArgs.executor_factory`.
///
/// Each island the server spawns gets a fresh `ParticleExecutor` instance
/// configured with the given tick rate (must match `TickEngineConfig.tick_rate_hz`).
pub fn particle_executor_factory(tick_rate_hz: u8) -> ExecutorFactory {
    Arc::new(move || -> Box<dyn WasmExecutor> { Box::new(ParticleExecutor::new(tick_rate_hz)) })
}
