# Linux development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

Linux with Wayland is the primary development target. New editor features land
here first, then move to Web and the native ports, as the
[platform workflow](../platforms/README.md#development-workflow) describes. The
client uses GTK4/libadwaita for controls and the shared wgpu renderer through
Vulkan.

For the Flatpak runtime, signed downloads and update repository, see
[Linux Flatpak](releasing.md#linux-flatpak).

## Prerequisites

Install a recent stable Rust toolchain, a C/C++ toolchain, `pkg-config`, and the
GTK 4.22+, libadwaita 1.9 and Wayland development packages. Enabled API features
are declared in [`apps/layer-linux/Cargo.toml`](../../apps/layer-linux/Cargo.toml).
The launcher builds its own [GTK 4.24.1, GLib 2.90.1 and Pango 1.58.2 runtime](../../tools/build/gtk-runtime/README.md),
which needs Meson 1.8+, Ninja, `patchelf`, `glslc`, `xz`, Wayland's scanner and protocol files,
DRM headers and GTK's development dependencies. The first build downloads
checksum-pinned sources, including TIFF and JPEG libraries that link statically,
so their system development packages are unnecessary; later builds use the cache.
The main SDK minimums are:

| Dependency | Minimum version |
| --- | --- |
| System GTK development headers / libadwaita | 4.22 / 1.9 |
| Meson | 1.8 |
| Cairo / Fontconfig / HarfBuzz | 1.18.2 / 2.17.0 / 11.0.0 |
| Wayland / Wayland protocols | 1.24.0 / 1.48 |

The Wayland scanner must match the installed Wayland client version.

`python3 tools/build/gtk-runtime/check-env.py` checks the full native prerequisite
set without building. If it fails, complete the distribution setup below before
running the launcher again.

Photo codecs and ICC color management are Rust crates; no libjpeg-turbo,
LittleCMS or HEIF system packages are needed.

On Fedora:

```bash
sudo dnf install gcc gcc-c++ pkgconf-pkg-config python3 curl patch tar \
  gtk4-devel libadwaita-devel wayland-devel libpng-devel zlib-devel \
  libffi-devel libmount-devel pcre2-devel cairo-devel fontconfig-devel freetype-devel \
  harfbuzz-devel fribidi-devel libthai-devel \
  meson ninja-build patchelf glslc xz wayland-protocols-devel libdrm-devel vulkan-headers
```

On Arch Linux or Omarchy:

```bash
bash tools/build/arch-setup.sh
```

This updates the system and installs the package recipe's complete dependencies,
plus the private-display test and Arch packaging tools. Pacman shows the transaction
before installing. Keep the system on one coherent repository snapshot; partial
upgrades and extracted SDK overlays are unsupported.

Fedora splits development headers into `-devel` packages; Arch includes them
with the libraries. `glslc` is provided by Fedora's `glslc` package and Arch's
`shaderc` package. Check the installed versions with:

```bash
pkg-config --modversion gtk4 libadwaita-1 wayland-client
```

The canvas needs a Wayland session and a hardware Vulkan driver with mailbox
presentation and premultiplied-alpha surfaces. There is no X11, GLES or CPU
canvas fallback. GTK chooses its own renderer for the controls.

## Build and run

```bash
./apps/layer-linux/run.sh
```

The launcher runs `cargo run` with the `dev-perf` profile and forwards its
arguments to the app; set `CAPY_RUST_PROFILE=release` for release comparisons.
See [build profiles](environment.md#build-profiles).

To develop against system GTK without building the local runtime, run Cargo
directly with no local GTK override in `LD_LIBRARY_PATH`:

```bash
cargo run --locked --profile dev-perf -p layer-linux
```

This needs only the GTK/libadwaita/Wayland development packages. Use it
without tablet-pad input or with a distribution GTK containing the crash fix;
the [runtime guide](../../tools/build/gtk-runtime/README.md#using-system-gtk)
explains the upstream bug.

External photo import decodes in `capy-photo-reader`, a confined helper that
requires seccomp and `/proc`; without confinement the import is refused. The
package ships the helper beside the application, and `run.sh` builds it.

Where to look in [`apps/layer-linux/src`](../../apps/layer-linux/src):
`main.rs` calls the library's `run`; `lib.rs` creates the application and handles
file launches. `canvas.rs` adapts the shared session and schedules frames against
Wayland presentation timing, `render_thread.rs` owns canvas GPU work, `wayland.rs`
presents into an app-owned subsurface beneath the GTK controls
([design record](../history/wayland-subsurface-feasibility.md)), and `files.rs`
supplies native dialogs and project/photo transport.

### Changing the host

- **Release `RefCell` borrows before callbacks.** GTK setters and host callbacks
  can synchronously dispatch an edit and refresh widgets. Take owned values or
  clone handles before those calls, and keep each collection borrow inside the
  state operation. Release borrows of workflows, sessions and GPU state before
  awaiting dialogs or worker results; when committing a prepared result,
  reborrow and check that its operation is still current.
- **Await alerts with `alert::choose`.** `choose_future` in libadwaita-rs 0.9.2
  keeps an `adw::AlertDialog` alive after it closes.

## Session restart

Quitting keeps every open drawing without a save prompt, and the next launch
restores them, as [session recovery](../internals/session-recovery.md) describes.
`recovery.rs` supplies the GTK timing and worker transport around shared
`SessionCapture`, `SessionManifest` and `SessionStore`. Saved sessions live in
`$XDG_STATE_HOME/capycanvas/sessions` for a packaged build and `capycanvas-devel`
for any other build; the [storage guide](../internals/storage.md) lists every
folder. Session journeys run with `--native-recovery` ([tests](#journeys-with-extra-settings)).

## Native text input

Editable submission paths use `input::guard_entry_activation` for entries and
`input::guard_editable_activation` for editable rows before attaching activate
callbacks or enabling a default dialog action. The guard preserves native IME
delivery and retains the physical key through commit, activation and release.
Candidate confirmation or cancellation must not also submit or dismiss the app
view. Existing Escape handlers consult the same ownership state; focus loss and
unmapping retire it.

## Language changes

Each window prepares language catalogs on a worker and publishes them between
interactions, following the [shared text rules](../ui/localization.md#shared-text).
Widgets that show shared text register with `Workspace::on_localization`; the
callback runs at once with the current `Localizer` and again on each
publication, and returns `false` to unsubscribe. Relabel text, tooltips and
accessible labels in place rather than replacing widgets, so drafts, selection
and focus survive.

`git grep -n 'fn native_.*language' apps/layer-linux/src` lists the language
journeys. `native_genuine_language_composition` gives an IBus acceptance runner
a private application identity; it needs workspace and session storage and
native compositor keys.

## Tests

Model tests run with `cargo test --locked -p layer-linux`, which also builds
`capy-photo-reader` for the confined `photo_reader` integration test. A
standalone `--lib` run needs `cargo build --locked -p layer-linux --bin capy-photo-reader`
first, with the same profile. When copying a test executable elsewhere, copy the
helper beside it. Tests that call file readers directly first initialize
`crate::storage::roots()` so anonymous files use the test's temporary folder,
including a private `CAPY_STORAGE_DIR` override.

Native tests are `#[ignore]`d GTK journeys in `tests.rs` and the `*_tests.rs`
modules, which are named for the area they cover. They need a real Wayland
display, a Vulkan GPU and injected input, so run them through the
private-compositor runner:

```bash
bash tools/performance/workspace-motion.sh gtk --native-test=native_canvas_bar_modes
```

Run pen journeys with `--tablet`; a journey's `#[ignore]` reason often names
the other settings it needs. Journeys check the app's behaviour with injected input: the tablet proxy never
stands in for a physical pen, and synthetic preedit never stands in for an input
method engine.

[`workspace-motion.sh`](../../tools/performance/workspace-motion.sh) builds the
release tests, starts a private D-Bus session, headless Mutter and PipeWire, and
prints the run directory that holds its logs, input records and fresh storage.
It needs Mutter with headless support, GJS and PipeWire besides the build
prerequisites. It uses isolated memory-backed compositor preferences and
disables Mutter's Alt+Space window menu so navigation keys reach the app; on a
desktop that reserves Alt+Space, Ctrl+Alt+Space remains the Zoom out chord.
Useful settings:

| Setting | Effect |
| --- | --- |
| `--native-test=<name>` | Runs the one test with that exact name. |
| `--tablet` | Adds tablet-v2 pen input through a Wayland proxy. |
| `--native-storage` | Gives the test the run's SQLite workspace directory instead of in-memory workspaces. |
| `--native-recovery` | Gives the test the run's session directory, so drawings are checkpointed and restored. |
| `LAYER_NATIVE_TEST_EXECUTABLE` | Absolute path of an already built test executable; skips the rebuild. |
| `LAYER_NATIVE_EVENT_MS` | Interval between injected events: 4 for workspace motion tests, 60 otherwise. Scoped motion tests need `4`. |
| `LAYER_NATIVE_TEST_TIMEOUT_MS` | Watchdog for one test, 180000 by default. |
| `LAYER_MOTION_MIN_HZ` | Presentation floor for scoped motion tests; use `114` on the 120 Hz monitor. |
| `CAPY_NATIVE_TEST_THEME` | Sets fixture windows to `light` or `dark`; run affected journeys once with each. |
| `LAYER_MOTION_VIEWPORT`, `LAYER_MOTION_SCALE` | Private monitor size (default `1600x1000`) and scale, for example `3200x2000` and `2`. |
| `LAYER_TEST_ARTIFACTS` | Absolute directory for captures and reports, where a test writes them. |
| `GTK_A11Y=test` | GTK's accessibility test backend at startup, for journeys that assert accessible names; otherwise `none`. |

Named modes such as `--drag-pickup`, `--workspace-motion`, `--color-panel` or
`--icons` select a fixed test; the mode list is in
[`native-input.js`](../../apps/layer-linux/bench/native-input.js). Test-specific
instructions live with the feature's guide under [`docs/ui/`](../ui/README.md).

[`tools/performance/gtk-raster.sh`](../../tools/performance/gtk-raster.sh) runs an
already built test executable on the same kind of private display, for tests
that need no injected input, such as `native_application_file_launch`.

After changing the tablet proxy, run its tests with
`python3 -m unittest discover -s apps/layer-linux/bench -p 'test_*.py'`.

### Journeys with extra settings

| Journey | Needs |
| --- | --- |
| `native_session_restart`, `native_session_restart_saved_origins`, `native_document_files`, `native_object_only_shared_images` | `--native-recovery` |
| `native_dialog_lines_match_the_palette` | `--native-storage` |
| `native_workspace_switcher_press_feedback` | `--native-storage`, then again with `--tablet` |
| `native_three_finger_scrub`, `native_pressure_calibration` | `--tablet` |
| `native_layer_relationship_review`, pen branch | `LAYER_RELATIONSHIP_PEN=1` and `--tablet` |
| `native_layers_control_copy`, `native_zoom_settings_and_canvas_preferences`, `native_live_language_switching` | `GTK_A11Y=test` |
| `native_clock_portal_preferences` | `GSETTINGS_BACKEND=memory` |
| `native_jpeg_open_import_and_paste` | `LAYER_RASTER_FIXTURES` set to the JPEG reference directory from `tools/validation/jpeg_interchange.py` |
| `native_tiff_open_import_and_paste` | `LAYER_RASTER_FIXTURES` containing the libtiff reference set |
| `native_localized_photo_histogram_bounds`, `native_hue_ranges_colorize_retains_values`, `native_pointwise_filters_controls_and_persistence` | Viewports 1100 and 640 pixels wide, such as `LAYER_MOTION_VIEWPORT=1100x800` and `640x1000` |
| `native_colorize_threshold_visible_artwork`, `native_animated_filter_moves` | A viewport 1100 pixels wide |
| `native_panel_tab_lower_contours` | `LAYER_NATIVE_TEST_TIMEOUT_MS=300000`, at `LAYER_MOTION_SCALE` 1 and 2 |
| `native_window_resize_presentations` | `LAYER_NATIVE_EVENT_MS=4`, `LAYER_NATIVE_INPUT_TRACE=1` and `LAYER_TEST_ARTIFACTS` |
| `native_window_resize_inventory` | `GTK_DEBUG=snapshot`, apart from timing runs |
| `native_workspace_tab_motion_input` | `LAYER_NATIVE_EVENT_MS=4 LAYER_MOTION_MIN_HZ=114` |

### Writing a journey

- **One test per process, isolated storage.** GTK runs on one thread, and each
  native test owns the display, input protocol and storage for its run. Pass one
  exact name with `--exact --test-threads=1`; Cargo's filter is a substring match
  and can start a second journey in the same process. A test stores files only
  under a fresh `CAPY_STORAGE_DIR`, and uses workspaces and sessions only when
  `data/workspaces` or `state/sessions` exist there. The runners create these
  and a private `XDG_DATA_HOME`, which `native_test_app` requires so the file
  chooser never lists or records your recent files.
- **Inject input only on the private display.** `native-input.js` drives
  Mutter's RemoteDesktop API and refuses any display not named `layer-bench-*`.
  Never point it at a desktop session.
- **`GDK_DEBUG=no-portals:color-mgmt`.** GTK 4.22 binds the Wayland
  color-management protocol only with `color-mgmt`; the app sets it in `main()`,
  which tests do not run. `no-portals` gives file dialogs GTK's in-process
  chooser, which tests can drive, and the runners replace the lost desktop font
  and window settings with GNOME's defaults from
  [`gtk-config`](../../tools/performance/gtk-config/gtk-4.0/settings.ini)
  through `XDG_CONFIG_DIRS`. `gtk-raster.sh` and `workspace-motion.sh gtk` set
  all of this; plain `cargo test` sets none of it.
- **Reuse the harness.** Drive real GTK dialogs with `native_test_app`,
  `named::<T>`, `widgets`, `descendant`, `ui_session`, `ui_session_mut` and
  `pump`. Tool-settings journeys share `tool_settings_workspace` for toolbar
  commands and panel placement.
- **Set up the layout under test.** A fixture supplies the workspace, header and
  panel controls it needs, so changes to shipped workspaces don't remove them.
- **Wait, then assert.** Wait for workspace readiness and for operations to finish
  before asserting, and check persistence by reopening, not only by reading rows.
- **Target what is mapped.** Find a published choice by its localized label
  inside the mapped panel, then send native contacts to its containing button.
  Popovers have their own native origins; use `screen_point` for their rows.
- **Tablet proxy limits.** `--tablet` pen serials cannot authorize compositor
  drag-and-drop; use mouse and touch for those journeys. The proxy also drops its
  connection when Quick Mask or Selection Layer rows change, so journeys through
  those modes run without `--tablet`. A pen contact with `"tool":"eraser"` uses
  the pen's eraser end.

### Run a journey without the runner

A focused test can also run without the runner inside an existing Wayland
session, for example to attach a debugger. Give it fresh storage:

```bash
gtk_test_dir=$(mktemp -d)
mkdir -p "$gtk_test_dir/data/workspaces" "$gtk_test_dir/xdg-data"
CAPY_STORAGE_DIR="$gtk_test_dir" XDG_DATA_HOME="$gtk_test_dir/xdg-data" \
GDK_BACKEND=wayland GSK_RENDERER=vulkan G_DEBUG=fatal-criticals RUST_BACKTRACE=1 \
  cargo test --locked --release -p layer-linux \
  workspace::tests::workspace_switcher_tests::native_active_workspace_delete \
  -- --ignored --exact --test-threads=1 --nocapture
```

## Troubleshooting

- **Startup crash with a tablet pad, or an arrow cursor flash on pen entry.** The
  [runtime guide](../../tools/build/gtk-runtime/README.md#using-system-gtk)
  distinguishes GTK's pad-event crash from its pen-entry cursor glitch. The
  package and `run.sh` build and use the same patched GTK, cached in
  `target/gtk-runtime` or `CAPY_GTK_BUILD_DIR`. Direct Cargo commands and tests
  use system GTK unless the local runtime is selected explicitly:

  ```bash
  bash tools/build/gtk-runtime/build.sh target/gtk-runtime target/gtk-runtime/prefix
  LD_LIBRARY_PATH="$PWD/target/gtk-runtime/prefix/lib" cargo test --locked --release -p layer-linux
  ```

- **Canvas colors look darker than the controls.** The canvas surface describes
  itself with the explicit piecewise sRGB transfer function (color-management v2
  TF 14), never legacy TF 9, which Mutter treats as gamma 2.2. Check that
  `GDK_DEBUG` reaches GTK with `color-mgmt`, and that the compositor offers the
  protocol; without it, canvas and controls fall back to untagged sRGB together.

## Stage a native bundle

Arch's setup command includes Node.js, `strip`, `desktop-file-validate` and
`cargo-about`. On other distributions, install these packaging tools too;
`cargo install cargo-about --version 0.9.2 --features cli --locked` installs
the license tool when the distribution does not package it.

```bash
node apps/layer-linux/package.mjs
dist/capycanvas-linux/bin/capycanvas
```

The packager builds a release binary with the `release-identity` feature, which
gives it the application ID `art.capycanvas.CapyCanvas` and the `capycanvas`
folders; every other build runs as `art.capycanvas.CapyCanvas.Devel` with
`capycanvas-devel` folders, so it never shares a running instance or files with
an installed package ([storage](../internals/storage.md)). It bundles the
[patched GTK runtime](../../tools/build/gtk-runtime/README.md) with its sources
and rebuild script in `share/doc/capycanvas-gtk`; libadwaita and the remaining
dependencies stay system requirements.

The output holds the executable, desktop entry, AppStream metainfo, `.capy` MIME
definition, icon, runtime filters, GTK runtime and notices. Staging replaces only
a directory carrying the generated `.capy-package` marker and installs nothing;
the [publication guide](publication.md) covers distribution. The GTK and Web
packagers share `tools/build/about.toml`, need original license texts including
vendored dependencies, and accept `LAYER_CARGO_ABOUT` for the notice generator.

To check that a relocated package opens photos with its bundled GTK, on a
private compositor with isolated settings:

```bash
python3 tools/validation/gtk_package_photo.py \
  --binary /path/to/relocated/capycanvas-linux/bin/capycanvas \
  --photo /path/to/photo.heic --photo /path/to/photo.avif \
  --output artifacts/package-photo-check
```

### Flatpak runtime and portals

The [AppStream metainfo](../../apps/layer-linux/art.capycanvas.CapyCanvas.metainfo.xml)
declares desktop and tablet support with a minimum shortest display edge of 600
logical pixels; phone layouts are unsupported. Validate changes with
`appstreamcli validate --no-net apps/layer-linux/art.capycanvas.CapyCanvas.metainfo.xml`
and `appstreamcli check-syscompat --details apps/layer-linux/art.capycanvas.CapyCanvas.metainfo.xml`.
GNOME Software can label the app Adaptive or warn about display compatibility on
a desktop without checking the declared limits; the AppStream check is the
reference.

The listing's translated text comes from the shared
[Fluent catalogs](../ui/localization.md#application-names);
`tools/build/app_names.py catalog` adds every shipped language to the compressed
catalog. Check the generator with
`python3 -m unittest discover -s tools/build -p 'test_app_names.py'`. Catalog
generation uses temporary `.Devel` build metadata so the SDK's Glycin icon
loader runs without a desktop portal; the exported application keeps
`art.capycanvas.CapyCanvas` as its identity.

With Flatpak and its host SVG image loader installed (`librsvg2-common` on
Debian or Ubuntu), build and export a local unsigned bundle:

```bash
bash packaging/flatpak/build.sh
bash packaging/flatpak/export.sh
```

The local output is `dist/flatpak/capycanvas-<version>-linux-x86_64.flatpak`.
Unsigned builds do not produce the reference or repository archive and do not
configure the published update source. Native distribution builds can also use
the [Arch recipe](../../packaging/arch/README.md).

Users need Flatpak, a Wayland session and hardware Vulkan; the runtime
supplies toolkit dependencies, not the [canvas requirements](#prerequisites).
The sandbox grants the Wayland socket for windows, clipboard and input, and GPU
devices for rendering. The battery indicator reads the kernel's
`/sys/class/power_supply` files, which Flatpak exposes read-only. To test it in
the sandbox, mount private power-supply fixtures read-only over the kernel paths
in an isolated namespace; do not add Flatpak permissions or change the real
system's files.

GTK uses the default portals for file dialogs, selected-file access and opening
links; the clock observes GNOME's time format through the Settings portal.
Settings, workspace libraries, recovery files and caches use Flatpak's private
application directories. The exported desktop entry forwards files through the
document portal when launched from a file manager.

If opening a selected file fails with `Transport endpoint is not connected`,
check the document portal's FUSE mount with
`findmnt -T "$XDG_RUNTIME_DIR/doc"`; its filesystem type should be `fuse.portal`.
A running `xdg-document-portal` service can still have a disconnected or missing
mount. Repair the shared service only when other sessions' Flatpak applications
are closed and the shared-machine rules allow it:
`systemctl --user restart xdg-document-portal.service`. Reopen applications so
their sandboxes receive the restored mount.
