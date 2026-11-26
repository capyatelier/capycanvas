#!/bin/bash
set -euo pipefail
trap 'CAPY_FAILURE=$?; printf "Apple release failed at line %s (exit %s)\n" "$LINENO" "$CAPY_FAILURE" >&2' ERR
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
case "${1:-}" in
  ipad) CAPY_SCHEME=CapyCanvas-iPad; CAPY_DEST='generic/platform=iOS'; CAPY_RUST_TARGET=aarch64-apple-ios ;;
  mac|mac-store) CAPY_SCHEME=CapyCanvas-Mac; CAPY_DEST='generic/platform=macOS'; CAPY_RUST_TARGET=aarch64-apple-darwin ;;
  *) echo 'Usage: release.sh ipad|mac|mac-store [build|export|publish]' >&2; exit 1 ;;
esac
CAPY_ACTION=${2:-build}
[[ $CAPY_ACTION == build || $CAPY_ACTION == export || $CAPY_ACTION == publish ]] || { echo 'Expected build, export or publish' >&2; exit 1; }
CAPY_VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$CAPY_APP/../../Cargo.toml")
CAPY_OUT="$CAPY_APP/../../dist/apple-$1"
CAPY_ARCHIVE="$CAPY_OUT/$CAPY_SCHEME.xcarchive"
CAPY_APP_BUNDLE="$CAPY_ARCHIVE/Products/Applications/$CAPY_SCHEME.app"
CAPY_AUTH=(-allowProvisioningUpdates -authenticationKeyPath "${CAPY_APPLE_KEY:-}"
  -authenticationKeyID "${CAPY_APPLE_KEY_ID:-}" -authenticationKeyIssuerID "${CAPY_APPLE_ISSUER:-}")
if [[ $CAPY_ACTION != build ]]; then
  [[ $1 != mac && -n "${CAPY_APPLE_TEAM:-}" && -d "$CAPY_ARCHIVE" && -f "${CAPY_APPLE_KEY:-}" &&
    -n "${CAPY_APPLE_KEY_ID:-}" && -n "${CAPY_APPLE_ISSUER:-}" ]] || { echo 'Store export needs an iPad or Mac Store archive and Apple credentials' >&2; exit 1; }
  CAPY_EXPORT_DESTINATION=export
  [[ $CAPY_ACTION != publish ]] || CAPY_EXPORT_DESTINATION=upload
  cat > "$CAPY_OUT/export.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>method</key><string>app-store-connect</string>
<key>destination</key><string>$CAPY_EXPORT_DESTINATION</string><key>teamID</key><string>$CAPY_APPLE_TEAM</string>
<key>signingStyle</key><string>automatic</string><key>manageAppVersionAndBuildNumber</key><false/></dict></plist>
PLIST
  exec xcodebuild -exportArchive -archivePath "$CAPY_ARCHIVE" -exportOptionsPlist "$CAPY_OUT/export.plist" \
    -exportPath "$CAPY_OUT/export" "${CAPY_AUTH[@]}"
fi
python3 "$CAPY_APP/scripts/prepare.py"
python3 "$CAPY_APP/scripts/project.py"
node "$CAPY_APP/scripts/package-notices.mjs" "$CAPY_APP/Generated/licenses" "$CAPY_RUST_TARGET"
rm -rf "$CAPY_OUT"
mkdir -p "$CAPY_OUT"
if [[ -z "${CAPY_APPLE_TEAM:-}" ]]; then
  CAPY_SIGNING=(CODE_SIGNING_ALLOWED=NO)
elif [[ $1 == mac ]]; then
  CAPY_SIGNING=("DEVELOPMENT_TEAM=$CAPY_APPLE_TEAM" CODE_SIGN_STYLE=Manual
    'CODE_SIGN_IDENTITY=Developer ID Application' OTHER_CODE_SIGN_FLAGS=--timestamp)
else
  CAPY_SIGNING=("DEVELOPMENT_TEAM=$CAPY_APPLE_TEAM" CODE_SIGN_STYLE=Automatic 'CODE_SIGN_IDENTITY=Apple Development' "${CAPY_AUTH[@]}")
fi
xcodebuild archive -quiet -project "$CAPY_APP/CapyCanvas.xcodeproj" -scheme "$CAPY_SCHEME" \
  -destination "$CAPY_DEST" -archivePath "$CAPY_ARCHIVE" \
  -resultBundlePath "$CAPY_OUT/$CAPY_SCHEME.xcresult" \
  -derivedDataPath "${CAPY_DERIVED_DATA:-$CAPY_APP/DerivedData}" "${CAPY_SIGNING[@]}" 2>&1 | tee "$CAPY_OUT/archive.log"
[[ $1 != mac ]] && exit 0
mkdir "$CAPY_OUT/dmg"
ditto "$CAPY_APP_BUNDLE" "$CAPY_OUT/dmg/Capy Canvas.app"
ln -s /Applications "$CAPY_OUT/dmg/Applications"
CAPY_DMG="$CAPY_APP/../../dist/capycanvas-$CAPY_VERSION-macos-arm64.dmg"
CAPY_DMG_SIZE=$(python3 - "$CAPY_OUT/dmg" <<'PY'
import os, sys
def fail(error):
    raise error
size = sum(max(os.lstat(os.path.join(parent, name)).st_size, 4096)
           for parent, directories, files in os.walk(sys.argv[1], followlinks=False, onerror=fail)
           for name in directories + files)
print(f'{max(128, (2 * size + 1048575) // 1048576 + 64)}m')
PY
)
{ printf 'DMG capacity: %s (HFS+)\n' "$CAPY_DMG_SIZE"; df -h "$CAPY_OUT"; du -sh "$CAPY_OUT/dmg"; hdiutil info; } > "$CAPY_OUT/dmg.log" 2>&1
hdiutil create -verbose -fs HFS+ -size "$CAPY_DMG_SIZE" -volname 'Capy Canvas' -srcfolder "$CAPY_OUT/dmg" -format UDZO -ov "$CAPY_DMG" 2>&1 | tee -a "$CAPY_OUT/dmg.log"
hdiutil verify -verbose "$CAPY_DMG" 2>&1 | tee -a "$CAPY_OUT/dmg.log"
[[ -n "${CAPY_APPLE_TEAM:-}" ]] || exit 0
codesign --sign 'Developer ID Application' --timestamp "$CAPY_DMG"
CAPY_NOTARY=(--key "$CAPY_APPLE_KEY" --key-id "$CAPY_APPLE_KEY_ID" --issuer "$CAPY_APPLE_ISSUER")
CAPY_SUBMISSION=$(xcrun notarytool submit "$CAPY_DMG" "${CAPY_NOTARY[@]}" --output-format json | jq -r .id)
for _ in 1 2 3 4 5; do xcrun notarytool wait "$CAPY_SUBMISSION" "${CAPY_NOTARY[@]}" && break; sleep 60; done
xcrun stapler staple "$CAPY_DMG" || { xcrun notarytool log "$CAPY_SUBMISSION" "${CAPY_NOTARY[@]}"; exit 1; }
spctl --assess --type open --context context:primary-signature --verbose "$CAPY_DMG"
