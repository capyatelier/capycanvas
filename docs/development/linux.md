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
The launcher also builds the [patched GTK runtime](../../tools/build/gtk-runtime/README.md),
which needs Meson, Ninja, `glslc`, `sassc`, Wayland protocol files, DRM headers
and GTK's development dependencies. Its TIFF and JPEG libraries build from
checksum-pinned sources and link statically; their system development packages
are unnecessary. The first build downloads those sources; later builds use the cache.
Photo codecs and ICC color management are Rust crates; no libjpeg-turbo,
LittleCMS or HEIF system packages are needed.

On Fedora:

```bash
sudo dnf install gcc gcc-c++ pkgconf-pkg-config python3 curl patch tar \
  gtk4-devel libadwaita-devel wayland-devel libpng-devel zlib-devel \
  meson ninja-build glslc sassc wayland-protocols-devel libdrm-devel vulkan-headers
```

On Arch Linux or Omarchy, with a current system:

```bash
sudo pacman -S --needed base-devel rust python curl glib2-devel gtk4 libadwaita wayland \
  libpng zlib meson ninja shaderc sassc wayland-protocols libdrm vulkan-headers
```

GTK still requires its native toolkit libraries, fonts and Wayland/Vulkan
interfaces. These are Linux host requirements, separate from the portable
Rust core. Fedora splits development headers into `-devel` packages; Arch
includes them with the libraries. `glslc` is provided by Fedora's `glslc`
package and Arch's `shaderc` package. `sassc` is a build tool on both.

```bash
pkg-config --modversion gtk4 libadwaita-1 wayland-client
```

The canvas needs a Wayland session and a hardware Vulkan driver with mailbox
presentation and premultiplied-alpha surfaces. There is no X11, GLES or CPU
canvas fallback. GTK chooses its own renderer for the controls.

On Omarchy the native libraries are in the base install; add Rust with
`omarchy install dev-env rust`. The app runs directly in its Wayland session and
advertises `art.capycanvas.CapyCanvas`, matching its desktop entry and icon.

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

This needs the GTK/libadwaita/Wayland development packages, but none of the
additional GTK source-build tools. System GTK can serve setups without the
affected tablet-pad input, or a distribution build containing the crash fix.
The [runtime guide](../../tools/build/gtk-runtime/README.md#using-system-gtk)
explains the upstream bug and the limits of application workarounds.

Where to look in [`apps/layer-linux/src`](../../apps/layer-linux/src):
`main.rs` calls the library's `run`; `lib.rs` creates the application and handles
file launches. `canvas.rs` adapts
the shared session and schedules frames against Wayland presentation timing,
`render_thread.rs` owns canvas GPU work, `wayland.rs` presents into an app-owned
subsurface beneath the GTK controls ([design record](../history/wayland-subsurface-feasibility.md)),
and `files.rs` supplies native dialogs and project/photo transport.

GTK setters and host callbacks can synchronously dispatch an edit and refresh
widgets. Release `RefCell` guards before those calls by taking owned values or
cloning handles; keep each collection borrow inside the state operation.

Queued frames retain immutable typed scene snapshots and shared source roots.
Save, recovery, export, clipboard and color/source comparisons enqueue a capture
barrier beside those roots. File workers wait for the preceding successful GPU
submission and use its integrated effect phases; the GTK input thread keeps
processing events. Native saves write source-only packages without a preview.

The clock reads GNOME's time-format preference asynchronously through the
Settings portal and follows live changes and service restarts. When unavailable,
it uses GSettings and the locale. Check the portal and fallback paths on the
private display with
`GSETTINGS_BACKEND=memory bash tools/performance/workspace-motion.sh gtk --native-test=native_clock_portal_preferences`.

Imported packages retain their original backing through tab admission, embedded
effect validation and asynchronous GPU startup. A failed preparation restores the
current drawing and presents the shared `PackageView`; cancellation stays a
cancelled open. Unsupported and recovered packages also retain their original
backing in that view. GTK presents the bounded preview and shared reason in a read-only
dialog. Copy Original File writes the retained package bytes atomically on a
worker. Export Preview Image writes the verified PNG to a separate destination
and rejects the original path and filesystem aliases before publication. Closing
the dialog leaves the editable drawing and recovery source intact.

## Session restart

Quitting preserves every open drawing without a save prompt. The next launch
restores the active drawing first, then the other tabs in their saved order,
including clean and untitled drawings. Camera, selection, editing targets,
modified state and bounded Undo/Redo history come from the shared private
session codec. An ordinary restart retains the normal title. Drawings restored after an
interrupted exit or from a previous complete checkpoint carry the shared
recovered caption until Save succeeds. Closing a tab still uses Save/Discard/Cancel and removes its
membership durably before retiring the private copy.

`recovery.rs` supplies the GTK timing and worker transport around shared
`SessionCapture`, `SessionManifest` and `SessionStore`. Private checkpoints run
at a two-second observation interval, skip unchanged state and reuse immutable
resource files. File encoding, validation, publication and cleanup run on
workers. The window stops accepting new edits during an orderly exit and waits
asynchronously for drawing and workspace publication. Storage failure keeps the
window open and preserves the previous complete checkpoint.

Saved sessions live in `$XDG_STATE_HOME/capycanvas/sessions` for a packaged
build and `capycanvas-devel` for any other build; the
[storage guide](../internals/storage.md) lists every folder. Window and drawing
locks protect live owners;
unfinished restoration attempts are retained and skipped on subsequent launch.
Saving to a restored destination verifies its original immutable bytes before
replacement; an unavailable or externally changed destination opens Save As.
Save also opens the destination chooser when the retained file is no longer
writable, including read-only document-portal grants from file transfers.
Restoring a saved drawing also checks that original on a worker. A missing or
changed original keeps the private copy unsaved until Save or explicit Discard.
The private checkpoint never overwrites the artist's project file.

`native_session_restart` checks automatic multi-tab restart, order, active tab,
camera, modified state, retained Undo/Redo and explicit close cancellation and
discard. `native_session_restart_saved_origins` checks that intact saved originals
remain clean, missing or changed originals require an explicit close decision,
and Save As preserves the restored drawing. It also changes Selection Layer
overlay opacity and visibility through native controls, checks the displayed pixels
and clean artwork state, and verifies that private restart retains the display
settings while portable files omit them. `native_document_files` checks saved
project pixels and preservation of the clean drawing's private copy. Run each
through the private-display runner with `--native-recovery`, and set `CAPY_NATIVE_TEST_THEME=light`
or `dark` for both presentations.

## Native text input

Editable submission paths use `input::guard_entry_activation` for entries and
`input::guard_editable_activation` for editable rows before attaching activate
callbacks or enabling a default dialog action. The guard preserves native IME
delivery and retains the physical key through commit, activation and release.
Candidate confirmation or cancellation must not also submit or dismiss the app
view. Existing Escape handlers consult the same ownership state; focus loss and
unmapping retire it.

## Language changes

The GTK window prepares language catalogs on a worker, then adopts the shared
localization snapshot between canvas interactions, menu selections and native
composition key sequences. Existing numeric editors, layer rename fields and command search
entries retain their widgets and drafts. Each window has its own published
snapshot; the application's settings action delivers the current preference to
other windows before persistence completes. Mapped and future widgets use the
window's current Pango language. Shared search queries are forwarded from the
native editable change signal so a projection cannot overwrite an edit while
GTK's delayed search signal is pending.
Open drawer tabs retain their widgets and refresh their shared titles, tooltips
and accessibility labels when the window publishes a language. Authored toolbar
names stay literal.

`native_live_language_switching` exercises the Language preference in both
themes and verifies retained editors, document revisions and numeric refusals.
It checks built-in and authored drawer tab titles without replacing tab widgets.
Its fixture enables the numeric Brush size control before opening that drawer;
the default Brush size panel shows presets.
Run it on the private display with the same command used for other native tests.

Numeric controls retain the shared refusal from the last edit. Language changes
refresh that caption without parsing a draft or changing its selection, focus,
value or input signals. `native_numeric_error_live_language` checks spin and slider
editors in every shipped language and both themes; its preedit signals are synthetic.

`native_live_language_documents_light` and
`native_live_language_documents_dark` cover every shipped language through
drawing, undo/redo and Unicode project save/reopen/export. `native_text_language` checks
glyph fallback and actual allocated Thai paragraph wrapping, including narrow
windows and larger text. Compact workspace captions retain their complete
accessible label while ellipsizing inside the shared header allocation.
Shared context menus use nested native popovers so legitimate repeated submenu
labels do not become toolkit page identities. `native_localized_nested_menus`
checks the actual Edit and Layer → Organize actions in every shipped language
and both themes, including identical Turkish captions.
Canvas-bar menu journeys follow each mapped nested popover and use
`screen_point` for its rows; popup surfaces have their own native origins.

`native_localization_callback_registration` checks subscriptions added during
language publication. Existing subscribers keep their order; new subscribers
receive the current language immediately and join later publications.
`native_proof_dial_callback_registration` checks the same registration boundary
for Proof dial input in both themes. Dial emission retains an immutable callback
snapshot without copying the list on ordinary motion.

`native_preferences_text_menu_live_language` checks localized text-edit actions,
native selection and deferred publication while the Preferences menu is open.

Standalone import and profile windows receive prepared contexts and retain
their own shared transition. `native_bare_profile_language_transition` checks
late registration, cancellation, future windows and independent input boundaries.
`native_profile_picker_live_language` and `native_profile_library_live_language`
check raw ICC names, localized absence, search and retained controls without
rereading files. `native_white_balance_live_language` checks retained calibration
buttons and notices. Run each through `--native-test=<name>` on the private display.

Edit Color keeps its widgets, an open refused value, its selection and its
message across language changes; the shared editor relabels its rows and
re-reads the refusal in the new language without changing the draft.
Color and gradient buttons, including effect buttons that use the selected
color, refresh their current shared captions through weak workspace callbacks.
A stale document refuses color acceptance through the
shared message notice, so its failure caption also follows language changes.
The wrapped status and error label stays outside the scrollable fields so
narrow dialogs reserve space for its complete text.
`native_color_editor_live_language` checks SDR and HDR drafts, warnings,
range-refused exposure, Unicode text and selection, retained controls and
document history in every shipped language and both themes. It also checks
French/German with narrow windows, larger text and actual status bounds,
gradient captions and stale document acceptance. Its preedit signals are synthetic; genuine engine
composition remains a separate private-display acceptance check.

`native_histogram_live_language` checks both Histogram presentations and the
retained Properties page, numeric focus, selection and targeted adjustment
buttons in every shipped language and both themes. It preserves the statistics
data and query time while changing copy, exercises source and channel choices,
and defers publication while the native channel popup is open.
`native_localized_photo_histogram_bounds` checks the default Photo workspace's
Histogram and Waveform plots, controls and translated labels against their
visible dock bounds in every language and both themes. Their logarithmic-count
checkbox uses the shared native wrapping control on its own row, keeping the
plot's edge bins and precision status visible when translated labels are long.
Properties action and clipping captions wrap inside the native viewport and keep
complete tooltips. Selected choices use the compact dropdown helper, retaining
full tooltip and popup text when the closed caption ellipsizes. The same fixture
checks inner control and glyph bounds,
including narrow French/German layouts with larger text.
Use `LAYER_MOTION_VIEWPORT=1100x800` for its normal run or `640x1000` for
the small viewport run; it verifies the actual window allocation.

`native_proof_live_language` checks preparing, prepared and unavailable captions
in every shipped language and both themes. It retains the LUT identity and raw
profile names while changing copy, and preserves the literal ICC diagnostic.
Preparing is checked with work paused; language changes must not start a worker.

`native_genuine_language_composition` supplies a private application identity for
IBus acceptance runners. It requires isolated storage and native compositor keys;
engine traffic and captures establish Telex/Anthy preedit, commit and deferred
publication. The Image Size width journey checks the same dirty numeric editor,
accepted value and typed refusal through native Enter and Escape. Engine traffic
distinguishes cancellation from an engine committing preedit on Escape.
Thai Kesmanee commits directly and does not establish active preedit.

## Tests

The private runner uses isolated memory-backed compositor preferences and
disables Mutter’s Alt+Space window menu there so navigation keys reach the app.
On a desktop that reserves Alt+Space, Ctrl+Alt+Space remains the Zoom out chord.

Model tests run with `cargo test --locked -p layer-linux`. Native tests are
`#[ignore]`d GTK journeys in `tests.rs` and the `*_tests.rs` modules. They need a
real Wayland display, a Vulkan GPU and injected input, so run them through the
private-compositor runner:

```bash
bash tools/performance/workspace-motion.sh gtk --native-test=native_canvas_bar_modes
```

`native_scroll_wheel_input` exercises wheel pan, Shift-wheel horizontal pan and
Ctrl-wheel zoom with the middle or right mouse button held and with neither
held, in both themes. It also checks release and active-stroke exclusion through
the private compositor's native mouse and keyboard delivery. Scroll callbacks
without an event position use the native surface's pointer position and modifier
state before converting to canvas coordinates.

GTK has one battery source: the Linux kernel's power-supply files.
`system_status::power::tests` cover its file parsing, device symlinks, live changes
and peripheral exclusion without a window. Shared `layer-ui` tests cover the same
reader's percentage validation, low warnings and energy-weighted system batteries.
`native_fullscreen_header_clock_and_battery`
checks visibility, charging and low-battery presentation; run it in both themes.
For sandbox acceptance, mount private power-supply fixtures read-only over the
kernel paths in an isolated namespace and exercise the packaged reader's live
polling. Do not add Flatpak permissions or override the real system's files.

Photo binding changes use `native_multiple_photo_import_chooser` for native
multi-select import, retained samples, paint above and below photos, save/reopen
and undo. `native_photo_file_drops` checks canvas and layer destinations and
opens project drops in document tabs. Pair these with the source rasterization,
source profile repair and document color journeys, setting
`CAPY_NATIVE_TEST_THEME=light` and `dark` for each.

`native_object_only_shared_images` opens the fixed built-in, ICC and Nearest
fixtures after removing their hidden paint and paper layers. It waits for native
presentations without input before checking pixels, shared image ownership and
F64 placement, then checks visibility Undo/Redo, native save/reopen, surface
replacement and private recovery. Run in both themes with `--native-recovery`.

`native_palette_entry_composition` checks editable submission, the real GTK
default-button action and candidate-key retirement in both themes. Genuine
engine composition is a separate acceptance check in a private display.

Pen pressure changes use `native_pressure_calibration --tablet` in both themes,
then `native_curve_graph_numbers_pages_and_history` for the shared widget.
See [pen pressure](../ui/pen-pressure.md) for sustained-motion measurement.
The tablet proxy reserves its injected object IDs and translates compositor
object IDs using the installed Wayland protocol XML. Run its clipboard/offer
regressions with `python3 -m unittest discover -s apps/layer-linux/bench -p 'test_*.py'`.

Numeric widget changes use `native_number_controls`, `native_slider_feedback`
and the toolbar component mouse/touch, pen and value-control journeys. They cover
both themes, editing, slider feedback, popovers and toolbar allocation.
Tool-settings journeys share `tool_settings_workspace` for toolbar commands and
panel placement; restore actions, waits and interaction assertions stay in callers.
Selection journeys locate published variant choices by their localized labels
inside the mapped Tools or slot Brushes panel, then send native contacts to the
containing button.
`native_enclose_fill_pointer_workflow` checks Enclose and Fill with native mouse
contacts in both themes: two fully enclosed reference holes fill a separate paint
layer, a hole crossed by the lasso stays empty, and the reference stays exact.
It also checks the shared Fill controls, one-step Undo/Redo and Escape cancellation.
Artifact helpers preserve literal paths and each capture's warm-up wait; held
warm textures and theme loops remain in their callers.
Docking and ink checks retain pressure editing, native wrapping and GPU pixels.
Drop rules and tile/grip bounds belong to `native_layout_drop_input`,
`native_toolbar_sizing` and `native_ribbon_allocation`.
`native_spatial_filter_windows` checks a 24 MP photo with chained Gaussian blurs
at 50% zoom, panning and radius changes in both themes.
`native_hue_ranges_colorize_retains_values` and
`native_pointwise_filters_controls_and_persistence` cover Hue range pages,
Colorize, Threshold and Photo Filter controls, direct Invert/Desaturate insertion,
atomic slider history and save/reopen. Run at 640 and 1100 pixels wide; both
journeys exercise light and dark themes.
`native_hue_colorize_keyboard_focus_and_common_draft` checks native activation
and retained common fields. `native_property_draft_is_retired_when_document_changes_with_reused_layer_ids`
checks that opening another document cannot carry a pending number into it.
`native_colorize_threshold_visible_artwork` is a focused check of Colorize and
Threshold controls and rendered artwork pixels in both themes. Run it at 1100
pixels wide after renderer changes; it does not measure motion.

[`workspace-motion.sh`](../../tools/performance/workspace-motion.sh) builds the
release tests, starts a private D-Bus session, headless Mutter and PipeWire, and
prints the run directory that holds its logs, input records and fresh storage.
It needs Mutter with headless support, GJS and PipeWire besides the build
prerequisites. Useful settings:

| Setting | Effect |
| --- | --- |
| `--native-test=<name>` | Runs the one test with that exact name. |
| `--tablet` | Adds tablet-v2 pen input through a Wayland proxy. |
| `--native-storage` | Gives the test the run's SQLite workspace directory instead of in-memory workspaces. |
| `--native-recovery` | Gives the test the run's session directory, so drawings are checkpointed and restored. |
| `LAYER_NATIVE_TEST_EXECUTABLE` | Absolute path of an already built test executable; skips the rebuild. |
| `CAPY_NATIVE_TEST_THEME` | Sets fixture windows to `light` or `dark`; run affected journeys once with each. |
| `LAYER_MOTION_VIEWPORT`, `LAYER_MOTION_SCALE` | Private monitor size (default `1600x1000`) and scale, for example `3200x2000` and `2`. |
| `LAYER_TEST_ARTIFACTS` | Absolute directory for captures and reports, where a test writes them. |

Named modes such as `--drag-pickup`, `--workspace-motion`, `--color-panel` or
`--icons` select a fixed test; the mode list is in
[`native-input.js`](../../apps/layer-linux/bench/native-input.js). Test-specific
instructions live with the feature's guide under [`docs/ui/`](../ui/README.md).
[`tools/performance/gtk-raster.sh`](../../tools/performance/gtk-raster.sh) runs an
already built test executable on the same kind of private display, for tests
that need no injected input, such as `native_application_file_launch`.

Rules and pitfalls:

- **One test per process, isolated storage.** GTK runs on one thread, and each
  native test owns the display, input protocol and storage for its run. Pass one
  exact name with `--exact --test-threads=1`; Cargo's filter is a substring match
  and can start a second journey in the same process. Test builds never fall
  back to your own settings, workspaces or recovery files. A test stores files
  only under a fresh `CAPY_STORAGE_DIR`, and uses its workspaces and sessions
  only when `data/workspaces` or `state/sessions` exist there; the runners
  create the folder and those subfolders.
- **Inject input only on the private display.** `native-input.js` drives
  Mutter's RemoteDesktop API and refuses any display not named `layer-bench-*`.
  Never point it at a desktop session.
- **`GDK_DEBUG=no-portals:color-mgmt`.** GTK 4.22 binds the Wayland
  color-management protocol only with `color-mgmt` and has no public API for it.
  The app adds it in `main()` before GTK starts, but tests do not run `main()`.
  `no-portals` makes file dialogs use GTK's in-process chooser, which tests can
  drive, instead of the desktop portal. Without the portal GTK also loses the
  desktop's font and window settings and falls back to fontconfig's `Sans`, so
  the runners give GTK GNOME's defaults from
  [`gtk-config`](../../tools/performance/gtk-config/gtk-4.0/settings.ini)
  through `XDG_CONFIG_DIRS`. `gtk-raster.sh` and `workspace-motion.sh gtk` set
  all of this; plain `cargo test` sets none of it.
- **Tablet proxy limits.** `--tablet` pen serials cannot authorize compositor
  drag-and-drop; use mouse and touch for those journeys. The proxy also drops its
  connection when Quick Mask or Selection Layer rows change, so journeys through
  those modes run without `--tablet`. It never stands in for a physical pen.
- **Alerts.** Await `adw::AlertDialog` with `alert::choose`, not
  `choose_future`, which in libadwaita-rs 0.9.2 keeps the dialog alive after it
  closes.

A focused test can also run without the runner inside an existing Wayland
session, for example to attach a debugger. Give it fresh storage:

```bash
gtk_test_dir=$(mktemp -d)
mkdir -p "$gtk_test_dir/data/workspaces"
CAPY_STORAGE_DIR="$gtk_test_dir" \
GDK_BACKEND=wayland GSK_RENDERER=vulkan G_DEBUG=fatal-criticals RUST_BACKTRACE=1 \
  cargo test --locked --release -p layer-linux \
  workspace::tests::workspace_switcher_tests::native_active_workspace_delete \
  -- --ignored --exact --test-threads=1 --nocapture
```

Reuse `native_test_app`, `named::<T>`, `widgets`, `descendant`, `ui_session`, `ui_session_mut` and `pump`
to drive real GTK dialogs. Wait for workspace readiness and for operations to finish before
asserting, and check persistence by reopening, not only by reading rows.

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

Besides the build prerequisites, install Node.js, `strip`,
`desktop-file-validate`, `cargo-about`, and GTK's own build dependencies with
Meson, Ninja, `glslc` and `sassc`:

```bash
cargo install cargo-about --version 0.9.2 --features cli --locked
node apps/layer-linux/package.mjs
dist/capycanvas-linux/bin/capycanvas
```

The packager builds a release binary with the `release-identity` feature, which
gives it the application ID `art.capycanvas.CapyCanvas` and the `capycanvas`
folders; every other build runs as `art.capycanvas.CapyCanvas.Devel` with
`capycanvas-devel` folders, so it never shares a running instance or files with
an installed package ([storage](../internals/storage.md)). It also builds a
pinned GTK 4.22.4 with the
[tablet patches](../../tools/build/gtk-runtime/README.md), cached in
`target/gtk-runtime` (`CAPY_GTK_BUILD_DIR` overrides it). The `bin/capycanvas`
executable finds the bundled `libgtk-4.so.1` through its embedded library path.
System GTK is never replaced; libadwaita and GTK's other dependencies stay system
requirements, so this is a native bundle for compatible distributions. The
[Flatpak](releasing.md#linux-flatpak) uses the GNOME runtime for those dependencies.
`share/doc/capycanvas-gtk` carries the GTK source,
patches, license, checksums and a rebuild script.

The output holds the executable, desktop entry, AppStream metainfo, `.capy` MIME
definition, icon, runtime filters, GTK runtime and notices. Photo codecs are compiled in, so a
moved package needs no codec path. Staging replaces only a directory carrying
the generated `.capy-package` marker. The GTK and Web packagers share
`tools/build/about.toml`, need original license texts including vendored
dependencies, and accept `LAYER_CARGO_ABOUT` for the notice generator.

Staging does not install anything. An installer must register the desktop entry
and refresh the desktop and MIME databases; the [publication guide](publication.md)
covers distribution.

To check that a relocated package opens photos with its bundled GTK, on a
private compositor with isolated settings:

```bash
python3 tools/validation/gtk_package_photo.py \
  --binary /path/to/relocated/capycanvas-linux/bin/capycanvas \
  --photo /path/to/photo.heic --photo /path/to/photo.avif \
  --output artifacts/package-photo-check
```

### Flatpak runtime and portals

AppStream catalog generation runs with temporary `.Devel` build metadata so the
SDK's Glycin icon loader can run without a desktop portal during builds. The
exported application keeps `art.capycanvas.CapyCanvas` as its identity.

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

Users need their distribution's Flatpak package, a Wayland session and hardware
Vulkan support. The Flatpak runtime supplies toolkit dependencies; it does not
remove the [canvas requirements](#prerequisites).

The sandbox grants the Wayland socket for windows, clipboard and input, and GPU
devices for rendering. The battery indicator reads the kernel's
`/sys/class/power_supply` data on a worker at startup and every 30 seconds;
Flatpak exposes these files and their device targets read-only. System batteries
are combined by their energy capacities; peripheral batteries are excluded.
Unavailable or incomplete readings hide the indicator.

GTK uses the default portals for file dialogs, selected-file access and opening
links; the clock observes GNOME's time format through the Settings portal.
Settings, workspace libraries, recovery files and caches use Flatpak's private
application directories. The exported desktop entry forwards files through the
document portal when launched from a file manager. Software's permission summary
does not list these per-file portal grants as access to the user's folders.

If opening a selected file fails with `Transport endpoint is not connected`,
check the document portal's FUSE mount with
`findmnt -T "$XDG_RUNTIME_DIR/doc"`; its filesystem type should be `fuse.portal`.
A running `xdg-document-portal` service can still have a disconnected or missing
mount. Repair the shared service only when other sessions' Flatpak applications
are closed and the shared-machine rules allow it:
`systemctl --user restart xdg-document-portal.service`. Reopen applications so
their sandboxes receive the restored mount.
