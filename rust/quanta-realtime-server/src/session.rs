use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::error::SendError;

/// Transport protocol type for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    Quic,
    WebTransport,
    WebSocket,
}

/// Cross-transport session interface.
///
/// Consumed by T49 (WebTransport), T51 (reconnection), T57 (zone transitions).
pub trait Session: Send + Sync {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError>;

    /// Stub for now — full stream multiplexing belongs to T49.
    fn send_reliable(&self, stream_id: u32, data: &[u8]) -> Result<(), SendError>;

    fn recv_datagram(&self) -> Option<Vec<u8>>;

    fn transport_type(&self) -> TransportType;

    fn rtt(&self) -> Duration;
}

/// QUIC session backed by a `quinn::Connection`.
///
/// A background task drains datagrams from the Quinn connection into an
/// mpsc channel, allowing `recv_datagram` to be sync.
pub struct QuicSession {
    connection: quinn::Connection,
    datagram_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
}

impl QuicSession {
    /// Create a new QuicSession, spawning a background datagram reader task.
    pub fn new(connection: quinn::Connection) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let conn = connection.clone();
        tokio::spawn(async move {
            loop {
                match conn.read_datagram().await {
                    Ok(bytes) => {
                        if tx.send(bytes.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            connection,
            datagram_rx: Mutex::new(rx),
        }
    }
}

impl Session for QuicSession {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError> {
        let max = self
            .connection
            .max_datagram_size()
            .ok_or(SendError::DatagramTooLarge {
                size: data.len(),
                max: 0,
            })?;
        if data.len() > max {
            return Err(SendError::DatagramTooLarge {
                size: data.len(),
                max,
            });
        }
        self.connection
            .send_datagram(bytes::Bytes::copy_from_slice(data))
            .map_err(|e| SendError::ConnectionLost(e.to_string()))
    }

    fn send_reliable(&self, _stream_id: u32, _data: &[u8]) -> Result<(), SendError> {
        // Stub — full stream multiplexing belongs to T49
        Err(SendError::StreamClosed)
    }

    fn recv_datagram(&self) -> Option<Vec<u8>> {
        self.datagram_rx.lock().ok()?.try_recv().ok()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }

    fn rtt(&self) -> Duration {
        self.connection.rtt()
    }
}
