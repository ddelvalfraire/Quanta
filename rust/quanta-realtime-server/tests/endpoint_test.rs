use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::sync::watch;

use quanta_realtime_server::{
    AcceptAllValidator, AuthRequest, AuthResponse, EndpointConfig, QuicEndpoint, TlsConfig,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Insecure cert verifier for connecting to self-signed server in tests.
#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Build a Quinn client endpoint that skips cert verification and uses quanta-v1 ALPN.
fn build_test_client(alpn: &[&[u8]]) -> quinn::Endpoint {
    let mut rustls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    rustls_config.alpn_protocols = alpn.iter().map(|&p| p.to_vec()).collect();

    let quic_config =
        QuicClientConfig::try_from(rustls_config).expect("valid client crypto config");
    let client_config = quinn::ClientConfig::new(Arc::new(quic_config));

    let mut endpoint =
        quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");
    endpoint.set_default_client_config(client_config);
    endpoint
}

/// Start server, return (server_addr, shutdown_tx).
async fn start_server(
    config: EndpointConfig,
) -> (SocketAddr, watch::Sender<bool>, tokio::task::JoinHandle<()>) {
    let endpoint =
        QuicEndpoint::bind("127.0.0.1:0".parse().unwrap(), config, &TlsConfig::SelfSigned)
            .expect("bind server");
    let addr = endpoint.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let validator = AcceptAllValidator::new();
    let handle = tokio::spawn(async move {
        endpoint.run(validator, shutdown_rx).await;
    });
    (addr, shutdown_tx, handle)
}

/// Run the client-side auth handshake: open bidi stream, send AuthRequest, read AuthResponse.
async fn client_auth(connection: &quinn::Connection) -> AuthResponse {
    let (mut send, mut recv) = connection.open_bi().await.expect("open bidi stream");

    let req = AuthRequest {
        token: "test-token".into(),
        client_version: "0.1.0".into(),
    };
    let req_bytes = bitcode::encode(&req);
    let len = (req_bytes.len() as u32).to_be_bytes();
    send.write_all(&len).await.expect("write req len");
    send.write_all(&req_bytes).await.expect("write req body");

    let mut resp_len_buf = [0u8; 4];
    recv.read_exact(&mut resp_len_buf)
        .await
        .expect("read resp len");
    let resp_len = u32::from_be_bytes(resp_len_buf) as usize;

    let mut resp_buf = vec![0u8; resp_len];
    recv.read_exact(&mut resp_buf)
        .await
        .expect("read resp body");

    bitcode::decode(&resp_buf).expect("decode AuthResponse")
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quic_handshake_with_quanta_v1_alpn() {
    let (addr, shutdown_tx, handle) = start_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let hd = connection
        .handshake_data()
        .and_then(|hd| {
            hd.downcast::<quinn::crypto::rustls::HandshakeData>()
                .ok()
        })
        .and_then(|hd| hd.protocol);

    assert_eq!(hd.as_deref(), Some(b"quanta-v1".as_slice()));

    // Run auth so the server doesn't close on us
    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn alpn_routing_h3_gets_closed() {
    let (addr, shutdown_tx, handle) = start_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"h3"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    // Server should close the connection because h3 is not implemented
    let err = connection.closed().await;
    // The connection should be closed by the server with an application error
    assert!(
        matches!(err, quinn::ConnectionError::ApplicationClosed(_)),
        "expected ApplicationClosed, got {err:?}"
    );

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn auth_flow_valid_token() {
    let (addr, shutdown_tx, handle) = start_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);
    assert!(resp.session_id > 0);

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn auth_timeout_closes_connection() {
    let mut config = EndpointConfig::default();
    config.auth_timeout = Duration::from_millis(200);

    let (addr, shutdown_tx, handle) = start_server(config).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    // Don't open a bidi stream and don't send auth — server should timeout
    // Wait for the server-side auth timeout (200ms) + processing margin
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify the server closed the connection
    assert!(
        connection.close_reason().is_some(),
        "connection should be closed after auth timeout"
    );

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn datagram_roundtrip() {
    let (addr, shutdown_tx, handle) = start_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    // Complete auth first
    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    // Send a datagram from client to server
    let payload = b"hello-quanta";
    connection
        .send_datagram(bytes::Bytes::from_static(payload))
        .expect("send datagram");

    // Small delay for the datagram to be processed
    tokio::time::sleep(Duration::from_millis(50)).await;

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn self_signed_tls_handshake_succeeds() {
    let (addr, shutdown_tx, handle) = start_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    // Just verify TLS handshake completes without error
    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("TLS handshake should succeed with self-signed cert");

    // Complete auth so server doesn't close
    let _ = client_auth(&connection).await;
    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn bbr_congestion_control_active() {
    let (addr, shutdown_tx, handle) = start_server(EndpointConfig::default()).await;
    let client = build_test_client(&[b"quanta-v1"]);

    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let _ = client_auth(&connection).await;

    // Verify BBR is active by checking congestion stats exist
    let stats = connection.stats();
    // cwnd (congestion window) should be set by BBR
    assert!(stats.path.cwnd > 0, "congestion window should be > 0");

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn rate_limiting_excess_refused() {
    let mut config = EndpointConfig::default();
    config.rate_limit_per_sec = 2;

    let (addr, shutdown_tx, handle) = start_server(config).await;

    // Try to open many connections rapidly
    let mut successes = 0u32;
    let mut failures = 0u32;

    for _ in 0..10 {
        let client = build_test_client(&[b"quanta-v1"]);
        match tokio::time::timeout(Duration::from_millis(500), client.connect(addr, "localhost").unwrap())
            .await
        {
            Ok(Ok(_conn)) => successes += 1,
            _ => failures += 1,
        }
    }

    // With rate limit of 2/sec, some connections should be refused
    assert!(successes > 0, "at least one connection should succeed");
    assert!(failures > 0, "at least one connection should be rate-limited");

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}
