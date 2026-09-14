#!/usr/bin/env bash
# Run an already built GTK test on an isolated 120 Hz Wayland display.
# Keeping compilation separate makes before/after measurement reproducible.
set -euo pipefail
raster_script=$(realpath "${BASH_SOURCE[0]}")
if [[ "${1:-}" != --session ]]; then
    exec dbus-run-session -- bash "$raster_script" --session "$@"
fi
shift
if [[ $# != 3 ]]; then
    echo 'Usage: gtk-raster.sh TEST_EXECUTABLE TEST_FILTER REPORT_PREFIX' >&2
    exit 2
fi
raster_binary=$(realpath "$1")
raster_filter=$2
raster_report=$(realpath -m "$3")
raster_run_dir=$(mktemp -d /tmp/capy-gtk-raster.XXXXXX)
mkdir -p "$(dirname "$raster_report")"
export XDG_RUNTIME_DIR="$raster_run_dir/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
export WAYLAND_DISPLAY=capy-raster-validation
export GDK_BACKEND=wayland GSK_RENDERER=vulkan GTK_A11Y=none
# Native file tests drive the in-process GTK chooser. Portal dialogs live in a
# different process and cannot be exercised by those widget signal assertions.
export GDK_DEBUG=no-portals
export LAYER_SETTINGS_FILE="$raster_run_dir/settings.json"
export CAPY_WORKSPACE_DIR="$raster_run_dir/workspaces"
export CAPY_RECOVERY_DIR="$raster_run_dir/recovery"
export LAYER_PACING_REPORT="$raster_report.json"
unset DISPLAY
env -u G_DEBUG mutter --headless --wayland --no-x11 \
    --virtual-monitor=1600x1000@120 --wayland-display="$WAYLAND_DISPLAY" \
    >"$raster_report-mutter.log" 2>&1 &
raster_compositor_pid=$!
trap 'kill "$raster_compositor_pid" 2>/dev/null || true; wait "$raster_compositor_pid" 2>/dev/null || true' EXIT
for ((attempt=0; attempt<100; attempt++)); do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$raster_compositor_pid" 2>/dev/null || { cat "$raster_report-mutter.log"; exit 1; }
    sleep .1
done
[[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]]
cd "$(dirname "$raster_script")/../../apps/layer-linux"
G_DEBUG=fatal-criticals "$raster_binary" "$raster_filter" --ignored --test-threads=1 \
    --nocapture 2>&1 | tee "$raster_report.log"
