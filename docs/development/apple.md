# macOS and iPadOS development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Apple clients use AppKit on macOS and UIKit on iPadOS. They share Swift
editor components and the `layer-apple` Rust bridge built around `NativeHost`.
Metal renders the canvas through a `CAMetalLayer` behind native controls. The
[package README](../../apps/layer-apple/README.md) describes the source layout,
and the [Apple porting guide](../APPLE_PORTING_GUIDE.md) defines the audit,
cleanup and validation workflow. Features usually land on GTK, Web and Android first.
[Apple persistence](../../apps/layer-apple/PERSISTENCE.md),
[Apple input](../../apps/layer-apple/INPUT.md) and
[Apple performance](../../apps/layer-apple/PERFORMANCE.md) describe files and
sessions, Pencil and tablet input, and frame measurement.

## Prerequisites

Native builds and AppKit/UIKit fixtures need an Apple Silicon Mac with Xcode,
the iOS simulator runtime, Python 3 and Rust. Rust bridge tests and the portable
Foundation fixtures also run on Linux. [Test devices](devices.md#apple)
lists the Mac and iPad. The build targets arm64 macOS, arm64 iPad devices and
the arm64 iOS simulator:

```bash
rustup target add aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim
```

The iPad app and UI test runner require iPadOS 17.5 or later, including the
12.9-inch second-generation iPad Pro. On iPadOS 18 and later, scrolling and
fitted sheets use the current SwiftUI APIs. On iPadOS 17, native scroll
observation preserves layer offsets and scroll cancellation, and sheet detents
follow the content height.

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
Rust library through `scripts/rust.sh`. Mac and simulator builds use generic
destinations. Arguments after the platform are passed to `xcodebuild`.

- `CAPY_CONFIGURATION=Release` builds optimized Swift and Rust.
- `CAPY_DESTINATION='id=DEVICE_UDID'` selects a device destination.
- `CAPY_DERIVED_DATA` changes the output directory (default
  `apps/layer-apple/DerivedData`, ignored by Git).
- iOS Rust output lives in `target/ios-<deployment target>` so a build cannot
  reuse libraries compiled for a newer minimum OS. `CAPY_RUST_BUILD_ROOT` on
  the `xcodebuild` command line overrides both the Cargo output and library path;
  keep each override specific to its deployment target.
- macOS builds are ad-hoc signed unless `CAPY_APPLE_TEAM` is set.

Check linker output for Rust objects built for a newer OS: the link succeeds
with those warnings, but the app no longer runs on its declared minimum. Build
both Xcode targets to check Swift types; parsing Swift sources alone does not
check closure calls or platform-specific APIs.

### Xcode project

`apps/layer-apple/CapyCanvas.xcodeproj` is tracked, and `scripts/project.py`
regenerates it deterministically from the Swift files under `Shared/`, `iOS/`
and `macOS/`; files in `Tests/` folders go to the UI test targets. Every
`build.sh` run rewrites it. Never edit the project in Xcode or by hand: change
`project.py`, run it, and commit the regenerated project with the change. Adding
or removing a Swift file also changes the project, so commit that diff too.

`scripts/prepare.py` fills the ignored `Generated/` directory with the shared
icons and brush previews from `apps/layer-web`, the iPad app icon from
`iOS/AppIcon.png` and the license files. Edit those sources, never `Generated/`.
`scripts/release.sh mac|ipad` archives a [release](releasing.md#release-workflow)
with its dependency notices, writing an Xcode result bundle and `archive.log`
under `dist/apple-<platform>`; only `release.sh ipad publish` uploads.

### Identity, sandbox and privacy

Both apps share one bundle identifier, so the App Store sells them as one
Universal Purchase: `art.capycanvas.CapyCanvas` for Release and
`art.capycanvas.CapyCanvas.dev` for Debug. A Debug build therefore never opens
a Release build's files. `CAPY_APPLE_BUNDLE_ID=<id>` on the `xcodebuild` command
line is the only identity override; the UI test runners append `.tests`. The
apps installed under the old `art.capycanvas.apple.ipad` and
`art.capycanvas.apple.mac` identifiers are separate apps that these builds
never read or replace; leave them and their data alone.

The Mac app runs in the App Sandbox with the hardened runtime in every
configuration. `macOS/App/CapyCanvas.entitlements` grants
`com.apple.security.app-sandbox`, user-selected read/write access and
`com.apple.security.files.bookmarks.app-scope` for retained file grants. Its
storage folders live in `~/Library/Containers/<bundle id>/Data`. The iPad app
needs no entitlements. Both `Info.plist` files set
`ITSAppUsesNonExemptEncryption` to false: the apps only hash data and make no
network connections.

`Shared/PrivacyInfo.xcprivacy` is copied into both apps. It declares no
tracking and no collected data, and lists every required-reason API the Swift,
Rust and vendored code uses:

| Category | Reasons | Uses |
| --- | --- | --- |
| File timestamp | `C617.1`, `3B52.1` | `stat`/`fstat` on the app's own stores, including ordering unrestored drawing sessions, and on files the painter opened or chose |
| System boot time | `35F9.1` | `ProcessInfo.systemUptime` for animation and tap timing; `mach_absolute_time` in wgpu's Metal presentation timer |

Update the manifest in the same change when code starts using another
required-reason API (for example `UserDefaults`, disk capacity keys or active
keyboards). The workspace store links the system SQLite, which needs no
disk-space reason, so every link of `liblayer_apple.a` adds `-lsqlite3`.

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
  --console art.capycanvas.CapyCanvas.dev
```

Or open the project in Xcode and run the `CapyCanvas-Mac` or `CapyCanvas-iPad`
scheme. Attach Xcode or LLDB to the launched process for breakpoints and native
errors. Wait for each build or install to finish before launching.

**Isolated installs.** Installed apps can hold an artist's drawings; never
replace them or clear their data. Pass `CAPY_APPLE_BUNDLE_ID=<your id>` to
`xcodebuild` to build under another identity; its UI test target uses the
`.tests` suffix, so the regular editor stays installed. Benchmarks use their own
identity and DerivedData directory as well. A free Personal Team profile limits
how many apps a device may hold, and the XCTest runner counts as one; do not
remove the artist's apps to make room.

**Private storage.** `CAPY_STORAGE_DIR` puts every store of a launch in one
folder instead of the installation's folders. A relative name is a folder inside
the app's temporary folder, which the system clears; tests can name it without
knowing the sandboxed app's paths. Give every experiment and test its own name
so it never reads or changes the installation's settings, workspaces or
drawings.

**Debug fixture variables.** Release builds ignore these.

| Variable | Effect |
| --- | --- |
| `CAPY_INITIAL_ACTIONS` | JSON array of shared actions applied once after restoration and the first surface size. |

For example, this opens the Tool panel without driving the Mac menu bar:

```sh
open -n --env CAPY_STORAGE_DIR=capy-try-tool-panel \
  --env CAPY_INITIAL_ACTIONS='[{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":true}}]' \
  apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

## Editor integration

Swift presents shared state and forwards native input. Keep these contracts when
changing the bridge, menus, settings, publications or layers:

- **One owner thread.** `NativeOwner` runs Rust and Metal state, rendering and
  drawable acquisition in FIFO order on one Foundation thread with an 8 MiB
  stack; GCD timers enqueue work onto it. File reads, image decoding, hashing and
  preview copies run on the existing file and image workers, never on UI or input.
- **C bridge.** Typed generation handles for sessions, jobs and output buffers
  fail without dereferencing host addresses when invalid, expired, wrong-kind or
  busy. Every `CapyText` result is owned; `NativeBuffers.swift` copies it and
  releases the token. `native/include/CapyApple.h` documents each call's lifetimes.
- **Metal layers.** Configure surfaces inside a balanced Core Animation
  transaction with implicit actions disabled: view-backed layers on the main
  queue, standalone layers on their owner. Never query AppKit from the render
  queue. The render owner configures the iPad canvas's standalone
  `ObservedMetalLayer`; main-actor layout changes only its frame and contents
  scale. When changing this boundary, run `tests/metal-layer-threading.swift` and
  `tests/managed-display.swift`, check `CA_DEBUG_TRANSACTIONS=1` and Main Thread
  Checker output, and measure resize motion in Release.
- **Shared gates and defaults.** Check `CommandId::available_on`,
  `HeaderItem::available_on`, `Platform` capabilities and `layout_presets.rs`.
  Enable Apple gates with the presentation and shared platform tests. Compare
  `WorkspacePreset::layout(platform)` before and after for every preset/platform.
- **Publications.** Handle small `layer-host` packets such as
  `{command_search, revision}` in `EditorSnapshotState.receive` without promoting
  the content revision. Publish `header_presentation` atomically with the
  resolved layout. Attach visibility queries such as `canvas_bar_layout` to an
  always present container; `.task` and `.onAppear` do not run on an empty
  SwiftUI `Group`.
- **Retained panels.** Resolve each workspace container by stable ID and read its
  indexed panel values together before constructing lazy content. Constrained
  bodies keep the shared `min_body_height` inside an outer `EditorScrollView`;
  tabs and footer grips stay outside that viewport.
- **Dialogs and menus.** Keep common sizing, footer margins and Mac button faces
  in `EditorPopupSurface.swift`. Use `EditorActionMenu` and `editorPopover` with
  one `EditorPopoverHost` at the editor or sheet root, and register each
  presented source's popup lifetime with the workspace. Adapt these instead of
  adding per-menu presenters or gesture overrides.
- **OS presentation.** macOS top-level menus use the OS menu bar; iPad menus use
  the same command catalog in-app, and native iPadOS window controls replace the
  shared Fullscreen command. The shared editor owns document tabs; native window
  tabbing is disabled. Present a Mac file panel from the topmost attached sheet
  of its drawing window, and an iPad document picker from a controller attached
  to its editor window after the current transition.
- **Settings pages.** macOS sheets have no title bar; put sub-page titles and
  Back buttons in the content. Forward nonediting type-to-search input through
  the shared key predicate; native text, IME and shortcut capture keep their keys.
- **Text fields.** iPadOS consumes Escape before SwiftUI handlers, so use UIKit
  text fields with priority key commands. Assign fonts and placeholders only when
  they change.
- **Layers.** Drop previews and releases query `layer_drop`; reject stale
  replies and commit the release through the shared `Drop`. Object layers use
  ordinary rows and shared actions ([image objects](../ui/image-objects.md)).

## Tool bubbles

`WorkspaceBubbles` retains bubble models apart from their placement, the
stateless `capy_apple_bubble_ui` bridge uses the shared hit geometry, and the
workspace's `NativeReorderInput` owns native capture. On iPadOS,
`ColorTouchSurface` forwards direct wheel, arc and ring touches as `color_touch`
input ([color undo](../ui/color-picker.md#color-undo)); its portable fixture runs
on Linux too:

```sh
swiftc -parse-as-library apps/layer-apple/Shared/Bridge/ColorTouchSurface.swift \
  apps/layer-apple/tests/color-touch-surface.swift -o artifacts/color-touch-surface
artifacts/color-touch-surface
```

The mounted fixtures run the production editor and Metal canvas for both Apple
policies and themes:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/tool-bubbles.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/hdr-tool-bubbles.swift
```

Run the [UI tests](#ui-tests) `testToolBubbles`, `testHDRToolBubbles`,
`testHDRToolBubblesF32` (each with a `Dark` variant) and
`testHDRToolBubbleTextEntry` separately for macOS and iPadOS. Native fonts and
window chrome differ from GTK and Web, so visual checks compare authored state,
camera, geometry and colors. [Simulated input](#tests) does not establish
physical Pencil or touch behavior. Measure motion with
[Apple performance](../../apps/layer-apple/PERFORMANCE.md) and the
[measurement rules](../performance/measuring.md).

## Tests

[Testing](testing.md) lists which checks a change needs.

| Where | Checks |
| --- | --- |
| Linux or Mac | The Python script tests below and the portable Swift fixtures |
| Hardware GPU, Linux or Mac | [Rust bridge](#rust-bridge) tests and the [command coverage audit](#command-coverage-audit) |
| Mac, no GPU | `bash apps/layer-apple/scripts/test-without-gpu.sh` |
| Mac with Metal | [Swift fixtures](#swift-fixtures) that attach a canvas |
| Mac with a simulator or iPad | [UI tests](#ui-tests) through `xcodebuild test` |

The Python script tests run without Xcode; the release-script tests use stubbed
build tools and never sign or publish:

```sh
python3 -m unittest discover -s apps/layer-apple/tests -p 'test_*.py'
```

Simulated input has one limit everywhere: XCTest, supplied touches, recognizer
events and tablet records do not establish physical Pencil, trackpad or tablet
sensor behavior, OS input dispatch or performance. Check those on hardware.

### Rust bridge

```sh
cargo test --locked -p layer-apple --lib -- --test-threads=1
cargo test -p layer-apple tests::photo -- --test-threads=1
```

The tests drive both Apple platform policies through the real C ABI and a
hardware GPU, comparing exact document pixels through Undo and Redo. Read
viewport output as well as authored pixels, so a correct export cannot hide a
blank canvas. Filter by module (`tests::input`, `tests::session`,
`tests::papers`, `tests::workspace` and the other `*_tests.rs` files under
`native/src`), and reuse `fixtures::tempfile()` for private file descriptors.
The ignored 61 MP regression needs a disposable sRGB JPEG:

```sh
CAPY_APPLE_PHOTO_JPEG=/path/to/photo.jpg cargo test -p layer-apple --lib \
  large_jpeg_gpen_preserves_photo_through_save_and_gpu_recovery -- \
  --ignored --nocapture --test-threads=1
```

### Swift fixtures

The fixtures in `apps/layer-apple/tests` run the production Swift sources on the
Mac without XCTest or a simulator:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
```

`test-project-files.sh` builds the Rust library, compiles `Shared/`, `macOS/`
and `tests/support` with the given fixture (default `tests/project-files.swift`)
and runs it with `CAPY_STORAGE_DIR` inside its own temporary folder, which it
deletes afterwards. Both Apple policies run on the Mac with that storage.

- `CAPY_TEST_ASSETS_APP` points at a built Mac app when a fixture needs the
  vector or filter assets.
- `CAPY_CONFIGURATION=Release` builds the fixture and Rust library optimized.
- `CAPY_TEST_BUILD_DIRECTORY` keeps the built fixture there instead of running
  it. Run it yourself, with a fresh `CAPY_STORAGE_DIR`, to pass arguments such as
  `canvas-native-input.swift --paper`, `--pen-response` (with
  `CAPY_PEN_THEME=light` or `dark`) or `--device-flags`, and
  `panel-measurements.swift --gpu`.

Fixtures that open windows take focus: run them one at a time and never
alongside a UI test batch. Choose fixtures by name from `tests/`;
`test-without-gpu.sh` runs those that attach no Metal surface.

Fixtures for one or two Swift sources compile directly; the fixture list in
`scripts/test-without-gpu.sh` names each one's sources. For example:

```sh
xcrun swiftc -parse-as-library apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o "$TMPDIR/capy-frame-driver" && "$TMPDIR/capy-frame-driver"
```

The Foundation-only queue and pen-response fixtures also run on Linux, as does
the [color touch fixture](#tool-bubbles). The queue fixture substitutes only the
unavailable Objective-C autorelease pool:

```sh
swiftc apps/layer-apple/Shared/Bridge/RenderOwnerQueue.swift \
  apps/layer-apple/tests/render-owner-queue.swift -o artifacts/render-owner-queue
artifacts/render-owner-queue
swiftc -parse-as-library apps/layer-apple/Shared/Bridge/JSON.swift \
  apps/layer-apple/Shared/Bridge/StylusTilt.swift \
  apps/layer-apple/tests/pen-response-input.swift -o artifacts/pen-response-input
artifacts/pen-response-input
```

The UIKit fixtures `canvas-modifiers.swift`, `canvas-hover.swift` and
`pencil-estimates.swift` run as a standalone scene app built from `Shared/` and
`iOS/`, with no runner script; start their checks from application launch so a
restored scene cannot bypass fixture entry. [Apple input](../../apps/layer-apple/INPUT.md#checks)
lists the input checks.

Other runners:

| Runner | Checks |
| --- | --- |
| `scripts/test-without-gpu.sh` | Direct-compile fixtures, project fixtures without a Metal surface, and the color input, project access and background expiration runners; logs and header captures under `artifacts/release-tests/apple` |
| `scripts/test-color-input.sh` | AppKit color text entry in all RGB spaces |
| `scripts/test-project-access.py` | Project writes through a file-only App Sandbox grant |
| `tests/background-expiration.py` | iPad background-task lease ordering |
| `scripts/test-persistence.sh` | Atomic settings files, backup exclusion and the settings owner |
| `scripts/test-native-rows.py` | UIKit row scrolling and contacts on one booted iPad simulator; `--fixture scenes` for per-scene cancellation, `--fixture scroll` for scroll observation |
| `scripts/test-workspace-scrolling.py` | Long workspace list on an iPad simulator |

### UI tests

Shared journeys are `check…` functions in `Shared/Tests`, run by `test…` methods
on `EditorLaunchTests` in both UI targets; platform-only journeys extend it in
`iOS/Tests` or `macOS/Tests`. Run them through `xcodebuild test` with
`-only-testing`. Use a fresh result-bundle path for each run and inspect the
`.xcresult` for failures and attachments:

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

`editorTestApplication()` gives each journey's application a new
`CAPY_STORAGE_DIR` name, kept across its relaunches, so journeys never touch
the installation's data. Do not create an `XCUIApplication` for the editor any
other way. The iOS simulator lacks Float32 filtering, so its canvas exercises the
[texel-load fallback](../internals/rendering.md#optional-float32-features) that
A12–A16 iPads use; hardware filtering, Pencil and timing need a physical iPad.

**Physical iPad.** Keep the device awake and unlocked and turn on
**Settings → Developer → Enable UI Automation**; Developer Mode alone does not
enable the UI runner. Resolve any device-trust, automation or capture prompt
before retrying. A runner that times out starting the test-manager session or
enabling automation before any test runs is inconclusive: read the result
bundle's `testmanagerd.log` and stop a leftover Automation Running session on
the iPad. If it recurs on an awake, unlocked device, stop relaunching; launch
your build through `devicectl`, use fixtures or Rust tests, and record touch
journeys as unverified.

To test apps already installed on the iPad, set `UseDestinationArtifacts` in the
`.xctestrun` target with `TestHostBundleIdentifier`,
`UITargetAppBundleIdentifier` and `TestBundleDestinationRelativePath`. Omit
`TestHostPath`, `TestBundlePath`, `UITargetAppPath` and
`DependentProductPaths`, which can make XCTest try to install an unavailable
bundle during `app.launch()`.

On older iPads, `testIPadCompatibilityJourneys` runs the light and dark size,
color, editor, layer, tab and workspace checks in one XCTest case;
`testIPadPanelCompatibilityJourneys`, `testIPadColorCompatibilityJourneys` and
`testIPadPanelLayoutCompatibilityJourneys` run subsets.

Some journeys need prepared data and skip or fail without it:

- **Preserved package.** For `testPreservedPackageCopyOriginal` and its dark
  variant, copy `crates/layer-core/src/package/codec/fixtures/unsupported-pass-through.capy`
  to `Unsupported.capy` in a fresh `Capy Files <UUID>` folder containing only that
  file. Set the test target's `.xctestrun` `EnvironmentVariables` entry
  `CAPY_FILE_TEST_TOKEN` to that UUID. On Mac, set `CAPY_UI_PACKAGE_DIRECTORY` to
  the folder's absolute path. On iPad, put the folder in the private test runner's
  Documents directory, enable that runner's file sharing, name its provider
  `Capy Test Files <UUID>` and set `CAPY_UI_PACKAGE_PROVIDER` to that display name.
- **Seeded sessions.** The iPad `testNativeDrawingLifecycleStress` pair opens
  private Release sessions from the owned `capy-test-drawing-*` folders named by
  `CAPY_UI_DRAWING_STORAGE_LIGHT` and `CAPY_UI_DRAWING_STORAGE_DARK`, each seeded
  with a painted session from the native workload harness. The
  `testMalformedRecoveryStillAllowsNewDrawing` pair uses the
  `capy-test-malformed-*` folders named by `CAPY_UI_MALFORMED_STORAGE_LIGHT` and
  `CAPY_UI_MALFORMED_STORAGE_DARK`. Both skip without their seed. Seed only the
  test app's container, never a user installation. Release ignores Debug initial
  actions.

When writing a journey:

- **Stable targets.** Place strokes relative to `editorPaper(in:)`, select by
  label rather than index, reveal lazy controls by stable identifier, and wait
  for published state. Check changed shared rules before repairing a failed
  journey, and keep its intended assertions.
- **Exact history.** Compare Undo/Redo pixels exactly after the stroke settles;
  the renderer redraws rounded pages at pen-up. Do not add display tolerances.
- **Menus.** Test menu bindings with native fixtures or manual journeys, and
  drive editor effects through `CAPY_INITIAL_ACTIONS` or in-app controls rather
  than the Mac system menu bar. On iPad, reveal a menu leaf in its scroll view
  before activating it.
- **Pencil and fingers.** XCTest cannot synthesize Pencil, and finger drags on
  the canvas navigate, so create artwork and selections through menus. Ordinary
  supplied pinches must move beyond tap slop and outlast the quick-pinch
  interval; run whole-canvas pinches and multi-finger taps in `withHiddenChrome`.
- **Text entry.** XCTest Escape does not reach iPad apps; use visible controls.
  Dismiss the iPad keyboard before activating controls it covers or counting
  rows in panels it shrinks, and verify the complete typed value before Return.
- **Accessible geometry.** iOS container frames are the union of their children,
  so don't reuse macOS widths; close open editor menus by the recorded button
  frame, since they hide their buttons from queries. Tap controls around the
  Color wheel's ring with `colorControlPoint`. On macOS, scope dialog-button
  captions to their window or sheet; application-wide queries include Touch Bar
  copies.
- **Window state.** In iPadOS 26 windowed scenes the system resize handle owns
  touches in the bottom-trailing corner; start touch drags elsewhere. macOS
  retains window sizes even with `-ApplePersistenceIgnoreState`; restore them in
  teardown.
- **Retries.** Gather evidence and a specific hypothesis before repeating an
  inconclusive run. Prefer existing Swift fixtures and Rust tests to new automation.

Known failures on iPad: `testCompactMenuShortcutAcrossPages` (Command-Z) and
`testSettingsTextSelectionShortcut` (Command-A) fail because XCTest key events
do not reach UIKit key commands. On the simulator, title-bar customization near
the window's top edge resizes the OS window instead.

### Command coverage audit

`command-coverage.json` records expected unavailable commands and handler/check
references for panel controls, preferences and properties. The audit checks
catalog entries, availability and resolved tool choices:

```sh
cargo run --locked -p layer-host --example inventory -- --gpu > "$TMPDIR/capy-inventory.json"
python3 apps/layer-apple/scripts/audit-commands.py "$TMPDIR/capy-inventory.json"
python3 apps/layer-apple/scripts/test-property-audit.py "$TMPDIR/capy-inventory.json"
```

The audit establishes catalog coverage, not working native workflows. The
inventory visits every catalog tool command and brush preset, then follows the
rendered choices. Run its regression checks with
`cargo test --locked -p layer-host --example inventory`.

## Debugging and evidence

- Discover destinations with `xcrun devicectl list devices` and
  `xcodebuild -showdestinations -project apps/layer-apple/CapyCanvas.xcodeproj -scheme CapyCanvas-iPad`.
- For a startup failure, copy the canvas error panel's selectable diagnostics
  (app and OS versions, hardware model, failing Metal stage) before changing the
  installation. If it shows only a headline, connect the iPad to Mac Console and
  filter for `Capy Canvas native diagnostic:`, `Capy Canvas renderer diagnostic:`
  or `CapyCanvas GPU stopped:` while reproducing. A recovery banner after the
  canvas loads is a separate saved-session failure.
- Keep raw logs, screenshots and traces in ignored `artifacts/` or
  `apps/layer-apple/DerivedData/`. Record the tested revision and launched PID,
  and close only your own test processes.
- For pixel comparisons with Web, use the [visual tools](../../tools/visual/README.md)
  with matching state, viewport, scale and color space on native and local
  Chrome.
- Measure performance in Release builds as described in
  [Apple performance](../../apps/layer-apple/PERFORMANCE.md). Keep builds and UI
  automation idle while timing.
