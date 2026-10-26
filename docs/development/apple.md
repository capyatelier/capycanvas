# macOS and iPadOS development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Apple clients use AppKit on macOS and UIKit on iPadOS. They share Swift
editor components and the `layer-apple` Rust bridge built around `NativeHost`.
Metal renders the canvas through a `CAMetalLayer` behind native controls. The
[package README](../../apps/layer-apple/README.md) describes the source layout,
and the [Apple porting guide](../APPLE_PORTING_GUIDE.md) defines the audit,
cleanup and validation workflow. Features usually land on GTK, Web and Android first.
Both hosts present [tool bubbles](../ui/tool-bubbles.md) through the shared
native views and actions.

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
follow the content height. Passive UIKit bridges opt out of SwiftUI hit testing;
chrome contact observation stays on the window and converts to editor coordinates.
Keyboard-only background views have no touch area.
Fitted Color panels use their natural height without a scroll container on
iPadOS 17, preserving the entire control target at the same position.
Both versions use the same renderer and brush quality.

`NativeOwner` runs Rust and Metal state on one Foundation thread with an 8 MiB
stack. Its queue preserves FIFO input, delayed retries and the asynchronous
startup gate; GCD timers enqueue work onto that thread. Final detach and destruction
drain there before the thread exits. Launch constructs the shared session directly
on its owner rather than moving a non-`Send` App across a worker boundary.
The queue fixture can run without an Apple UI:

```sh
swiftc apps/layer-apple/Shared/Bridge/RenderOwnerQueue.swift \
  apps/layer-apple/tests/render-owner-queue.swift -o artifacts/render-owner-queue
artifacts/render-owner-queue
```

The Linux fixture substitutes only the unavailable Objective-C autorelease pool;
it verifies queue mechanics, not Metal, SwiftUI or native Apple integration.

The C bridge uses typed generation handles for sessions, jobs and output buffers.
Sessions belong to their creating render thread; worker jobs retain owned state.
Invalid, expired, wrong-kind and busy handles fail without dereferencing host
addresses. A job already using a handle retains its state through concurrent
retirement. The caller still supplies valid, exclusive C input and output pointers
and keeps native descriptors and Metal objects alive for their documented calls.

Every `CapyText` result is owned. `NativeBuffers.swift` copies text and pixel
ranges of at most 8 MiB and releases the corresponding token. Metadata reads
initialize outputs on failure and return separately owned text handles. Image
copies run on workers; scope delivery preserves delta order and checks the
drawing identity before publishing to the UI.

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
destinations, so the project's ARM64 target does not depend on the architecture
of the build process, including when CodeQL traces it.
Arguments after the platform are passed to `xcodebuild`.

- `CAPY_CONFIGURATION=Release` builds optimized Swift and Rust.
- `CAPY_DESTINATION='id=DEVICE_UDID'` selects a device destination.
- `CAPY_DERIVED_DATA` changes the output directory (default
  `apps/layer-apple/DerivedData`, ignored by Git).
- iOS Rust output lives in `target/ios-<deployment target>` so a build cannot
  reuse libraries compiled for a newer minimum OS. `CAPY_RUST_BUILD_ROOT` on
  the `xcodebuild` command line overrides both the Cargo output and library path.
- macOS builds are ad-hoc signed unless `CAPY_APPLE_TEAM` is set.

Keep each `CAPY_RUST_BUILD_ROOT` override specific to its deployment target;
do not reuse a directory built for a different minimum OS. Check linker output
for Rust objects built for a newer OS. A successful link with those warnings
does not establish support for the app's declared minimum.

Build both Xcode targets to check Swift types; parsing Swift sources alone does
not check closure calls or platform-specific APIs. For startup failures, record
the app version/build and OS version before changing the installation. The canvas
error panel shows selectable native diagnostics with the app version/build, OS
version and hardware model identifier. Metal errors retain the failing startup
stage and available adapter details; launch and panic boundaries preserve the
underlying error. Missing hardware adapters and adapters below the canvas limits
use the shared localized unsupported-GPU explanation before the Metal diagnostic;
`layer_host::unsupported_adapter` owns the hardware and limits checks. Include
this text when reporting a failure. In older builds
that show only the headline, connect the iPad to Mac Console and filter for
`Capy Canvas native diagnostic:`, `Capy Canvas renderer diagnostic:` or
`CapyCanvas GPU stopped:` while reproducing. A recovery banner after the canvas
loads is a separate saved-session failure.

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
`scripts/release.sh` also adds the Rust toolchain notices and, through
`scripts/package-notices.mjs` and the pinned `cargo-about`, the exact notices of
the Rust dependencies linked for the archived target.
The iPad archive is retained after building. `scripts/release.sh ipad publish`
uploads that existing signed archive after the other platform packages pass;
building an archive never uploads it.
The Xcode generation phase declares its resource outputs so incremental builds
compile the newly generated assets and localized names.

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
`com.apple.security.files.bookmarks.app-scope` for retained file grants. Files
reach the app through Open and Save panels, Finder, drag and drop and the clipboard. Its
storage folders live in `~/Library/Containers/<bundle id>/Data`. The iPad app
needs no entitlements.

Both `Info.plist` files set `ITSAppUsesNonExemptEncryption` to false, so
uploads skip the export-compliance question: the apps only hash data and make
no network connections. The Mac app's category is
`public.app-category.graphics-design`.

`Shared/PrivacyInfo.xcprivacy` is copied into both apps. It declares no
tracking and no collected data, matching the privacy policy: the apps make no
network connections, and crash reports come only through Apple's own
diagnostics. It lists every required-reason API the Swift, Rust and vendored
code uses:

| Category | Reasons | Uses |
| --- | --- | --- |
| File timestamp | `C617.1`, `3B52.1` | `stat`/`fstat` on the app's own stores, including ordering unrestored drawing sessions, and on files the painter opened or chose |
| System boot time | `35F9.1` | `ProcessInfo.systemUptime` for animation and tap timing; `mach_absolute_time` in wgpu's Metal presentation timer |

The workspace store links the SQLite that ships with macOS and iPadOS rather
than the bundled copy other platforms build, so its file-system checks stay in
Apple's library and need no disk-space reason. Every link of `liblayer_apple.a`,
in the Xcode project and the Swift fixture scripts, adds `-lsqlite3`.

Update the manifest in the same change when code starts using another
required-reason API (for example `UserDefaults`, disk capacity keys or active
keyboards).

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

The bridge converts each serial-owned app pointer once per operation. Recovery
preparation uses the same mutable owner through a normal Rust reborrow. Plain
write-only FFI outputs accept aligned, exclusively writable uninitialized storage;
the bridge writes whole values without first constructing initialized references.
Borrowed preview pointers remain valid only until the documented next task
mutation or release.

- **Shared gates and defaults.** Check `CommandId::available_on`,
  `HeaderItem::available_on`, `Platform` capabilities and `layout_presets.rs`.
  Preserve absent optional identities in native projections; Quick Mask's row ID
  is zero and an absent rename owner does not start name editing.
  Enable Apple gates with the presentation and shared platform tests. Compare
  `WorkspacePreset::layout(platform)` before and after for every preset/platform.
- **New drawings and Paper.** `NewDrawingDialog.swift` presents the shared
  [New drawing](../ui/new-drawing.md) view as a sheet. It opens with the
  window's screen and `upcoming_untitled`, sends `new_drawing` actions, reuses
  Edit Color's form and the Choose Texture tiles, and continues New through
  `capy_project_new` only after Create. On iPad, use the native numeric text
  adapter so every edit reaches the session before keyboard dismissal or Create.
  `testNewDrawingDialog` and its Dark variant walk the dialog, including
  Texture visibility and the Import… tile;
  `testNewDrawingAndExportCancellation` checks cancellation. `DrawingWorkload`
  creates its drawing on flat paper so performance rows stay comparable.
- **Paper library.** Both Apple hosts present shared Paper texture tiles,
  selection and Scale. Use the shared installation papers directory and storage,
  import and identity APIs. Startup and Import/Add/Load/Rename/Delete use immutable
  owner captures, file-worker preparation and owner adoption. File reads, image
  decoding and preview copies stay off UI/input. Imported images remain authored
  data when library entries are renamed or deleted. Keep only three strongly loaded
  Paper images; weak canonical owners follow drawing/history lifetimes across
  deletion and eviction. A conflicting cached owner uses the existing file worker
  to verify the full Paper hash and return the destination editor's retained
  source, including after package/recovery/tab adoption. No image decode or hash
  runs on UI/input, and immutable resource identity checks remain strict.
  Retire completed, failed and
  cancelled requests; stale results cannot edit another drawing. Reuse shared
  labels, validation and history. Prepare remembered Paper material on the existing
  settings/file worker before initial drawing construction or explicit New;
  constructors consume the captured settings, language and texture together.
  Missing library material keeps the built-in fallback and remembered ID, raises
  one shared file-open notice and does not enqueue an automatic duplicate load.
  Recovery keeps its authored paper; late cache reads cannot replace current art.
  Walk import cancellation, reload/restart, tile
  actions, exact artwork Undo/Redo and file-provider paths on Mac/iPad in both
  themes; mounted fixtures do not prove OS delivery.
- **Menus.** Use `EditorActionMenu` and `editorPopover`, with one
  `EditorPopoverHost` at the editor or sheet root outside clipped panels. Adapt
  existing presenters for reproduced native integration needs. Each presented source
  registers its popup lifetime with the workspace, including native sheets; shared outside-contact handling
  keeps the parent drawer open for child-menu contacts. Sources owned by one native
  interaction keep distinct lifetime tokens with that interaction’s exclusion identity. Avoid speculative
  gesture overrides, menu/drag handoffs or per-menu presenters. Keep menus fast,
  theme colors readable and opaque, and animation minimal, without
  glass shine. Preserve native keyboard and accessibility conventions.
  Apply a custom button label’s `contentShape` after its final frame so blank
  space inside the control accepts input with its text and icon.
  UIKit menu holds add failure priority only within their own native host view.
  Exclude pan and pinch recognizers from this added priority.
  The iPad chrome observer attaches passively to the editor window. It converts
  contacts through the noninteractive editor marker and admits only points inside
  that marker's bounds. Canvas contacts stay on the canvas input path; native
  controls receive touches without delay, cancellation or gesture prevention.
  Admitted contacts feed workspace outside-contact handling, preserving popup
  ownership and the slider preview's exemptions for its own bounds and anchor.
- **Window ownership.** The root native window bridge owns the document delegate,
  file-picker presenter, close callback and fullscreen binding. Canvas replacement
  keeps those bindings. Retire them only while the retiring delegate still owns
  that window, and restore the original downstream delegate.
- **OS presentation.** macOS top-level menus use the OS menu bar; the header keeps
  its title, Zen and other controls over the live canvas. Zen clears native window
  controls. Remove the designated native Help submenu by identity after launch
  and main-menu insertion, retaining an unattached help menu alongside the empty
  Help command group. iPad menus use the same command catalog in-app; native
  iPadOS window controls provide full screen, so the shared Fullscreen command is
  unavailable. Present a Mac file panel from the topmost attached sheet of its
  drawing window. A second sheet attached to the base window waits for the first
  to close, so a preview that remains open during delivery must own its Save panel.
  Arm export delivery before publishing preview dismissal; dismissal observers
  can run synchronously.
  Keep delivery armed until the drawing window has detached its export sheet.
  Observe that window’s sheet-end notification before presenting the destination panel.
  Blocked editor content must also be hidden from accessibility while the
  presented controls retain their own identifiers.
  On iPad, present document pickers from a controller attached to their editor
  window after its current transition completes. Retain the native picker until
  its delegate completes selection or cancellation.
- **Panel checkboxes.** Use the shared accent and accent foreground when checked,
  and the panel border line when unchecked. Let captions request wrapping.
- **Scopes.** Measure the toolbar, axes, Log row and footer independently. Give
  the graph the remaining allocated height while keeping its natural workspace
  measurement independent of the current allocation.
  Run the retained `panel-measurements` binary with `--gpu` for mounted scope
  allocation checks; its default run stays in `test-without-gpu.sh`.
- **Independent publications.** Handle small `layer-host` packets such as
  `{command_search, revision}` in `EditorSnapshotState.receive` without promoting
  the content revision or refreshing retained panels and thumbnails.
  Stage `header_presentation` from geometry packets beside the resolved layout;
  keep it outside the authored workspace layout and publish them atomically.
- **Retained panels.** Resolve each workspace container by stable ID and read its
  indexed panel values together before constructing lazy content. Observe
  expansion separately for placement and input admission. Removed containers
  withdraw their native sources; surviving containers keep live metadata.
  Constrained bodies retain the shared `min_body_height` inside an outer
  `EditorScrollView`; tabs and footer grips stay outside that viewport. Intersect
  Navigator placements with the body viewport so scrolling clips its native image
  before the header and grip.
- **Palette footer.** Allocate at most 55% of the proposed row width to the
  chooser. Measure the color details at the remaining width and centre both
  controls in the resulting row. Footer sizing must not publish view state.
- **Visibility queries.** Attach queries such as `canvas_bar_layout` to an always
  present container; `.task` and `.onAppear` do not run on an empty SwiftUI `Group`.
- **Layer relationships.** Draw shared endpoints over measured row and content
  thumbnail frames, clipped to the scroll viewport, using current `relationship`
  and `text` palette roles. Forward supplied attachment/swipe actions and preserve
  local visibility under inherited hiding. Preview and release query `layer_drop`
  with epoch, raw target, fraction and hit surface; reject cancelled or stale
  document replies, and commit the raw release hit through shared `Drop`.
  Object layers use ordinary rows and shared actions; see [image objects](../ui/image-objects.md).
- **Settings pages.** macOS sheets have no title bar. Put sub-page titles and Back
  buttons in the content; navigation titles and `.navigation` toolbar items are hidden.
  Forward nonediting type-to-search input using the shared key predicate, then
  follow `preferences.search_focus`. Native text, IME and shortcut capture keep
  their keys. Normalize SwiftUI navigation and function keys before testing the
  shared predicate, and preserve typed character case.
  While search results are displayed, the native page list has no selection;
  choosing any page forwards the shared Page action, which clears search.
  Reset retires that row's number or custom-color draft before forwarding the
  shared reset. Late Done and focus callbacks cannot commit a retired editor;
  ordinary snapshot updates preserve the current focused draft.
  `testNumericSettingsDone` and `testSettingsTextState`, with their Dark variants,
  cover native Done, page activation and focused draft reset in both themes.
  Build `tests/settings-links.swift` with `scripts/test-project-files.sh` and
  `CAPY_TEST_BUILD_DIRECTORY` to retain its executable. Run it with
  `--typed-search` for native list/button arrival, text editing, IME callbacks
  and shortcut capture; its default route checks Settings link handling.
- **Bubble pickup.** An admitted drag retains capture after its source drawer
  closes, through release or cancellation. A popup cancels the current edit
  before retiring its contact. Shared cancellation restores displaced bubbles
  and the preceding workspace. Canonical native Escape dismisses stamp previews.
- **New drawing.** Admit pending numeric edits before changing choices or
  opening a drawer. An invalid expression keeps its field and the accepted
  setup. Restore focus to the control that opened each drawer; focus search
  and preset naming when their native fields mount. Forward unconsumed Escape
  to shared Back, preserving native text and composition handling. Add and Use
  Color commit the focused color draft before forwarding their shared action.
- **Lazy controls in journeys.** Reveal unmounted scroll children by their
  stable identifier before querying availability. Read shared counts instead
  of counting the currently mounted accessibility children.
- **UIKit fields.** iPadOS consumes Escape before SwiftUI handlers. Use UIKit text
  fields with priority key commands, explicit height, and fonts/placeholders assigned
  only when changed, to avoid repeated size invalidation.
- **Native control cancellation.** Cancel a control's owned contact once. Idle
  AppKit controls forward `cancelOperation` through the next responder's
  `tryToPerform`; the superclass need not implement that action.
- **Inline workspace names.** Preserve the native text field's accessibility
  while renaming. The field owns Space, Return and text composition; row
  shortcuts apply outside editing.

## Tool bubbles

`WorkspaceBubbles` retains each bubble's model independently of its placement.
`CanvasFrameDriver` advances shared motion and native transitions while they are
active, then sleeps. The overlay remains in Zen. The stateless
`capy_apple_bubble_ui` bridge uses the shared hit geometry; gaps between parts
remain canvas. The workspace's existing `NativeReorderInput` owns native source
capture, holds, cancellation and two-finger takeover. A second finger cancels
the captured native edit before starting the bubble's move or resize.
On Mac, a primary down forwarded to AppKit keeps its matching release when a
tear-out takes capture. Directly captured contacts retain their release ownership
through cancellation. Native buttons suppress activation after a card drag;
the mounted fixture checks both the next ordinary click and dragging its footer.

The color ring uses `ColorWheelDrawing`, native swatches and the shared color
viewing transform, with the Color panel's separate SDR and HDR presentation.
Floating-point drawings use the shared HDR wheel contour and intensity arc.
The arc forwards transactional contacts, reset and keyboard adjustments to Rust.
Its continuous angular gradient and opaque marker use shared preview mapping for
the current display headroom. Linear HDR values remain extended-range; SDR
displays use the drawing's rendition. The shared zero-EV tick follows range
expansion. The whole contour registers as one glass region, including its lower
arc and inward corners.
`ColorUIResourceCache` retains shared layout or intensity responses until their
inputs change; moving a bubble reuses its color samples. The native slider
accessibility representation publishes the shared EV value, range and disabled
state, with adjustments forwarded to the shared action.
Wheel fields and hue guides are prepared on the image worker.
Native drawing clips every field to the published circle, square or triangle;
HDR field textures contain opaque samples beyond the visible shape.
Brush dabs use the same SDR or HDR color presentation and retain their decoded
image across metadata-only replies; pending
readback keeps the frame clock awake, and a ready unchanged dab stops polling.
The Layers card uses the drawer's `LayerInspector`, layer rows, Properties and
thumbnail cache. The footer uses shared width thresholds and moves hidden
commands into More. Each mounted list and folded face keeps its own thumbnail
demand. Glass registrations carry circular or squircle regions, arc bands and
strength in logical editor points; the native renderer applies display scale.
HDR color plates register their shared contour as a single region; lengths scale
with the display while the end angle and fade strength remain unchanged.
The card and folded count observe placement directly, so their glass follows
stroke fading and homing even when their retained content stays unchanged.
Menu replies retain drawing, source and request ownership. Shared headings and
keep-open choices survive native menu projection.

[Color undo](../ui/color-picker.md#color-undo) brackets each Color panel wheel
and intensity-arc press with `color_contact`. On iPadOS, `ColorTouchSurface`
forwards the direct touches on the wheel circle, the arc and the color ring as
`color_touch` input, one surface per panel or bubble layer, and the second finger
retires the single-finger pick. Two fingers lift a bubble only after they move
8 points. A secondary click inside the wheel circle, and on the Mac also on the
arc, opens the shared `color_wheel` menu; the iPadOS arc has no menu because a
hosting controller over the arc would take the wheel's touches. The portable
fixture runs on Linux too:

```sh
swiftc -parse-as-library apps/layer-apple/Shared/Bridge/ColorTouchSurface.swift \
  apps/layer-apple/tests/color-touch-surface.swift -o artifacts/color-touch-surface
artifacts/color-touch-surface
```

The native taps, the threshold and both menus still need a check on a Mac and
an iPad.

Labels use the app's native system font at the shared size, matching its other
native controls. Font rasterization and window chrome differ from GTK and Web.
Visual checks match authored state, camera, geometry and colors while retaining
the unaltered full-window image differences.

`tests/tool-bubbles.swift` mounts the production editor and Metal canvas for both
Apple policies and themes. `EditorLaunchTests/testToolBubbles` and
`testToolBubblesDark` exercise the full applications. Run them separately for
macOS and iPadOS. Supplied touch or tablet records do not establish physical
Pencil behavior. Use [Apple performance](../../apps/layer-apple/PERFORMANCE.md)
and the [measurement rules](../performance/measuring.md) for motion qualification.

`tests/hdr-tool-bubbles.swift` creates F16 and F32 drawings through the actual
new-drawing pipeline and checks the intensity transaction, exact cancellation,
reset, keyboard input, second-finger takeover and color-context changes for both
Apple policies and themes. `testHDRToolBubbles`, `testHDRToolBubblesDark`,
`testHDRToolBubblesF32` and `testHDRToolBubblesF32Dark` exercise native app input.
`testHDRToolBubbleTextEntry` checks UIKit text-field input after arc keyboard
focus as a separate journey.

Language publications can contain the first ready workspace view. Their readiness
transition uses the ordinary workspace handler, preserving native form drafts.
`tests/workspace-coordinator.swift` checks that recovery starts once in this order.

## Tests

Rust bridge and Python script tests run on Linux. Swift fixtures and native
builds need a Mac.
[Testing](testing.md) lists which checks a change needs.

The Python script tests run without Xcode:

```sh
python3 -m unittest discover -s apps/layer-apple/tests -p 'test_*.py'
```

The release-script fixtures use temporary directories and stubbed build tools to
check archive reuse and upload requirements without signing or publishing.

### Rust bridge

```sh
cargo test --locked -p layer-apple --lib -- --test-threads=1
cargo test -p layer-apple tests::photo -- --test-threads=1
```

The tests drive both Apple platform policies through the real C ABI and a
hardware GPU, comparing exact document pixels through Undo and Redo.
`native_rotation_snapping_menu_and_gestures_follow_both_platform_policies`
checks the shared menu action and native gesture bridge without a GPU.
`testZoomReadout` and `testZoomReadoutDark` check the native Snap rotation
checkbox, unchanged view when toggling it, slider snapping and exact typed
angles and button steps on both Apple clients.
`apple_fill_selection_survives_background_checkpoint_undo_redo_and_restart`
checks recovery history and the first presentation. It reads viewport output
as well as authored pixels, so a correct export cannot hide a blank canvas.
Reuse `fixtures::tempfile()` for private file descriptors. Filter by module
(`tests::input`, `tests::recovery`, `tests::workspace` and the other
`*_tests.rs` files under `native/src`). The windowless `tests::toolbar_component`
checks category and subtool choices, remembered Enclose and Fill, and Source
actions through the native bridge in both themes and platform policies.
`apple_shared_image_objects_keep_f64_poses_pixels_and_history_across_workers_and_restart`
opens the fixed built-in, ICC and Nearest image-object packages through Apple
project workers. It checks shared image ownership, F64 placement, exact visibility
history, save/reopen, renderer replacement and private recovery with pending Redo.
Object layers use ordinary layer rows, selection, visibility, ordering and
menus. Place Image and external paste create one named Object layer per image;
Convert to Object Layer and Rasterize Layer use
the shared background conversion jobs.
The photo-owner fixture compares reopened layer rows and swipe targets by stack
position because runtime handles are allocated afresh when reading a package.
The ignored 61 MP regression needs a
disposable sRGB JPEG:

```sh
CAPY_APPLE_PHOTO_JPEG=/path/to/photo.jpg cargo test -p layer-apple --lib \
  large_jpeg_gpen_preserves_photo_through_save_and_gpu_recovery -- \
  --ignored --nocapture --test-threads=1
```

### Live interface language

Profile imports, saved-profile libraries and profile menus consume the shared
catalog's `profile_copy` through `capyProfileCopy`. Source interpretation uses the
same copy and retains a typed shared source caption by language, channel family
and sample depth, including floating-point HDR depths. Proof controls and numeric
popups use `proof_copy`; proof profile labels use the localized proof form. These
views retain profile names literally and keep ICC reads on the existing file queue.
The Paper Rename sheet reads its heading from `capyNativeCopy`, so changing the
interface language preserves its open draft and typed name while updating the heading.
The mounted `tests/canvas-native-input.swift --paper` journey checks this in both
themes while retaining the draft, library history and exact artwork assertions.

The focused Mac command context carries the published language and shared headings
with its editor.
Standard File, Edit and View headings use their public native action roles;
Window uses the designated windows menu. Their titles come from the shared menu
models and reconcile after native menu construction without replacing actions,
shortcuts or responder routing.

Language changes use the same `NativeOwner` and `EditorStore`. Rust's shared
transition rejects superseded preparation; a separate language queue warms the
immutable context and English command aliases. The drawing owner publishes
bootstrap, catalog, language generation and changed editor views together after
native composition and captured input end. Transient menus and popovers defer
publication; a sheet's presentation lifetime only blocks workspace chrome. Its
labels can update while the sheet retains its draft. The Metal surface and document undo
history keep their owners. Stateless numeric, caption, color and toolbar requests
carry a language tag from the control's published snapshot and use prepared
contexts only.

`EditorPersistence` broadcasts accepted settings before its atomic disk write.
Existing scenes and newly opened scenes therefore receive the accepted language
when storage fails; the initiating scene still exposes its failure and retry
state. Its in-memory settings cache lives only for that persistence owner's
lifetime. System language resolves `Locale.preferredLanguages` when a request is
made, including selecting System again.

Application menus and quit alerts retain the last published shared copy when a
window loses focus or a new window is still loading.

The portable regression `apple_language_publication_is_atomic_deferred_and_window_local`
checks coalescing, deferred publication, complete matching copy, independent
windows and unchanged document/camera state. `EditorLaunchTests/testLiveInterfaceLanguage`
and `testLiveInterfaceLanguageDark` switch Settings to 日本語 and back and check that
the open Settings sheet and the panel tabs relabel without a restart. They also
reopen the slider stamp preview and check both bookmark captions in each language.
They make a visible edit with a mouse stroke on Mac and Select all pixels followed
by Fill selection on iPad, where finger drags navigate. Both keep exact artwork
and Undo/Redo checks through export and close cancellation.
On Apple hardware, also switch while editing color/numeric drafts, renaming with marked text, drawing, dragging,
using open dialogs, and activating parked drawings in both themes. Run
`tests/persistence.swift` for cross-scene delivery after a failed write.

Saved ICC library rows and Print profile menus retain worker-inspected metadata
without ICC bytes. Publishing a language reprojects those descriptors through
`ColorUiRequest::ProfileEntries`; it does not reread or decode the saved profiles.

### Navigation input

Pen pressure and tilt use the shared curve editor in one opaque modeless utility
above the canvas and expanded columns. Switching response kinds replaces native
curve capture state. Tilt presents shared input and adjusted direction vectors
with three independent axis toggles. Apply stays disabled until a pen is known;
mouse edits made before the first pen sample remain available to that pen.
Window observers forward dialog pen samples through the render owner using the
shared `pen_input` action, without painting or stealing input from native controls.
Normal canvas contacts retain their native contact IDs. macOS captures the
pointing device's proximity identity; drivers without a proximity identity share
the generic pen profile. UIKit has one persistent `apple:pencil` profile because
its public input APIs do not identify individual Pencils. Its native frame comes
from the screen's fixed portrait coordinate space converted into canvas axes.
macOS keeps the AppKit tablet frame; physical display rotation is unverified.
The utility is an accessibility container; its curve plot exposes its shared
label and point count as one accessible element, like effect curves. Its native
accessibility action focuses the plot for keyboard editing. Handle absent curve
axes while pressure and effect views retire.
`EditorLaunchTests/testPressureCalibrationLight` and `testPressureCalibrationDark`
check the Preferences entry, draggable title, control insertion and drag-out
removal, Reset, Firmer, Lighter, Apply, Cancel and Close. The macOS journey also
checks painting while the utility stays open. On iPadOS, check that final drawing
step with a physical Pencil; XCTest cannot synthesize Pencil pressure. These
journeys require Xcode and native Apple hardware; Swift parsing alone does not
establish UI behavior.
`tests/persistence-owner.swift` verifies exact pressure response restoration
after Cancel and durable Apply values after a fresh owner opens the same private
settings directory, on both Apple policies.
`testTiltCalibrationLight` and `testTiltCalibrationDark` check cold opening,
independent axis toggles, Reset/Cancel, dragging and pressure/tilt switching.
The persistence-owner fixture also checks tilt sensor binding and durable
response/axis restoration across fresh owners and contact aliases in both themes.
`tests/canvas-native-input.swift --pen-response` supplies native tablet callbacks,
activates the mounted response controls and Apply, then verifies the exact source
profile on disk and through a fresh owner. Run it with `CAPY_PEN_THEME=light` and
`dark`. The `--device-flags` route checks captured rotation capabilities, matching
devices, zero rotation and terminal samples through the native recording.
The GPU-free Rust `apple_tilt_` tests cover both Apple policies through the C
request boundary. The portable Swift input fixture checks actual action encoding,
projected tilt and all four native coordinate frames:

```sh
swiftc -parse-as-library apps/layer-apple/Shared/Bridge/JSON.swift \
  apps/layer-apple/Shared/Bridge/StylusTilt.swift \
  apps/layer-apple/tests/pen-response-input.swift -o artifacts/pen-response-input
artifacts/pen-response-input
```

Native cursors read the shared `navigation_cursor` snapshot, including command,
modifier and restored-document changes. Toolbar and header double activation use
the shared control metadata and action; Swift retains only native click timing.
Drawing-cycle shortcuts come from the shared command catalog and settings.
Apple menu bindings project the shared primary Tab chord to physical Control-Tab;
ordinary primary chords use Command. Saved shortcuts retain their shared schema,
and native menus and custom popups consume the same published bindings.

Zoom has a separate toolbar control; Hand and Rotate view share their remembered
group. `EditorLaunchTests/testZoomTool` and `testZoomToolDark` drive click-in/out,
horizontal and vertical scrubs, area Zoom and click-only mode with native mouse
or touch, and check unchanged artwork and history.
The AppKit and UIKit canvas input fixtures also change the wheel and two-finger
rotation preferences. Control retains wheel zoom, Shift pans horizontally, and
disabling rotation preserves pinch zoom. UIKit checks consecutive direct-touch
pairs with reused identities and complete contact retirement.

AppKit magnify/rotate phases and UIKit pinch/rotation recognizers form one group
while they overlap. The first accepted event begins the shared view gesture;
remaining deltas retain that starting view for Previous view. Contact exclusion
and interruption remain in the native adapters. The input fixtures check
separate, overlapping, cancelled and initially blocked groups with two-way view
recall and unchanged artwork history. Supplied recognizer events do not establish
physical trackpad or Pencil sensor behavior.

Layer-list trackpad contacts opt the hosting `NSView` into indirect touches.
A window-scoped monitor forwards raw gesture-event phases and leaves the
original events available to AppKit. Registrations share the view's touch mask
and restore it when the last owner detaches. Trackpad device extents already use
points; apply the window's backing scale once when forwarding coordinates.

Packed pointer APIs carry explicit device flags independently of predicted and
estimated sample metadata. AppKit captures measured barrel rotation from the
matching device's proximity rotation capability and retains it through the
contact's terminal sample. UIKit retains a positive roll estimate-property bit
from Pencil Down through predictions and delayed corrections. A zero angle keeps
that confirmed capability; numeric angles alone never establish it. UIKit exposes
no general roll-support query, so contacts without that positive evidence and
hover remain unconfirmed. Neither adapter identifies a screenless pen from its
tool kind. Physical rotation and indirect-device checks remain separate from
injected input tests.

### Swift fixtures

The fixtures in `apps/layer-apple/tests` run the production Swift sources on the
Mac without XCTest or a simulator:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
```

`test-project-files.sh` builds the Rust library, compiles `Shared/`, `macOS/`
and `tests/support` with the given fixture (default `tests/project-files.swift`)
and runs it with `CAPY_STORAGE_DIR` inside its own temporary folder, which it
deletes afterwards. Both Apple policies run on the Mac with that storage. Point
`CAPY_TEST_ASSETS_APP` at a built Mac app when a fixture needs the vector or
filter assets. Fixtures that open windows take focus: run them one at a time and
never alongside a UI test batch.

`EditorLaunchTests/testIPadCompatibilityJourneys` runs the existing light/dark
size, color, editor, layer, tab and workspace checks in one XCTest case. Each
named activity launches a fresh private application and terminates it afterwards.
This avoids repeated test-runner handoffs on older iPads while retaining the
individual checks. `testOwnerWorkloadShaderFirstUseBothThemes` similarly groups
the synthetic painting and history checks; it does not measure physical Pencil
input or tier performance.
`testIPadPanelCompatibilityJourneys` runs only the color, layer, tab and workspace
activities when the size dialogs and editor captures are already verified.
Its Color and panel-layout activities can also run separately with
`testIPadColorCompatibilityJourneys` and `testIPadPanelLayoutCompatibilityJourneys`.
`EditorMenuChecks/testPanelGripTapBounds` checks the adjacent Color button edges
and panel-grip tap, hold and double tap in both themes.
`testColorPanelResizeTargets` resizes the Color panel below its natural height
and checks control bounds and button targets in both themes.
`testColorEntryKeepsDraftFocus` pauses while typing a partial hex value, then
finishes the same draft and checks the committed color in both themes.
Color-entry journeys verify the complete typed value before sending Return,
using the existing confirmed-input helper. Fill-thumbnail journeys reopen Layers
after an outside Undo button contact before checking its texture and mask rows;
Cancel and Use Color still independently assert that their parent drawer survives.
New drawing journeys commit typed numbers by leaving the field (Tab on Mac,
keyboard dismissal on iPad) before Create.
Paper import uses the shared file-provider lookup for both complete filenames
and the system's separate name and extension labels.
Text-entry focus checks compare identifiers explicitly so unnamed native file
picker fields retain their identity. Export journeys dismiss filename editing
before activating the native save button.

`tests/canvas-readiness.swift` runs both Apple policies without windows or input.
It checks that a submitted backdrop stays preparing, then pumps the real renderer
until document startup updates the native canvas accessibility value without
another submission. Run it with
`bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/canvas-readiness.swift`.

Native document titles, suggested save filenames and pending close prompts come
from shared document state and requests. Document alerts and export warnings use
shared localized copy; native I/O errors retain their original details.
Missing workspace owners and drawing-window presenters use the active bootstrap
recovery copy, so these native failures follow interface language changes.
`tests/project-files.swift` checks imported photo names, localized save suggestions
and language changes while a close prompt remains pending on both Apple policies.
It also delivers files before initial state publication and before Metal attachment,
with private recovery storage and in memory-only sessions.
Workspace readiness starts recovery once the canvas is ready; recovery completion
retries queued file delivery.

`tests/drawing-tabs.swift` checks independent drawing history, save destinations,
recovery, close cancellation, and closing a tab before opening several files.
Shared Next/Previous drawing requests complete before switching documents, so a
parked request cannot replay on reactivation. The fixture checks repeated cycles,
custom and disabled shortcuts, restored defaults and Hand navigation aliases on
both Apple policies and themes.
It exercises an unreadable session through new checkpoints, Retry, Later and
close while verifying that the failed copy remains unchanged. It also checks the
memory-only owner used when storage locations are unavailable: checkpoint and close callbacks complete
without retrying an absent session store. No private recovery copy is written
in that mode.

An unreadable drawing checkpoint stays indexed while other drawings reopen and
new work can checkpoint. The recovery banner's Later button dismisses its error;
it does not discard the stored drawing. Retry opens repaired copies beside the
current drawings. Superseded pre-release records are preserved without conversion.

`tests/startup-owner.swift` exercises a failed native launch through queued
catalog, surface and frame calls, verifies the original cause and an idle frame
driver, and presses native Restart while artwork recovery is pending. Its
light/dark warning journeys press Retry and Later with a damaged private copy.
Editing gates cover the canvas and editor controls; recovery notices remain
interactive. Dismissing a notice retains its diagnostic and saved bytes.
Renderer, workspace, document progress and recovery notices share one bounded
stack with native scrolling when the notices exceed the window height. Each
diagnostic scrolls within its own height limit, leaving room for its actions.
The native canvas defines the editor size; workspace chrome overlays it so
retained panel geometry cannot enlarge the viewport beyond its window.
The fixture checks action bounds, scrolling and cancellation in both themes,
including a 700 × 500 editor with several simultaneous warnings.
An unreadable settings file reports its original error while shared defaults
remain usable; that read warning does not prevent a successful drawing flush.
Workspace transport failures settle pending lifecycle callbacks independently
of the last published busy state. Retry also repeats controller initialization.

`tests/reorder-lifecycle-publication.swift` mounts, removes and remounts the
actual native input view. Initial attachment has no model cancellation; removal
defers publication until SwiftUI releases its view update, and a new contact
invalidates older pending cleanup. Input interruption still cancels immediately.

The iPad startup stress journeys take private session roots through
`CAPY_UI_DRAWING_STORAGE_LIGHT` and `CAPY_UI_DRAWING_STORAGE_DARK`, seeded with
a painted session from the current native workload harness. The malformed-copy
journeys use separate roots named by `CAPY_UI_MALFORMED_STORAGE_LIGHT` and
`CAPY_UI_MALFORMED_STORAGE_DARK`. Seed only the test app's container, never a
user installation. These opt-in journeys skip when their seed is absent.

Fixtures for a single Swift file compile directly, for example:

```sh
xcrun swiftc -parse-as-library apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o "$TMPDIR/capy-frame-driver" && "$TMPDIR/capy-frame-driver"
```

The iPad canvas admits frames only while its scene is active and its view is in
a window. Scene changes update the presentation gate before waking the driver;
pending completions cannot restart an inactive display link. The frame-driver
fixture checks gate loss during a pending frame and admission after reopening.
The drawing lifecycle stress journeys check backgrounding and resuming on iPad.

Workspace opening and saving notices read the active bootstrap's recovery copy
and refresh with the interface language.

The AppKit `tests/native-menu-titles.swift` fixture pairs with
`macOS/Platform/NativeMenuTitles.swift` and checks translated headings, reordered
menus, unrelated titles, ambiguous roles and retained native actions.

The same pattern pairs `json-lookup`, `reorder-contact`, `estimated-input`,
`frame-trace`, `drawing-workload-plan` and `native-input-gate` with their `Shared/Bridge` sources
(`JSON.swift`, `ReorderContact.swift`, `EstimatedInput.swift`, `FrameTrace.swift`,
`DrawingWorkloadPlan.swift` and `NativeInputGate.swift`).

`reorder-contact` checks selection at native press before release or drag pickup.
`tests/workspace-native-input.swift` checks immediate drawer-tab selection and
retained source identity through native mouse and pen reordering.
`tests/column-stacks.swift` covers inside-edge resizing with mouse and pen in both
themes. `testColumnWidthReset` and `testColumnWidthResetDark` cover native
handle resizing and double-press reset, plus inside-edge reach on macOS.
`tests/workspace-tabs.swift` checks that a late selection publication updates the
retained preview's panel shade while its identity and geometry stay fixed.
It also mounts `WorkspaceTabStrip` over a contrasting backdrop and checks its
upper corners, sides and concave feet in both themes, with opaque and translucent
fills and fractional placement.
Panel hover uses the shared left/right `TabPresentation.hover_bottom` values.
`WorkspaceTabShape` renders their signed radii with the same squircle segments
as the active silhouette; see the [lower corner rules](../ui/squircle-corners.md#radii).

Other runners in `scripts/` and `tests/`:

| Runner | Checks |
| --- | --- |
| `scripts/test-without-gpu.sh` | Direct Swift state/input fixtures, owner/workspace persistence, SwiftUI publication, workspace layout/dragging, appearance and window state, header geometry in both themes, color input and sandbox file grants without attaching Metal surfaces; logs and header captures under `artifacts/release-tests/apple` |
| `scripts/test-persistence.sh` | Atomic settings files, backup exclusion and the settings owner |
| `scripts/test-color-input.sh` | AppKit color text entry in all RGB spaces |
| `scripts/test-project-access.py` | Project writes through a file-only App Sandbox grant |
| `cargo test -p layer-apple tests::session` | Session restart, history, durable removal, preserved unreadable copies, retry beside new work, and new windows adopting unrestored sessions on both policies |
| `cargo test -p layer-apple tests::papers` | Paper import/history, failed loads, preview leases and New drawing visibility; choice positions and preview counts follow the shared paper catalog |
| `tests/background-expiration.py` | iPad background-task lease ordering |
| `scripts/test-native-rows.py` | UIKit row scrolling and contacts on one booted iPad simulator (`--fixture scenes` for per-scene cancellation, `--fixture scroll` for native offset restoration and nested-scroll observation) |
| `tests/test_rust_build.py` | Cargo deployment target, output-directory isolation and custom build-root propagation |
| `scripts/test-workspace-scrolling.py` | Long workspace list on an iPad simulator |

GPU-free workspace fixtures wait for completed controller operations and test
claim release directly. An unattached canvas keeps artwork recovery pending, so
full-editor lifecycle barriers report failure after workspace storage finishes.
Successful full-editor save and close belong in the attached-canvas fixtures.
The coordinator fixture also checks in-memory startup when SQLite cannot open
the original database, which remains unchanged.
The sandbox file fixture checks the resolved file identity and access boundaries;
bookmark staleness is metadata rather than a failed file grant.

Release archives write an Xcode result bundle and `archive.log` under
`dist/apple-<platform>`. Failed release jobs retain these build reports separately
from package artifacts, and the release script reports the failing line and status.

The UIKit fixtures `canvas-modifiers.swift`, `canvas-hover.swift` and
`pencil-estimates.swift` run as a standalone scene app built from the `Shared/`
and `iOS/` sources; they have no runner script. Start their supplied-callback
checks from application launch so a restored scene configuration cannot bypass
fixture entry. They use a standalone Metal surface and do not test OS input
dispatch or physical Pencil sensors.
The dry-media force oracle explicitly selects linear calibration in its private
settings, so supplied zero pressure remains zero at the material input.
Its horizontal dry strokes retain an exact outside-row pixel oracle; shared
contact geometry checks the band because side poses can exceed the writing Size.

### UI tests

Painting journeys use the current Tool Set categories and omit redundant single-category buttons. Filter journeys return to Layers before inspecting or deleting rows. Supplied ordinary pinch contacts last longer than the quick-pinch interval; quick fit has its own checks.

`testPreservedPackageCopyOriginal` and its dark variant open the preserved-package
sheet, use the native Save panel and compare every copied and retained input byte.
They also check the original drawing, layer list, title and exact Undo/Redo pixels.
Copy `crates/layer-core/src/package/codec/fixtures/unsupported-pass-through.capy`
to `Unsupported.capy` in a fresh `Capy Files <UUID>` folder containing only that
file. Set the test target's `.xctestrun` `EnvironmentVariables` entry
`CAPY_FILE_TEST_TOKEN` to that UUID. On Mac, set `CAPY_UI_PACKAGE_DIRECTORY` to
the folder's absolute path. On iPad, put the folder in the private test runner's
Documents directory, enable that runner's file sharing, name its provider
`Capy Test Files <UUID>` and set `CAPY_UI_PACKAGE_PROVIDER` to that display name.

Shared journeys are `check…` functions in `Shared/Tests`, called by `test…`
methods in `Shared/Tests/EditorLaunchTests.swift`, compiled by both UI targets.
Dismiss the iPad keyboard before activating controls it covers or counting rows
in panels that the shared keyboard clearance shrinks, such as a new Selection
Layer's name editor. A keyboard counts only while its frame overlaps the editor
window; the number pad leaves the system keyboard offscreen in the hierarchy.
Poll keyboard and number-pad existence with targeted accessibility queries. Read
a floating number pad's complete hierarchy outside the short waiter before
checking its dismissal region.

Paper choice and Scale retain shared history and the temporary relief preview; the relief does not enter artwork exports.

`testStrengthAndSmoothing` and its dark variant check Strength separately from
Opacity, the ungrouped Smoothing control, per-brush values, Reset and Quick Mask
omission.
On macOS they also paint and check exact Undo and Redo pixels.
`testToolbarComponents` and its dark variant check canonical preview stamps at
zero and full Strength, bookmarks and retirement when the drawing or tool changes.
Slider preview images retain the shared alpha bytes; construct them on the
background utility task.
Accepted stamp images refresh the preview from the current control value and
bookmark state.

`testDryMediaBrushes` and `testDryMediaBrushesDark` select the seven dry presets
and check primary material controls, edits and Reset in both themes. On macOS
they also paint and compare exact displayed pixels through Undo and Redo. The iPad journeys check
controls and capture previews; finger drags navigate rather than paint.
`tests/canvas-modifiers.swift` supplies Pencil pressure and tilt through the
production UIKit callbacks for all three dry poses in both themes, then checks
bounded pigment and exact exported artwork through Undo and Redo. This fixture
uses the device's Metal renderer; supplied contacts do not verify OS dispatch or
physical Pencil sensors.

`testLayerPropertiesDrawer` and `testLayerPropertiesDrawerDark` exercise the
Layers header drawer, growth, compact Properties, All layers, retained list scroll
and Group properties in the native editor. They require an Apple SDK and a rendering-capable device.
`tests/layer-row-input.swift` drives native Lock, Duplicate and Delete swipe
actions with Undo, then checks authored Paper drag cancellation and the protected
Quick Mask row. AppKit workspace scrollers overlay their indicators so system
scrollbar preferences and native reconfiguration preserve the shared content
width. Both hosts show permitted panel scrollbars for actual mouse input and
clear them for touch or pen without moving the viewport. Layer bodies consume
the shared pickup devices: mouse drags immediately; touch and pen holds keep
their context menu and use the handle to reorder. Displaced rows keep their
handles hidden until the swipe returns, and an existing contact cannot gain
pickup during that return.
`tests/layer-row-lifecycle.swift` checks offscreen sources, removal, renaming,
remounting and document replacement. Rejected shared drop previews retire without
dispatching an edit. Accepted feedback can name an adjacent row after hierarchy
normalization; the fixture checks that hint, final order and one-step Undo/Redo.
Palette grids measure their allocated content width before sizing tiles,
including the narrower Color and Palettes tool drawer.
Panel measurement caches retain known row sizes while panels remain configured but hidden; removing a panel configuration clears its cached measurements.
AppKit chrome and pen-scroll hit tests convert window points into the target's
superview coordinates, including flipped and offset containers.
`tests/header-native-input.swift` mounts the editor to check ordinary and Zen
Capy press ownership, activation, stable bounds and focus-loss cleanup, as well
as title-bar customization. The About/application-menu and title-bar drawer
journeys run in both themes; the iPad menu journey moves the pointer onto a closed
heading before clicking, then checks free hover and held-pointer motion between
open headings. Use element hover for free motion and recorded coordinates for
held-pointer motion; retain the closed-menu and resulting command assertions.
Layer pickup consumes the shared effective lock, including inherited group locks.
A locked row retains context input; unlocking cannot promote its existing capture.
Locking an admitted contact retires it and any drag preview before another move.
The same layer-row fixture checks body and handle contacts for every input device.
Platform-only journeys extend that class in `iOS/Tests` or `macOS/Tests`.
The iPad `testOwnerWorkloadShaderFirstUse` pair checks five tools while synthetic
native-owner samples arrive. After the producer stops, Clear Entire Layer
establishes a visible edit; exact Undo restores the painted crop and Redo restores
the cleared crop.
The iPad `testNativeDrawingLifecycleStress` and dark variant open seeded private
Release sessions with saved theme and workspace settings. They check native
navigation, exact stroke pixels and Undo/Redo through ten background/foreground
cycles and a cold relaunch. Set `CAPY_UI_DRAWING_STORAGE_LIGHT` and
`CAPY_UI_DRAWING_STORAGE_DARK` to the corresponding owned
`capy-test-drawing-*` directories; Release ignores Debug initial actions.

On iPad, menu leaves may use command identifiers or action-label identifiers.
Resolve the exposed leaf and reveal it in its containing scroll view before
activation; offscreen existence does not establish native input delivery. History
helpers locate the active workspace's toolbar controls rather than assuming a
particular toolbar ID; retain the enabled-state and artwork Undo/Redo checks.

On macOS, scope dialog-button captions to their window or matching sheet.
Application-wide queries include Touch Bar copies that XCTest cannot click;
window queries may include toolbar buttons with the same caption. Keep the
visible button and resulting document-state assertions. Native menu commands wait
for enabled state before activation; an asynchronous New or Open can dismiss its
form while the drawing still loads and document commands remain disabled.

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

`testAlphaConversionFilters` and `testAlphaConversionFiltersDark` exercise
Threshold's color and transparency choices, hidden alpha-threshold retention,
Brightness to Opacity, exact history and drawing recovery through native controls.
The compact Properties header keeps Add Filter at its natural width; the layer
type takes the remaining space, including when the panel needs a scrollbar.
`testFilterArtworkAndHistory` and `testFilterSearchPreviewAndProperties`, with
their Dark variants, check filter artwork, history, search and native controls.
Convert catalog text sizes from points with the shared 4/3 factor before
allocating native text. Panel choices, including configuration layer selection,
use the shared panel control height and input fill. Ordinary select labels clip;
resource, proof and layer-blend labels ellipsize. Label weight and disabled
appearance follow the corresponding control role. The layer-blend menu centers its
label and uses the shared twelve-point chevron with six-point trailing padding.
The compact workspace menu uses a bold label without a disclosure glyph. Its text follows the Web header’s narrow-width sizing.
Ordinary selects combine six-point authored padding with the reference browser's inner margins
of four points leading and twelve trailing, giving text insets of ten and eighteen points.
Canvas status chips align to the bottom of the shared status rectangle, with
a four-point horizontal inset and twelve-point gaps between chips.
`testLiveColorTab` and its Dark variant check the initial paint, retained Color
tab icon updates and the transparency circle. Run these on both native targets;
simulator compilation does not establish Metal canvas behavior.

`testImageObjects` and `testImageObjectsDark` exercise Convert to Object Layer,
ordinary Object layer rows, duplication, ordering and visibility,
and the painting refusal's Rasterize Layer action. Undo and Redo check both the
rows and exact displayed pixels. The commands use native toolbar buttons and
row menus. `tests/image-import-owner.swift` covers placement, external paste,
batch import, cancellation and file-worker handoffs under both Apple policies.

`testVectorLayers` and `testVectorLayersDark` press the Layers footer's New
vector layer button with Pencil chosen and check the selected Vector row, the
switch to G-Pen, the Eraser's three path modes at the top of the Tools list,
reselecting the vector row after its paint layer below, and the Merge Down
notice whose action turns the paths into pixels. Vector rows are found by their
shared default name, because the content icon is hidden from accessibility.
On macOS, mouse strokes also check presented pixels: the Pencil
notice's Use G-Pen and New Paint Layer actions, Fill inside an ellipse outline
and outside it on a new paint layer below, Erase up to intersection between two
crossings, and the merged line parts. The iPad journeys skip canvas input.

Layer-row icon buttons are as wide as the shared panel control height, including
the mask link between thumbnails. Rows, the header, the footer and the drawer's
disclosure share a six-point side inset (`layerEdgeInset`), and drawers and the
Layers bubble show rows edge to edge, so every first icon lines up with the eye. Keep the hit target independent of its smaller glyph. Mask-transform
journeys use the inline link control and check independent and linked transforms,
cancellation and exact Undo/Redo pixels in both themes.

- **Menu routing.** Test the app's menu bindings with native fixtures or manual
  journeys. For editor-effect automation, use `CAPY_INITIAL_ACTIONS` or in-app
  controls rather than coordinate-testing the Mac system menu bar.
- **Pencil.** XCTest cannot synthesize Pencil. Finger drags navigate unless the
  session claims them for transform handles/boxes or Warp nodes. Create artwork
  and selections through menus; drawing acceptance still needs physical Pencil.
- **Text entry.** XCTest Escape does not reach iPad apps; use visible controls.
  Scroll fields above the software keyboard, replace text without relying on
  triple-tap selection, and tap a picker's button rather than its static label.
- **Accessible geometry.** iOS container frames are the union of their children;
  do not reuse macOS width expectations. Open editor menus hide their buttons
  from queries: close using the recorded button frame and reveal scrolled rows.
  The Color wheel's square accessibility frame makes XCTest report the swatches,
  shape buttons and readout around its ring as not hittable, although touches
  reach them; tap their visible points with `colorControlPoint`.
- **Window corners.** In iPadOS 26 windowed scenes, including a window that
  fills the screen, the system resize handle owns touches in the window's
  bottom-trailing corner. Start touch drags elsewhere; `testColumnStacks` drags
  the leading column's grip onto the trailing column.
- **Two-finger input.** Whole-canvas XCTest pinches and multi-finger taps can
  place a finger on the header or panels. Run each one in `withHiddenChrome`,
  which enters Zen through the visible Capy control, then return to read the
  camera and check artwork history. Diagnose delivery from actual UIKit
  touch locations in the scene's orientation. Supplied pinch contacts must move
  beyond native tap slop; smaller movements can correctly restore the starting
  view and run the configured multi-finger tap action.
- **Stable targets.** Place strokes relative to `editorPaper(in:)`, select by
  label rather than index, reveal controls, and wait for published state. Check
  changed shared rules before repairing a failed journey; keep its intended assertions.
- **Window state.** macOS retains window sizes even with `-ApplePersistenceIgnoreState`.
  Use full screen for a wider work area; restore resizing or full screen in
  teardown so later journeys start consistently.
- **Exact history.** Compare Undo/Redo pixels exactly after the stroke settles;
  the renderer redraws rounded pages at pen-up. Do not add display tolerances.
- **Retries.** Gather evidence and a specific hypothesis before repeating an
  inconclusive run. Prefer existing Swift fixtures and Rust tests to new automation.
- `editorTestApplication()` gives each journey's application a new
  `CAPY_STORAGE_DIR` name, kept across its relaunches, so journeys never touch
  the installation's data. Do not create an `XCUIApplication` for the editor
  any other way.
- The iOS simulator lacks Float32 filtering, so its canvas exercises the
  [texel-load fallback](../internals/rendering.md#optional-float32-features)
  that A12–A16 iPads use. Hardware filtering, Pencil and timing need a
  physical iPad.
- On a physical iPad, keep the device awake and unlocked and turn on
  **Settings → Developer → Enable UI Automation**; Developer Mode alone does not
  enable the UI runner.
- A timeout initiating the test-manager control session before any test starts
  is an inconclusive harness run. Inspect the result bundle's `testmanagerd.log`;
  if the iPad still shows Automation Running after cancellation, stop that
  session on the iPad before starting a fresh run.
- To test apps already installed on the iPad, set `UseDestinationArtifacts` in
  the `.xctestrun` target with `TestHostBundleIdentifier`,
  `UITargetAppBundleIdentifier` and `TestBundleDestinationRelativePath`. Omit
  `TestHostPath`, `TestBundlePath`, `UITargetAppPath` and
  `DependentProductPaths`, which can make XCTest try to install an unavailable
  bundle during `app.launch()`.
- Resolve any device-trust or automation and capture permission prompt before
  retrying a failed run. A physical iPad runner that times out while enabling
  automation is an inconclusive harness run. Check screen state and the
  test-manager authorization/startup logs before retrying. If the same timeout
  recurs while the device is awake and unlocked, stop repeating runner launches.
  Launch your own build through `devicectl` and use native component fixtures
  or Rust tests for independent checks. Record touch journeys as unverified;
  component input does not establish touch or Pencil delivery.

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
The inventory visits every catalog tool command and brush preset, then follows
the rendered choices; flattened or singleton tool groups cannot hide a preset.
It restores editing memory before each tool path and compares all
authored filter values, including values hidden by conditional controls. Projected
control readbacks retain their published types; Undo, Redo and rejected locked
or disabled edits also compare the complete authored effect and spatial mapping.
Each property trial restores its starting values after checking Reset, preserving
the availability of later controls. Property schema comparisons exclude curve
and gradient interaction epochs while retaining
their action targets. Run its regression checks with
`cargo test --locked -p layer-host --example inventory`.
Release validation also generates a CPU inventory and checks its property
evidence; the full command audit still needs the hardware tool paths above.

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

The Mac frame scheduler reads window visibility on the main actor before
admitting presentation. Hidden-window wakes remain paused; visibility changes
redraw and resume the retained renderer. Metal acquisition must not query AppKit
from the render queue. Check minimize/restore with retained pixels and history,
and inspect Main Thread Checker output when changing this boundary.

Metal surface configuration runs inside a balanced Core Animation transaction
with implicit actions disabled. View-backed layers configure and commit on the
main queue; standalone layers configure on their calling owner without a run
loop. The layer lock is released before dispatch and before transaction commit,
including error returns and Rust unwinding. Rendering, pipeline creation and
drawable acquisition stay on the render owner. `tests/metal-layer-threading.swift`
checks the actual setters' thread and transaction scope on view-backed and
standalone layers, including resized reattachment, without opening a window.
`tests/managed-display.swift` retains its standalone background-setter, rendered
pixel, display-tag, resize, layer-replacement and unchanged saved-artwork checks
for both Apple policies. Retain `CA_DEBUG_TRANSACTIONS=1` diagnostics when checking
this boundary, and measure affected resize motion in Release.

The iPad canvas mounts a standalone `ObservedMetalLayer` beneath its ordinary
UIView backing layer. UIKit owns the backing layer; the serial render owner
configures the Metal layer, including drawable size, color space and drawable
timeout. Main-actor layout changes only its frame and contents scale without
animation. Check mounted device presentation and UIKit thread diagnostics when
changing this ownership boundary.
