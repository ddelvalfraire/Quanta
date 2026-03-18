mod macros;
mod nats;

#[rustler::nif]
fn ping<'a>(env: rustler::Env<'a>) -> rustler::Term<'a> {
    macros::nif_safe!(env, {
        use rustler::Encoder;
        true.encode(env)
    })
}

rustler::init!("Elixir.Quanta.Nifs.Native", load = nats::load);
