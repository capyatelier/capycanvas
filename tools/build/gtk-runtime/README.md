# Packaged GTK runtime

The normal Linux packager invokes `build.sh BUILD_DIRECTORY PREFIX` and ships
the resulting GTK 4.22.4 library. `pad-event-surface.patch` prevents a null
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
```

The source archive is pinned by SHA-256. The build needs GTK's system development
dependencies, Meson, Ninja, `pkg-config`, a C compiler and `glslc`. It neither
installs system packages nor replaces system GTK. An optional `deps/usr` under
the build directory can supply extracted development headers/tools, as in the
recorded Fedora review build. The prefix contains the library, source archive,
license, patch, standalone build script and a checksum manifest.

`apps/layer-linux/package.mjs` defaults to `target/gtk-runtime` as its cache;
`CAPY_GTK_BUILD_DIR` can select another cache. The launcher selects the bundled
library using an executable-relative path that survives relocation and symlinks.
Applications can replace the local library with a compatible rebuilt version.
GTK's other shared dependencies and libadwaita remain system requirements.

From a relocated package, rebuild the supplied source without a network fetch:

```sh
bash share/doc/capycanvas-gtk/build.sh /tmp/capy-gtk-build /tmp/capy-gtk-prefix
```

Package smoke validation checks the actual mapped GTK library path;
source-file presence alone is not the startup qualification.
