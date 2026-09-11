#!/bin/bash
# Direct SwiftUI component capture using the real app's compiled vector assets.
set -euo pipefail
if [[ $# != 3 ]]; then
  echo 'Usage: capture-toolbar.sh FIXTURE.json OUTPUT.png BUILT_MAC_APP' >&2
  exit 2
fi
apple_sources="$(cd "$(dirname "$0")/.." && pwd)"
capture_root="$(mktemp -d "${TMPDIR:-/tmp}/capy-toolbar.XXXXXX")"
trap 'rm -rf "$capture_root"' EXIT
capture_bundle="$capture_root/Capture.app/Contents"
mkdir -p "$capture_bundle/MacOS" "$capture_bundle/Resources"
cp "$3/Contents/Resources/Assets.car" "$capture_bundle/Resources/Assets.car"
cat > "$capture_bundle/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Capture</string>
<key>CFBundleIdentifier</key><string>art.capycanvas.toolbar-capture</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
xcrun swiftc -parse-as-library -module-cache-path "$capture_root/modules" \
  "$apple_sources/Shared/Bridge/JSON.swift" \
  "$apple_sources/Shared/Editor/EditorStyle.swift" \
  "$apple_sources/Shared/Editor/ColorSwatch.swift" \
  "$apple_sources/Shared/Editor/ToolbarTileContent.swift" \
  "$apple_sources/tests/toolbar-capture.swift" -o "$capture_bundle/MacOS/Capture"
"$capture_bundle/MacOS/Capture" "$1" "$2"
