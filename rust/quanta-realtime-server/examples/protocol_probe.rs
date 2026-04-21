//! External protocol probe for `quanta-realtime-server`.
//!
//! Connects to an already-running server as a separate process, negotiates
//! QUIC with the `quanta-v1` ALPN, runs the length-prefixed bitcode auth
//! handshake, prints the decoded `AuthResponse`, and exits 0 on success /
//! non-zero on any failure.
//!
//! Usage:
//!     cargo run -p quanta-realtime-server \
//!         --example protocol_probe --features test-utils -- \
//!         --addr 127.0.0.1:4443 \
//!         [--token qk_rw_dev_devdevdevdevdevdevdevdevdevdevde]
//!
//! This is the external verification counterpart to `wire_endtoend_test.rs`:
//! the test proves the library composes; this probe proves the *binary*
//! serves the protocol across process boundaries.

use std::net::SocketAddr;
use std::time::Duration;

use quinn::VarInt;

use quanta_realtime_server::auth::{AuthRequest, AuthResponse};
use quanta_realtime_server::testing::endpoint_helpers::build_test_client;

const DEFAULT_TOKEN: &str = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

async fn run(addr: SocketAddr, token: String) -> Result<AuthResponse, String> {
    let endpoint = build_test_client(&[b"quanta-v1"]);

    let connecting = endpoint
        .connect(addr, "localhost")
        .map_err(|e| format!("connect: {e}"))?;
    let connection = tokio::time::timeout(Duration::from_secs(5), connecting)
        .await
        .map_err(|_| "handshake timeout after 5s".to_string())?
        .map_err(|e| format!("handshake: {e}"))?;

    let negotiated = connection
        .handshake_data()
        .and_then(|hd| hd.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|hd| hd.protocol)
        .map(|p| String::from_utf8_lossy(&p).into_owned())
        .unwrap_or_else(|| "none".into());
    eprintln!(
        "[probe] connected alpn={negotiated} rtt={:?}",
        connection.rtt()
    );

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|e| format!("open bi: {e}"))?;

    let req = AuthRequest {
        token,
        client_version: CLIENT_VERSION.into(),
        session_token: None,
        transfer_token: None,
    };
    let req_bytes = bitcode::encode(&req);
    let req_len = (req_bytes.len() as u32).to_be_bytes();
    send.write_all(&req_len)
        .await
        .map_err(|e| format!("write len: {e}"))?;
    send.write_all(&req_bytes)
        .await
        .map_err(|e| format!("write body: {e}"))?;
    eprintln!("[probe] sent AuthRequest {} bytes", req_bytes.len());

    let mut resp_len_buf = [0u8; 4];
    recv.read_exact(&mut resp_len_buf)
        .await
        .map_err(|e| format!("read resp len: {e}"))?;
    let resp_len = u32::from_be_bytes(resp_len_buf) as usize;
    if resp_len > 65_536 {
        return Err(format!("response too large: {resp_len} bytes"));
    }
    let mut resp_buf = vec![0u8; resp_len];
    recv.read_exact(&mut resp_buf)
        .await
        .map_err(|e| format!("read resp body: {e}"))?;

    let resp: AuthResponse = bitcode::decode(&resp_buf).map_err(|e| format!("decode resp: {e}"))?;

    connection.close(VarInt::from_u32(0), b"probe done");
    endpoint.wait_idle().await;

    Ok(resp)
}

fn parse_args() -> (SocketAddr, String) {
    let mut addr: Option<SocketAddr> = None;
    let mut token: Option<String> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--addr" => addr = it.next().and_then(|v| v.parse().ok()),
            "--token" => token = it.next(),
            "-h" | "--help" => {
                eprintln!("usage: protocol_probe --addr <host:port> [--token <token>]");
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }
    let addr = addr.unwrap_or_else(|| "127.0.0.1:4443".parse().unwrap());
    let token = token.unwrap_or_else(|| DEFAULT_TOKEN.into());
    (addr, token)
}

#[tokio::main]
async fn main() {
    let (addr, token) = parse_args();
    eprintln!("[probe] connecting to {addr}");

    match run(addr, token).await {
        Ok(resp) => {
            println!(
                "OK accepted={} session_id={} reason={:?}",
                resp.accepted, resp.session_id, resp.reason
            );
            if !resp.accepted {
                std::process::exit(3);
            }
        }
        Err(e) => {
            eprintln!("FAIL {e}");
            std::process::exit(1);
        }
    }
}
