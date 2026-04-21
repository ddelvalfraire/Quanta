//! Load-generator binary for `particle-server`.
//!
//! Usage: `quanta-load --addr IP:PORT [--clients N] [--duration 60s]
//!                      [--ramp 5s] [--input-hz 20]`

use std::net::SocketAddr;
use std::time::Duration;

use quanta_particle_demo::load::{run_load, LoadConfig, DEFAULT_DEV_TOKEN};
use tracing::info;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quanta_particle_demo=info,quanta_load=info".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let mut addr: Option<SocketAddr> = None;
    let mut clients: u32 = 10;
    let mut duration_secs: u64 = 10;
    let mut ramp_secs: u64 = 2;
    let mut input_hz: u16 = 20;
    let mut token: String =
        std::env::var("QUANTA_DEV_TOKEN").unwrap_or_else(|_| DEFAULT_DEV_TOKEN.into());

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => addr = Some(next_arg(&mut args, "--addr")?.parse()?),
            "--clients" => clients = next_arg(&mut args, "--clients")?.parse()?,
            "--duration" => duration_secs = parse_secs(&next_arg(&mut args, "--duration")?)?,
            "--ramp" => ramp_secs = parse_secs(&next_arg(&mut args, "--ramp")?)?,
            "--input-hz" => input_hz = next_arg(&mut args, "--input-hz")?.parse()?,
            "--token" => token = next_arg(&mut args, "--token")?,
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            other => {
                eprintln!("unknown flag: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
    }
    let addr = addr.ok_or("missing required --addr")?;

    let cfg = LoadConfig {
        addr,
        clients,
        duration: Duration::from_secs(duration_secs),
        ramp: Duration::from_secs(ramp_secs),
        input_hz,
        token,
    };
    info!(%addr, clients, duration_secs, ramp_secs, input_hz, "starting load run");

    let t0 = std::time::Instant::now();
    let s = run_load(cfg).await;
    let elapsed = t0.elapsed().as_secs_f64().max(1e-6);
    let ok = s.connects_succeeded.max(1) as f64;

    println!("summary:");
    println!("  attempted:   {}", s.connects_attempted);
    println!(
        "  transport:   {} QUIC connected ({} transport failures)",
        s.transport_ok,
        s.connects_attempted.saturating_sub(s.transport_ok)
    );
    println!(
        "  auth:        {} accepted ({} auth failures)",
        s.connects_succeeded,
        s.transport_ok.saturating_sub(s.connects_succeeded)
    );
    println!(
        "  connects:    {} / {} ({:.1}%)",
        s.connects_succeeded,
        s.connects_attempted,
        100.0 * s.connects_succeeded as f64 / s.connects_attempted.max(1) as f64
    );
    println!("  disconnects: {} mid-run", s.disconnects_midrun);
    println!(
        "  sent:        {} datagrams, {} bytes ({:.1} Kbps/client avg)",
        s.datagrams_sent,
        s.bytes_sent,
        s.bytes_sent as f64 * 8.0 / 1000.0 / elapsed / ok
    );
    println!(
        "  recv:        {} datagrams, {} bytes ({:.1} Kbps/client avg)",
        s.datagrams_received,
        s.bytes_received,
        s.bytes_received as f64 * 8.0 / 1000.0 / elapsed / ok
    );

    Ok(())
}

fn next_arg(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn parse_secs(s: &str) -> Result<u64, std::num::ParseIntError> {
    s.trim_end_matches('s').parse()
}

fn print_usage() {
    eprintln!(
        "usage: quanta-load --addr IP:PORT [--clients N] [--duration 60s] [--ramp 5s] [--input-hz 20] [--token <bearer>]"
    );
}
