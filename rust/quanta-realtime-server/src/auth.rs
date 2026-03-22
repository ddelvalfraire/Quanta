use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::EndpointError;

const MAX_AUTH_REQUEST_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct AuthRequest {
    pub token: String,
    pub client_version: String,
    /// For fast reconnect (Tier 2): the session_id from a previous auth.
    /// `None` for first-time connections.
    pub session_token: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct AuthResponse {
    pub session_id: u64,
    pub accepted: bool,
    pub reason: String,
}

pub trait AuthValidator: Send + Sync {
    fn validate(&self, req: &AuthRequest) -> Result<AuthResponse, EndpointError>;
}

#[cfg(any(test, feature = "test-utils"))]
pub struct AcceptAllValidator {
    counter: std::sync::atomic::AtomicU64,
}

#[cfg(any(test, feature = "test-utils"))]
impl AcceptAllValidator {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            counter: std::sync::atomic::AtomicU64::new(1),
        })
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl AuthValidator for AcceptAllValidator {
    fn validate(&self, _req: &AuthRequest) -> Result<AuthResponse, EndpointError> {
        let session_id = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(AuthResponse {
            session_id,
            accepted: true,
            reason: String::new(),
        })
    }
}

pub async fn run_auth_handshake(
    send: &mut (impl AsyncWrite + Unpin),
    recv: &mut (impl AsyncRead + Unpin),
    validator: &dyn AuthValidator,
) -> Result<AuthResponse, EndpointError> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| EndpointError::Auth(format!("read request length: {e}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_AUTH_REQUEST_BYTES {
        return Err(EndpointError::Auth(format!(
            "request too large: {len} bytes, max {MAX_AUTH_REQUEST_BYTES}"
        )));
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| EndpointError::Auth(format!("read request body: {e}")))?;

    let req: AuthRequest =
        bitcode::decode(&buf).map_err(|e| EndpointError::Auth(format!("decode request: {e}")))?;

    let response = validator.validate(&req)?;

    let resp_bytes = bitcode::encode(&response);
    let resp_len = (resp_bytes.len() as u32).to_be_bytes();
    send.write_all(&resp_len)
        .await
        .map_err(|e| EndpointError::Auth(format!("write response length: {e}")))?;
    send.write_all(&resp_bytes)
        .await
        .map_err(|e| EndpointError::Auth(format!("write response body: {e}")))?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcode_roundtrip_auth_request() {
        let req = AuthRequest {
            token: "test-token-123".into(),
            client_version: "0.1.0".into(),
            session_token: None,
        };
        let bytes = bitcode::encode(&req);
        let decoded: AuthRequest = bitcode::decode(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn bitcode_roundtrip_auth_request_with_session_token() {
        let req = AuthRequest {
            token: "reconnect".into(),
            client_version: "0.1.0".into(),
            session_token: Some(42),
        };
        let bytes = bitcode::encode(&req);
        let decoded: AuthRequest = bitcode::decode(&bytes).unwrap();
        assert_eq!(req, decoded);
        assert_eq!(decoded.session_token, Some(42));
    }

    #[test]
    fn bitcode_roundtrip_auth_response() {
        let resp = AuthResponse {
            session_id: 42,
            accepted: true,
            reason: String::new(),
        };
        let bytes = bitcode::encode(&resp);
        let decoded: AuthResponse = bitcode::decode(&bytes).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn accept_all_validator_returns_ok() {
        let validator = AcceptAllValidator::new();
        let req = AuthRequest {
            token: "anything".into(),
            client_version: "0.0.1".into(),
            session_token: None,
        };
        let resp = validator.validate(&req).unwrap();
        assert!(resp.accepted);
        assert!(resp.session_id > 0);
    }

    #[test]
    fn accept_all_validator_increments_session_id() {
        let validator = AcceptAllValidator::new();
        let req = AuthRequest {
            token: "t".into(),
            client_version: "v".into(),
            session_token: None,
        };
        let r1 = validator.validate(&req).unwrap();
        let r2 = validator.validate(&req).unwrap();
        assert_eq!(r2.session_id, r1.session_id + 1);
    }
}
