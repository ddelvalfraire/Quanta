#[rustler::nif]
fn ping() -> bool {
    true
}

rustler::init!("Elixir.Quanta.Nifs.Native");
