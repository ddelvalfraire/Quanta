use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::EndpointError;

const MAX_AUTH_REQUEST_BYTES: usize = 65_536;

/// The well-known dev token that ships in `main.rs`'s fallback path. Exposed
/// here so the library-level startup guard can recognise it and refuse to
/// run on non-loopback bind addresses (review finding C-3).
pub const DEFAULT_DEV_TOKEN: &str = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct AuthRequest {
    pub token: String,
    pub client_version: String,
    /// For fast reconnect (Tier 2): the session_id from a previous auth.
    /// `None` for first-time connections.
    pub session_token: Option<u64>,
    /// For cross-server zone transfer: a signed `ZoneTransferToken` from the source server.
    pub transfer_token: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct AuthResponse {
    pub session_id: u64,
    pub accepted: bool,
    pub reason: String,
}

pub trait AuthValidator: Send + Sync {
    fn validate(&self, req: &AuthRequest) -> Result<AuthResponse, EndpointError>;

    /// Returns `true` when this validator is configured with the shipped
    /// hardcoded dev token. `run_server` uses this to refuse to start on
    /// non-loopback addresses, preventing an operator from accidentally
    /// exposing the demo credential to the public internet (C-3).
    /// Default: `false` — production validators keep their own secrets.
    fn is_insecure_dev_token(&self) -> bool {
        false
    }
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

/// Production-path validator that accepts requests whose `token` exactly
/// matches the configured dev token. Used for local demo scenarios — NOT
/// suitable for production deployments.
pub struct DevTokenValidator {
    expected_token: String,
}

impl DevTokenValidator {
    pub fn new(expected_token: impl Into<String>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            expected_token: expected_token.into(),
        })
    }
}

/// Draw a fresh 64-bit session id from the operating system's CSPRNG.
///
/// Sequential counters leak the total-session count and let any authenticated
/// client trivially enumerate every other active session id (review finding
/// C-1). `getrandom` pulls from the OS entropy source, so two consecutive
/// session ids have no predictable relationship. The returned value is
/// never zero — zero is the sentinel used for rejected auth responses.
fn fresh_session_id() -> u64 {
    loop {
        let mut buf = [0u8; 8];
        if getrandom::getrandom(&mut buf).is_err() {
            // CSPRNG failure is a platform-level fault. Fall back to a
            // non-zero nanosecond timestamp so the server still makes
            // forward progress rather than panicking.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            if now != 0 {
                return now;
            }
            continue;
        }
        let id = u64::from_le_bytes(buf);
        if id != 0 {
            return id;
        }
    }
}

impl AuthValidator for DevTokenValidator {
    fn validate(&self, req: &AuthRequest) -> Result<AuthResponse, EndpointError> {
        // Constant-time compare to avoid timing oracles even in dev.
        let a = req.token.as_bytes();
        let b = self.expected_token.as_bytes();
        let matches = a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .fold(0u8, |acc, (x, y)| acc | (x ^ y))
                == 0;

        if !matches {
            return Ok(AuthResponse {
                session_id: 0,
                accepted: false,
                reason: "invalid token".into(),
            });
        }
        Ok(AuthResponse {
            session_id: fresh_session_id(),
            accepted: true,
            reason: String::new(),
        })
    }

    fn is_insecure_dev_token(&self) -> bool {
        // Constant-time compare avoids leaking information about partial
        // matches via timing.
        let a = self.expected_token.as_bytes();
        let b = DEFAULT_DEV_TOKEN.as_bytes();
        a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .fold(0u8, |acc, (x, y)| acc | (x ^ y))
                == 0
    }
}

/// Run the auth handshake: read an `AuthRequest`, validate it, send the
/// `AuthResponse`. Returns the response and the client's `session_token`
/// (if any) for reconnection verification.
pub async fn run_auth_handshake(
    send: &mut (impl AsyncWrite + Unpin),
    recv: &mut (impl AsyncRead + Unpin),
    validator: &dyn AuthValidator,
) -> Result<(AuthResponse, Option<u64>), EndpointError> {
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

    let session_token = req.session_token;
    let response = validator.validate(&req)?;

    let resp_bytes = bitcode::encode(&response);
    let resp_len = (resp_bytes.len() as u32).to_be_bytes();
    send.write_all(&resp_len)
        .await
        .map_err(|e| EndpointError::Auth(format!("write response length: {e}")))?;
    send.write_all(&resp_bytes)
        .await
        .map_err(|e| EndpointError::Auth(format!("write response body: {e}")))?;

    Ok((response, session_token))
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
            transfer_token: None,
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
            transfer_token: None,
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
            transfer_token: None,
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
            transfer_token: None,
        };
        let r1 = validator.validate(&req).unwrap();
        let r2 = validator.validate(&req).unwrap();
        assert_eq!(r2.session_id, r1.session_id + 1);
    }

    fn dev_req(token: &str) -> AuthRequest {
        AuthRequest {
            token: token.into(),
            client_version: "0.1.0".into(),
            session_token: None,
            transfer_token: None,
        }
    }

    #[test]
    fn dev_token_validator_accepts_matching_token() {
        let v = DevTokenValidator::new("qk_rw_dev_xxx");
        let r = v.validate(&dev_req("qk_rw_dev_xxx")).unwrap();
        assert!(r.accepted);
        assert!(r.session_id > 0);
    }

    #[test]
    fn dev_token_validator_rejects_wrong_token() {
        let v = DevTokenValidator::new("qk_rw_dev_xxx");
        let r = v.validate(&dev_req("bad")).unwrap();
        assert!(!r.accepted);
        assert_eq!(r.reason, "invalid token");
    }

    #[test]
    fn dev_token_validator_rejects_empty_token() {
        let v = DevTokenValidator::new("qk_rw_dev_xxx");
        let r = v.validate(&dev_req("")).unwrap();
        assert!(!r.accepted);
    }

    #[test]
    fn dev_token_validator_rejects_length_mismatch() {
        let v = DevTokenValidator::new("qk_rw_dev_xxx");
        // one char short
        let r = v.validate(&dev_req("qk_rw_dev_xx")).unwrap();
        assert!(!r.accepted);
    }
}
