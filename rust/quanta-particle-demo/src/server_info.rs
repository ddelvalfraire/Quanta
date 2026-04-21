//! On-startup JSON info file for the browser demo.
//!
//! Writes the QUIC listen address, cert SHA-256, and the compiled schema
//! bytes so a Vite dev server can serve them over `/server-info.json`.
//! The cert hash changes every server restart (fresh self-signed cert),
//! so the browser tab must be reloaded after a restart.

use std::net::SocketAddr;
use std::path::Path;

use quanta_core_rs::schema::export::export_schema;
use serde::Serialize;
use tracing::info;

use crate::schema::particle_schema;

#[derive(Serialize)]
pub struct ServerInfo {
    pub quic_addr: String,
    pub cert_sha256_hex: String,
    pub schema_version: u8,
    pub schema_bytes_hex: String,
}

pub fn write_server_info(
    path: &Path,
    quic_addr: SocketAddr,
    cert_sha256: [u8; 32],
) -> std::io::Result<()> {
    let schema = particle_schema();
    let schema_bytes = export_schema(schema);
    let info = ServerInfo {
        quic_addr: quic_addr.to_string(),
        cert_sha256_hex: hex::encode(cert_sha256),
        schema_version: schema.version,
        schema_bytes_hex: hex::encode(&schema_bytes),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(&info).expect("ServerInfo serializes");
    std::fs::write(path, body)?;
    info!(path = %path.display(), "wrote server info");
    Ok(())
}
