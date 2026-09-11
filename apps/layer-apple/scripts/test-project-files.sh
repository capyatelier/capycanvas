#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
CAPY_CHECK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/capy-project-files.XXXXXX")"
trap 'rm -rf "$CAPY_CHECK_DIR"' EXIT
CAPY_TARGET_DIR="${CARGO_TARGET_DIR:-$CAPY_APP/../../target}"
CAPY_SOURCES=()
while IFS= read -r CAPY_SOURCE; do CAPY_SOURCES+=("$CAPY_SOURCE"); done < <(
  rg --files "$CAPY_APP/Shared" "$CAPY_APP/macOS" -g '*.swift' | rg -v '/Tests/|/CapyCanvasMacApp.swift$'
)
cargo build --manifest-path "$CAPY_APP/../../Cargo.toml" -p layer-apple --target aarch64-apple-darwin
xcrun swiftc -import-objc-header "$CAPY_APP/native/include/CapyApple.h" \
  "${CAPY_SOURCES[@]}" "${1:-$CAPY_APP/tests/project-files.swift}" \
  -L "$CAPY_TARGET_DIR/aarch64-apple-darwin/debug" -llayer_apple -lc++ \
  -framework Metal -framework QuartzCore -framework Security -framework AppKit -framework SwiftUI \
  -o "$CAPY_CHECK_DIR/check"
"$CAPY_CHECK_DIR/check"
