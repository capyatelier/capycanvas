#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
CAPY_REPORTS="${CAPY_REPORTS:-$CAPY_APP/../../artifacts/release-tests/apple}"
mkdir -p "$CAPY_REPORTS/tmp"
export TMPDIR="$(cd "$CAPY_REPORTS/tmp" && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
CAPY_FAILED=0
check() {
  local name=$1
  shift
  if "$@" > "$CAPY_REPORTS/$name.log" 2>&1; then
    echo "$name: passed"
  else
    echo "$name: failed"
    CAPY_FAILED=1
  fi
  cat "$CAPY_REPORTS/$name.log"
}
while read -r name source extra; do
  sources=("$CAPY_APP/$source")
  if [[ -n "$extra" ]]; then sources+=("$CAPY_APP/$extra"); fi
  binary="$TMPDIR/$name"
  check "$name-compile" xcrun swiftc -parse-as-library -target arm64-apple-macos15.0 \
    "${sources[@]}" "$CAPY_APP/tests/$name.swift" -o "$binary"
  if [[ -x "$binary" ]]; then check "$name" "$binary"; rm "$binary"; fi
done <<'FIXTURES'
json-lookup Shared/Bridge/JSON.swift
reorder-contact Shared/Bridge/ReorderContact.swift
estimated-input Shared/Bridge/EstimatedInput.swift
pen-response-input Shared/Bridge/JSON.swift Shared/Bridge/StylusTilt.swift
frame-trace Shared/Bridge/FrameTrace.swift
drawing-workload-plan Shared/Bridge/DrawingWorkloadPlan.swift
native-input-gate Shared/Bridge/NativeInputGate.swift
frame-driver Shared/Bridge/CanvasFrameDriver.swift
process-termination Shared/Bridge/ProcessTermination.swift
numeric-edit Shared/Editor/NumericEditState.swift
slider-input Shared/Bridge/LatestOwnerInput.swift Shared/Bridge/RenderOwnerQueue.swift
native-menu-titles macOS/Platform/NativeMenuTitles.swift
system-status Shared/Bridge/SystemStatus.swift macOS/Platform/BatterySource.swift
FIXTURES
for fixture in application-links settings-links owner-autorelease persistence persistence-owner snapshot-projection snapshot-views header-persistence column-stack-persistence workspace-manager workspace-coordinator workspace-tabs workspace-notice workspace-motion workspace-drawers workspace-menu-actions phone-bar-input window-presentation dialog-page-layout panel-measurements reorder-lifecycle-publication editor-appearance color-field-cache native-context-menu squircle-geometry; do
  check "$fixture" bash "$CAPY_APP/scripts/test-project-files.sh" "$CAPY_APP/tests/$fixture.swift"
done
CAPY_ASSETS_APP="$TMPDIR/HeaderAssets.app"
mkdir -p "$CAPY_ASSETS_APP/Contents/Resources"
check prepare-assets python3 "$CAPY_APP/scripts/prepare.py"
check header-assets xcrun actool --compile "$CAPY_ASSETS_APP/Contents/Resources" \
  --platform macosx --minimum-deployment-target 15.0 --target-device mac "$CAPY_APP/Generated/SharedAssets.xcassets"
check header-controls env CAPY_TEST_ASSETS_APP="$CAPY_ASSETS_APP" CAPY_HEADER_CAPTURES="$CAPY_REPORTS/header-controls" \
  bash "$CAPY_APP/scripts/test-project-files.sh" "$CAPY_APP/tests/header-controls.swift"
check background-expiration python3 "$CAPY_APP/tests/background-expiration.py"
check project-access python3 "$CAPY_APP/scripts/test-project-access.py"
check color-input bash "$CAPY_APP/scripts/test-color-input.sh"
exit "$CAPY_FAILED"
