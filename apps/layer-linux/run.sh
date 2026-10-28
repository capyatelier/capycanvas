#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
python3 tools/build/check-rust-version.py
gtk_build=$(realpath -m "${CAPY_GTK_BUILD_DIR:-target/gtk-runtime}")
bash tools/build/gtk-runtime/build.sh "$gtk_build" "$gtk_build/prefix"
export LD_LIBRARY_PATH="$gtk_build/prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
cargo build --locked --profile "${CAPY_RUST_PROFILE:-dev-perf}" -p layer-linux --bin capy-photo-reader
exec cargo run --locked --profile "${CAPY_RUST_PROFILE:-dev-perf}" -p layer-linux -- "$@"
