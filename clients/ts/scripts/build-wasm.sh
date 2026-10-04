#!/usr/bin/env bash
# Builds the wasm module into src/wasm/ (git-ignored): the wasm-bindgen `web`
# glue plus inline.ts, the .wasm as base64, so the package loads the same way
# in Node, Bun, Deno, browsers and edge runtimes (no file or URL access).
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version 0.2.129
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
out="$here/src/wasm"
rm -rf "$out"
bash "$here/../../crates/client-wasm/build.sh" "$out" web
node "$here/scripts/inline-wasm.mjs" "$out"
