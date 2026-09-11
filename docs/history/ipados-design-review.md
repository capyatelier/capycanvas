# iPadOS design review

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

**Scope amendment:** the implementation now requires native **iPadOS and macOS
at every milestone**, sharing the Rust behavior/rendering, Apple bridge and
Swift editor components. [The Apple goal and acceptance tracker](apple-acceptance.md)
is the current objective and evidence matrix. The review below records the
original source analysis; its iPad-first development sequence and tool/device
availability observations are historical, not the current acceptance scope.
The agreed Mac adaptation uses the OS menu bar for top-level menus and moves
Zen right of the native window controls; the canvas remains behind the header.

Reviewed September 10, 2026 against commit `e5669eb`. This is a source/design
review and proposed development sequence. No iPad app, device build, or physical
latency measurement was produced during this review. Android measurements below
are the repository's recorded results, not measurements repeated on this Mac.

The existing architecture is a good foundation for a native iPad app. Reuse
`layer-ui`, `layer-engine`, `layer-core`, and `layer-render-wgpu`; add a SwiftUI
control layer, a UIKit Pencil input view, and Metal surface integration. Android
provides the most useful host example because it already separates native UI
from the engine/render owner. Its presentation and event handling need deliberate
Apple-specific adaptations.

**Repository structure for multiple platforms**

Use one Apple app family under `apps/layer-apple`, with separate iOS/iPadOS and
macOS application targets and shared Apple libraries. This replaces the earlier
proposal to start with an isolated `apps/layer-ios/native` bridge. macOS should
share the editor and Metal integration from the beginning, while retaining its
own AppKit input, windowing and lifecycle code.

Proposed source layout; these directories are created as implementation needs
them, not as empty placeholder projects:

```text
crates/
  layer-core/                 # Document and edits, every platform
  layer-engine/               # Input interpretation and brush dynamics
  layer-render/               # Renderer contract
  layer-render-wgpu/          # Shared pixel engine, all GPU backends
  layer-ui/                   # Actions, layout, catalog and semantic state
  layer-host/                 # Native session facade shared with Android
  layer-ffi/                  # Platform-neutral C canvas ABI
apps/
  layer-apple/
    CapyCanvas.xcodeproj/     # Separate iPadOS and macOS app/test targets
    Shared/
      Bridge/                # Swift wrappers, transport and buffer ownership
      Editor/                # Shared Apple editor views and styling
      Settings/              # Reusable native settings components
    iOS/
      App/                   # iPadOS entry point and scenes
      Input/                 # UIKit/Pencil events and gestures
      Canvas/                # UIKit Metal view and display integration
      Platform/              # iPad file services and lifecycle
      Tests/
    macOS/
      App/                   # macOS entry point, menus and windows
      Input/                 # AppKit tablet, mouse and keyboard adapters
      Canvas/                # AppKit Metal view and display integration
      Platform/              # Mac file services and lifecycle
      Tests/
    native/
      Cargo.toml             # Apple host static library for both targets
      include/               # Hand-maintained Apple bridge C interface
      src/                   # Apple transport and Metal surface ownership
    scripts/                 # Cargo/Xcode builds, packaging and test launchers
  layer-android/
  layer-linux/
  layer-web/
  layer-windows/             # Future Windows host
assets/                      # Canonical cross-platform artwork/filter assets
tools/visual/                # Shared scenarios and screenshot comparison tools
artifacts/ui/parity/          # Ignored generated captures, overlays and diffs
```

Rust application policy stays in `crates/`, including additions required by
more than one host. `apps/layer-apple/native` contains only Apple integration;
it must not become a second engine or a home for general settings/layout rules.
Keep the Swift editor components independent of UIKit/AppKit view types and
inject the platform canvas/input/services. Platform-specific settings navigation
can wrap shared settings components. Use explicit target membership/modules to
isolate UIKit and AppKit code rather than scattering platform conditionals
through every shared view.

The iOS directory names the SDK/API family and initially contains the iPadOS app;
the macOS target is a native AppKit host. Shared code and tests can be exercised
by both targets at every milestone. Generated static libraries, XCFrameworks,
derived build data, generated binding output and signing files stay outside
tracked source. Package products go under ignored `dist/` and Rust products
under `target/`.

Some currently shared icons and brush previews reside under `apps/layer-web`.
When consolidating those assets, move their canonical source to `assets/` and
update all existing consumers together. Build steps may stage copies into app
bundles; each platform must not maintain its own edited source copy. Keep
platform launchers and UI tests close to their hosts, and put cross-platform
visual fixtures/comparison logic in `tools/visual`. Existing hosts do not need a
wholesale directory migration to introduce the Apple family.

**Visual direction agreed for iPadOS**

The main editor should preserve the existing desktop/web visual design: shared
toolbar and panel geometry, icons, brush previews, palette, typography roles,
docking, document title, HUD and Zen behavior. Settings may use a native iPad
appearance and navigation while retaining Rust-owned options, validation,
search and persistence semantics.

The Metal canvas fills the entire app content view, including the area behind
the app's title/header bar. Transparent native header and HUD containers sit
above that same live surface. Pan and zoom therefore move the actual artwork
behind the title and controls; this does not require a screenshot, a copied
header texture, or a second canvas render. Keep the shared work area for Fit
Canvas separate from the full-window GPU viewport. Opening panels and toggling
Zen must not resize the canvas or shift its camera.

Style the editor's native views explicitly to match the existing app. The
main header should retain its transparent background; native defaults must not
introduce an opaque navigation strip or an unrequested glass/blur treatment.
Settings can use standard iPad lists, switches, pickers and adaptive list/detail
navigation. Its appearance need not reproduce Android's settings overlay.

System-owned window controls, status indicators and window corners remain under
iPadOS control. Position interactive header items using the platform's safe-area
and window-control-aware layout guides, while allowing the canvas background to
extend underneath. This permits close editor parity with the small layout
adjustments needed for system controls and varying window sizes. Apple describes
these custom-bar layout accommodations in
[Make your UIKit app more flexible](https://developer.apple.com/videos/play/wwdc2025/282/).

Add visual acceptance checks for artwork continuing behind the header while
panning/zooming, unchanged camera across Zen/panel changes, light/dark parity,
correct hit testing through transparent space, and header clearance in fullscreen
and windowed modes. Rendering remains on the dedicated owner; native UI
composition must not turn pen movement into whole-workspace state updates.

**How visual parity will be checked during implementation**

Use the web editor at the same source revision as the visual reference for each
implemented editor component. Reuse shared assets, palette, catalog and layout
values; do not independently transcribe design constants into Swift.

The existing [Chrome harness](../../apps/layer-web/test.mjs) and
[GTK/web parity checks](../../apps/layer-web/parity.mjs) already set viewport size
and device scale, capture PNGs, check control geometry and sample rendered colors.
The main launcher currently selects Linux-specific graphics flags; a maintained
macOS launch configuration is needed when extending that suite for iPad.

A separate local smoke check succeeded on this Mac with Chrome 153 and its Apple
Metal hardware adapter: a 1200×900 CSS viewport at scale 2 produced 2400×1800
screenshots in both themes, with the app reporting GPU-ready and no JavaScript
exceptions. Ignored local evidence is in `artifacts/ui/ipados-parity/`, including
`chrome-capture.mjs`, `chrome-capture.json`, and the two `web-*.png` images.
This verifies local browser capture; an iPad comparison awaits the native host.

For each comparison, match the native app content bounds, drawable scale,
document, camera, workspace, selected tool, theme and UI state. Capture browser
content without Chrome's frame and capture the native app without Simulator's
device bezel. Handle native system-control regions explicitly; preserve the
app's title-bar area in the comparison. Use a shared viewport/inset fixture where
system layout requires offsets, rather than moving screenshots until they match.
Record dimensions, scale, insets, source revision and browser/OS versions.
Wait for assets, fonts, GPU work and animations to settle; normalize captures to
the same color space and control hover, focus and cursor state.

Produce side-by-side images, an alignment overlay, a per-pixel absolute-difference
heatmap, and regional mismatch statistics. Compare physical pixels directly
without resizing the images. Use strict checks for geometry, flat colors and
shared assets, with small explicit tolerances for rasterized edges, text and
shadows. Existing cross-toolkit geometry tests permit one logical pixel for
rounding; tighter checks should be used where allocations can match exactly.
Keep the raw diff visible so tolerances cannot hide missing controls or wrong
spacing. Native font rendering and GPU arithmetic mean whole-screen byte identity
is not a sensible cross-platform acceptance requirement.

Run this check as the header, ribbons, panels, docking and Zen are implemented,
with light/dark, landscape/portrait and resized-window fixtures. Include painted
and zoomed artwork behind the header, selected/disabled controls, popup placement,
floating panels and changed layer state. Fix unintended differences before
accepting each component; changing a baseline must be an explicit design decision.
Settings retains native visual freedom and receives behavior, accessibility and
native snapshot checks. Review native device captures as well as simulator output.

**Tools to install now**

The inspected Mac is Apple silicon running macOS 26.1. Rust and Apple's Command
Line Tools are installed; full Xcode and Rust's iOS targets were absent.

| Item | Action |
| --- | --- |
| Full Xcode | On the current macOS version, use Xcode 26.3 from [Apple Developer Downloads](https://developer.apple.com/download/all/). Apple's current table lists Xcode 26.6 as requiring macOS 26.2 or later; update macOS first to use that release. |
| iOS platform support | Open Xcode, complete first-launch setup, and install the iOS platform support and simulator runtime. The iOS SDK/runtime covers iPadOS. |
| Rust device target | `rustup target add aarch64-apple-ios` |
| Rust simulator target | `rustup target add aarch64-apple-ios-sim` for this Apple-silicon Mac. |
| Device signing | Sign into an Apple Account in Xcode, pair the iPad, trust the Mac, and enable Developer Mode on the device. A free account supports personal-device development. |
| Profiling | Use Xcode's Instruments, Metal GPU capture, and memory tools. Include Time Profiler, Allocations, and Metal System Trace in the device-validation workflow. |

After installing Xcode in `/Applications/Xcode.app`, select and check it:

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
xcodebuild -version
xcrun --sdk iphoneos --show-sdk-path
xcrun simctl list devices available
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

Choose an Xcode release whose device support covers the test iPad's OS. We do not
need Android Studio, CocoaPods, Flutter, or a separate Swift installation for the
proposed host. A Cargo build script and Xcode's own tools can build the static
libraries and package a device/simulator XCFramework. Paid program enrollment can
wait until distribution needs it.

Sources: [Xcode compatibility](https://developer.apple.com/xcode/system-requirements),
[Rust iOS targets](https://doc.rust-lang.org/rustc/platform-support/apple-ios.html),
[Apple account setup](https://developer.apple.com/help/account/basics/about-your-developer-account),
[membership capabilities](https://developer.apple.com/support/compare-memberships/),
[Developer Mode](https://developer.apple.com/documentation/xcode/enabling-developer-mode-on-a-device),
[Xcode tools](https://developer.apple.com/documentation/xcode).

**What transfers from Android**

| Current Android implementation | Proposed iPad equivalent |
| --- | --- |
| Compose controls driven by Rust catalog/actions/snapshots | SwiftUI controls driven by the same semantic state; UIKit where input or native editing needs it. |
| SurfaceView behind transparent controls | A stable UIKit view backed by CAMetalLayer, with native controls above it. |
| One HandlerThread/Looper owns UiSession, device, queue and presentation | One serial engine/render owner; UIKit and SwiftUI remain on the main thread. |
| JNI numeric pen batches, pooled arrays and reusable Rust scratch | A small C ABI carrying fixed numeric records in owned, reusable buffers. |
| Choreographer now/presentation timestamps | Display-link timing converted to the same monotonic clock as Pencil samples. |
| Revision-gated JSON UI snapshots | Changed-state snapshots decoded off the input path and published to SwiftUI only when needed. |
| Surface detach preserves the document/session | Scene/surface lifecycle separate from session lifetime, with explicit device recovery. |

The [architecture](../architecture.md), [shared UI design](../ui/shared-ui.md), and
[platform adapters](../platforms/README.md) already specify these boundaries.
[`Platform::Ios`](../../crates/layer-ui/src/settings.rs) already exists, including
Apple shortcut behavior. The renderer accepts a platform-selected device and
queue through
[`WgpuRasterizer::from_wgpu`](../../crates/layer-render-wgpu/src/lib.rs), so the host
can share the same Metal device for painting and presentation.

The Android model is concrete in
[`CanvasHost.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt),
[`CanvasSurfaceView.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasSurfaceView.kt),
and [`android.rs`](../../apps/layer-android/native/src/android.rs). Preserve the
absence of a full-canvas CPU pixel round trip in the pen-to-screen path. Auxiliary
thumbnail, export, and region-history transfers are separate operations with
explicit ownership and scheduling.

**Performance evidence and its limits**

The [Android implementation report](android-implementation.md#emulator-measurements)
records these results after allocation cleanup, using a 2560×1600 Android
emulator in 120 Hz mode and 128 px brushes:

| Brush | Full CPU callback p95 / p99 | Recorded composited fps range |
| --- | --- | --- |
| G-Pen | 11.41 / 18.08 ms | 105.7–111.4 |
| Natural Blender | 14.33 / 21.42 ms | 76.7–97.0 |
| Watercolor Wash | 20.46 / 28.84 ms | 62.0–71.5 |

These numbers do not establish a 120 Hz product. SurfaceFlinger fps covers each
run's last 127 valid presentations, while callback distributions combine all
measured frames across four runs per brush. Input processing p95 was only
0.015–0.016 ms. Reducing allocations cut publication/rescheduling work, but did
not reliably improve frame tails. For iPad, profile drawable acquisition,
render work, and presentation before spending time optimizing a small FFI call.

The [offscreen benchmark](../development/gpu-raster-benchmarks.md) and
[optimization log](optimization-log.md) establish useful renderer headroom on the
recorded workstation. They exclude surface acquisition, compositor scheduling,
scanout, startup, and pipeline warm-up. Their 8.33 ms gate and simulated tip-gap
results cannot establish iPad input-to-photon latency. Measure first-stroke and
first-use filter latency separately from warmed steady-state performance.

**Gaps to resolve before copying the host**

1. **The C ABI is not yet a complete app bridge.**
   [`LayerCanvas`](../../crates/layer-ffi/src/lib.rs) owns
   `CanvasEngine<WgpuRasterizer>`, while Android owns `UiSession<Renderer>`.
   The existing header supplies useful record/status conventions and a staticlib
   build, but does not expose the complete UI session, host requests, or Metal
   surface attachment. Add an Apple-shared `apps/layer-apple/native` staticlib with an
   opaque session handle, batched input, actions/snapshots, lifecycle operations,
   and explicit buffer ownership. Preserve panic/error containment across C.
   Build device and simulator slices separately; both are arm64 on this Mac but
   target different platforms. Set consistent deployment targets in Rust and Xcode.

2. **Pencil estimates need a correction mechanism.**
   [`PenEvent`](../../crates/layer-engine/src/input.rs) carries sequence, pressure,
   tilt, prediction flags and camera revision, but has no estimation-update ID or
   operation to amend an earlier sample. The
   [feedback design](../reference/instant-stroke-feedback.md) acknowledges this future work.
   UIKit can later correct force, altitude and azimuth; use
   `estimationUpdateIndex` and `touchesEstimatedPropertiesUpdated` to identify
   those updates. Do not append a correction as another movement or mislabel it
   as prediction. Design stable sample IDs and updates to the provisional tail,
   with an explicit policy for corrections arriving after finalization or pen-up.
   Destination-aware brushes require consistent replay of their brush state.
   [Apple Pencil input](https://developer.apple.com/documentation/uikit/handling-input-from-apple-pencil)
   and [estimated properties](https://developer.apple.com/documentation/uikit/uitouch/estimatedpropertiesexpectingupdates)
   describe this separate lifecycle.

3. **Capture time and backlog behavior need stronger contracts.**
   Android's nine-number batch does not carry a camera revision;
   [`App::pointer`](../../apps/layer-android/native/src/app.rs) assigns the revision
   when the render worker processes it. A delayed sample can therefore receive a
   newer transform. Capture viewport scale and the displayed camera revision
   with the batch, and order resize/action/input messages explicitly.
   Also, Android's eight-entry buffer pool bounds reuse, not outstanding work:
   `pointerBuffer` allocates when exhausted and `worker.post` can accumulate
   messages. Use measured queue limits and queue-age telemetry. Preserve real
   samples and stroke boundaries; replace obsolete predictions/hover work first,
   and define explicit overflow behavior rather than silently losing pen-up.

4. **Metal needs its own presentation policy.**
   Android prefers Mailbox, falling back to Fifo, with maximum frame latency two.
   The pinned wgpu 30.0.1 Metal backend advertises Fifo on iOS and maps latency
   hints to drawable count. Start with the supported Fifo path and measure
   one-versus-two-frame latency hints on device. Acquire drawables on the render
   owner so acquisition cannot block Pencil delivery. Create/size the UIKit view
   and layer on the main thread, and give the native renderer a retained layer
   with explicit teardown ordering. The pinned wgpu source provides
   `SurfaceTargetUnsafe::CoreAnimationLayer` under its Metal configuration.

   For the first integration, use CADisplayLink timing with the existing wgpu
   surface acquisition path; allow at most one outstanding frame request and
   pause when idle. Pass the actual target timestamp, not a hard-coded 8.33 ms
   offset. CAMetalDisplayLink is worth evaluating next, but it supplies a
   drawable: using that drawable requires compatible wgpu/Metal integration,
   not a second independent call to acquire another drawable. Frame-rate
   preferences are requests subject to system policy.
   [CADisplayLink](https://developer.apple.com/documentation/quartzcore/cadisplaylink),
   [CAMetalDisplayLink](https://developer.apple.com/documentation/quartzcore/cametaldisplaylink),
   [ProMotion guidance](https://developer.apple.com/documentation/quartzcore/optimizing-iphone-and-ipad-apps-to-support-promotion-displays).

5. **Sparse layers still need an iPad memory budget.**
   The [renderer](../../crates/layer-render-wgpu/src/lib.rs) uses 256×256 paint pages,
   but also allocates a full-document RGBA8 composite. Calculated pixel storage
   for one 4096×4096 RGBA8 image is 64 MiB; 32 fully painted layers alone require
   2 GiB before previews, destination companions, wet state, filters, imported
   images, undo records and drawables. Empty layers are cheap; the benchmark's
   layer count is not a guarantee for densely painted documents. Include the new
   [connected-region scratch resources](../../crates/layer-render-wgpu/src/region_requests.rs)
   in stress workloads. There is no complete host memory-pressure policy in the
   Android example. Establish device-dependent limits, cache reclamation and
   allocation admission checks, using process memory as well as GPU counters.
   Memory warnings are best effort, so preventive limits matter.
   [Apple memory guidance](https://developer.apple.com/documentation/xcode/responding-to-low-memory-warnings).

6. **Surface survival is not document persistence.**
   Android retains the session across surface replacement and persists settings
   and workspace; its documented drawing document remains in memory. iPad scene
   deactivation, suspension and process termination require an autosave/recovery
   design. Stop presentation when inactive, resolve the active contact exactly
   once, and preserve session/document ownership independently of the view.
   Do not transplant Android's synchronous surface-destruction wait onto UIKit's
   main thread. Test resize, orientation, window resizing and recovery while
   input is queued. Add persistence before treating the prototype as a place to
   keep artwork.

**Pencil and native UI acceptance**

Collect chronological coalesced touches immediately, snapshot their numeric
values, and route predicted touches only to the shared replaceable preview.
Use precise view coordinates converted to drawable pixels, normalize supported
force, and map altitude/azimuth into the shared tilt convention. Gate hover,
barrel roll, squeeze and double-tap behavior on actual device/OS support. Keep
finger navigation and palm rejection separate from Pencil contact. Avoid
stacking an additional Swift smoothing/prediction algorithm on the Rust model.

The recent [web pen fix](../../apps/layer-web/app.js) illustrates why terminal input
needs platform-specific tests: normal lift, cancellation, focus/scene loss and
hover exit are distinct events. Do not map every UIKit cancellation to commit
or blindly copy Android's cancel-on-hover-exit rule. Specify which interruptions
preserve accepted ink, which reject a contact, and verify lift never removes a
completed stroke or includes prediction in document history.

The [Android visual audit](android-ui-audit.md) also offers a practical lesson:
porting semantics did not automatically preserve layout. It took explicit checks
to correct oversized controls, default theme styling, popup placement, drag
coordinates and text-editing resets. Start iPad screenshot/geometry tests with
shared toolbar sizes, palette, assets and docking rules. Use native iPad focus,
accessibility, menus and text editing; adapt settings list/detail navigation to
available width. Keep the canvas view stable as SwiftUI updates controls. A
settings input shield must not intercept its own sliders and gestures.

**Suggested development order**

1. Build the shared crates for device and simulator; link a minimal Swift host
   through the app C ABI. Prove one Metal canvas, Pencil down/move/up, undo/redo,
   and background/foreground recovery before filling out the workspace.
2. Implement the complete input contract, including coalescing, corrections,
   prediction, camera revisions, cancellation, palm rejection and bounded
   transport. Add physical-Pencil event traces alongside deterministic tests.
3. Instrument the worker and presentation, run real-device brush tests, and set
   memory limits. Begin autosave/recovery in this stage.
4. Translate panels/settings/customization from shared state, using Android's
   behavior and visual audit as acceptance references.
5. Validate the full renderer and workflow on the oldest supported iPad and a
   ProMotion iPad, including sustained drawing, multitasking and memory pressure.

For performance, record input arrival, queue age, CPU preparation, acquisition,
GPU completion, and actual presentation separately; aggregate p50/p95/p99,
maximums and missed deadlines across all repetitions. A 120 Hz frame interval is
8.33 ms and a 60 Hz interval is 16.67 ms. Keep those frame budgets separate from
measured input-to-present latency. Retain the repository's ambitious latency
objective as an unproven acceptance target until device measurements exist.
Exercise G-Pen, erase, Natural Blender, Watercolor Wash, liquify, large brushes,
4K multilayer documents, prediction on/off, and pen-up. Include a sustained
session to expose thermal changes and idle checks to verify the render loop sleeps.

Metal's [drawable presentation callback](https://developer.apple.com/documentation/metal/mtldrawable/addpresentedhandler(_:))
and [presentedTime](https://developer.apple.com/documentation/metal/mtldrawable/presentedtime)
can support actual presentation instrumentation, subject to the native drawable
access available in the selected integration. Hardware input-to-photon checks
still require physical observation. Apple's
[profiling guidance](https://developer.apple.com/documentation/xcode/improving-your-app-s-performance)
and [Metal Simulator guidance](https://developer.apple.com/documentation/metal/developing-metal-apps-that-run-in-simulator)
support using the simulator for functional coverage and devices for performance.

The test iPad, iPadOS version and Pencil model remain to be specified. Those
choices should determine the minimum deployment target, optional Pencil features,
Xcode/device compatibility, memory budget and expected refresh-rate range.
