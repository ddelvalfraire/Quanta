use std::fmt;

#[derive(Debug)]
pub enum EndpointError {
    Tls(String),
    Bind(std::io::Error),
    Quinn(quinn::ConnectionError),
    Auth(String),
    WebTransport(String),
    WebSocket(String),
    Send(SendError),
    Sync(crate::sync::SyncError),
    ZoneTransfer(crate::zone_transfer::TransferError),
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(msg) => write!(f, "TLS error: {msg}"),
            Self::Bind(err) => write!(f, "bind error: {err}"),
            Self::Quinn(err) => write!(f, "QUIC connection error: {err}"),
            Self::Auth(msg) => write!(f, "auth error: {msg}"),
            Self::WebTransport(msg) => write!(f, "WebTransport error: {msg}"),
            Self::WebSocket(msg) => write!(f, "WebSocket error: {msg}"),
            Self::Send(err) => write!(f, "send error: {err}"),
            Self::Sync(err) => write!(f, "sync error: {err}"),
            Self::ZoneTransfer(err) => write!(f, "zone transfer error: {err}"),
        }
    }
}

impl std::error::Error for EndpointError {}

impl From<crate::zone_transfer::TransferError> for EndpointError {
    fn from(err: crate::zone_transfer::TransferError) -> Self {
        Self::ZoneTransfer(err)
    }
}

#[derive(Debug)]
pub enum SendError {
    DatagramTooLarge { size: usize, max: usize },
    ConnectionLost(String),
    StreamClosed,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatagramTooLarge { size, max } => {
                write!(f, "datagram too large: {size} bytes, max {max}")
            }
            Self::ConnectionLost(reason) => write!(f, "connection lost: {reason}"),
            Self::StreamClosed => write!(f, "stream closed"),
        }
    }
}

impl std::error::Error for SendError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_error_display() {
        let err = EndpointError::Tls("bad cert".into());
        assert_eq!(err.to_string(), "TLS error: bad cert");

        let err = EndpointError::Auth("forbidden".into());
        assert_eq!(err.to_string(), "auth error: forbidden");
    }

    #[test]
    fn send_error_display() {
        let err = SendError::DatagramTooLarge { size: 2000, max: 1200 };
        assert_eq!(err.to_string(), "datagram too large: 2000 bytes, max 1200");

        let err = SendError::ConnectionLost("timeout".into());
        assert_eq!(err.to_string(), "connection lost: timeout");

        let err = SendError::StreamClosed;
        assert_eq!(err.to_string(), "stream closed");
    }
}
