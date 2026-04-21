use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::error::SendError;
use crate::session::{Session, TransportStats, TransportType};

pub const FLAG_RELIABLE: u8 = 0x00;
pub const FLAG_UNRELIABLE: u8 = 0x01;

/// Static RTT estimate for WebSocket connections.
const WS_RTT_ESTIMATE: Duration = Duration::from_millis(100);

/// Minimum valid frame size: 1-byte flags header + at least 1 byte of payload.
pub const MIN_FRAME_LEN: usize = 2;

pub fn encode_frame(flags: u8, data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(1 + data.len());
    frame.push(flags);
    frame.extend_from_slice(data);
    frame
}

pub fn decode_frame(data: &[u8]) -> Option<&[u8]> {
    if data.len() >= MIN_FRAME_LEN {
        Some(&data[1..])
    } else {
        None
    }
}

pub struct WsSession {
    outbound_tx: mpsc::Sender<Vec<u8>>,
    datagram_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
}

impl WsSession {
    pub fn new(outbound_tx: mpsc::Sender<Vec<u8>>, datagram_rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            outbound_tx,
            datagram_rx: Mutex::new(datagram_rx),
        }
    }
}

impl Session for WsSession {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError> {
        self.outbound_tx
            .try_send(encode_frame(FLAG_UNRELIABLE, data))
            .map_err(|_| SendError::ConnectionLost("ws outbound channel full".into()))
    }

    fn send_reliable(&self, _stream_id: u32, data: &[u8]) -> Result<(), SendError> {
        self.outbound_tx
            .try_send(encode_frame(FLAG_RELIABLE, data))
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

    fn transport_stats(&self) -> TransportStats {
        TransportStats::default()
    }

    /// Sends a shutdown sentinel to the background write task, which closes the socket.
    fn close(&self, _reason: &str) {
        let _ = self.outbound_tx.try_send(Vec::new());
    }
}
