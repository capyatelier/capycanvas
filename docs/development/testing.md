# Testing

[Developer guide](README.md)

Different checks answer different questions. Shared tests exercise document and
editor rules without a window. Host tests exercise native controls and input.
GPU tests check pixels and rendering cost. Device tests establish whether real
pen and display behaviour matches the intended interaction. None substitutes for
another, and there is no CI: run the checks for every area your change touches.

## Checks by change type

Run the rows that match your change, then walk the affected user journeys on each
affected host. Scope Cargo commands with `-p`; `cargo test --workspace` pulls in
GPU and platform crates that a given machine may not be able to run.

| Change | Checks |
| --- | --- |
| Shared Rust (`layer-core`, `-engine`, `-ui`, `-workspace`, `-color`, `-render`) | `cargo test --locked -p <crate>` for each changed crate, then `cargo check --locked -p layer-linux --tests` and `cargo check --locked -p layer-web --target wasm32-unknown-unknown` for the consumers. |
| Renderer, shaders or runtime filters | `cargo test --locked -p layer-render-wgpu <filter>` and `-p layer-host` on a hardware GPU. If shader cache-key inputs changed, `python3 tools/build/test_shader_generation.py`. Frame-path changes need [measurements](#performance). |
| Pen prediction or stroke placement | `cargo test --locked -p layer-engine --features prediction-bench`, then `cargo build --locked -p layer-engine --release --features prediction-bench --examples` and `python3 tools/prediction/replay-bank.py --output artifacts/strokes/bank --check`. |
| GTK | `cargo test --locked -p layer-linux` for the model tests, then each affected journey on the private display: `bash tools/performance/workspace-motion.sh gtk --native-test=<name>`, adding `--tablet` for pen journeys ([Linux](linux.md#tests)). |
| Web | `bash apps/layer-web/build.sh`, the pure tests `node --test apps/layer-web/{run,package,frame,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls}.test.mjs`, then the affected journeys with `bash tools/performance/workspace-motion.sh web --<journey>` (headed, hardware WebGPU) or `node apps/layer-web/test.mjs --headless --<journey>` against `run.sh`. Packaging changes: `node apps/layer-web/package.mjs && node apps/layer-web/test.mjs --package` ([Web](web.md#tests)). |
| Android | Without a device: `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a` in `apps/layer-android`. On a reserved tablet, through `tools/devices/devices.py run`: install both APKs with `adb install -r`, then `am instrument -w -e class art.capycanvas.<Class>[#method] $CAPY_APPLICATION_ID.test/androidx.test.runner.AndroidJUnitRunner`. Read `OK` or `FAILURES!!!`, not the exit status ([Android](android.md#device-tests)). |
| Apple | Anywhere: `cargo test --locked -p layer-apple --lib -- --test-threads=1` and `python3 apps/layer-apple/tests/test_icon_assets.py`. On a Mac: the Swift fixtures, the command audit, and the XCTest journeys in the [Apple guide](apple.md#tests); journeys that render the canvas need a physical iPad. |
| Windows | From any OS: `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`. On Windows: `apps/layer-windows/scripts/test-without-gpu.ps1`, then the `exercise-*.ps1` fixtures for the changed area; from Linux, `tools/windows-vm/windows-vm.py check` and `fixtures <name>`. Device-removal, HDR and performance checks need a hardware GPU ([Windows](windows.md)). |
| Dependencies or `Cargo.lock` | `cargo deny --locked check licenses sources` ([publication](publication.md)). |
| Scripts under `tools/` | `python3 -m unittest discover -s tools/<dir> -p 'test_*.py'`, or `node --test` on the script's `.test.mjs`. |
| Docs only | Check every link, path, command and test name you wrote or whose target you changed. |

## Shared Rust

Renderer tests create and destroy their own GPU device, so
[`.cargo/config.toml`](../../.cargo/config.toml) runs four test threads unless
`RUST_TEST_THREADS` or `--test-threads` says otherwise. Some Linux drivers limit
live Vulkan devices per process or deadlock while many threads create and destroy
devices; use `--test-threads=1` for device-heavy filters. Headless renderers keep
one wgpu instance for the process lifetime, because reloading the driver
eventually exhausts static TLS.

`layer-render-wgpu` unit tests and `software-adapter-tests` builds accept a CPU
adapter when `LAYER_TEST_SOFTWARE_GPU=1` is set. Those runs check numbers only;
they are never performance evidence, and production hosts always require a
hardware GPU.

## UI and input

- **Both themes.** Check changed UI in light and dark themes, and brush changes in
  both live drawing and replay.
- **Drags.** Reorder gestures follow the [drag convention](../ui/drag-and-reorder.md)
  and its [validation matrix](../ui/drag-and-reorder.md#required-validation-when-implementing).
  Test mouse, touch and pen separately, and assert that movement before a hold
  does not reorder where a hold is required. The matrices run with
  `workspace-motion.sh gtk --drag-pickup` and `--column-drops`, `test.mjs
  --drag-pickup`, `--layer-hold`, `--long-press-drag` and `--drag-cursors` on
  Web, `AndroidInteractionTest`, and `exercise-layer-pickup.ps1` and
  `exercise-workspace-pickup.ps1` on Windows.
- **Parity.** Web is compared with GTK, and each native port with Web, at the
  same document, workspace, theme, viewport and scale. For GTK and Web, run
  `cargo test --locked --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1`,
  then `node apps/layer-web/test.mjs --parity` in a headed browser. The
  [visual tools](../../tools/visual/README.md) capture the other hosts; never
  rescale or crop a capture to hide a difference.
- **Icons.** `node apps/layer-web/test.mjs --icons`, `workspace-motion.sh gtk
  --icons` at scale 1 and with `LAYER_MOTION_SCALE=2`, Android `AndroidIconTest`,
  and `python3 apps/layer-apple/tests/test_icon_assets.py`.
- **Popups on GTK.** Run popup and menu checks without `--tablet`: the tablet
  proxy's synthetic serials cannot take compositor popup grabs.
- **Real pens.** Injected input checks logic, not the OS driver. Input changes
  need a real stylus on the affected host: pressure, tilt, hover, cancellation
  and fast curves, plus touch navigation and resuming after the app was in the
  background.

## Performance

[Performance targets](../PERFORMANCE_TARGETS.md) defines the tiers and targets,
and [measuring](../performance/measuring.md) the rules and commands. Keep CPU
submission, GPU completion and presentation latency separate: a short submission
time does not establish a frame rate. Offscreen GPU numbers from
`cargo run --locked --release -p layer-bench -- --scenario <name>`
([benchmark workloads](gpu-raster-benchmarks.md)) help compare revisions on one
machine but never replace a measurement on the tier's device. Regenerate bundled
brush previews with `cargo run --locked --release -p layer-bench -- --brush-previews`.

## Known failures on main

These fail on `main` independently of your change. Don't chase them unless they
are your task, and remove an entry when you fix it.

- GTK `native_selection_pen_input`, and tests that open the portal file chooser.
- Android: 7 of 15 `AndroidTitleBarTest` cases;
  `detachedPanelsKeepBodiesAndWiderResizeTargets`;
  `AndroidInteractionTest#cachedPanelsMatchDirectDrawing` (light docked panels);
  the last assertion of `AndroidHostTest#cameraNavigationPublishesOnlyReadoutUpdates`;
  `AndroidInteractionTest#menuBodyAndExtendedTabDropsAcrossDevices` and
  `#drawerTabsKeepActiveColorsAndPadding`.
- Windows `exercise-tab-pickup.ps1 -Device touch`: after the Layers group is torn
  off, the injected contact reaches neither XAML nor the canvas, so the drag never
  finishes. Mouse passes.
- iPad XCTest `testCompactMenuShortcutAcrossPages` and
  `testSettingsTextSelectionShortcut`: XCTest keys don't reach UIKit key commands.
- Headless Web `--toolbar-components`, `--tonal-selection`, `--editor` and
  `--layers`, and 8 pen side-button cases in `pointer.test.mjs`.
- `cargo clippy -- -D warnings` stops in `layer-core` on lints new in Clippy 1.96.
  New code adds no warnings.
