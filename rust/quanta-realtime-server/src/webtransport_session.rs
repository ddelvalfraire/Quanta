use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::debug;

use crate::error::SendError;
use crate::session::{Session, TransportStats, TransportType};

pub struct WebTransportSession {
    session: web_transport_quinn::Session,
    datagram_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
}

impl WebTransportSession {
    pub fn new(session: web_transport_quinn::Session) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let s = session.clone();
        tokio::spawn(async move {
            loop {
                match s.read_datagram().await {
                    Ok(bytes) => {
                        if tx.send(bytes.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        debug!(error = %e, "webtransport datagram read ended");
                        break;
                    }
                }
            }
        });
        Self {
            session,
            datagram_rx: Mutex::new(rx),
        }
    }
}

impl Session for WebTransportSession {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError> {
        self.session
            .send_datagram(bytes::Bytes::copy_from_slice(data))
            .map_err(|e| SendError::ConnectionLost(e.to_string()))
    }

    fn send_reliable(&self, _stream_id: u32, _data: &[u8]) -> Result<(), SendError> {
        Err(SendError::StreamClosed)
    }

    fn recv_datagram(&self) -> Option<Vec<u8>> {
        self.datagram_rx.lock().ok()?.try_recv().ok()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::WebTransport
    }

    fn rtt(&self) -> Duration {
        self.session.rtt()
    }

    fn transport_stats(&self) -> TransportStats {
        TransportStats::default()
    }

    fn close(&self, reason: &str) {
        self.session.close(0, reason.as_bytes());
    }
}
