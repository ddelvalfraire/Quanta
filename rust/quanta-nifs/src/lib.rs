mod macros;
mod nats;

#[rustler::nif]
fn ping() -> bool {
    true
}

rustler::init!("Elixir.Quanta.Nifs.Native", load = nats::load);
