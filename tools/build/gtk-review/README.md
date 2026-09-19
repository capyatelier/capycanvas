# GTK runtime for HDR review

Normal Linux packages now include this fix through
[`../gtk-runtime/build.sh`](../gtk-runtime/build.sh) and
`apps/layer-linux/package.mjs`. This directory's `build.sh` is the compatibility
entry point for the archived review launcher and delegates to that same recipe.
The package includes the complete source archive, patch, LGPL license and rebuild
instructions; it does not depend on a user's review-artifact directory.

`bash tools/build/gtk-review/build.sh` builds GTK 4.22.4 into
`artifacts/color-m4/review/runtime`. It never installs system packages.
The source archive is pinned by SHA256 and retained next to its build.
Normal GTK development dependencies, Meson, Ninja, and glslc are required.

The recorded Fedora 44 build used the existing system development environment
plus these downloaded, **uninstalled** packages: `libtiff-devel`,
`libjpeg-turbo-devel`, `libwebp-devel`, `liblerc-devel`, `libdrm-devel`, `glslc`
(all x86_64), and `wayland-protocols-devel` (noarch). The RPMs and extraction
helper are retained in `artifacts/color-m4/gtk-runtime`; their headers and
pkg-config files are local to that directory. Shared dependencies still resolve
to the existing system libraries. Missing optional video playback is disabled;
the Wayland, X11, Vulkan, print, and tablet code follows GTK's normal configuration.

The 2026-09-17 desktop crash dump identifies event type 27 (`GDK_PAD_GROUP_MODE`),
a null event surface, and a fault in `gdk_surface_handle_event` when checking
whether that surface is mapped. Wayland's pad mode callback updates its device
state and constructs an event from `seat->keyboard_focus`, which can be null.
The patch returns “unhandled” for an event without a surface. Targeted events,
pad mode tracking, and tablet/pen input are unchanged. This is a local dependency
fix, not a workaround that disables tablet support or delays window creation.

The app binary also works with compatible system GTK installations; systems
with this GTK bug still need the runtime patch or an upstream equivalent.
Physical pen/touch ergonomics and other platform runtimes remain unqualified.

Evidence and source paths: `docs/history/color-management-gtk-m4-feedback.md`.
