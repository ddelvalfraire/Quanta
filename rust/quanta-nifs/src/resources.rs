use rustler::Resource;

use crate::wasm_runtime::ActorStoreData;

/// Shared wasmtime Engine (created once, cached).
pub struct EngineResource(pub wasmtime::Engine);
impl Resource for EngineResource {}

/// Compiled WASM component (per actor type).
pub struct ComponentResource(pub wasmtime::component::Component);
impl Resource for ComponentResource {}

/// Shared wasmtime Linker with WASI + actor store data (cached per engine).
/// Linker::instantiate takes &self, so no lock needed — ResourceArc provides &T.
pub struct LinkerResource(pub wasmtime::component::Linker<ActorStoreData>);
impl Resource for LinkerResource {}

// NatsConnectionResource — T07 (depends on NatsInner, not yet defined)
// LoroDocResource — Phase 2 (depends on Mutex<loro::LoroDoc>)
