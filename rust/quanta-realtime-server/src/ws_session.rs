use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;
use crate::error::SendError;
use crate::session::{Session, TransportType};

/// Frame flag: bit 0 indicates unreliable hint (best-effort delivery).
const FLAG_UNRELIABLE: u8 = 0x01;

/// Static RTT estimate for WebSocket connections.
/// Ping/pong-based measurement is a follow-up.
const WS_RTT_ESTIMATE: Duration = Duration::from_millis(100);

pub struct WsSession {
    outbound_tx: mpsc::Sender<Vec<u8>>,
    datagram_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
}

impl WsSession {
    pub fn new(
        outbound_tx: mpsc::Sender<Vec<u8>>,
        datagram_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            outbound_tx,
            datagram_rx: Mutex::new(datagram_rx),
        }
    }
}

impl Session for WsSession {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError> {
        let mut frame = Vec::with_capacity(1 + data.len());
        frame.push(FLAG_UNRELIABLE);
        frame.extend_from_slice(data);
        self.outbound_tx
            .try_send(frame)
            .map_err(|_| SendError::ConnectionLost("ws outbound channel full".into()))
    }

    fn send_reliable(&self, _stream_id: u32, data: &[u8]) -> Result<(), SendError> {
        let mut frame = Vec::with_capacity(1 + data.len());
        frame.push(0x00); // no flags = reliable
        frame.extend_from_slice(data);
        self.outbound_tx
            .try_send(frame)
            .map_err(|_| SendError::ConnectionLost("ws outbound channel full".into()))
    }

    fn recv_datagram(&self) -> Option<Vec<u8>> {
        self.datagram_rx.lock().ok()?.try_recv().ok()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }

    fn rtt(&self) -> Duration {
        WS_RTT_ESTIMATE
    }

    fn close(&self, _reason: &str) {
        // Dropping the outbound_tx will cause the background write task to end,
        // which in turn closes the WebSocket connection.
    }
}
