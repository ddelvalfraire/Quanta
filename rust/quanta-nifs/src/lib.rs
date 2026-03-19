mod macros;
mod safety;
mod resources;
mod codec;
mod wasm_runtime;
mod nats_jetstream;
mod loro_engine;
mod ephemeral_store;
mod nats;

use rustler::Env;

#[rustler::nif]
fn ping<'a>(env: rustler::Env<'a>) -> rustler::Term<'a> {
    macros::nif_safe!(env, {
        use rustler::Encoder;
        true.encode(env)
    })
}

fn load(env: Env, info: rustler::Term) -> bool {
    env.register::<resources::EngineResource>().is_ok()
        && env.register::<resources::ComponentResource>().is_ok()
        && env.register::<resources::LinkerResource>().is_ok()
        && env.register::<resources::LoroDocResource>().is_ok()
        && env.register::<resources::EphemeralStoreResource>().is_ok()
        && nats::load(env, info)
}

rustler::init!("Elixir.Quanta.Nifs.Native", load = load);
