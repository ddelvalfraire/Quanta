use quanta_realtime_server::config::ServerConfig;

fn main() {
    // TODO(T45): wire up tokio runtime, NATS, and capacity publisher
    let _config = ServerConfig::default();
    println!("quanta-realtime-server: not yet wired (see T45)");
}
