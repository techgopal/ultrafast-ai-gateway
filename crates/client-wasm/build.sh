#!/usr/bin/env bash
# Builds the WebAssembly package into <out> (default: crates/client-wasm/pkg).
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version 0.2.129   (the pinned crate version)
# Usage: build.sh [out-dir] [wasm-bindgen target: nodejs|web|bundler]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${1:-$here/pkg}"
target="${2:-nodejs}"
cd "$root"
cargo build -p ultrafast-client-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target "$target" --out-dir "$out" \
  "${CARGO_TARGET_DIR:-$root/target}/wasm32-unknown-unknown/release/ultrafast_client_wasm.wasm"
