# macOS and iPadOS development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Apple clients use AppKit on macOS and UIKit on iPadOS. They share Swift
editor components and the `layer-apple` Rust bridge built around `NativeHost`.
Metal renders the canvas through a `CAMetalLayer` behind native controls. The
[package README](../../apps/layer-apple/README.md) describes the source layout,
and the [Apple porting guide](../APPLE_PORTING_GUIDE.md) holds the porting rules
and XCTest pitfalls. Features usually land on GTK, Web and Android first.

Artwork tasks retain shared authored roots and captured effect phases. Native
saves write source-only packages through coordinated atomic replacement. A
package the shared reader cannot admit for editing stays in the shared read-only
`PackageView` without replacing the editable document. Imported native backing
stays alive through renderer admission so unavailable execution capabilities can
present the original package; cancellation remains a cancelled request. Copy Original File preserves its complete
backing; Export Preview Image writes the verified PNG through the existing
coordinated destination or private export-as-copy staging flow. Direct writes
compare the retained security-scoped source URL and filesystem identity before
replacement to keep the original package intact.

## Prerequisites

Everything except the Rust bridge tests needs an Apple Silicon Mac with Xcode,
the iOS simulator runtime, Python 3 and Rust. [Test devices](devices.md#apple)
lists the Mac and iPad. The build targets arm64 macOS, arm64 iPad devices and
the arm64 iOS simulator:

```bash
rustup target add aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim
```

The scripts default to `/Applications/Xcode.app/Contents/Developer`; set
`DEVELOPER_DIR` if Xcode is elsewhere. A physical iPad also needs Developer Mode,
pairing, a development certificate and a profile covering the device. A first
Personal Team install may require trusting the developer account in the iPad's
Settings. Team IDs, keys and provisioning profiles are never committed.

## Build

```bash
bash apps/layer-apple/scripts/build.sh macos
bash apps/layer-apple/scripts/build.sh simulator
CAPY_APPLE_TEAM=YOUR_TEAM_ID bash apps/layer-apple/scripts/build.sh device
```

`build.sh` runs `scripts/prepare.py` and `scripts/project.py`, then builds with
`xcodebuild`; it does not install or launch. The Xcode build phases compile the
Rust library through `scripts/rust.sh`.

- `CAPY_CONFIGURATION=Release` builds optimized Swift and Rust.
- `CAPY_DESTINATION='id=DEVICE_UDID'` selects a device destination.
- `CAPY_DERIVED_DATA` changes the output directory (default
  `apps/layer-apple/DerivedData`, ignored by Git).
- macOS builds are ad-hoc signed unless `CAPY_APPLE_TEAM` is set.

### Xcode project

`apps/layer-apple/CapyCanvas.xcodeproj` is tracked, and `scripts/project.py`
regenerates it deterministically from the Swift files under `Shared/`, `iOS/`
and `macOS/`; files in `Tests/` folders go to the UI test targets. Every
`build.sh` run rewrites it. Never edit the project in Xcode or by hand: change
`project.py`, run it, and commit the regenerated project with the change. Adding
or removing a Swift file also changes the project, so commit that diff too.

`scripts/prepare.py` fills the ignored `Generated/` directory with the shared
icons and brush previews from `apps/layer-web` and the license files. Edit those
sources, never `Generated/`.

## Install and run

Launch a local Mac build:

```sh
open apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

Install and launch on the iPad:

```sh
xcrun devicectl list devices
xcrun devicectl device install app --device DEVICE_ID \
  apps/layer-apple/DerivedData/Build/Products/Debug-iphoneos/CapyCanvas-iPad.app
xcrun devicectl device process launch --device DEVICE_ID \
  --console art.capycanvas.apple.ipad
```

Or open the project in Xcode and run the `CapyCanvas-Mac` or `CapyCanvas-iPad`
scheme. Attach Xcode or LLDB to the launched process for breakpoints and native
errors. Wait for each build or install to finish before launching.

**Isolated installs.** The regular apps hold an artist's drawings; never
replace them or clear their data. Pass `CAPY_APPLE_BUNDLE_ID=<your id>` to
`xcodebuild` to build under another identity; its UI test target uses the
`.tests` suffix, so the regular editor stays installed. Benchmarks use their own
identity and DerivedData directory as well. A free Personal Team profile limits
how many apps a device may hold, and the XCTest runner counts as one; do not
remove the artist's apps to make room.

**Debug fixture variables.** Release builds ignore all of these.

| Variable | Effect |
| --- | --- |
| `CAPY_INITIAL_ACTIONS` | JSON array of shared actions applied once after restoration and the first surface size. Also disables persistence unless a namespace is set. |
| `CAPY_PERSISTENCE_NAMESPACE` | A UUID; settings, workspaces and recovery live in a private `test-<uuid>` folder. |
| `CAPY_DISABLE_PERSISTENCE=1` | Runs with memory-only storage. |

For example, this opens the Tool panel without driving the Mac menu bar:

```sh
open -n --env CAPY_INITIAL_ACTIONS='[{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":true}}]' \
  apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

## Tests

On Linux only the Rust bridge tests run; everything else needs the Mac.
[Testing](testing.md) lists which checks a change needs.

### Rust bridge

```sh
cargo test --locked -p layer-apple --lib -- --test-threads=1
cargo test -p layer-apple tests::photo -- --test-threads=1
```

The tests drive both Apple platform policies through the real C ABI and a
hardware GPU, comparing exact document pixels through Undo and Redo. Reuse
`fixtures::tempfile()` for private file descriptors. Filter by module (`tests::input`, `tests::recovery`, `tests::workspace` and the other
`*_tests.rs` files under `native/src`). The ignored 61 MP regression needs a
disposable sRGB JPEG:

```sh
CAPY_APPLE_PHOTO_JPEG=/path/to/photo.jpg cargo test -p layer-apple --lib \
  large_jpeg_gpen_preserves_photo_through_save_and_gpu_recovery -- \
  --ignored --nocapture --test-threads=1
```

### Live interface language

Language changes use the same `NativeOwner` and `EditorStore`. Rust's shared
transition rejects superseded preparation; a separate language queue warms the
immutable context and English command aliases. The drawing owner publishes
bootstrap, catalog, language generation and changed editor views together after
native composition and captured input end. The Metal surface and document undo
history keep their owners. Stateless numeric, caption, color and toolbar requests
carry a language tag from the control's published snapshot and use prepared
contexts only.

`EditorPersistence` broadcasts accepted settings before its atomic disk write.
Existing scenes and newly opened scenes therefore receive the accepted language
when storage fails; the initiating scene still exposes its failure and retry
state. Its in-memory settings cache lives only for that persistence owner's
lifetime. System language resolves `Locale.preferredLanguages` when a request is
made, including selecting System again.

The portable regression `apple_language_publication_is_atomic_deferred_and_window_local`
checks coalescing, deferred publication, complete matching copy, independent
windows and unchanged document/camera state. `EditorLaunchTests/testLiveInterfaceLanguage`
and `testLiveInterfaceLanguageDark` switch Settings to 日本語 and back and check that
the open Settings sheet and the panel tabs relabel without a restart. On Apple
hardware, also switch
while editing color/numeric drafts, renaming with marked text, drawing, dragging,
using open dialogs, and activating parked drawings in both themes. Run
`tests/persistence.swift` for cross-scene delivery after a failed write.

### Swift fixtures

The fixtures in `apps/layer-apple/tests` run the production Swift sources on the
Mac without XCTest or a simulator:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
```

`test-project-files.sh` builds the Rust library, compiles `Shared/`, `macOS/`
and `tests/support` with the given fixture (default `tests/project-files.swift`)
and runs it. Both Apple policies run on the Mac with temporary storage. Point
`CAPY_TEST_ASSETS_APP` at a built Mac app when a fixture needs the vector or
filter assets. Fixtures that open windows take focus: run them one at a time and
never alongside a UI test batch.

Fixtures for a single Swift file compile directly, for example:

```sh
xcrun swiftc -parse-as-library apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o "$TMPDIR/capy-frame-driver" && "$TMPDIR/capy-frame-driver"
```

The same pattern pairs `json-lookup`, `reorder-contact`, `estimated-input`,
`frame-trace` and `drawing-workload-plan` with their `Shared/Bridge` sources
(`JSON.swift`, `ReorderContact.swift`, `EstimatedInput.swift`, `FrameTrace.swift`
and `DrawingWorkloadPlan.swift`). Other runners in `scripts/` and `tests/`:

| Runner | Checks |
| --- | --- |
| `scripts/test-persistence.sh` | Atomic settings files and the settings owner |
| `scripts/test-color-input.sh` | AppKit color text entry in all RGB spaces |
| `scripts/test-project-access.py` | Project writes through a file-only App Sandbox grant |
| `cargo test -p layer-apple tests::session` | Session restart, history and durable removal on both policies |
| `tests/background-expiration.py` | iPad background-task lease ordering |
| `scripts/test-native-rows.py` | UIKit row scrolling and contacts on one booted iPad simulator (`--fixture scenes` for per-scene cancellation) |
| `scripts/test-workspace-scrolling.py` | Long workspace list on an iPad simulator |

The UIKit fixtures `canvas-modifiers.swift`, `canvas-hover.swift` and
`pencil-estimates.swift` run as a standalone scene app built from the `Shared/`
and `iOS/` sources; they have no runner script.

### UI tests

XCTest journeys run through `xcodebuild test` with `-only-testing`. Use a fresh
result-bundle path for each run and inspect the `.xcresult` for failures and
attachments:

```sh
xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-iPad -destination 'platform=iOS Simulator,id=SIMULATOR_ID' \
  -derivedDataPath apps/layer-apple/DerivedData/Simulator \
  -resultBundlePath artifacts/ui/ipad-launch.xcresult \
  -only-testing:CapyCanvas-iPadTests/EditorLaunchTests/testCompleteEditorCapture \
  CODE_SIGNING_ALLOWED=NO test
xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/Mac \
  -resultBundlePath artifacts/ui/mac-launch.xcresult \
  -only-testing:CapyCanvas-MacTests/EditorLaunchTests/testCompleteEditorCapture \
  DEVELOPMENT_TEAM=YOUR_TEAM_ID CODE_SIGN_IDENTITY='Apple Development' test
```

- Journeys launch with their own persistence namespace or with persistence
  disabled, and never touch the artist's data.
- The iOS simulator lacks Float32 filtering, so journeys that render the canvas
  need a physical iPad. UIKit component checks still run on the simulator.
- On a physical iPad, keep the device awake and unlocked and turn on
  **Settings → Developer → Enable UI Automation**; Developer Mode alone does not
  enable the UI runner.
- To test apps already installed on the iPad, set `UseDestinationArtifacts` in
  the `.xctestrun` target with `TestHostBundleIdentifier`,
  `UITargetAppBundleIdentifier` and `TestBundleDestinationRelativePath`. Omit
  `TestHostPath`, `TestBundlePath`, `UITargetAppPath` and
  `DependentProductPaths`, which can make XCTest try to install an unavailable
  bundle during `app.launch()`.
- Resolve any device-trust or automation and capture permission prompt before
  retrying a failed run. A physical iPad runner that times out while enabling
  automation is a runner failure, not a product result; it usually means the
  screen went to sleep. Launching your own test build with
  `xcrun devicectl device process launch --device DEVICE_ID <bundle id>` wakes
  it; terminate that process before the next run.

Known failures on iPad: `testCompactMenuShortcutAcrossPages` (Command-Z) and
`testSettingsTextSelectionShortcut` (Command-A) fail because XCTest key events
do not reach UIKit key commands, and on the simulator title-bar customization
near the window's top edge resizes the OS window instead. No product workaround
is adopted for either.

### Command coverage audit

`command-coverage.json` records expected unavailable commands and handler/check
references for panel controls, preferences and properties. The audit checks
catalog entries, availability and resolved tool choices:

```sh
cargo run --locked -p layer-host --example inventory -- --gpu > "$TMPDIR/capy-inventory.json"
python3 apps/layer-apple/scripts/audit-commands.py "$TMPDIR/capy-inventory.json"
python3 apps/layer-apple/scripts/test-property-audit.py "$TMPDIR/capy-inventory.json"
```

Passing the audit establishes catalog coverage, not working native workflows.

## Debugging and evidence

- Discover destinations with `xcrun devicectl list devices` and
  `xcodebuild -showdestinations -project apps/layer-apple/CapyCanvas.xcodeproj -scheme CapyCanvas-iPad`.
- Keep raw logs, screenshots and traces in ignored `artifacts/` or
  `apps/layer-apple/DerivedData/`. Record the tested revision and launched PID,
  and close only your own test processes.
- For pixel comparisons with Web, use the [visual tools](../../tools/visual/README.md)
  with matching state, viewport, scale and color space on native and local
  Chrome.
- Measure performance in Release builds as described in
  [Apple performance](../../apps/layer-apple/PERFORMANCE.md). Keep builds and UI
  automation idle while timing.
