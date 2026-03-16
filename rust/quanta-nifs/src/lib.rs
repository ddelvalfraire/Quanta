mod safety;
mod resources;
mod codec;
mod wasm_runtime;
mod nats_jetstream;
mod loro_engine;

use rustler::Env;

#[rustler::nif]
fn ping() -> bool {
    true
}

fn load(env: Env, _: rustler::Term) -> bool {
    env.register::<resources::EngineResource>().is_ok()
        && env.register::<resources::ComponentResource>().is_ok()
        && env.register::<resources::LinkerResource>().is_ok()
}

rustler::init!("Elixir.Quanta.Nifs.Native", load = load);
