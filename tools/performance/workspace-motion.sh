#!/usr/bin/env bash
# Release builds, real input, and an isolated display; never moves a user's pointer.
set -euo pipefail
motion_script=$(realpath "${BASH_SOURCE[0]}")
cd "$(dirname "$motion_script")/../.."
if [[ "${1:-}" != --session ]]; then
    exec dbus-run-session -- bash "$motion_script" --session "${@}"
fi
shift
motion_platform=${1:-gtk}
shift || true
if [[ $# -eq 0 ]]; then set -- --workspace-motion; fi
case "$motion_platform" in
    gtk) cargo test --locked --release -p layer-linux --no-run ;;
    web) bash apps/layer-web/build.sh ;;
    *) echo 'Usage: workspace-motion.sh [gtk|web] [test flag]' >&2; exit 2 ;;
esac
motion_run_dir=$(mktemp -d /tmp/capy-workspace-motion.XXXXXX)
export XDG_RUNTIME_DIR="$motion_run_dir/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
export WAYLAND_DISPLAY=layer-bench-motion
export GDK_BACKEND=wayland GSK_RENDERER=vulkan GTK_A11Y=none
export LAYER_NATIVE_INPUT_DIR="$motion_run_dir/input"
export LAYER_SETTINGS_FILE="$motion_run_dir/settings.json"
export CAPY_WORKSPACE_DIR="$motion_run_dir/workspaces"
mkdir "$LAYER_NATIVE_INPUT_DIR" "$CAPY_WORKSPACE_DIR"
unset DISPLAY
printf 'Workspace test logs: %s\n' "$motion_run_dir"
env -u G_DEBUG mutter --headless --wayland --no-x11 \
    --virtual-monitor="1600x1000@${LAYER_MOTION_REFRESH:-120}" \
    --wayland-display="$WAYLAND_DISPLAY" >"$motion_run_dir/mutter.log" 2>&1 &
motion_compositor_pid=$!
pipewire >"$motion_run_dir/pipewire.log" 2>&1 &
motion_pipewire_pid=$!
motion_server_pid=
trap 'kill ${motion_server_pid:+"$motion_server_pid"} "$motion_pipewire_pid" "$motion_compositor_pid" 2>/dev/null || true' EXIT
for ((attempt=0; attempt<100; attempt++)); do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$motion_compositor_pid" 2>/dev/null || { cat "$motion_run_dir/mutter.log"; exit 1; }
    sleep .1
done
[[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]]
if [[ "$motion_platform" == gtk ]]; then
    G_DEBUG=fatal-criticals gjs apps/layer-linux/bench/native-input.js "$@" 2>&1 | tee "$motion_run_dir/test.log"
else
    motion_port=${LAYER_WEB_PORT:-4179}
    python3 -m http.server "$motion_port" --bind 127.0.0.1 --directory apps/layer-web >"$motion_run_dir/server.log" 2>&1 &
    motion_server_pid=$!
    export LAYER_WEB_URL="http://127.0.0.1:$motion_port"
    sleep .2
    kill -0 "$motion_server_pid"
    if [[ "$*" == --workspace-motion || "$*" == --workspace-resize ]]; then
        gjs apps/layer-linux/bench/native-input.js "--web-${1#--}" 2>&1 | tee "$motion_run_dir/test.log"
    else
        node apps/layer-web/test.mjs "$@" 2>&1 | tee "$motion_run_dir/test.log"
    fi
fi
