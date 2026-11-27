#!/bin/sh
# Builds the module for wasm and the harness natively, then runs the harness (see docs/history/spikes/wasm/wasm-generator-spike.md).
# The build folder is outside the repository: set TARGET to a scratch folder (default /e/s/vx/wasm-spike-target).
set -e
cd "$(dirname "$0")"
TARGET="${TARGET:-/e/s/vx/wasm-spike-target}"
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown -p generator_module --target-dir "$TARGET"
cargo build --release -p harness --target-dir "$TARGET"
"$TARGET/release/harness" "$TARGET/wasm32-unknown-unknown/release/generator_module.wasm" "${1:-2}"
