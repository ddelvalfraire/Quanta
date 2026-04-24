use rustler::{Binary, Encoder, Env, NewBinary, Term};
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Trap};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        fuel_exhausted,
        memory_exceeded,
    }
}

const STDOUT_CAPACITY: usize = 65_536;

fn classify_error<'a>(env: Env<'a>, err: wasmtime::Error) -> Term<'a> {
    if let Some(t) = err.downcast_ref::<Trap>() {
        if matches!(*t, Trap::OutOfFuel) {
            return (atoms::error(), atoms::fuel_exhausted()).encode(env);
        }
    }
    let msg = format!("{:#}", err);
    if msg.contains("memory") || msg.contains("grow") {
        (atoms::error(), atoms::memory_exceeded()).encode(env)
    } else {
        (atoms::error(), msg).encode(env)
    }
}

fn execute_js_inner(
    wasm_bytes: &[u8],
    js_source: &[u8],
    fuel: u64,
    memory_limit: usize,
) -> Result<(String, String), wasmtime::Error> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let module = Module::new(&engine, wasm_bytes)?;

    let stdout_pipe = MemoryOutputPipe::new(STDOUT_CAPACITY);
    let stderr_pipe = MemoryOutputPipe::new(STDOUT_CAPACITY);
    let stdin_pipe = MemoryInputPipe::new(js_source.to_vec());

    let stdout_reader = stdout_pipe.clone();
    let stderr_reader = stderr_pipe.clone();

    let limits = StoreLimitsBuilder::new()
        .memory_size(memory_limit)
        .trap_on_grow_failure(true)
        .build();

    let wasi = WasiCtxBuilder::new()
        .stdin(stdin_pipe)
        .stdout(stdout_pipe)
        .stderr(stderr_pipe)
        .build_p1();

    let mut store = Store::new(&engine, (wasi, limits));
    store.limiter(|data| &mut data.1);
    store.set_fuel(fuel)?;

    let mut linker: Linker<(WasiP1Ctx, StoreLimits)> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |data| &mut data.0)?;

    let instance = linker.instantiate(&mut store, &module)?;

    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .or_else(|_| instance.get_typed_func::<(), ()>(&mut store, ""))
        .map_err(|_| wasmtime::Error::msg("no _start export found"))?;

    match start.call(&mut store, ()) {
        Ok(()) => {}
        Err(e) => {
            // If there's stdout content, the JS ran partially — still capture it
            let out = String::from_utf8_lossy(&stdout_reader.contents()).into_owned();
            let err_out = String::from_utf8_lossy(&stderr_reader.contents()).into_owned();
            if !out.is_empty() || !err_out.is_empty() {
                // JS runtime error (e.g. uncaught exception) — stderr has the error
                let combined_err = if err_out.is_empty() {
                    format!("{:#}", e)
                } else {
                    err_out
                };
                return Ok((out, combined_err));
            }
            return Err(e);
        }
    }

    let stdout = String::from_utf8_lossy(&stdout_reader.contents()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_reader.contents()).into_owned();

    Ok((stdout, stderr))
}

#[rustler::nif(schedule = "DirtyCpu")]
pub fn execute_js<'a>(
    env: Env<'a>,
    wasm_bytes: Binary<'a>,
    js_source: Binary<'a>,
    fuel: u64,
    memory_limit: u64,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        match execute_js_inner(
            wasm_bytes.as_slice(),
            js_source.as_slice(),
            fuel,
            memory_limit as usize,
        ) {
            Ok((stdout, stderr)) => {
                let mut stdout_bin = NewBinary::new(env, stdout.len());
                stdout_bin.as_mut_slice().copy_from_slice(stdout.as_bytes());
                let mut stderr_bin = NewBinary::new(env, stderr.len());
                stderr_bin.as_mut_slice().copy_from_slice(stderr.as_bytes());
                (
                    atoms::ok(),
                    Binary::from(stdout_bin),
                    Binary::from(stderr_bin),
                )
                    .encode(env)
            }
            Err(e) => classify_error(env, e),
        }
    })
}
