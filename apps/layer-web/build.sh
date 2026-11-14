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
layer_profile="${2:-${CAPY_RUST_PROFILE:-dev-perf}}"
cargo build --locked --profile "$layer_profile" -p layer-web-link --target wasm32-unknown-unknown
# Cargo calls the dev/test output directory "debug", not the profile name.
layer_directory="$layer_profile"
if [[ "$layer_profile" == dev || "$layer_profile" == test ]]; then layer_directory=debug; fi
"$layer_bindgen" --target web --out-dir "${1:-apps/layer-web/pkg}" \
  "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/$layer_directory/layer_web.wasm"
python3 tools/build/web-icons.py "$(dirname -- "${1:-apps/layer-web/pkg}")/icons.svg"
layer_launch_path="$(dirname -- "${1:-apps/layer-web/pkg}")/launch.css"
layer_launch_tmp=$(mktemp "${layer_launch_path}.XXXXXX")
trap 'rm -f -- "$layer_launch_tmp"' EXIT
cargo run --locked --quiet --profile "$layer_profile" -p layer-ui --example launch_style > "$layer_launch_tmp"
chmod 644 "$layer_launch_tmp"
mv -- "$layer_launch_tmp" "$layer_launch_path"
trap - EXIT
