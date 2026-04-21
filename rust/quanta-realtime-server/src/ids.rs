//! Small shared helpers for building server identifiers — so demo binaries
//! (e.g. `quanta-particle-demo`) don't have to copy this logic.

use std::time::{SystemTime, UNIX_EPOCH};

/// Build a short, process-unique server identifier with the given prefix
/// (e.g. `"srv"` or `"particle"`). Combines a millisecond-resolution wall
/// timestamp with the OS pid; collision probability at reasonable fleet
/// sizes is negligible for dev/demo use.
pub fn generate_server_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    format!("{prefix}-{:08x}{:04x}", (nanos / 1_000_000) as u32, pid as u16)
}
