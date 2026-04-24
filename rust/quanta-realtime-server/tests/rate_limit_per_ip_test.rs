//! Regression guard for M-5: the endpoint-level rate limiter must be keyed
//! on source IP. Previously a single `governor::DefaultDirectRateLimiter`
//! was shared across every inbound handshake, so one fast client could
//! exhaust the budget for every other client. With a keyed limiter, each
//! IP has its own bucket and noisy neighbours stay confined.
//!
//! This test exercises the per-IP check API directly so the assertion
//! doesn't depend on socket scheduling. It fails while the limiter is
//! global: IP B is refused after IP A exhausts the shared budget.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use quanta_realtime_server::endpoint::QuicEndpoint;
use quanta_realtime_server::tls::TlsConfig;
use quanta_realtime_server::EndpointConfig;

#[tokio::test]
async fn rate_limiter_is_keyed_on_source_ip() {
    let mut config = EndpointConfig::default();
    // Small burst budget so a single noisy IP hits the ceiling quickly.
    config.rate_limit_per_sec = 4;

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let endpoint =
        QuicEndpoint::bind(addr, config, &TlsConfig::SelfSigned).expect("endpoint binds");

    let ip_a: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    let ip_b: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

    // Exhaust IP A's bucket well past the per-second budget. A *direct*
    // (global) limiter would also reject IP B after this loop; a keyed
    // limiter must keep IP B's bucket untouched.
    for _ in 0..32 {
        let _ = endpoint.check_ip(ip_a);
    }

    let allowed_b = endpoint.check_ip(ip_b);
    assert!(
        allowed_b,
        "per-IP rate limiter: IP B must still be allowed after IP A exhausts its own bucket"
    );
}
