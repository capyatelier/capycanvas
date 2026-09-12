#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"
CAPY_CHECK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/capy-project-files.XXXXXX")"
trap 'rm -rf "$CAPY_CHECK_DIR"' EXIT
CAPY_TARGET_DIR="${CARGO_TARGET_DIR:-$CAPY_APP/../../target}"
CAPY_CHECK_EXECUTABLE="$CAPY_CHECK_DIR/check"
# Optional real vector assets for direct SwiftUI captures. This temporary bundle
# has its own identity; tests still inject their storage dependency explicitly.
if [[ -n "${CAPY_TEST_ASSETS_APP:-}" ]]; then
  CAPY_CHECK_BUNDLE="$CAPY_CHECK_DIR/Check.app/Contents"
  mkdir -p "$CAPY_CHECK_BUNDLE/MacOS" "$CAPY_CHECK_BUNDLE/Resources"
  cp "$CAPY_TEST_ASSETS_APP/Contents/Resources/Assets.car" "$CAPY_CHECK_BUNDLE/Resources/Assets.car"
  cat > "$CAPY_CHECK_BUNDLE/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Check</string>
<key>CFBundleIdentifier</key><string>art.capycanvas.component-check</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
  CAPY_CHECK_EXECUTABLE="$CAPY_CHECK_BUNDLE/MacOS/Check"
fi
CAPY_SOURCES=()
while IFS= read -r CAPY_SOURCE; do CAPY_SOURCES+=("$CAPY_SOURCE"); done < <(
  rg --files "$CAPY_APP/Shared" "$CAPY_APP/macOS" -g '*.swift' | rg -v '/Tests/|/CapyCanvasMacApp.swift$'
)
cargo build --manifest-path "$CAPY_APP/../../Cargo.toml" -p layer-apple --target aarch64-apple-darwin
xcrun swiftc -import-objc-header "$CAPY_APP/native/include/CapyApple.h" \
  "${CAPY_SOURCES[@]}" "${1:-$CAPY_APP/tests/project-files.swift}" \
  -L "$CAPY_TARGET_DIR/aarch64-apple-darwin/debug" -llayer_apple -lc++ \
  -framework Metal -framework QuartzCore -framework Security -framework AppKit -framework SwiftUI \
  -o "$CAPY_CHECK_EXECUTABLE"
"$CAPY_CHECK_EXECUTABLE"
