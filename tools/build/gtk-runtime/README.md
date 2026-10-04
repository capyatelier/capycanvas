# Packaged GTK runtime

The Linux packager and development launcher invoke `build.sh BUILD_DIRECTORY PREFIX`
and use the resulting GTK 4.22.4 library. `pad-event-surface.patch` prevents a null
Wayland pad-mode event surface from being dereferenced before keyboard focus.
Device state and targeted input remain enabled.
`tablet-proximity-cursor.patch` waits for the pen's first positioned motion
before delivering its window entry. A proximity-only frame must not choose a
cursor using stale coordinates and briefly flash an arrow over the canvas.
It also suppresses GDK's cached surface cursor during entry, allowing GTK's
widget pick to install the first visible cursor.

After building, check the actual patched callbacks with split proximity/motion
frames, stale cursor suppression, canvas/control cursors, repeated motion, and
departure before any motion:

```sh
python3 tools/build/gtk-runtime/test-tablet-entry.py target/gtk-runtime/gtk-4.22.4
python3 tools/build/gtk-runtime/test-runtime.py \
  target/gtk-runtime/prefix target/gtk-runtime/gtk-4.22.4/subprojects
python3 tools/build/gtk-runtime/test-configure.py
```

The source archive is pinned by SHA-256. TIFF and JPEG use GTK's checksum-pinned
Meson wraps and link statically into the local GTK library. System TIFF and JPEG
development packages are unnecessary. TIFF enables JPEG and deflate compression;
its optional JBIG, LERC, LZMA, WebP and Zstandard dependencies are disabled.
The build needs GTK's remaining system development dependencies, Wayland protocol
files, DRM headers, Meson, Ninja, `pkg-config`, a C compiler, `glslc` and `sassc`. It neither
installs system packages nor replaces system GTK. An optional `deps/usr` under
the build directory can supply extracted development headers/tools, as in the
recorded Fedora review build. The prefix contains the library, source archive,
licenses, patches, standalone build script and a checksum manifest, including
the TIFF and JPEG source archives and Meson build overlays. Rebuilds seed Meson's
package cache from these shipped sources, so they need no codec downloads.

`apps/layer-linux/package.mjs` and `run.sh` default to `target/gtk-runtime` as their cache;
`CAPY_GTK_BUILD_DIR` can select another cache. The launcher selects the bundled
library using an executable-relative path that survives relocation and symlinks.
Applications can replace the local library with a compatible rebuilt version.
GTK's other shared dependencies and libadwaita remain system requirements.
When a recipe introduces options absent from a configured cache, setup uses
Meson's `--wipe`; known options use incremental reconfiguration. The installed
library stays intact until the replacement finishes. The shipped rebuild script
includes the same `configure.py` helper.

From a relocated package, rebuild the supplied source without a network fetch:

```sh
bash share/doc/capycanvas-gtk/build.sh "$PWD/gtk-build" "$PWD/gtk-prefix"
```

Package smoke validation checks the actual mapped GTK library path;
source-file presence alone is not the startup qualification.
