#!/usr/bin/env bash
# Compatibility entry point for the archived review launcher. Normal packages
# use the same runtime recipe through apps/layer-linux/package.mjs.
set -euo pipefail
review_repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)
review_build="$review_repo/artifacts/color-m4/gtk-runtime"
review_runtime="$review_repo/artifacts/color-m4/review/runtime"
bash "$review_repo/tools/build/gtk-runtime/build.sh" "$review_build" "$review_build/prefix"
mkdir -p "$review_runtime"
cp "$review_build/prefix/lib/libgtk-4.so.1" "$review_runtime/libgtk-4.so.1.new"
mv -f "$review_runtime/libgtk-4.so.1.new" "$review_runtime/libgtk-4.so.1"
cp "$review_build/prefix/share/doc/capycanvas-gtk/COPYING" "$review_runtime/COPYING.gtk"
cp "$review_repo/tools/build/gtk-runtime/pad-event-surface.patch" "$review_runtime/"
cp "$review_repo/tools/build/gtk-runtime/tablet-proximity-cursor.patch" "$review_runtime/"
cp "$review_build/prefix/manifest.json" "$review_runtime/manifest.json"
