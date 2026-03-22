use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::sync::{mpsc, watch};

use quanta_realtime_server::reconnect::ReconnectTier;
use quanta_realtime_server::session_store::{RetainedSession, SessionStore};
use quanta_realtime_server::sync::{
    recv_initial_state_stream, send_initial_state_stream, EntityPayload, InitialStateMessage,
};
use quanta_realtime_server::types::EntitySlot;
use quanta_realtime_server::{
    AuthRequest, AuthResponse, AuthValidator, ConnectedClient, EndpointConfig, EndpointError,
    QuicEndpoint, TlsConfig,
};

// ---------------------------------------------------------------------------
// Helpers (duplicated from endpoint_test.rs — minimal subset)
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

async fn start_server(
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

async fn client_auth(connection: &quinn::Connection) -> AuthResponse {
    let (mut send, mut recv) = connection.open_bi().await.expect("open bidi stream");

    let req = AuthRequest {
        token: "test-token".into(),
        client_version: "0.1.0".into(),
        session_token: None,
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

fn make_entities(count: usize) -> Vec<EntityPayload> {
    (0..count)
        .map(|i| EntityPayload {
            entity_slot: i as u32,
            state: vec![(i & 0xFF) as u8; 8],
        })
        .collect()
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bulk_transfer_200_entities() {
    let (addr, mut session_rx, shutdown_tx, handle, _store) =
        start_server(EndpointConfig::default()).await;

    let client_endpoint = build_test_client(&[b"quanta-v1"]);
    let connection = client_endpoint
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");

    let resp = client_auth(&connection).await;
    assert!(resp.accepted);

    // Wait for the server-side ConnectedClient.
    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client should arrive")
        .expect("channel should not be closed");
    let server_conn = connected.quic_connection.expect("should have quic_connection");

    // Server sends initial state, client receives it — run concurrently.
    let msg = InitialStateMessage {
        baseline_tick: 42,
        flags: 0,
        schema_version: 1,
        compiled_schema: None,
        entities: make_entities(200),
    };

    let msg_clone = msg.clone();
    let timeout = Duration::from_secs(5);

    let (server_result, client_result) = tokio::join!(
        send_initial_state_stream(&server_conn, &msg_clone, timeout),
        recv_initial_state_stream(&connection, timeout),
    );

    let ack = server_result.expect("server should get ack");
    assert_eq!(ack.baseline_tick, 42);

    let received_msg = client_result.expect("client should receive message");
    assert_eq!(received_msg, msg);

    connection.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn fast_reconnect_with_retained_session() {
    let config = EndpointConfig::default();
    let (addr, mut session_rx, shutdown_tx, handle, store) = start_server(config).await;

    // First connection.
    let client = build_test_client(&[b"quanta-v1"]);
    let connection = client
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp = client_auth(&connection).await;
    assert!(resp.accepted);
    let session_id = resp.session_id;

    let connected = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");
    assert!(matches!(connected.reconnect_tier, ReconnectTier::Cold));

    // Simulate disconnect: insert a retained session for this session_id.
    {
        let mut s = store.lock().unwrap();
        s.insert(
            session_id,
            RetainedSession {
                baseline_tick: 500,
                visible_entities: vec![EntitySlot(0), EntitySlot(1)],
                input_seq: 10,
                session_token: session_id,
                created_at: tokio::time::Instant::now(),
            },
        );
    }

    connection.close(0u32.into(), b"disconnect");

    // Reconnect — the new connection gets the same session_id from the validator.
    // We need a validator that returns the same session_id. Since TestValidator
    // auto-increments, the new session_id won't match. For this test, we
    // manually insert a retained session for the next expected session_id.
    let next_session_id = session_id + 1;
    {
        let mut s = store.lock().unwrap();
        s.insert(
            next_session_id,
            RetainedSession {
                baseline_tick: 500,
                visible_entities: vec![EntitySlot(0), EntitySlot(1)],
                input_seq: 10,
                session_token: next_session_id,
                created_at: tokio::time::Instant::now(),
            },
        );
    }

    let client2 = build_test_client(&[b"quanta-v1"]);
    let connection2 = client2
        .connect(addr, "localhost")
        .expect("connect")
        .await
        .expect("handshake");
    let resp2 = client_auth(&connection2).await;
    assert!(resp2.accepted);
    assert_eq!(resp2.session_id, next_session_id);

    let connected2 = tokio::time::timeout(Duration::from_secs(1), session_rx.recv())
        .await
        .expect("client")
        .expect("channel");

    match connected2.reconnect_tier {
        ReconnectTier::Fast { retained } => {
            assert_eq!(retained.baseline_tick, 500);
            assert_eq!(retained.input_seq, 10);
        }
        ReconnectTier::Cold => panic!("expected Fast reconnect tier"),
    }

    connection2.close(0u32.into(), b"done");
    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}
