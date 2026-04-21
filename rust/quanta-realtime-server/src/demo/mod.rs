//! Particle World demo: a minimal 2D authoritative simulation used as the
//! reference target for the realtime server stack. Not part of the
//! platform's generic surface — everything here is application code that
//! happens to live in the server crate during Phase 2.
//!
//! The module is additive-only: existing callers that don't opt into
//! `ExecutorKind::Particle` see no behavior change.

pub mod executor;
pub mod input;
pub mod schema;
