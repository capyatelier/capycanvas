#!/bin/bash
set -euo pipefail
# A separate native source lets XCTest exercise cross-application drag delivery.
# This belongs only to the Mac UI-test build, never either shipping app.
CAPY_TEST_APP="$BUILT_PRODUCTS_DIR/PhotoDragSource.app"
mkdir -p "$CAPY_TEST_APP/Contents/MacOS"
cat > "$CAPY_TEST_APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>PhotoDragSource</string>
<key>CFBundleIdentifier</key><string>art.capycanvas.tests.photosource</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
xcrun swiftc -parse-as-library "$SRCROOT/tests/photo-drag-source.swift" \
  -framework AppKit -o "$CAPY_TEST_APP/Contents/MacOS/PhotoDragSource"
codesign --force --sign - "$CAPY_TEST_APP"
