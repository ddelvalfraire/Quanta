use std::collections::HashMap;
use std::sync::Mutex;

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

/// Loro CRDT document wrapped in Mutex for thread safety.
/// LoroDoc is Send but !Sync — Mutex provides the Sync bound required by ResourceArc.
/// GenServer serializes access so contention is minimal; the Mutex exists for correctness.
pub struct LoroDocResource(pub Mutex<LoroDocInner>);
impl Resource for LoroDocResource {}

pub struct LoroDocInner {
    pub doc: loro::LoroDoc,
    pub text_styles: HashMap<String, loro::StyleConfig>,
}
