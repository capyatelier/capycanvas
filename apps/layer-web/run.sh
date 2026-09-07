#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."

layer_bindgen="${LAYER_WASM_BINDGEN:-}"
if [[ -n "$layer_bindgen" ]]; then
  if ! command -v "$layer_bindgen" >/dev/null; then
    echo "LAYER_WASM_BINDGEN is not executable or on PATH: $layer_bindgen" >&2
    exit 1
  fi
elif command -v wasm-bindgen >/dev/null; then
  layer_bindgen=wasm-bindgen
elif [[ -x "${CARGO_HOME:-$HOME/.cargo}/bin/wasm-bindgen" ]]; then
  layer_bindgen="${CARGO_HOME:-$HOME/.cargo}/bin/wasm-bindgen"
else
  echo 'Install the matching tool: cargo install wasm-bindgen-cli --version 0.2.128 --locked' >&2
  exit 1
fi
cargo build --release -p layer-web --target wasm32-unknown-unknown
"$layer_bindgen" --target web --out-dir apps/layer-web/pkg target/wasm32-unknown-unknown/release/layer_web.wasm
exec python3 -m http.server "${LAYER_WEB_PORT:-4173}" --bind 127.0.0.1 --directory apps/layer-web
