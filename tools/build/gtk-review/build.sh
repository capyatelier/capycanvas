#!/usr/bin/env bash
# Local review runtime only. Never install or replace the system GTK library.
set -euo pipefail
review_repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)
review_build="$review_repo/artifacts/color-m4/gtk-runtime"
review_runtime="$review_repo/artifacts/color-m4/review/runtime"
mkdir -p "$review_build" "$review_runtime"
review_archive="$review_build/gtk-4.22.4.tar.xz"
if [[ ! -f "$review_archive" ]]; then
    curl --fail --location https://download.gnome.org/sources/gtk/4.22/gtk-4.22.4.tar.xz -o "$review_archive"
fi
python3 - "$review_archive" <<'PY'
import hashlib, pathlib, sys
assert hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest() == '51bd9f60c7d23a665a556c7364c21fb2e4e282566b3e7e092455e8f910330893', 'GTK source hash mismatch'
PY
if [[ ! -d "$review_build/gtk-4.22.4" ]]; then
    tar -xf "$review_archive" -C "$review_build"
fi
if ! rg -q 'surface == NULL \|\| !GDK_SURFACE_IS_MAPPED' "$review_build/gtk-4.22.4/gdk/gdksurface.c"; then
    patch -d "$review_build/gtk-4.22.4" -p1 < "$review_repo/tools/build/gtk-review/pad-event-surface.patch"
fi
# Optional headers extracted from Fedora RPMs; system development packages work too.
if [[ -d "$review_build/deps/usr" ]]; then
    export PKG_CONFIG_PATH="$review_build/deps/usr/lib64/pkgconfig:$review_build/deps/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    export PATH="$review_build/deps/usr/bin:$PATH"
fi
review_config=()
if [[ -f "$review_build/build/build.ninja" ]]; then review_config+=(--reconfigure); fi
meson setup "${review_config[@]}" "$review_build/build" "$review_build/gtk-4.22.4" \
    --buildtype=release --wrap-mode=nofallback -Dbuild-demos=false -Dbuild-tests=false \
    -Dbuild-testsuite=false -Dbuild-examples=false -Dintrospection=disabled \
    -Dmedia-gstreamer=disabled -Ddocumentation=false -Dman-pages=false
ninja -C "$review_build/build" -j "${CAPY_BUILD_JOBS:-8}" gtk/libgtk-4.so.1.2200.4
# Replace the local runtime atomically, even if a previous review instance is open.
cp "$review_build/build/gtk/libgtk-4.so.1.2200.4" "$review_runtime/libgtk-4.so.1.new"
mv -f "$review_runtime/libgtk-4.so.1.new" "$review_runtime/libgtk-4.so.1"
cp "$review_build/gtk-4.22.4/COPYING" "$review_runtime/COPYING.gtk"
cp "$review_repo/tools/build/gtk-review/pad-event-surface.patch" "$review_runtime/"
cat > "$review_runtime/SOURCE.txt" <<'SOURCE'
GTK 4.22.4, LGPL 2.1 or later. Source retained in ../../gtk-runtime.
https://download.gnome.org/sources/gtk/4.22/gtk-4.22.4.tar.xz
SHA256 51bd9f60c7d23a665a556c7364c21fb2e4e282566b3e7e092455e8f910330893
Local change: pad-event-surface.patch. Rebuild: tools/build/gtk-review/build.sh
This library is selected only by the HDR review launcher.
SOURCE
