#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
exec cargo run --locked --profile "${CAPY_RUST_PROFILE:-dev-perf}" -p layer-linux -- "$@"
