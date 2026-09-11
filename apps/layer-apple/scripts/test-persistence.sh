#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
CAPY_CHECK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/capy-persistence.XXXXXX")"
trap 'rm -rf "$CAPY_CHECK_DIR"' EXIT
CAPY_BRIDGE="$CAPY_APP/Shared/Bridge"

xcrun swiftc "$CAPY_BRIDGE/AtomicJSONFile.swift" "$CAPY_BRIDGE/EditorPersistence.swift" \
  "$CAPY_APP/tests/persistence.swift" -o "$CAPY_CHECK_DIR/files"
"$CAPY_CHECK_DIR/files"

bash "$CAPY_APP/scripts/test-project-files.sh" "$CAPY_APP/tests/persistence-owner.swift"
