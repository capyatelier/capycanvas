#!/usr/bin/env bash
# Run from the repository root with the primary debug app/test APK installed.
# Usage: bash run-live.sh NAME existing|swept 1|2 [turns=1] [prediction-ms=16] [duration-ms=15000] [strokes=2]
set -euo pipefail
capy_name=${1:?case name required}
capy_algorithm=${2:?algorithm required}
capy_preset=${3:?preset required}
[[ "$capy_name" =~ ^[a-z0-9-]+$ ]]
[[ "$capy_algorithm" == existing || "$capy_algorithm" == swept ]]
[[ "$capy_preset" == 1 || "$capy_preset" == 2 ]]
capy_adb=${CAPY_ADB:-${ANDROID_HOME:-$HOME/Android/Sdk}/platform-tools/adb}
capy_serial=${CAPY_WACOM_SERIAL:-5ll21u1002931}
capy_out=artifacts/swept-brush/live-app
mkdir -p "$capy_out"
"$capy_adb" -s "$capy_serial" shell dumpsys thermalservice > "$capy_out/$capy_name.thermal-before.txt"
"$capy_adb" -s "$capy_serial" shell am instrument -w \
  -e class art.capycanvas.AndroidRasterTest#largePhotoWideBrushAttribution \
  -e wideBrush true -e wideBrushAlgorithm "$capy_algorithm" -e wideBrushPreset "$capy_preset" \
  -e wideBrushSize 2048 -e wideBrushTurns "${4:-1}" -e wideBrushPrediction true \
  -e wideBrushPredictionMs "${5:-16}" -e motionDurationMs "${6:-15000}" -e wideBrushRuns "${7:-2}" \
  art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner | tee "$capy_out/$capy_name.log"
"$capy_adb" -s "$capy_serial" pull /sdcard/Android/data/art.capycanvas/files/wide-brush-attribution.json "$capy_out/$capy_name.json"
"$capy_adb" -s "$capy_serial" shell dumpsys thermalservice > "$capy_out/$capy_name.thermal-after.txt"
# A failed instrumentation run can leave an old result. Never silently analyze it.
rg -q 'OK \(1 test\)' "$capy_out/$capy_name.log"
"$capy_adb" -s "$capy_serial" pull /sdcard/Android/data/art.capycanvas/files/wide-brush-result.png "$capy_out/$capy_name.png"
node crates/layer-render-wgpu/examples/swept_brush/analyze-live.mjs "$capy_out/$capy_name.json" > "$capy_out/$capy_name.summary.json"
