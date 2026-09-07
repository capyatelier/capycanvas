#!/usr/bin/env bash
# Shared Wasm build for the development launcher and static packager.
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
layer_profile="${2:-release}"
cargo build --locked --profile "$layer_profile" -p layer-web --target wasm32-unknown-unknown
"$layer_bindgen" --target web --out-dir "${1:-apps/layer-web/pkg}" \
  "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/$layer_profile/layer_web.wasm"
