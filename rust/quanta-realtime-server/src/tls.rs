use std::sync::Arc;

use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::error::EndpointError;

pub const ALPN_PROTOCOLS: &[&[u8]] = &[b"h3", b"quanta-v1"];

pub enum TlsConfig {
    File { cert_path: String, key_path: String },
    SelfSigned,
}

/// SHA-256 digest of the first (leaf) certificate's DER encoding.
///
/// Browsers pass this value to `WebTransport` via `serverCertificateHashes`
/// to accept a self-signed cert without a CA chain. Intermediates are
/// ignored — the spec hashes only the end-entity certificate.
pub fn compute_cert_sha256(certs: &[CertificateDer<'_>]) -> [u8; 32] {
    let first = certs
        .first()
        .expect("server config always has at least one cert");
    let mut hasher = Sha256::new();
    hasher.update(first.as_ref());
    hasher.finalize().into()
}

pub fn build_server_config(
    tls: &TlsConfig,
    transport: quinn::TransportConfig,
) -> Result<(quinn::ServerConfig, [u8; 32]), EndpointError> {
    let (certs, key) = match tls {
        TlsConfig::File {
            cert_path,
            key_path,
        } => load_pem_files(cert_path, key_path)?,
        TlsConfig::SelfSigned => {
            warn!("using self-signed TLS certificate — not suitable for production");
            generate_self_signed()?
        }
    };

    let cert_sha256 = compute_cert_sha256(&certs);

    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| EndpointError::Tls(e.to_string()))?;

    rustls_config.alpn_protocols = ALPN_PROTOCOLS.iter().map(|&p| p.to_vec()).collect();

    let quic_config =
        QuicServerConfig::try_from(rustls_config).map_err(|e| EndpointError::Tls(e.to_string()))?;

    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));
    server_config.transport_config(Arc::new(transport));

    Ok((server_config, cert_sha256))
}

fn load_pem_files(
    cert_path: &str,
    key_path: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), EndpointError> {
    let cert_data =
        std::fs::read(cert_path).map_err(|e| EndpointError::Tls(format!("read cert: {e}")))?;
    let key_data =
        std::fs::read(key_path).map_err(|e| EndpointError::Tls(format!("read key: {e}")))?;

    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut &cert_data[..])
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| EndpointError::Tls(format!("parse cert PEM: {e}")))?;

    let key = rustls_pemfile::private_key(&mut &key_data[..])
        .map_err(|e| EndpointError::Tls(format!("parse key PEM: {e}")))?
        .ok_or_else(|| EndpointError::Tls("no private key found in PEM file".into()))?;

    Ok((certs, key))
}

pub(crate) fn generate_self_signed(
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), EndpointError> {
    use rcgen::{CertificateParams, DnType, KeyPair, PKCS_ECDSA_P256_SHA256};
    use time::{Duration, OffsetDateTime};

    let mut params = CertificateParams::new(vec!["localhost".into()])
        .map_err(|e| EndpointError::Tls(format!("rcgen params: {e}")))?;
    params
        .distinguished_name
        .push(DnType::CommonName, "quanta-particle-server");
    // WebTransport `serverCertificateHashes` requires validity ≤ 14 days.
    // Start a minute in the past so freshly-generated certs aren't rejected
    // by client clocks with slight skew.
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::minutes(1);
    params.not_after = now + Duration::days(13);

    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .map_err(|e| EndpointError::Tls(format!("rcgen keypair: {e}")))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| EndpointError::Tls(format!("rcgen sign: {e}")))?;

    let cert_der = CertificateDer::from(cert);
    let key_der = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

    Ok((vec![cert_der], key_der))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EndpointConfig;

    #[test]
    fn self_signed_cert_produces_valid_server_config() {
        let transport = EndpointConfig::default().build_transport_config();
        let result = build_server_config(&TlsConfig::SelfSigned, transport);
        assert!(result.is_ok());
        let (_, hash) = result.unwrap();
        // Hash is deterministic length, must be 32 bytes.
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn alpn_protocols_are_set() {
        assert_eq!(ALPN_PROTOCOLS.len(), 2);
        assert_eq!(ALPN_PROTOCOLS[0], b"h3");
        assert_eq!(ALPN_PROTOCOLS[1], b"quanta-v1");
    }

    #[test]
    fn file_tls_with_bad_path_returns_error() {
        let transport = EndpointConfig::default().build_transport_config();
        let result = build_server_config(
            &TlsConfig::File {
                cert_path: "/nonexistent/cert.pem".into(),
                key_path: "/nonexistent/key.pem".into(),
            },
            transport,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("read cert"));
    }

    #[test]
    fn cert_sha256_is_32_bytes() {
        let (certs, _) = generate_self_signed().expect("cert");
        let h = compute_cert_sha256(&certs);
        assert_eq!(h.len(), 32);
    }
}
