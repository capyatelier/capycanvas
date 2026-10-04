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
python3 - "$gtk_recipe/sources" "$gtk_build/gtk-4.22.4/subprojects" <<'PY'
import configparser, pathlib, shutil, sys
sources, projects = map(pathlib.Path, sys.argv[1:])
for name in ['libtiff', 'libjpeg-turbo']:
    wrap = configparser.ConfigParser(interpolation=None)
    wrap.read(projects / (name + '.wrap'))
    for kind in ['source', 'patch']:
        filename = wrap['wrap-file'][kind + '_filename']
        if (sources / filename).is_file():
            (projects / 'packagecache').mkdir(exist_ok=True)
            shutil.copy2(sources / filename, projects / 'packagecache' / filename)
PY
if [[ -d "$gtk_build/deps/usr" ]]; then
    export PKG_CONFIG_PATH="$gtk_build/deps/usr/lib64/pkgconfig:$gtk_build/deps/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    export PATH="$gtk_build/deps/usr/bin:$PATH"
    export LD_LIBRARY_PATH="$gtk_build/deps/usr/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
gtk_config=()
if [[ -f "$gtk_build/build/build.ninja" ]]; then gtk_config+=(--reconfigure); fi
meson setup "${gtk_config[@]}" "$gtk_build/build" "$gtk_build/gtk-4.22.4" \
    --buildtype=release -Dc_args=-fPIC --wrap-mode=nofallback --force-fallback-for=libtiff,libjpeg-turbo \
    -Dlibtiff:default_library=static -Dlibtiff:jbig=disabled -Dlibtiff:lerc=disabled \
    -Dlibtiff:lzma=disabled -Dlibtiff:webp=disabled -Dlibtiff:zstd=disabled \
    -Dlibtiff:jpeg=enabled -Dlibtiff:zlib=enabled \
    -Dlibjpeg-turbo:default_library=static \
    -Dlibjpeg-turbo:jpeg-turbo=disabled -Dlibjpeg-turbo:tests=disabled \
    -Dbuild-demos=false -Dbuild-tests=false \
    -Dbuild-testsuite=false -Dbuild-examples=false -Dintrospection=disabled \
    -Dmedia-gstreamer=disabled -Ddocumentation=false -Dman-pages=false -Dx11-backend=false
ninja -C "$gtk_build/build" -j "${CAPY_BUILD_JOBS:-8}" gtk/libgtk-4.so.1.2200.4
cp "$gtk_build/build/gtk/libgtk-4.so.1.2200.4" "$gtk_prefix/lib/libgtk-4.so.1.new"
mv -f "$gtk_prefix/lib/libgtk-4.so.1.new" "$gtk_prefix/lib/libgtk-4.so.1"
cp "$gtk_build/gtk-4.22.4/COPYING" "$gtk_docs/COPYING"
cp "$gtk_build/$gtk_archive" "$gtk_docs/sources/"
cp "$gtk_recipe/pad-event-surface.patch" "$gtk_recipe/tablet-proximity-cursor.patch" "$gtk_recipe/build.sh" "$gtk_docs/"
python3 - "$gtk_prefix" "$gtk_build/gtk-4.22.4/subprojects" <<'PY'
import configparser, hashlib, json, pathlib, shutil, sys
p = pathlib.Path(sys.argv[1])
projects = pathlib.Path(sys.argv[2])
docs = p / 'share/doc/capycanvas-gtk'
files = ['lib/libgtk-4.so.1', 'share/doc/capycanvas-gtk/COPYING',
         'share/doc/capycanvas-gtk/sources/gtk-4.22.4.tar.xz',
         'share/doc/capycanvas-gtk/pad-event-surface.patch',
         'share/doc/capycanvas-gtk/tablet-proximity-cursor.patch', 'share/doc/capycanvas-gtk/build.sh']
dependencies = {}
for name in ['libtiff', 'libjpeg-turbo']:
    wrap = configparser.ConfigParser(interpolation=None)
    wrap.read(projects / (name + '.wrap'))
    data = wrap['wrap-file']
    for kind in ['source', 'patch']:
        source = projects / 'packagecache' / data[kind + '_filename']
        assert hashlib.sha256(source.read_bytes()).hexdigest() == data[kind + '_hash'], source
        shutil.copy2(source, docs / 'sources' / source.name)
        files.append(str((docs / 'sources' / source.name).relative_to(p)))
    licenses = docs / name
    licenses.mkdir(exist_ok=True)
    for filename in ['LICENSE.md', 'LICENSE.build']:
        shutil.copy2(projects / data['directory'] / filename, licenses / filename)
        files.append(str((licenses / filename).relative_to(p)))
    dependencies[name] = dict(version=data['wrapdb_version'].split('-')[0], source=data['source_url'])
manifest = dict(version='4.22.4', license='LGPL-2.1-or-later',
                source='https://download.gnome.org/sources/gtk/4.22/gtk-4.22.4.tar.xz',
                dependencies=dependencies,
                rebuild='bash share/doc/capycanvas-gtk/build.sh "$PWD/gtk-build" "$PWD/gtk-prefix"',
                files={f: hashlib.sha256((p/f).read_bytes()).hexdigest() for f in files})
(p/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
PY
