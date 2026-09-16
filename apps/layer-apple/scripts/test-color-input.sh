#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
export MACOSX_DEPLOYMENT_TARGET=15.0
CAPY_CHECK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/capy-color-input.XXXXXX")"
trap 'rm -rf "$CAPY_CHECK_DIR"' EXIT
CAPY_TARGET_DIR="${CARGO_TARGET_DIR:-$CAPY_APP/../../target}"
cargo build --locked --manifest-path "$CAPY_APP/../../Cargo.toml" -p layer-apple --target aarch64-apple-darwin
xcrun swiftc -parse-as-library -target arm64-apple-macos15.0 \
  -import-objc-header "$CAPY_APP/native/include/CapyApple.h" \
  "$CAPY_APP/Shared/Bridge/JSON.swift" "$CAPY_APP/Shared/Bridge/ColorUI.swift" \
  "$CAPY_APP/Shared/Editor/ColorSwatch.swift" "$CAPY_APP/Shared/Editor/ColorEditor.swift" \
  "$CAPY_APP/tests/color-editor.swift" \
  -L "$CAPY_TARGET_DIR/aarch64-apple-darwin/debug" -llayer_apple -lc++ \
  -framework Metal -framework QuartzCore -framework Security -framework AppKit -framework SwiftUI \
  -o "$CAPY_CHECK_DIR/check"
"$CAPY_CHECK_DIR/check"
