use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::error::EndpointError;

/// Generate a self-signed certificate for use in tests.
///
/// Thin wrapper around `tls::generate_self_signed()` — does not duplicate
/// the rcgen logic.
pub fn generate_test_certs() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), EndpointError> {
    let (certs, key) = crate::tls::generate_self_signed()?;
    let cert = certs.into_iter().next().expect("at least one certificate");
    Ok((cert, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_cert_and_key() {
        let (cert, _key) = generate_test_certs().unwrap();
        assert!(!cert.is_empty());
    }
}
