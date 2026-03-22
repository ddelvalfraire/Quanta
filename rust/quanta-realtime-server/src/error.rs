use std::fmt;

/// Errors produced by the QUIC endpoint.
#[derive(Debug)]
pub enum EndpointError {
    Tls(String),
    Bind(std::io::Error),
    Quinn(quinn::ConnectionError),
    Auth(String),
    RateLimited,
    DatagramTooLarge { size: usize, max: usize },
    Send(SendError),
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(msg) => write!(f, "TLS error: {msg}"),
            Self::Bind(err) => write!(f, "bind error: {err}"),
            Self::Quinn(err) => write!(f, "QUIC connection error: {err}"),
            Self::Auth(msg) => write!(f, "auth error: {msg}"),
            Self::RateLimited => write!(f, "rate limited"),
            Self::DatagramTooLarge { size, max } => {
                write!(f, "datagram too large: {size} bytes, max {max}")
            }
            Self::Send(err) => write!(f, "send error: {err}"),
        }
    }
}

impl std::error::Error for EndpointError {}

/// Errors produced when sending data on a session.
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

        let err = EndpointError::RateLimited;
        assert_eq!(err.to_string(), "rate limited");

        let err = EndpointError::DatagramTooLarge { size: 2000, max: 1200 };
        assert_eq!(err.to_string(), "datagram too large: 2000 bytes, max 1200");
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
