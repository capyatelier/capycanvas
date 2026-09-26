#!/usr/bin/env bash
# Build a replaceable, application-local GTK. Never install system libraries.
set -euo pipefail
gtk_recipe=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
gtk_build=$(realpath -m "${1:?Usage: build.sh BUILD_DIRECTORY PREFIX}")
gtk_prefix=$(realpath -m "${2:?Usage: build.sh BUILD_DIRECTORY PREFIX}")
mkdir -p "$gtk_build" "$gtk_prefix/lib" "$gtk_prefix/share/doc/capycanvas-gtk/sources"
gtk_docs="$gtk_prefix/share/doc/capycanvas-gtk"
gtk_archive=gtk-4.22.4.tar.xz
if [[ ! -f "$gtk_build/$gtk_archive" ]]; then
    if [[ -f "$gtk_recipe/sources/$gtk_archive" ]]; then
        cp "$gtk_recipe/sources/$gtk_archive" "$gtk_build/"
    else
        curl --fail --location "https://download.gnome.org/sources/gtk/4.22/$gtk_archive" -o "$gtk_build/$gtk_archive"
    fi
fi
python3 - "$gtk_build/$gtk_archive" <<'PY'
import hashlib, pathlib, sys
assert hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest() == '51bd9f60c7d23a665a556c7364c21fb2e4e282566b3e7e092455e8f910330893', 'GTK source hash mismatch'
PY
# Re-extract the verified archive before applying a changed patch.
gtk_patch_hash=$(cat "$gtk_recipe/pad-event-surface.patch" "$gtk_recipe/tablet-proximity-cursor.patch" | sha256sum | cut -d ' ' -f 1)
if [[ ! -f "$gtk_build/patched.sha256" ]] || [[ $(cat "$gtk_build/patched.sha256") != "$gtk_patch_hash" ]]; then
    tar -xf "$gtk_build/$gtk_archive" -C "$gtk_build"
    patch -d "$gtk_build/gtk-4.22.4" -p1 < "$gtk_recipe/pad-event-surface.patch"
    patch -d "$gtk_build/gtk-4.22.4" -p1 < "$gtk_recipe/tablet-proximity-cursor.patch"
    echo "$gtk_patch_hash" > "$gtk_build/patched.sha256"
fi
if [[ -d "$gtk_build/deps/usr" ]]; then
    export PKG_CONFIG_PATH="$gtk_build/deps/usr/lib64/pkgconfig:$gtk_build/deps/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    export PATH="$gtk_build/deps/usr/bin:$PATH"
fi
gtk_config=()
if [[ -f "$gtk_build/build/build.ninja" ]]; then gtk_config+=(--reconfigure); fi
meson setup "${gtk_config[@]}" "$gtk_build/build" "$gtk_build/gtk-4.22.4" \
    --buildtype=release --wrap-mode=nofallback -Dbuild-demos=false -Dbuild-tests=false \
    -Dbuild-testsuite=false -Dbuild-examples=false -Dintrospection=disabled \
    -Dmedia-gstreamer=disabled -Ddocumentation=false -Dman-pages=false -Dx11-backend=false
ninja -C "$gtk_build/build" -j "${CAPY_BUILD_JOBS:-8}" gtk/libgtk-4.so.1.2200.4
cp "$gtk_build/build/gtk/libgtk-4.so.1.2200.4" "$gtk_prefix/lib/libgtk-4.so.1.new"
mv -f "$gtk_prefix/lib/libgtk-4.so.1.new" "$gtk_prefix/lib/libgtk-4.so.1"
cp "$gtk_build/gtk-4.22.4/COPYING" "$gtk_docs/COPYING"
cp "$gtk_build/$gtk_archive" "$gtk_docs/sources/"
cp "$gtk_recipe/pad-event-surface.patch" "$gtk_recipe/tablet-proximity-cursor.patch" "$gtk_recipe/build.sh" "$gtk_docs/"
python3 - "$gtk_prefix" <<'PY'
import hashlib, json, pathlib, sys
p = pathlib.Path(sys.argv[1])
files = ['lib/libgtk-4.so.1', 'share/doc/capycanvas-gtk/COPYING',
         'share/doc/capycanvas-gtk/sources/gtk-4.22.4.tar.xz',
         'share/doc/capycanvas-gtk/pad-event-surface.patch',
         'share/doc/capycanvas-gtk/tablet-proximity-cursor.patch', 'share/doc/capycanvas-gtk/build.sh']
manifest = dict(version='4.22.4', license='LGPL-2.1-or-later',
                source='https://download.gnome.org/sources/gtk/4.22/gtk-4.22.4.tar.xz',
                rebuild='bash share/doc/capycanvas-gtk/build.sh /tmp/capy-gtk-build /tmp/capy-gtk-prefix',
                files={f: hashlib.sha256((p/f).read_bytes()).hexdigest() for f in files})
(p/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
PY
