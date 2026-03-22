use std::sync::Arc;

use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::error::EndpointError;

/// ALPN protocols supported by the server.
pub const ALPN_PROTOCOLS: &[&[u8]] = &[b"h3", b"quanta-v1"];

/// TLS certificate source.
pub enum TlsConfig {
    /// Load cert and key from PEM files.
    File {
        cert_path: String,
        key_path: String,
    },
    /// Generate a self-signed certificate (dev/test only).
    SelfSigned,
}

/// Build a Quinn `ServerConfig` from TLS config and transport config.
pub fn build_server_config(
    tls: &TlsConfig,
    transport: quinn::TransportConfig,
) -> Result<quinn::ServerConfig, EndpointError> {
    let (certs, key) = match tls {
        TlsConfig::File {
            cert_path,
            key_path,
        } => load_pem_files(cert_path, key_path)?,
        TlsConfig::SelfSigned => generate_self_signed()?,
    };

    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| EndpointError::Tls(e.to_string()))?;

    rustls_config.alpn_protocols = ALPN_PROTOCOLS.iter().map(|&p| p.to_vec()).collect();
    rustls_config.max_early_data_size = u32::MAX; // Enable 0-RTT

    let quic_config = QuicServerConfig::try_from(rustls_config)
        .map_err(|e| EndpointError::Tls(e.to_string()))?;

    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));
    server_config.transport_config(Arc::new(transport));

    Ok(server_config)
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

/// Generate a self-signed certificate for localhost (dev/test).
fn generate_self_signed(
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), EndpointError> {
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .map_err(|e| EndpointError::Tls(format!("rcgen: {e}")))?;

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
        let server_config = build_server_config(&TlsConfig::SelfSigned, transport);
        assert!(server_config.is_ok());
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
}
