#!/bin/bash
# Without CAPY_APPLE_TEAM this checks an unsigned archive. With it, the iPad build
# is cloud-signed and uploaded with the App Store Connect API key, and the Mac app
# is signed with the Developer ID certificate in the keychain and notarized.
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
python3 "$CAPY_APP/scripts/prepare.py"
python3 "$CAPY_APP/scripts/project.py"
case "${1:-}" in
  ipad) CAPY_SCHEME=CapyCanvas-iPad; CAPY_DEST='generic/platform=iOS' ;;
  mac) CAPY_SCHEME=CapyCanvas-Mac; CAPY_DEST='generic/platform=macOS' ;;
  *) echo 'Usage: release.sh ipad|mac' >&2; exit 1 ;;
esac
CAPY_VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$CAPY_APP/../../Cargo.toml")
CAPY_OUT="$CAPY_APP/../../dist/apple-$1"
rm -rf "$CAPY_OUT"
CAPY_ARCHIVE="$CAPY_OUT/$CAPY_SCHEME.xcarchive"
CAPY_APP_BUNDLE="$CAPY_ARCHIVE/Products/Applications/$CAPY_SCHEME.app"
CAPY_AUTH=(-allowProvisioningUpdates -authenticationKeyPath "${CAPY_APPLE_KEY:-}"
  -authenticationKeyID "${CAPY_APPLE_KEY_ID:-}" -authenticationKeyIssuerID "${CAPY_APPLE_ISSUER:-}")
if [[ -z "${CAPY_APPLE_TEAM:-}" ]]; then
  CAPY_SIGNING=(CODE_SIGNING_ALLOWED=NO)
elif [[ $1 == mac ]]; then
  CAPY_SIGNING=("DEVELOPMENT_TEAM=$CAPY_APPLE_TEAM" CODE_SIGN_STYLE=Manual
    'CODE_SIGN_IDENTITY=Developer ID Application' OTHER_CODE_SIGN_FLAGS=--timestamp)
else
  CAPY_SIGNING=("DEVELOPMENT_TEAM=$CAPY_APPLE_TEAM" "${CAPY_AUTH[@]}")
fi
xcodebuild archive -quiet -project "$CAPY_APP/CapyCanvas.xcodeproj" -scheme "$CAPY_SCHEME" \
  -destination "$CAPY_DEST" -archivePath "$CAPY_ARCHIVE" \
  -derivedDataPath "${CAPY_DERIVED_DATA:-$CAPY_APP/DerivedData}" "${CAPY_SIGNING[@]}"
if [[ $1 == ipad ]]; then
  [[ -n "${CAPY_APPLE_TEAM:-}" ]] || exit 0
  cat > "$CAPY_OUT/export.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>method</key><string>app-store-connect</string>
<key>destination</key><string>upload</string><key>teamID</key><string>$CAPY_APPLE_TEAM</string></dict></plist>
PLIST
  exec xcodebuild -exportArchive -archivePath "$CAPY_ARCHIVE" -exportOptionsPlist "$CAPY_OUT/export.plist" \
    -exportPath "$CAPY_OUT/export" "${CAPY_AUTH[@]}"
fi
mkdir "$CAPY_OUT/dmg"
ditto "$CAPY_APP_BUNDLE" "$CAPY_OUT/dmg/Capy Canvas.app"
ln -s /Applications "$CAPY_OUT/dmg/Applications"
CAPY_DMG="$CAPY_APP/../../dist/capycanvas-$CAPY_VERSION-macos-arm64.dmg"
hdiutil create -quiet -volname 'Capy Canvas' -srcfolder "$CAPY_OUT/dmg" -format UDZO -ov "$CAPY_DMG"
[[ -n "${CAPY_APPLE_TEAM:-}" ]] || exit 0
codesign --sign 'Developer ID Application' --timestamp "$CAPY_DMG"
xcrun notarytool submit "$CAPY_DMG" --wait \
  --key "$CAPY_APPLE_KEY" --key-id "$CAPY_APPLE_KEY_ID" --issuer "$CAPY_APPLE_ISSUER"
xcrun stapler staple "$CAPY_DMG"
spctl --assess --type open --context context:primary-signature --verbose "$CAPY_DMG"
