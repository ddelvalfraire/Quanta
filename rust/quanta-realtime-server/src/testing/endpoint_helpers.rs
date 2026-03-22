use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::sync::{mpsc, watch};

use crate::auth::{AuthRequest, AuthResponse, AuthValidator};
use crate::config::EndpointConfig;
use crate::endpoint::QuicEndpoint;
use crate::error::EndpointError;
use crate::reconnect::ConnectedClient;
use crate::session_store::SessionStore;
use crate::tls::TlsConfig;

/// Test-only validator that accepts all connections with auto-incrementing session IDs.
pub struct TestValidator {
    counter: AtomicU64,
}

impl TestValidator {
    pub fn new() -> Arc<Self> {
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

/// Insecure cert verifier for connecting to self-signed server in tests.
#[derive(Debug)]
pub struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    pub fn new() -> Arc<Self> {
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

/// Build a QUIC client endpoint with the given ALPN protocols and no cert verification.
pub fn build_test_client(alpn: &[&[u8]]) -> quinn::Endpoint {
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

/// Start a test server with a `SessionStore`. Returns the server address,
/// a receiver for connected clients, shutdown sender, join handle, and store.
pub async fn start_test_server(
    config: EndpointConfig,
) -> (
    SocketAddr,
    mpsc::Receiver<ConnectedClient>,
    watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<SessionStore>>,
) {
    let store = Arc::new(Mutex::new(SessionStore::new(
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
    let store_clone = store.clone();
    let handle = tokio::spawn(async move {
        endpoint
            .run(validator, session_tx, store_clone, shutdown_rx)
            .await;
    });
    (addr, session_rx, shutdown_tx, handle, store)
}

/// Run the client-side auth handshake with no session token.
pub async fn client_auth(connection: &quinn::Connection) -> AuthResponse {
    client_auth_with_token(connection, None).await
}

/// Run the client-side auth handshake with an optional session token for reconnection.
pub async fn client_auth_with_token(
    connection: &quinn::Connection,
    session_token: Option<u64>,
) -> AuthResponse {
    let (mut send, mut recv) = connection.open_bi().await.expect("open bidi stream");

    let req = AuthRequest {
        token: "test-token".into(),
        client_version: "0.1.0".into(),
        session_token,
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
