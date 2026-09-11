#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
python3 "$CAPY_APP/scripts/prepare.py"
python3 "$CAPY_APP/scripts/project.py"
CAPY_PLATFORM="${1:-simulator}"
CAPY_OPTIONS=(-quiet)
case "$CAPY_PLATFORM" in
  simulator) CAPY_DEST='generic/platform=iOS Simulator'; CAPY_SCHEME=CapyCanvas-iPad; CAPY_OPTIONS+=(CODE_SIGNING_ALLOWED=NO) ;;
  device)
    : "${CAPY_APPLE_TEAM:?Set CAPY_APPLE_TEAM to your Apple signing Team ID}"
    CAPY_DEST="${CAPY_DESTINATION:-generic/platform=iOS}"
    CAPY_SCHEME=CapyCanvas-iPad
    CAPY_OPTIONS+=(-allowProvisioningUpdates -allowProvisioningDeviceRegistration "DEVELOPMENT_TEAM=$CAPY_APPLE_TEAM")
    ;;
  macos) CAPY_DEST='platform=macOS,arch=arm64'; CAPY_SCHEME=CapyCanvas-Mac; CAPY_OPTIONS+=(CODE_SIGNING_ALLOWED=NO) ;;
  *) echo 'Usage: build.sh [simulator|device|macos]' >&2; exit 1 ;;
esac
exec xcodebuild -project "$CAPY_APP/CapyCanvas.xcodeproj" -scheme "$CAPY_SCHEME" \
  -configuration "${CAPY_CONFIGURATION:-Debug}" -destination "$CAPY_DEST" \
  -derivedDataPath "${CAPY_DERIVED_DATA:-$CAPY_APP/DerivedData}" "${CAPY_OPTIONS[@]}" build
