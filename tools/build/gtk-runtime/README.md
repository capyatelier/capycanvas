# Packaged GTK runtime

The Linux packager and development launcher invoke `build.sh BUILD_DIRECTORY PREFIX`
and use the resulting GTK 4.24.1, GLib 2.90.1 and Pango 1.58.2 libraries.
`pad-event-surface.patch` backports
[GTK !10171](https://gitlab.gnome.org/GNOME/gtk/-/merge_requests/10171) at commit
`c6613dd0ad4e1bde77b6cd2e48cfcc80c8e63f33`, with its focus/device guard extended
to button, ring, strip and dial events. One checked-in patch contains both changes;
builds use the pinned release archive and do not fetch a moving branch or MR diff.

Pad events use the surface from pad entry independently of keyboard focus.
Events without a pad target or device are discarded before construction; mode
state still updates. References are released on leave and pad removal. If a
surface is destroyed before leave, the retained reference keeps it valid and
normal GTK dispatch drops its unmapped events. If destruction precedes entry
dispatch, the guards also handle the missing surface and device.
`tablet-proximity-cursor.patch` waits for the pen's first positioned motion
before delivering its window entry. A proximity-only frame must not choose a
cursor using stale coordinates and briefly flash an arrow over the canvas.
It also suppresses GDK's cached surface cursor during entry, allowing GTK's
widget pick to install the first visible cursor.

`vulkan-memory-pool.patch` pools up to four large allocations per memory type,
using eight size classes per power-of-two range and retaining mapped blocks.
Returned blocks expire after 100 ms, checked on returns and by a timer that stops
when empty.
Small buddy allocations and texture-cache cleanup are unchanged. Remove this
patch when the pinned GTK release provides equivalent bounded reuse.

## Using system GTK

Capy Canvas uses public GTK APIs and can link to system GTK. Direct Cargo
builds use it unless `LD_LIBRARY_PATH` selects the local runtime; see the
[Linux guide](../../../docs/development/linux.md#build-and-run).

The crash fix corrects GTK's event producers. A Wayland tablet-pad mode
event can arrive after pad entry but before keyboard focus. GTK creates the
event with `seat->keyboard_focus` as its surface, then
`gdk_surface_handle_event` dereferences it before emitting `GdkSurface::event`.
Application event controllers cannot discard the event before this access.
Dropping NULL surfaces in dispatch alone also misses GTK's earlier debug sanity
check and still misroutes events when the keyboard targets a different window.
GTK 4.22.4 and 4.22.5 contain this path, and the official
[GTK 4.24.1 source](https://download.gnome.org/sources/gtk/4.24/gtk-4.24.1.tar.xz)
still contains both the nullable event surface and its unchecked dereference.
Requiring a newer version alone does not establish that the crash is fixed.

GTK's documented
[`GDK_WAYLAND_DISABLE`](https://docs.gtk.org/gtk4/running.html#gdk-wayland-disable)
can disable `zwp_tablet_manager_v2`, but that removes native pen pressure,
tool axes and pad input together. It is unsuitable as the painting client's
default workaround. System GTK is usable without the affected pad input, or
with a distribution build that fixes this event path.

Remove the pad patch when the pinned GTK release contains independent pad focus
and guards every pad-event producer, then run the pad regression below and
verify pad input while opening and closing windows. The cursor patch addresses
a separate visual glitch; verify pen entry separately before removing it.
The bundled runtime can be retired when supported system GTK builds contain the
input fixes and equivalent bounded Vulkan allocation reuse.

## Build and validate

After building, check the actual patched callbacks with split proximity/motion
frames, stale cursor suppression, canvas/control cursors, repeated motion, and
departure before any motion:

```sh
python3 tools/build/gtk-runtime/test-tablet-entry.py target/gtk-runtime/gtk-4.24.1
python3 tools/build/gtk-runtime/test-pad-focus.py target/gtk-runtime/gtk-4.24.1
python3 tools/build/gtk-runtime/test-vulkan-memory.py target/gtk-runtime/gtk-4.24.1
python3 tools/build/gtk-runtime/test-runtime.py \
  target/gtk-runtime/prefix target/gtk-runtime/gtk-4.24.1/subprojects
python3 tools/build/gtk-runtime/test-configure.py
python3 tools/build/gtk-runtime/test-environment.py
```

The pad check executes GTK's actual callbacks without a display, checking all
event producers, independent keyboard/pad focus, retained mode state, missing
devices, surface destruction before initial entry, tablet reassociation and
balanced surface references through destruction, leave and removal.
It rejects the unpatched source and missing producer guards when an event has
no target. Native pen journeys still run through the private compositor in the
[Linux guide](../../../docs/development/linux.md#tests).

The GTK, GLib and Pango source archives are pinned by SHA-256. TIFF and JPEG use GTK's checksum-pinned
Meson wraps and link statically into the local GTK library. System TIFF and JPEG
development packages are unnecessary. TIFF enables JPEG and deflate compression;
its optional JBIG, LERC, LZMA, WebP and Zstandard dependencies are disabled.

Install the [Linux prerequisites](../../../docs/development/linux.md#prerequisites).
The builder checks the native tools, the remaining SDK package versions, actual
headers and GLib resource compiler before source downloads or compilation.
GLib and Pango build first; GTK resolves their uninstalled pkg-config files
explicitly, including when reconfiguring an existing cache. System Pango 1.57
therefore does not prevent building GTK, which needs Pango 1.58 or newer.
The builder uses the installed SDK; extracted packages under the build cache
do not supply tools, headers or libraries.
The runtime builder neither installs system packages nor replaces system GTK.
The prefix contains the libraries, source archives,
licenses, patches, standalone build script and a checksum manifest, including
the TIFF and JPEG source archives and Meson build overlays. Rebuilds seed Meson's
package cache from these shipped sources, so they need no codec downloads.

`apps/layer-linux/package.mjs` and `run.sh` default to `target/gtk-runtime` as their cache;
`CAPY_GTK_BUILD_DIR` can select another cache. The launcher selects the bundled
library using an executable-relative path that survives relocation and symlinks.
Applications can replace the local library with a compatible rebuilt version.
GLib and Pango are bundled so GTK 4.24 runs with the documented older system SDK,
including Omarchy. Pango includes its Cairo and FreeType libraries and Thai
dictionary support; the fonts, shaping, rasterization and Wayland SDK remain
system dependencies with version requirements checked before the build.
Each private library resolves its siblings through `$ORIGIN`; system GTK, GLib and Pango
remain untouched. Libadwaita and GTK's remaining shared dependencies stay system
requirements.
When a recipe introduces options absent from a configured cache, setup uses
Meson's `--wipe`; source-version changes also replace the configured build. Known
options in the same source tree use incremental reconfiguration. The installed
library stays intact until the replacement finishes. The shipped rebuild script
includes the same `configure.py` and `check-env.py` helpers.

From a relocated package, rebuild the supplied source without a network fetch:

```sh
bash share/doc/capycanvas-gtk/build.sh "$PWD/gtk-build" "$PWD/gtk-prefix"
```

Package smoke validation checks the actual mapped GTK library path;
source-file presence alone is not the startup qualification.
`test-runtime.py` also checks private Pango's version and loaded path, lays out
multilingual text, loads system libadwaita against all nine private libraries,
and validates source/license checksums with `LD_LIBRARY_PATH` unset. CI builds
the launcher from a clean Arch SDK, runs this probe and repeats the launcher
build offline to catch incomplete source or dependency declarations.
