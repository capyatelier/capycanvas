#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
CAPY_CHECK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/capy-persistence.XXXXXX")"
trap 'rm -rf "$CAPY_CHECK_DIR"' EXIT
CAPY_BRIDGE="$CAPY_APP/Shared/Bridge"
CAPY_TARGET_DIR="${CARGO_TARGET_DIR:-$CAPY_APP/../../target}"

xcrun swiftc "$CAPY_BRIDGE/AtomicJSONFile.swift" "$CAPY_BRIDGE/EditorPersistence.swift" \
  "$CAPY_APP/tests/persistence.swift" -o "$CAPY_CHECK_DIR/files"
"$CAPY_CHECK_DIR/files"

cargo build --manifest-path "$CAPY_APP/../../Cargo.toml" -p layer-apple --target aarch64-apple-darwin
xcrun swiftc -import-objc-header "$CAPY_APP/native/include/CapyApple.h" \
  "$CAPY_BRIDGE/JSON.swift" "$CAPY_BRIDGE/AtomicJSONFile.swift" "$CAPY_BRIDGE/EditorPersistence.swift" \
  "$CAPY_BRIDGE/FrameTrace.swift" "$CAPY_BRIDGE/ObservedMetalLayer.swift" \
  "$CAPY_BRIDGE/LayerImageImport.swift" "$CAPY_BRIDGE/ProjectFileIO.swift" "$CAPY_BRIDGE/NativeOwner.swift" \
  "$CAPY_APP/tests/persistence-owner.swift" \
  -L "$CAPY_TARGET_DIR/aarch64-apple-darwin/debug" -llayer_apple -lc++ \
  -framework Metal -framework QuartzCore -framework Security -o "$CAPY_CHECK_DIR/owner"
"$CAPY_CHECK_DIR/owner"
