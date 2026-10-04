#!/bin/bash
set -euo pipefail
CAPY_APP="$(cd "$(dirname "$0")/.." && pwd)"
bash "$CAPY_APP/scripts/test-project-files.sh" "$CAPY_APP/tests/persistence.swift"
bash "$CAPY_APP/scripts/test-project-files.sh" "$CAPY_APP/tests/persistence-owner.swift"
