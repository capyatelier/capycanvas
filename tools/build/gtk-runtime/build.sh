#!/usr/bin/env bash
# Build a replaceable, application-local GTK. Never install system libraries.
set -euo pipefail
gtk_recipe=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
gtk_build=$(realpath -m "${1:?Usage: build.sh BUILD_DIRECTORY PREFIX}")
gtk_prefix=$(realpath -m "${2:?Usage: build.sh BUILD_DIRECTORY PREFIX}")
mkdir -p "$gtk_build" "$gtk_prefix/lib" "$gtk_prefix/share/doc/capycanvas-gtk/sources"
gtk_docs="$gtk_prefix/share/doc/capycanvas-gtk"
gtk_archive=gtk-4.24.1.tar.xz
glib_archive=glib-2.90.1.tar.xz
pango_archive=pango-1.58.2.tar.xz
python3 "$gtk_recipe/check-env.py"
for gtk_source in \
    "$gtk_archive e98abe720e16129c8c0f50761dee0a2e9ae2478055e31018f6cf977fcf6513e9 https://download.gnome.org/sources/gtk/4.24" \
    "$glib_archive 93c941aa17d5eb1d53fe838365f29a8b4e539c222a256d974ec8f30fc413e396 https://download.gnome.org/sources/glib/2.90" \
    "$pango_archive 342385b6ca3b7c73455d7c80a13b7dbe4489e00bc3bd4c5bd6ed4dce421e374a https://download.gnome.org/sources/pango/1.58"; do
    read -r gtk_source_file gtk_source_hash gtk_source_url <<< "$gtk_source"
    if [[ ! -f "$gtk_build/$gtk_source_file" ]]; then
        if [[ -f "$gtk_recipe/sources/$gtk_source_file" ]]; then
            cp "$gtk_recipe/sources/$gtk_source_file" "$gtk_build/"
        elif [[ ${CARGO_NET_OFFLINE:-false} == true ]]; then
            echo "GTK source archive is missing from the offline cache: $gtk_source_file" >&2
            exit 1
        else
            curl --fail --location "$gtk_source_url/$gtk_source_file" -o "$gtk_build/$gtk_source_file"
        fi
    fi
    python3 - "$gtk_build/$gtk_source_file" "$gtk_source_hash" <<'PY'
import hashlib, pathlib, sys
assert hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest() == sys.argv[2], 'Runtime source hash mismatch'
PY
done
# Re-extract the verified archive before applying a changed patch.
gtk_patch_hash=$(cat "$gtk_build/$gtk_archive" "$gtk_recipe/pad-event-surface.patch" "$gtk_recipe/tablet-proximity-cursor.patch" "$gtk_recipe/vulkan-memory-pool.patch" | sha256sum | cut -d ' ' -f 1)
if [[ ! -f "$gtk_build/patched.sha256" ]] || [[ $(cat "$gtk_build/patched.sha256") != "$gtk_patch_hash" ]]; then
    tar -xf "$gtk_build/$gtk_archive" -C "$gtk_build"
    patch -d "$gtk_build/gtk-4.24.1" -p1 < "$gtk_recipe/pad-event-surface.patch"
    patch -d "$gtk_build/gtk-4.24.1" -p1 < "$gtk_recipe/tablet-proximity-cursor.patch"
    patch -d "$gtk_build/gtk-4.24.1" -p1 < "$gtk_recipe/vulkan-memory-pool.patch"
    echo "$gtk_patch_hash" > "$gtk_build/patched.sha256"
fi
python3 - "$gtk_recipe/sources" "$gtk_build/gtk-4.24.1/subprojects" <<'PY'
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
gtk_wrap_mode=nofallback
if [[ ${CARGO_NET_OFFLINE:-false} == true ]]; then gtk_wrap_mode=nodownload; fi
gtk_c_args="${CFLAGS:+$CFLAGS }-fPIC $(printf '%q' "-ffile-prefix-map=$gtk_build=/capycanvas-gtk")"
if [[ ! -d "$gtk_build/glib-2.90.1" ]]; then tar -xf "$gtk_build/$glib_archive" -C "$gtk_build"; fi
python3 "$gtk_recipe/configure.py" "$gtk_build/glib-build" "$gtk_build/glib-2.90.1" \
    --prefix=/usr --libdir=lib --libexecdir=lib --buildtype=release --default-library=shared \
    "-Dc_args=$gtk_c_args" "-Dc_link_args=${LDFLAGS:-}" --wrap-mode="$gtk_wrap_mode" \
    "-Dpkg_config_path=${PKG_CONFIG_PATH:-}" \
    -Dtests=false -Dinstalled_tests=false -Ddocumentation=false -Dintrospection=disabled \
    -Dman-pages=disabled -Dselinux=disabled -Dlibmount=enabled -Dsysprof=disabled \
    -Dlibelf=disabled -Ddtrace=disabled -Dsystemtap=disabled -Dglib_debug=disabled
ninja -C "$gtk_build/glib-build" -j "${CAPY_BUILD_JOBS:-8}"
export PKG_CONFIG_PATH="$gtk_build/glib-build/meson-uninstalled${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
if [[ ! -d "$gtk_build/pango-1.58.2" ]]; then tar -xf "$gtk_build/$pango_archive" -C "$gtk_build"; fi
python3 "$gtk_recipe/configure.py" "$gtk_build/pango-build" "$gtk_build/pango-1.58.2" \
    --prefix=/usr --libdir=lib --buildtype=release --default-library=shared \
    "-Dc_args=$gtk_c_args" "-Dc_link_args=${LDFLAGS:-}" --wrap-mode="$gtk_wrap_mode" \
    "-Dpkg_config_path=$PKG_CONFIG_PATH" \
    -Ddocumentation=false -Dman-pages=false -Dintrospection=disabled \
    -Dbuild-testsuite=false -Dbuild-examples=false -Dxft=disabled -Dsysprof=disabled -Dlibthai=enabled
ninja -C "$gtk_build/pango-build" -j "${CAPY_BUILD_JOBS:-8}"
export PKG_CONFIG_PATH="$gtk_build/pango-build/meson-uninstalled:$PKG_CONFIG_PATH"
python3 "$gtk_recipe/configure.py" "$gtk_build/build" "$gtk_build/gtk-4.24.1" \
    --buildtype=release "-Dc_args=$gtk_c_args" "-Dc_link_args=${LDFLAGS:-}" \
    "-Dpkg_config_path=$PKG_CONFIG_PATH" \
    --wrap-mode="$gtk_wrap_mode" --force-fallback-for=libtiff,libjpeg-turbo \
    -Dlibtiff:default_library=static -Dlibtiff:jbig=disabled -Dlibtiff:lerc=disabled \
    -Dlibtiff:lzma=disabled -Dlibtiff:webp=disabled -Dlibtiff:zstd=disabled \
    -Dlibtiff:jpeg=enabled -Dlibtiff:zlib=enabled \
    -Dlibjpeg-turbo:default_library=static \
    -Dlibjpeg-turbo:jpeg-turbo=disabled -Dlibjpeg-turbo:tests=disabled \
    -Dbuild-demos=false -Dbuild-tests=false \
    -Dbuild-testsuite=false -Dbuild-examples=false -Dintrospection=disabled \
    -Dmedia-gstreamer=disabled -Ddocumentation=false -Dman-pages=false -Dx11-backend=false
ninja -C "$gtk_build/build" -j "${CAPY_BUILD_JOBS:-8}" gtk/libgtk-4.so.1.2400.1
for gtk_family in glib gobject gio gmodule gthread; do
    gtk_library="lib$gtk_family-2.0.so.0"
    cp "$gtk_build/glib-build/$gtk_family/$gtk_library" "$gtk_prefix/lib/$gtk_library.new"
    patchelf --set-rpath '$ORIGIN' "$gtk_prefix/lib/$gtk_library.new"
    mv -f "$gtk_prefix/lib/$gtk_library.new" "$gtk_prefix/lib/$gtk_library"
done
for gtk_family in pango pangocairo pangoft2; do
    gtk_library="lib$gtk_family-1.0.so.0"
    cp "$gtk_build/pango-build/pango/$gtk_library" "$gtk_prefix/lib/$gtk_library.new"
    patchelf --set-rpath '$ORIGIN' "$gtk_prefix/lib/$gtk_library.new"
    mv -f "$gtk_prefix/lib/$gtk_library.new" "$gtk_prefix/lib/$gtk_library"
done
cp "$gtk_build/build/gtk/libgtk-4.so.1.2400.1" "$gtk_prefix/lib/libgtk-4.so.1.new"
patchelf --set-rpath '$ORIGIN' "$gtk_prefix/lib/libgtk-4.so.1.new"
mv -f "$gtk_prefix/lib/libgtk-4.so.1.new" "$gtk_prefix/lib/libgtk-4.so.1"
cp "$gtk_build/gtk-4.24.1/COPYING" "$gtk_docs/COPYING"
cp "$gtk_build/$gtk_archive" "$gtk_docs/sources/"
cp "$gtk_build/$glib_archive" "$gtk_docs/sources/"
mkdir -p "$gtk_docs/glib"
cp "$gtk_build/glib-2.90.1/COPYING" "$gtk_docs/glib/"
cp "$gtk_build/$pango_archive" "$gtk_docs/sources/"
mkdir -p "$gtk_docs/pango"
cp "$gtk_build/pango-1.58.2/COPYING" "$gtk_docs/pango/"
cp "$gtk_recipe/pad-event-surface.patch" "$gtk_recipe/tablet-proximity-cursor.patch" "$gtk_recipe/vulkan-memory-pool.patch" "$gtk_recipe/build.sh" "$gtk_recipe/configure.py" "$gtk_recipe/check-env.py" "$gtk_docs/"
python3 - "$gtk_prefix" "$gtk_build/gtk-4.24.1/subprojects" <<'PY'
import configparser, hashlib, json, pathlib, shutil, sys
p = pathlib.Path(sys.argv[1])
projects = pathlib.Path(sys.argv[2])
docs = p / 'share/doc/capycanvas-gtk'
files = ['lib/libgtk-4.so.1', 'share/doc/capycanvas-gtk/COPYING',
         'share/doc/capycanvas-gtk/sources/gtk-4.24.1.tar.xz',
         'share/doc/capycanvas-gtk/pad-event-surface.patch',
         'share/doc/capycanvas-gtk/tablet-proximity-cursor.patch',
         'share/doc/capycanvas-gtk/vulkan-memory-pool.patch', 'share/doc/capycanvas-gtk/build.sh',
         'share/doc/capycanvas-gtk/configure.py', 'share/doc/capycanvas-gtk/check-env.py',
         'share/doc/capycanvas-gtk/sources/glib-2.90.1.tar.xz',
         'share/doc/capycanvas-gtk/glib/COPYING',
         'share/doc/capycanvas-gtk/sources/pango-1.58.2.tar.xz',
         'share/doc/capycanvas-gtk/pango/COPYING']
files += [f'lib/lib{name}-2.0.so.0' for name in ['glib', 'gobject', 'gio', 'gmodule', 'gthread']]
files += [f'lib/lib{name}-1.0.so.0' for name in ['pango', 'pangocairo', 'pangoft2']]
dependencies = dict(glib=dict(version='2.90.1', source='https://download.gnome.org/sources/glib/2.90/glib-2.90.1.tar.xz'),
                    pango=dict(version='1.58.2', source='https://download.gnome.org/sources/pango/1.58/pango-1.58.2.tar.xz'))
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
manifest = dict(version='4.24.1', license='LGPL-2.1-or-later',
                source='https://download.gnome.org/sources/gtk/4.24/gtk-4.24.1.tar.xz',
                dependencies=dependencies,
                rebuild='bash share/doc/capycanvas-gtk/build.sh "$PWD/gtk-build" "$PWD/gtk-prefix"',
                files={f: hashlib.sha256((p/f).read_bytes()).hexdigest() for f in files})
(p/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
PY
