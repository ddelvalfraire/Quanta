use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::SendError;
use crate::session::{Session, TransportType};

/// A mock session for testing that captures all sent data.
pub struct MockSession {
    inner: Mutex<MockSessionInner>,
    transport: TransportType,
    rtt: Duration,
}

struct MockSessionInner {
    sent_unreliable: Vec<Vec<u8>>,
    sent_reliable: Vec<(u32, Vec<u8>)>,
    pending_datagrams: VecDeque<Vec<u8>>,
    closed: bool,
}

impl MockSession {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MockSessionInner {
                sent_unreliable: Vec::new(),
                sent_reliable: Vec::new(),
                pending_datagrams: VecDeque::new(),
                closed: false,
            }),
            transport: TransportType::Quic,
            rtt: Duration::from_millis(20),
        }
    }

    pub fn with_transport(transport: TransportType) -> Self {
        Self {
            transport,
            ..Self::new()
        }
    }

    /// Take all unreliable messages sent so far, clearing the buffer.
    pub fn take_unreliable(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.inner.lock().unwrap().sent_unreliable)
    }

    /// Take all reliable messages sent so far, clearing the buffer.
    pub fn take_reliable(&self) -> Vec<(u32, Vec<u8>)> {
        std::mem::take(&mut self.inner.lock().unwrap().sent_reliable)
    }

    /// Push a datagram to be returned by the next `recv_datagram` call.
    pub fn push_datagram(&self, data: Vec<u8>) {
        self.inner
            .lock()
            .unwrap()
            .pending_datagrams
            .push_back(data);
    }

    /// Check whether `close` has been called.
    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }
}

impl Default for MockSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Session for MockSession {
    fn send_unreliable(&self, data: &[u8]) -> Result<(), SendError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(SendError::ConnectionLost("session closed".into()));
        }
        inner.sent_unreliable.push(data.to_vec());
        Ok(())
    }

    fn send_reliable(&self, stream_id: u32, data: &[u8]) -> Result<(), SendError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(SendError::StreamClosed);
        }
        inner.sent_reliable.push((stream_id, data.to_vec()));
        Ok(())
    }

    fn recv_datagram(&self) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .pending_datagrams
            .pop_front()
    }

    fn transport_type(&self) -> TransportType {
        self.transport
    }

    fn rtt(&self) -> Duration {
        self.rtt
    }

    fn close(&self, _reason: &str) {
        self.inner.lock().unwrap().closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_unreliable_sends() {
        let session = MockSession::new();
        session.send_unreliable(b"hello").unwrap();
        session.send_unreliable(b"world").unwrap();

        let msgs = session.take_unreliable();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0], b"hello");
        assert_eq!(msgs[1], b"world");

        // Buffer is cleared after take
        assert!(session.take_unreliable().is_empty());
    }

    #[test]
    fn captures_reliable_sends() {
        let session = MockSession::new();
        session.send_reliable(1, b"data").unwrap();

        let msgs = session.take_reliable();
        assert_eq!(msgs, vec![(1, b"data".to_vec())]);
    }

    #[test]
    fn datagram_fifo() {
        let session = MockSession::new();
        session.push_datagram(vec![1]);
        session.push_datagram(vec![2]);

        assert_eq!(session.recv_datagram(), Some(vec![1]));
        assert_eq!(session.recv_datagram(), Some(vec![2]));
        assert_eq!(session.recv_datagram(), None);
    }

    #[test]
    fn close_rejects_subsequent_sends() {
        let session = MockSession::new();
        session.close("bye");
        assert!(session.is_closed());
        assert!(session.send_unreliable(b"nope").is_err());
        assert!(session.send_reliable(0, b"nope").is_err());
    }

    #[test]
    fn transport_type_default_is_quic() {
        let session = MockSession::new();
        assert_eq!(session.transport_type(), TransportType::Quic);
    }

    #[test]
    fn with_transport_sets_type() {
        let session = MockSession::with_transport(TransportType::WebSocket);
        assert_eq!(session.transport_type(), TransportType::WebSocket);
    }
}
