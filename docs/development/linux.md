# Linux development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

Linux with Wayland is the primary development target. New editor features land
here first, then move to Web and the native ports, as the
[platform workflow](../platforms/README.md#development-workflow) describes. The
client uses GTK4/libadwaita for controls and the shared wgpu renderer through
Vulkan.

## Prerequisites

Install a recent stable Rust toolchain, a C/C++ toolchain, `pkg-config`, and the
GTK 4.22+, libadwaita 1.9 and Wayland development packages. Enabled API features
are declared in [`apps/layer-linux/Cargo.toml`](../../apps/layer-linux/Cargo.toml).
Photo codecs and ICC color management are Rust crates; no libjpeg-turbo,
LittleCMS or HEIF system packages are needed.

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

Where to look in [`apps/layer-linux/src`](../../apps/layer-linux/src):
`main.rs` calls the library's `run`; `lib.rs` creates the application and handles
file launches. `canvas.rs` adapts
the shared session and schedules frames against Wayland presentation timing,
`render_thread.rs` owns canvas GPU work, `wayland.rs` presents into an app-owned
subsurface beneath the GTK controls ([design record](../history/wayland-subsurface-feasibility.md)),
and `files.rs` supplies native dialogs and project/photo transport.

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

`native_preferences_text_menu_live_language` checks localized text-edit actions,
native selection and deferred publication while the Preferences menu is open.

Standalone import and profile windows receive prepared contexts and retain
their own shared transition. `native_bare_profile_language_transition` checks
late registration, cancellation, future windows and independent input boundaries.
`native_profile_picker_live_language` and `native_profile_library_live_language`
check raw ICC names, localized absence, search and retained controls without
rereading files. `native_white_balance_live_language` checks retained calibration
buttons and notices. Run each through `--native-test=<name>` on the private display.

Edit Color retains the shared `ColorFormCopy` from the last edit or display
capability change. Language publication projects its model captions, typed
refusal and cached gamut flags without parsing fields or converting colors.
The same combo row, list model, numeric fields and preview widgets remain in
place; immutable list strings update under the existing selection guard.
The selected color model appears below its title so longer titles leave the
choice readable in narrow dialogs.
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

Model tests run with `cargo test --locked -p layer-linux`. Native tests are
`#[ignore]`d GTK journeys in `tests.rs` and the `*_tests.rs` modules. They need a
real Wayland display, a Vulkan GPU and injected input, so run them through the
private-compositor runner:

```bash
bash tools/performance/workspace-motion.sh gtk --native-test=native_canvas_bar_modes
```

`native_palette_entry_composition` checks editable submission, the real GTK
default-button action and candidate-key retirement in both themes. Genuine
engine composition is a separate acceptance check in a private display.

Numeric widget changes use `native_number_controls`, `native_slider_feedback`
and the toolbar component mouse/touch, pen and value-control journeys. They cover
both themes, editing, slider feedback, popovers and toolbar allocation.
Tool-settings journeys share `tool_settings_workspace` for toolbar commands and
panel placement; restore actions, waits and interaction assertions stay in callers.
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
  back to your own settings, workspaces or recovery files. When a test needs
  real storage, give it fresh `CAPY_WORKSPACE_DIR`, `LAYER_SETTINGS_FILE` and
  `CAPY_RECOVERY_DIR` paths; the runners do this.
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
CAPY_WORKSPACE_DIR="$gtk_test_dir/workspaces" \
LAYER_SETTINGS_FILE="$gtk_test_dir/settings.json" \
GDK_BACKEND=wayland GSK_RENDERER=vulkan G_DEBUG=fatal-criticals RUST_BACKTRACE=1 \
  cargo test --locked --release -p layer-linux \
  workspace::tests::workspace_switcher_tests::native_active_workspace_delete \
  -- --ignored --exact --test-threads=1 --nocapture
```

Reuse `native_test_app`, `named::<T>`, `widgets`, `descendant`, `ui_session`, `ui_session_mut` and `pump`
to drive real GTK dialogs. Wait for workspace readiness and for operations to finish before
asserting, and check persistence by reopening, not only by reading rows.

## Troubleshooting

- **Startup crash with a tablet, or an arrow cursor flash on pen entry.** Upstream
  GTK 4.22.4 dereferences a null surface when a tablet pad reports a mode change
  before keyboard focus, and picks a cursor from stale coordinates when a pen
  enters. The package ships a patched GTK; `run.sh` and the tests use the system
  one. To run from source with the patches:

  ```bash
  bash tools/build/gtk-runtime/build.sh target/gtk-runtime target/gtk-runtime/prefix
  LD_LIBRARY_PATH="$PWD/target/gtk-runtime/prefix/lib" ./apps/layer-linux/run.sh
  ```

- **Canvas colors look darker than the controls.** The canvas surface describes
  itself with the explicit piecewise sRGB transfer function (color-management v2
  TF 14), never legacy TF 9, which Mutter treats as gamma 2.2. Check that
  `GDK_DEBUG` reaches GTK with `color-mgmt`, and that the compositor offers the
  protocol; without it, canvas and controls fall back to untagged sRGB together.

## Stage a native bundle

Besides the build prerequisites, install Node.js, `strip`,
`desktop-file-validate`, `cargo-about`, and GTK's own build dependencies with
Meson, Ninja and `glslc`:

```bash
cargo install cargo-about --version 0.9.2 --features cli --locked
node apps/layer-linux/package.mjs
dist/capycanvas-linux/bin/capycanvas
```

The packager builds a release binary and a pinned GTK 4.22.4 with the
[tablet patches](../../tools/build/gtk-runtime/README.md), cached in
`target/gtk-runtime` (`CAPY_GTK_BUILD_DIR` overrides it). The `bin/capycanvas`
launcher puts the bundled `libgtk-4.so.1` first on the library path; use it for
ordinary and file launches. System GTK is never replaced; libadwaita and GTK's
other dependencies stay system requirements, so this is a native bundle for
compatible distributions. `share/doc/capycanvas-gtk` carries the GTK source,
patches, license, checksums and a rebuild script.

The output holds the executable, desktop entry, `.capy` MIME definition, icon,
runtime filters, GTK runtime and notices. Photo codecs are compiled in, so a
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
