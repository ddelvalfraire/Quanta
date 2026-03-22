use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use quanta_realtime_server::{
    AuthRequest, AuthResponse, AuthValidator, EndpointConfig, EndpointError, Session, WsListener,
};

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

async fn start_ws_server(
    config: EndpointConfig,
) -> (
    std::net::SocketAddr,
    mpsc::Receiver<Box<dyn Session>>,
    watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let listener = WsListener::bind("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("bind ws");
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (session_tx, session_rx) = mpsc::channel(16);
    let validator = TestValidator::new();
    let handle = tokio::spawn(async move {
        listener.run(validator, session_tx, shutdown_rx).await;
    });
    (addr, session_rx, shutdown_tx, handle)
}

/// Connect as a WS client, send auth, return the (sink, stream) and auth response.
async fn ws_connect_and_auth(
    addr: std::net::SocketAddr,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
    AuthResponse,
) {
    let url = format!("ws://127.0.0.1:{}", addr.port());
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");
    let (mut sink, mut stream) = ws.split();

    // Send auth as raw bitcode binary message (no length prefix).
    let req = AuthRequest {
        token: "test-token".into(),
        client_version: "0.1.0".into(),
        session_token: None,
    };
    let req_bytes = bitcode::encode(&req);
    sink.send(Message::Binary(req_bytes.into()))
        .await
        .expect("send auth");

    // Read auth response.
    let resp_msg = stream.next().await.expect("response").expect("ws read");
    let resp_bytes = match resp_msg {
        Message::Binary(b) => b,
        other => panic!("expected binary, got {other:?}"),
    };
    let resp: AuthResponse = bitcode::decode(&resp_bytes).expect("decode response");

    (sink, stream, resp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_connect_and_auth_succeeds() {
    let (addr, mut session_rx, shutdown_tx, handle) =
        start_ws_server(EndpointConfig::default()).await;

    let (_sink, _stream, resp) = ws_connect_and_auth(addr).await;
    assert!(resp.accepted);
    assert!(resp.session_id > 0);

    let session = tokio::time::timeout(Duration::from_secs(2), session_rx.recv())
        .await
        .expect("session arrives")
        .expect("channel open");
    assert_eq!(
        session.transport_type(),
        quanta_realtime_server::TransportType::WebSocket
    );

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn ws_binary_frame_roundtrip() {
    let (addr, mut session_rx, shutdown_tx, handle) =
        start_ws_server(EndpointConfig::default()).await;

    let (mut sink, mut stream, _resp) = ws_connect_and_auth(addr).await;

    let session = tokio::time::timeout(Duration::from_secs(2), session_rx.recv())
        .await
        .expect("session arrives")
        .expect("channel open");

    // Server sends unreliable data to client via Session trait.
    let payload = b"hello-ws";
    session
        .send_unreliable(payload)
        .expect("send_unreliable");

    // Client should receive a binary frame with [flags:u8][payload].
    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("receive timeout")
        .expect("stream item")
        .expect("ws read");
    match msg {
        Message::Binary(data) => {
            assert_eq!(data[0] & 0x01, 0x01, "unreliable flag should be set");
            assert_eq!(&data[1..], payload);
        }
        other => panic!("expected binary, got {other:?}"),
    }

    // Client sends binary frame → server receives via recv_datagram.
    let client_payload = b"from-client";
    let mut frame = vec![0x01u8]; // unreliable flag
    frame.extend_from_slice(client_payload);
    sink.send(Message::Binary(frame.into()))
        .await
        .expect("send frame");

    let mut received = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if let Some(data) = session.recv_datagram() {
            received = Some(data);
            break;
        }
    }
    assert_eq!(received.as_deref(), Some(client_payload.as_slice()));

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn ws_close_is_graceful() {
    let (addr, mut session_rx, shutdown_tx, handle) =
        start_ws_server(EndpointConfig::default()).await;

    let (mut sink, _stream, _resp) = ws_connect_and_auth(addr).await;

    let _session = tokio::time::timeout(Duration::from_secs(2), session_rx.recv())
        .await
        .expect("session arrives")
        .expect("channel open");

    // Client sends close frame.
    sink.send(Message::Close(None))
        .await
        .expect("send close");

    // Give the background tasks time to notice.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = shutdown_tx.send(true);
    let _ = handle.await;
}
