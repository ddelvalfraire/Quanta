use std::sync::Arc;
use std::time::Duration;

use tokio::time::timeout;

use crate::error::EndpointError;

/// Client auth request sent on the first bidi stream.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct AuthRequest {
    pub token: String,
    pub client_version: String,
}

/// Server auth response.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct AuthResponse {
    pub session_id: u64,
    pub accepted: bool,
    pub reason: String,
}

/// Pluggable auth validator.
pub trait AuthValidator: Send + Sync {
    fn validate(&self, req: &AuthRequest) -> Result<AuthResponse, EndpointError>;
}

/// Test validator that accepts all connections.
pub struct AcceptAllValidator {
    counter: std::sync::atomic::AtomicU64,
}

impl AcceptAllValidator {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            counter: std::sync::atomic::AtomicU64::new(1),
        })
    }
}

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

/// Run the auth handshake on a bidi stream.
///
/// Protocol: read `[len:4 BE][bitcode AuthRequest]`, validate,
/// write `[len:4 BE][bitcode AuthResponse]`.
pub async fn run_auth_handshake(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    validator: &dyn AuthValidator,
    auth_timeout: Duration,
) -> Result<AuthResponse, EndpointError> {
    timeout(auth_timeout, async {
        // Read length-prefixed AuthRequest
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| EndpointError::Auth(format!("read request length: {e}")))?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > 65_536 {
            return Err(EndpointError::Auth(format!(
                "request too large: {len} bytes"
            )));
        }

        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf)
            .await
            .map_err(|e| EndpointError::Auth(format!("read request body: {e}")))?;

        let req: AuthRequest =
            bitcode::decode(&buf).map_err(|e| EndpointError::Auth(format!("decode request: {e}")))?;

        // Validate
        let response = validator.validate(&req)?;

        // Write length-prefixed AuthResponse
        let resp_bytes = bitcode::encode(&response);
        let resp_len = (resp_bytes.len() as u32).to_be_bytes();
        send.write_all(&resp_len)
            .await
            .map_err(|e| EndpointError::Auth(format!("write response length: {e}")))?;
        send.write_all(&resp_bytes)
            .await
            .map_err(|e| EndpointError::Auth(format!("write response body: {e}")))?;

        Ok(response)
    })
    .await
    .map_err(|_| EndpointError::Auth("auth timeout".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcode_roundtrip_auth_request() {
        let req = AuthRequest {
            token: "test-token-123".into(),
            client_version: "0.1.0".into(),
        };
        let bytes = bitcode::encode(&req);
        let decoded: AuthRequest = bitcode::decode(&bytes).unwrap();
        assert_eq!(req, decoded);
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
        };
        let r1 = validator.validate(&req).unwrap();
        let r2 = validator.validate(&req).unwrap();
        assert_eq!(r2.session_id, r1.session_id + 1);
    }
}
