mod safety;
mod resources;
mod bridge_codec;
mod codec;
mod wasm_runtime;
mod nats_jetstream;
mod loro_engine;
mod ephemeral_store;
mod nats;
mod schema_compiler;
mod delta_encoder;
mod js_executor;

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
