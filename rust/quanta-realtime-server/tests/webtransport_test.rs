use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use quanta_realtime_server::{
    AuthRequest, AuthResponse, AuthValidator, ConnectedClient, EndpointConfig, EndpointError,
    QuicEndpoint, TlsConfig,
};
use quanta_realtime_server::session_store::SessionStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct TestValidator {
    counter: AtomicU64,
}

impl TestValidator {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            counter: AtomicU64::new(1),
        })
    }
}

impl AuthValidator for TestValidator {
    fn validate(&self, _req: &AuthRequest) -> Result<AuthResponse, EndpointError> {
        let session_id = self.counter.fetch_add(1, Ordering::Relaxed);
        Ok(AuthResponse {
            session_id,
            accepted: true,
            reason: String::new(),
        })
    }
}

/// Build a WT client with no cert verification and an IPv4 endpoint
/// to match the server's IPv4 binding.
fn build_wt_client() -> web_transport_quinn::Client {
    let provider = rustls::crypto::ring::default_provider();

    let crypto = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))))
        .with_no_client_auth();

    let mut crypto = crypto;
    crypto.alpn_protocols = vec![b"h3".to_vec()];

    let quic_config =
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).expect("client config");
    let client_config = quinn::ClientConfig::new(Arc::new(quic_config));

    let endpoint =
        quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");

    web_transport_quinn::Client::new(endpoint, client_config)
}

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
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
        cert: &rustls::pki_types::CertificateDer<'_>,
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

async fn start_server(
    config: EndpointConfig,
) -> (
    SocketAddr,
    mpsc::Receiver<ConnectedClient>,
    watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let store = Arc::new(std::sync::Mutex::new(SessionStore::new(
        config.session_retain_duration,
        config.max_retained_sessions,
    )));
    let endpoint =
        QuicEndpoint::bind("127.0.0.1:0".parse().unwrap(), config, &TlsConfig::SelfSigned)
            .expect("bind server");
    let addr = endpoint.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (session_tx, session_rx) = mpsc::channel(16);
    let validator = TestValidator::new();
    let handle = tokio::spawn(async move {
        endpoint.run(validator, session_tx, store, shutdown_rx).await;
    });
    (addr, session_rx, shutdown_tx, handle)
}

/// Perform the client-side auth handshake over a WebTransport bidi stream.
async fn wt_client_auth(session: &web_transport_quinn::Session) -> AuthResponse {
    let (mut send, mut recv) = session.open_bi().await.expect("open bidi");

    let req = AuthRequest {
        token: "test-token".into(),
        client_version: "0.1.0".into(),
        session_token: None,
        transfer_token: None,
    };
    let req_bytes = bitcode::encode(&req);
    let len = (req_bytes.len() as u32).to_be_bytes();
    send.write_all(&len).await.expect("write len");
    send.write_all(&req_bytes).await.expect("write body");

    let mut resp_len_buf = [0u8; 4];
    recv.read_exact(&mut resp_len_buf)
        .await
        .expect("read resp len");
    let resp_len = u32::from_be_bytes(resp_len_buf) as usize;
    let mut resp_buf = vec![0u8; resp_len];
    recv.read_exact(&mut resp_buf)
        .await
        .expect("read resp body");
    bitcode::decode(&resp_buf).expect("decode resp")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn webtransport_session_establishment() {
    let (addr, mut session_rx, shutdown_tx, handle) =
        start_server(EndpointConfig::default()).await;

    let client = build_wt_client();
    let url: url::Url = format!("https://127.0.0.1:{}/", addr.port())
        .parse()
        .unwrap();
    let wt_session = client.connect(url).await.expect("wt connect");

    let resp = wt_client_auth(&wt_session).await;
    assert!(resp.accepted);

    let connected = tokio::time::timeout(Duration::from_secs(2), session_rx.recv())
        .await
        .expect("client should arrive")
        .expect("channel open");
    assert_eq!(
        connected.session.transport_type(),
        quanta_realtime_server::TransportType::WebTransport,
    );

    wt_session.close(0, b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn webtransport_datagram_roundtrip() {
    let (addr, mut session_rx, shutdown_tx, handle) =
        start_server(EndpointConfig::default()).await;

    let client = build_wt_client();
    let url: url::Url = format!("https://127.0.0.1:{}/", addr.port())
        .parse()
        .unwrap();
    let wt_session = client.connect(url).await.expect("wt connect");

    let _resp = wt_client_auth(&wt_session).await;

    let connected = tokio::time::timeout(Duration::from_secs(2), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");

    // Client sends datagram → server receives it
    let payload = b"hello-wt";
    wt_session
        .send_datagram(bytes::Bytes::from_static(payload))
        .expect("send datagram");

    let mut received = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if let Some(data) = connected.session.recv_datagram() {
            received = Some(data);
            break;
        }
    }
    assert_eq!(received.as_deref(), Some(payload.as_slice()));

    wt_session.close(0, b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}
