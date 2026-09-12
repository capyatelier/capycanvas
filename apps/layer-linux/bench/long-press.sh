#!/usr/bin/env bash
# Real touch events through an isolated Mutter/Wayland session.
set -euo pipefail
if [[ "${1:-}" != --session ]]; then
    exec dbus-run-session -- bash "$0" --session
fi
cd "$(dirname "${BASH_SOURCE[0]}")/../../.."
cargo test --locked --release -p layer-linux --no-run
hold_run_dir=$(mktemp -d /tmp/capy-long-press.XXXXXX)
export XDG_RUNTIME_DIR="$hold_run_dir/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
export WAYLAND_DISPLAY=layer-bench-long-press
export GDK_BACKEND=wayland GSK_RENDERER=vulkan GTK_A11Y=none
export LAYER_NATIVE_INPUT_DIR="$hold_run_dir/input"
export LAYER_SETTINGS_FILE="$hold_run_dir/settings.json"
mkdir "$LAYER_NATIVE_INPUT_DIR"
unset DISPLAY
printf 'Long-press test logs: %s\n' "$hold_run_dir"
# Keep app criticals fatal. Mutter 50's virtual touchscreen emits a GLib
# critical on native popup grabs, so do not inherit G_DEBUG in the compositor.
env -u G_DEBUG mutter --headless --wayland --no-x11 --virtual-monitor=1600x1000 \
    --wayland-display="$WAYLAND_DISPLAY" >"$hold_run_dir/mutter.log" 2>&1 &
hold_compositor_pid=$!
# RemoteDesktop touch coordinates use a monitor stream backed by PipeWire.
pipewire >"$hold_run_dir/pipewire.log" 2>&1 &
hold_pipewire_pid=$!
trap 'kill "$hold_pipewire_pid" "$hold_compositor_pid" 2>/dev/null || true; wait "$hold_pipewire_pid" "$hold_compositor_pid" 2>/dev/null || true' EXIT
for ((attempt=0; attempt<100; attempt++)); do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$hold_compositor_pid" 2>/dev/null || { cat "$hold_run_dir/mutter.log"; exit 1; }
    sleep 0.1
done
[[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]]
G_DEBUG=fatal-criticals gjs apps/layer-linux/bench/native-input.js --workspace-hold \
    2>&1 | tee "$hold_run_dir/test.log"
