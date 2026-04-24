mod bridge_codec;
mod codec;
mod delta_encoder;
mod ephemeral_store;
mod js_executor;
mod loro_engine;
mod nats;
mod nats_jetstream;
mod resources;
mod safety;
mod schema_compiler;
mod wasm_runtime;

use rustler::Env;

#[rustler::nif]
fn ping<'a>(env: rustler::Env<'a>) -> rustler::Term<'a> {
    safety::nif_safe!(env, {
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
        && env.register::<resources::CompiledSchemaResource>().is_ok()
        && nats::load(env, info)
}

rustler::init!("Elixir.Quanta.Nifs.Native", load = load);
