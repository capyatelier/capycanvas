#!/usr/bin/env bash
# Workspace switcher pinning and ordering through real mouse/touch input.
set -euo pipefail
if [[ "${1:-}" != --session ]]; then
    exec dbus-run-session -- bash "$0" --session
fi
cd "$(dirname "${BASH_SOURCE[0]}")/../../.."
cargo test --locked --release -p layer-linux --no-run
menu_run_dir=$(mktemp -d /tmp/capy-workspace-menus.XXXXXX)
export XDG_RUNTIME_DIR="$menu_run_dir/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
export WAYLAND_DISPLAY=layer-bench-workspace-menus
export GDK_BACKEND=wayland GSK_RENDERER=vulkan GTK_A11Y=none
export LAYER_NATIVE_INPUT_DIR="$menu_run_dir/input"
export LAYER_SETTINGS_FILE="$menu_run_dir/settings.json"
export CAPY_WORKSPACE_DIR="$menu_run_dir/workspaces"
mkdir "$LAYER_NATIVE_INPUT_DIR"
unset DISPLAY
printf 'Workspace menu test logs: %s\n' "$menu_run_dir"
env -u G_DEBUG mutter --headless --wayland --no-x11 --virtual-monitor=1600x1000 \
    --wayland-display="$WAYLAND_DISPLAY" >"$menu_run_dir/mutter.log" 2>&1 &
menu_compositor_pid=$!
pipewire >"$menu_run_dir/pipewire.log" 2>&1 &
menu_pipewire_pid=$!
trap 'kill "$menu_pipewire_pid" "$menu_compositor_pid" 2>/dev/null || true; wait "$menu_pipewire_pid" "$menu_compositor_pid" 2>/dev/null || true' EXIT
for ((attempt=0; attempt<100; attempt++)); do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$menu_compositor_pid" 2>/dev/null || { cat "$menu_run_dir/mutter.log"; exit 1; }
    sleep 0.1
done
[[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]]
G_DEBUG=fatal-criticals gjs apps/layer-linux/bench/native-input.js --workspace-switcher \
    2>&1 | tee "$menu_run_dir/test.log"
