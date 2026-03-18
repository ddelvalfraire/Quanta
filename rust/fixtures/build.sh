#!/usr/bin/env bash
# Build WASM test fixtures for quanta_nifs.
#
# Requires: rustup toolchain with wasm32-wasip2 target
#   rustup target add wasm32-wasip2
#
# Usage: bash rust/fixtures/build.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURE_OUT="$SCRIPT_DIR/../../apps/quanta_nifs/test/fixtures"

# Use rustup's rustc to ensure wasm32-wasip2 sysroot is available
RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
RUSTC="$(rustup which rustc --toolchain "$RUSTUP_TOOLCHAIN")"
export RUSTC

echo "Building counter-actor fixture..."
cargo build \
  --manifest-path "$SCRIPT_DIR/counter-actor/Cargo.toml" \
  --target wasm32-wasip2 \
  --release

mkdir -p "$FIXTURE_OUT"
cp "$SCRIPT_DIR/counter-actor/target/wasm32-wasip2/release/counter_actor.wasm" \
   "$FIXTURE_OUT/counter_actor.wasm"

echo "Fixture built: $FIXTURE_OUT/counter_actor.wasm"
wasm-tools component wit "$FIXTURE_OUT/counter_actor.wasm" 2>/dev/null | head -20 || true
