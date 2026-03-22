//! QUIC realtime transport server for the Quanta platform.
//!
//! Provides the endpoint, session, auth, and TLS plumbing for
//! direct QUIC connections (quanta-v1 ALPN) with WebTransport planned.

pub mod auth;
pub mod config;
pub mod connection;
pub mod endpoint;
pub mod error;
pub mod session;
pub mod tls;

#[cfg(any(test, feature = "test-utils"))]
pub use auth::AcceptAllValidator;
pub use auth::{AuthRequest, AuthResponse, AuthValidator};
pub use config::EndpointConfig;
pub use endpoint::QuicEndpoint;
pub use error::{EndpointError, SendError};
pub use session::{QuicSession, Session, TransportType};
pub use tls::TlsConfig;
