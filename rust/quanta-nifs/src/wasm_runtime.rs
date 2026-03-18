use hmac::{Hmac, Mac};
use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};
use sha2::Sha256;
use std::collections::HashMap;
use wasmtime::component::{Component, ComponentExportIndex, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::resources::{ComponentResource, EngineResource, LinkerResource};

mod atoms {
    rustler::atoms! {
        ok,
        error,
        fuel_exhausted,
        memory_exceeded,
        trap,
        not_exported,
        hmac_mismatch,
    }
}

// ---------------------------------------------------------------------------
// Store data
// ---------------------------------------------------------------------------

pub struct ActorStoreData {
    pub limits: StoreLimits,
    pub wasi_ctx: WasiCtx,
    pub resource_table: ResourceTable,
}

impl WasiView for ActorStoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

// ---------------------------------------------------------------------------
// Error encoding — all errors become {:error, reason} tuples
// ---------------------------------------------------------------------------

fn err_term<'a>(env: Env<'a>, msg: impl std::fmt::Display) -> Term<'a> {
    (atoms::error(), format!("{}", msg)).encode(env)
}

fn classify_wasm_error<'a>(env: Env<'a>, err: wasmtime::Error) -> Term<'a> {
    if let Some(t) = err.downcast_ref::<Trap>() {
        match *t {
            Trap::OutOfFuel => return (atoms::error(), atoms::fuel_exhausted()).encode(env),
            _ => {}
        }
    }
    let msg = format!("{:#}", err);
    if msg.contains("memory") || msg.contains("grow") {
        (atoms::error(), atoms::memory_exceeded()).encode(env)
    } else {
        (atoms::error(), atoms::trap()).encode(env)
    }
}

// ---------------------------------------------------------------------------
// HMAC helpers
// ---------------------------------------------------------------------------

const HMAC_TAG_LEN: usize = 32;
const HMAC_MIN_KEY_LEN: usize = 32;

type HmacSha256 = Hmac<Sha256>;

fn compute_hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key validated before use");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn verify_hmac(key: &[u8], data: &[u8], tag: &[u8]) -> bool {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key validated before use");
    mac.update(data);
    mac.verify_slice(tag).is_ok()
}

// ---------------------------------------------------------------------------
// Store / export helpers
// ---------------------------------------------------------------------------

fn create_store(engine: &Engine, fuel: u64, memory_limit: usize) -> Store<ActorStoreData> {
    let limits = StoreLimitsBuilder::new()
        .memory_size(memory_limit)
        .trap_on_grow_failure(true)
        .build();
    let data = ActorStoreData {
        limits,
        wasi_ctx: WasiCtxBuilder::new().build(),
        resource_table: ResourceTable::new(),
    };
    let mut store = Store::new(engine, data);
    store.limiter(|d| &mut d.limits);
    store.set_fuel(fuel).expect("fuel should be settable");
    store
}

fn get_actor_func_index(
    component: &Component,
    func_name: &str,
) -> Result<ComponentExportIndex, String> {
    let actor_idx = component
        .get_export_index(None, "quanta:actor/actor")
        .ok_or_else(|| "export quanta:actor/actor not found".to_string())?;
    component
        .get_export_index(Some(&actor_idx), func_name)
        .ok_or_else(|| format!("export {} not found in quanta:actor/actor", func_name))
}

// ---------------------------------------------------------------------------
// Effect encoding — WIT effects → Erlang map list
// ---------------------------------------------------------------------------

fn bytes_to_binary<'a>(env: Env<'a>, data: &[u8]) -> Term<'a> {
    let mut bin = NewBinary::new(env, data.len());
    bin.as_mut_slice().copy_from_slice(data);
    Binary::from(bin).encode(env)
}

fn encode_effects<'a>(env: Env<'a>, effects: &[WitEffect]) -> Term<'a> {
    let list: Vec<Term<'a>> = effects.iter().map(|e| encode_effect(env, e)).collect();
    list.encode(env)
}

fn encode_effect<'a>(env: Env<'a>, effect: &WitEffect) -> Term<'a> {
    match effect {
        WitEffect::Persist => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "persist".encode(env)).unwrap();
            m
        }
        WitEffect::Send(send) => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "send".encode(env)).unwrap();
            m = m.map_put("target".encode(env), send.target.as_str().encode(env)).unwrap();
            m = m.map_put("payload".encode(env), bytes_to_binary(env, &send.payload)).unwrap();
            if let Some(ref cid) = send.correlation_id {
                m = m.map_put("correlation_id".encode(env), cid.as_str().encode(env)).unwrap();
            }
            m
        }
        WitEffect::Reply(data) => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "reply".encode(env)).unwrap();
            m = m.map_put("data".encode(env), bytes_to_binary(env, data)).unwrap();
            m
        }
        WitEffect::SetTimer(timer) => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "set_timer".encode(env)).unwrap();
            m = m.map_put("name".encode(env), timer.name.as_str().encode(env)).unwrap();
            m = m.map_put("delay_ms".encode(env), timer.delay_ms.encode(env)).unwrap();
            m
        }
        WitEffect::CancelTimer(name) => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "cancel_timer".encode(env)).unwrap();
            m = m.map_put("name".encode(env), name.as_str().encode(env)).unwrap();
            m
        }
        WitEffect::EmitEvent(data) => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "emit_event".encode(env)).unwrap();
            m = m.map_put("data".encode(env), bytes_to_binary(env, data)).unwrap();
            m
        }
        WitEffect::Log(msg) => {
            let mut m = Term::map_new(env);
            m = m.map_put("type".encode(env), "log".encode(env)).unwrap();
            m = m.map_put("message".encode(env), msg.as_str().encode(env)).unwrap();
            m
        }
    }
}

// ---------------------------------------------------------------------------
// Envelope decoding — Erlang map → WIT Envelope
// ---------------------------------------------------------------------------

fn decode_envelope<'a>(term: Term<'a>) -> Result<WitEnvelope, String> {
    let map: HashMap<String, Term<'a>> = term.decode()
        .map_err(|_| "envelope must be a map with string keys".to_string())?;

    let source: String = map.get("source")
        .ok_or("envelope missing 'source'")?
        .decode().map_err(|_| "envelope 'source' must be a string".to_string())?;

    let payload_term = map.get("payload").ok_or("envelope missing 'payload'")?;
    let payload: Vec<u8> = payload_term.decode::<Binary>()
        .map(|b| b.as_slice().to_vec())
        .or_else(|_| payload_term.decode::<Vec<u8>>())
        .map_err(|_| "envelope 'payload' must be a binary or list".to_string())?;

    let correlation_id: Option<String> = match map.get("correlation_id") {
        Some(t) if t.is_atom() => None,
        Some(t) => Some(t.decode().map_err(|_| "envelope 'correlation_id' must be a string or nil".to_string())?),
        None => None,
    };

    Ok(WitEnvelope { source, payload, correlation_id })
}

// ---------------------------------------------------------------------------
// Result encoding helper
// ---------------------------------------------------------------------------

fn encode_handle_result<'a>(env: Env<'a>, result: &WitHandleResult) -> Term<'a> {
    let mut state_out = NewBinary::new(env, result.state.len());
    state_out.as_mut_slice().copy_from_slice(&result.state);
    (atoms::ok(), Binary::from(state_out), encode_effects(env, &result.effects)).encode(env)
}

// ---------------------------------------------------------------------------
// WIT types — manually derived to match quanta:actor WIT interface
// ---------------------------------------------------------------------------

#[derive(wasmtime::component::ComponentType, wasmtime::component::Lift)]
#[component(record)]
struct WitHandleResult {
    #[component(name = "state")]
    state: Vec<u8>,
    #[component(name = "effects")]
    effects: Vec<WitEffect>,
}

#[derive(wasmtime::component::ComponentType, wasmtime::component::Lift)]
#[component(variant)]
enum WitEffect {
    #[component(name = "persist")]
    Persist,
    #[component(name = "send")]
    Send(WitSendEffect),
    #[component(name = "reply")]
    Reply(Vec<u8>),
    #[component(name = "set-timer")]
    SetTimer(WitTimerEffect),
    #[component(name = "cancel-timer")]
    CancelTimer(String),
    #[component(name = "emit-event")]
    EmitEvent(Vec<u8>),
    #[component(name = "log")]
    Log(String),
}

#[derive(wasmtime::component::ComponentType, wasmtime::component::Lift)]
#[component(record)]
struct WitSendEffect {
    #[component(name = "target")]
    target: String,
    #[component(name = "payload")]
    payload: Vec<u8>,
    #[component(name = "correlation-id")]
    correlation_id: Option<String>,
}

#[derive(wasmtime::component::ComponentType, wasmtime::component::Lift)]
#[component(record)]
struct WitTimerEffect {
    #[component(name = "name")]
    name: String,
    #[component(name = "delay-ms")]
    delay_ms: u64,
}

#[derive(wasmtime::component::ComponentType, wasmtime::component::Lower)]
#[component(record)]
struct WitEnvelope {
    #[component(name = "source")]
    source: String,
    #[component(name = "payload")]
    payload: Vec<u8>,
    #[component(name = "correlation-id")]
    correlation_id: Option<String>,
}

// ---------------------------------------------------------------------------
// NIFs — each wraps an inner fn that uses ? for ergonomics
// ---------------------------------------------------------------------------

#[rustler::nif]
fn engine_new(env: Env) -> Term {
    crate::macros::nif_safe!(env, {
        match engine_new_inner() {
            Ok(arc) => (atoms::ok(), arc).encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

fn engine_new_inner() -> Result<ResourceArc<EngineResource>, wasmtime::Error> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;
    Ok(ResourceArc::new(EngineResource(engine)))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn component_compile<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    wasm_bytes: Binary<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        match Component::new(&engine_arc.0, wasm_bytes.as_slice()) {
            Ok(c) => (atoms::ok(), ResourceArc::new(ComponentResource(c))).encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif]
fn linker_new(env: Env, engine_arc: ResourceArc<EngineResource>) -> Term {
    crate::macros::nif_safe!(env, {
        match linker_new_inner(&engine_arc.0) {
            Ok(arc) => (atoms::ok(), arc).encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

fn linker_new_inner(engine: &Engine) -> Result<ResourceArc<LinkerResource>, wasmtime::Error> {
    let mut linker = Linker::<ActorStoreData>::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    Ok(ResourceArc::new(LinkerResource(linker)))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn component_serialize<'a>(
    env: Env<'a>,
    component_arc: ResourceArc<ComponentResource>,
    hmac_key: Binary<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let key = hmac_key.as_slice();
        if key.len() < HMAC_MIN_KEY_LEN {
            return err_term(env, format!("HMAC key must be at least {} bytes, got {}", HMAC_MIN_KEY_LEN, key.len()));
        }
        match component_arc.0.serialize() {
            Ok(serialized) => {
                let tag = compute_hmac(key, &serialized);
                let total_len = HMAC_TAG_LEN + serialized.len();
                let mut output = NewBinary::new(env, total_len);
                output.as_mut_slice()[..HMAC_TAG_LEN].copy_from_slice(&tag);
                output.as_mut_slice()[HMAC_TAG_LEN..].copy_from_slice(&serialized);
                (atoms::ok(), Binary::from(output)).encode(env)
            }
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn component_deserialize<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    bytes: Binary<'a>,
    hmac_key: Binary<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let key = hmac_key.as_slice();
        if key.len() < HMAC_MIN_KEY_LEN {
            return err_term(env, format!("HMAC key must be at least {} bytes, got {}", HMAC_MIN_KEY_LEN, key.len()));
        }
        let data = bytes.as_slice();
        if data.len() < HMAC_TAG_LEN {
            return err_term(env, "data too short for HMAC tag");
        }
        let (tag, serialized) = data.split_at(HMAC_TAG_LEN);
        if !verify_hmac(key, serialized, tag) {
            return (atoms::error(), atoms::hmac_mismatch()).encode(env);
        }
        // SAFETY: HMAC verified — bytes were produced by our own Component::serialize.
        match unsafe { Component::deserialize(&engine_arc.0, serialized) } {
            Ok(c) => (atoms::ok(), ResourceArc::new(ComponentResource(c))).encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn call_init<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    component_arc: ResourceArc<ComponentResource>,
    linker_arc: ResourceArc<LinkerResource>,
    init_payload: Binary<'a>,
    fuel: u64,
    memory_limit: u64,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        match call_init_inner(&engine_arc.0, &component_arc.0, &linker_arc.0, init_payload.as_slice(), fuel, memory_limit as usize) {
            Ok(result) => encode_handle_result(env, &result),
            Err(e) => classify_wasm_error(env, e),
        }
    })
}

fn call_init_inner(
    engine: &Engine, component: &Component, linker: &Linker<ActorStoreData>,
    payload: &[u8], fuel: u64, mem: usize,
) -> Result<WitHandleResult, wasmtime::Error> {
    let mut store = create_store(engine, fuel, mem);
    let instance = linker.instantiate(&mut store, component)?;
    let idx = get_actor_func_index(component, "init").map_err(wasmtime::Error::msg)?;
    let func = instance.get_typed_func::<(Vec<u8>,), (WitHandleResult,)>(&mut store, &idx)?;
    let (result,) = func.call(&mut store, (payload.to_vec(),))?;
    func.post_return(&mut store)?;
    Ok(result)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn call_handle_message<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    component_arc: ResourceArc<ComponentResource>,
    linker_arc: ResourceArc<LinkerResource>,
    state: Binary<'a>,
    envelope_term: Term<'a>,
    fuel: u64,
    memory_limit: u64,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let envelope = match decode_envelope(envelope_term) {
            Ok(e) => e,
            Err(msg) => return err_term(env, msg),
        };
        match call_handle_message_inner(&engine_arc.0, &component_arc.0, &linker_arc.0, state.as_slice(), envelope, fuel, memory_limit as usize) {
            Ok(result) => encode_handle_result(env, &result),
            Err(e) => classify_wasm_error(env, e),
        }
    })
}

fn call_handle_message_inner(
    engine: &Engine, component: &Component, linker: &Linker<ActorStoreData>,
    state: &[u8], envelope: WitEnvelope, fuel: u64, mem: usize,
) -> Result<WitHandleResult, wasmtime::Error> {
    let mut store = create_store(engine, fuel, mem);
    let instance = linker.instantiate(&mut store, component)?;
    let idx = get_actor_func_index(component, "handle-message").map_err(wasmtime::Error::msg)?;
    let func = instance.get_typed_func::<(Vec<u8>, WitEnvelope), (WitHandleResult,)>(&mut store, &idx)?;
    let (result,) = func.call(&mut store, (state.to_vec(), envelope))?;
    func.post_return(&mut store)?;
    Ok(result)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn call_handle_timer<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    component_arc: ResourceArc<ComponentResource>,
    linker_arc: ResourceArc<LinkerResource>,
    state: Binary<'a>,
    timer_name: String,
    fuel: u64,
    memory_limit: u64,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        match call_handle_timer_inner(&engine_arc.0, &component_arc.0, &linker_arc.0, state.as_slice(), &timer_name, fuel, memory_limit as usize) {
            Ok(result) => encode_handle_result(env, &result),
            Err(e) => classify_wasm_error(env, e),
        }
    })
}

fn call_handle_timer_inner(
    engine: &Engine, component: &Component, linker: &Linker<ActorStoreData>,
    state: &[u8], timer_name: &str, fuel: u64, mem: usize,
) -> Result<WitHandleResult, wasmtime::Error> {
    let mut store = create_store(engine, fuel, mem);
    let instance = linker.instantiate(&mut store, component)?;
    let idx = get_actor_func_index(component, "handle-timer").map_err(wasmtime::Error::msg)?;
    let func = instance.get_typed_func::<(Vec<u8>, String), (WitHandleResult,)>(&mut store, &idx)?;
    let (result,) = func.call(&mut store, (state.to_vec(), timer_name.to_string()))?;
    func.post_return(&mut store)?;
    Ok(result)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn call_migrate<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    component_arc: ResourceArc<ComponentResource>,
    linker_arc: ResourceArc<LinkerResource>,
    state: Binary<'a>,
    from_version: u32,
    fuel: u64,
    memory_limit: u64,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        if get_actor_func_index(&component_arc.0, "migrate").is_err() {
            return (atoms::error(), atoms::not_exported()).encode(env);
        }
        match call_migrate_inner(&engine_arc.0, &component_arc.0, &linker_arc.0, state.as_slice(), from_version, fuel, memory_limit as usize) {
            Ok(result) => encode_handle_result(env, &result),
            Err(e) => classify_wasm_error(env, e),
        }
    })
}

fn call_migrate_inner(
    engine: &Engine, component: &Component, linker: &Linker<ActorStoreData>,
    state: &[u8], from_version: u32, fuel: u64, mem: usize,
) -> Result<WitHandleResult, wasmtime::Error> {
    let mut store = create_store(engine, fuel, mem);
    let instance = linker.instantiate(&mut store, component)?;
    let idx = get_actor_func_index(component, "migrate").map_err(wasmtime::Error::msg)?;
    let func = instance.get_typed_func::<(Vec<u8>, u32), (WitHandleResult,)>(&mut store, &idx)?;
    let (result,) = func.call(&mut store, (state.to_vec(), from_version))?;
    func.post_return(&mut store)?;
    Ok(result)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn call_on_passivate<'a>(
    env: Env<'a>,
    engine_arc: ResourceArc<EngineResource>,
    component_arc: ResourceArc<ComponentResource>,
    linker_arc: ResourceArc<LinkerResource>,
    state: Binary<'a>,
    fuel: u64,
    memory_limit: u64,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        if get_actor_func_index(&component_arc.0, "on-passivate").is_err() {
            return (atoms::error(), atoms::not_exported()).encode(env);
        }
        match call_on_passivate_inner(&engine_arc.0, &component_arc.0, &linker_arc.0, state.as_slice(), fuel, memory_limit as usize) {
            Ok(result_state) => {
                let mut out = NewBinary::new(env, result_state.len());
                out.as_mut_slice().copy_from_slice(&result_state);
                (atoms::ok(), Binary::from(out)).encode(env)
            }
            Err(e) => classify_wasm_error(env, e),
        }
    })
}

fn call_on_passivate_inner(
    engine: &Engine, component: &Component, linker: &Linker<ActorStoreData>,
    state: &[u8], fuel: u64, mem: usize,
) -> Result<Vec<u8>, wasmtime::Error> {
    let mut store = create_store(engine, fuel, mem);
    let instance = linker.instantiate(&mut store, component)?;
    let idx = get_actor_func_index(component, "on-passivate").map_err(wasmtime::Error::msg)?;
    let func = instance.get_typed_func::<(Vec<u8>,), (Vec<u8>,)>(&mut store, &idx)?;
    let (result,) = func.call(&mut store, (state.to_vec(),))?;
    func.post_return(&mut store)?;
    Ok(result)
}
