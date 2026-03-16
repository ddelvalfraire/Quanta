use rustler::Resource;

/// Shared wasmtime Engine (created once, cached).
#[allow(dead_code)]
pub struct EngineResource(pub wasmtime::Engine);
impl Resource for EngineResource {}

/// Compiled WASM component (per actor type).
#[allow(dead_code)]
pub struct ComponentResource(pub wasmtime::component::Component);
impl Resource for ComponentResource {}

/// Shared wasmtime Linker (cached per engine).
#[allow(dead_code)]
pub struct LinkerResource(pub wasmtime::component::Linker<()>);
impl Resource for LinkerResource {}

// NatsConnectionResource — T07 (depends on NatsInner, not yet defined)
// LoroDocResource — Phase 2 (depends on Mutex<loro::LoroDoc>)
