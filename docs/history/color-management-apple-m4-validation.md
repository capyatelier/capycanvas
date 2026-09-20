# Apple color management through HDR

Implementation and validation, 2026-09-19. Parent: `c1472101`.

The shared Apple UI now exposes HDR creation and precision conversion, the
nonmodal Off/SDR/Print Proof panel, HDR Edit Color and intensity controls, HDR
histogram and curve axes, and portable BT.2020 PQ PNG delivery. The picker’s palette popup
was removed; existing saved palette data remains readable by the shared model.

## Ownership and rendering

- Shared Rust owns proof-mode selection and reveal, dial geometry/hit testing,
  numeric validation, rendition edits and one-contact history, picker coordinates
  and display mapping, histogram ranges, export normalization and range checks.
- AppKit/UIKit own pointer capture, focus, native menus, file pickers, display
  observations and presentation. Touch/Pencil input claims only the actual dial
  or intensity arc; these are direct-manipulation controls, not reorder targets.
- HDR documents use an RGBA16Float Metal surface tagged extended linear sRGB
  with EDR enabled. SDR documents retain the existing managed Display P3 path.
  Canvas, navigator and print presentation use the shared renderer. Current
  display headroom is observed on the main thread, including while drawing is
  idle; foreground HDR checks reuse the GPU-health timer. SDR retains its
  existing one-second health polling interval.
- One cancellable Rust snapshot worker builds the full-image local-tone guide.
  Artwork/device/document changes retire stale analysis. Camera and proof recipe
  edits reuse the guide. The picker retains its Float32 base field across EV and
  headroom changes. Its native bitmap worker has one running job and one
  replaceable pending request; cancelled views cannot publish stale pixels.
  The proof glass texture is generated once off the UI
  thread at the same 512-square resolution used by GTK.
- Export captures an immutable master and runs its CMM/preview/write work away
  from the drawing owner. HDR preflight runs before the save picker and the
  writer checks range again. Clipping requires an explicit output option.
  Export never bakes a display preview into the master.

The port exposed an existing shared validation gap: fills, gradients and figures
still accepted only 0–1 RGB. Their validation now follows the document depth;
F16 permits finite half-float RGB, while alpha, masks and integer documents keep
their bounds.

## Recorded checks

Raw logs, native component captures, device results and performance traces are
under ignored `artifacts/apple-color-management/` and the matching
`apps/layer-apple/DerivedData/ColorHDR*` directories. Tests use owned windows,
separate bundle identities and temporary persistence, without replacing artist
documents or preferences.

- Shared core/UI: 94 + 481 tests passed, including the added HDR operation,
  transported picker, invalid EV draft and histogram-axis checks.
- Apple Rust bridge: 73 tests passed; the optional external 61 MP photo fixture
  was skipped. Release builds passed for macOS, physical iPad and iPad simulator.
- Metal HDR owner test: both Apple policies retain above-white paint through
  exact save/reopen, renderer replacement, proof undo/cancel, SDR output and
  HDR PNG output/reimport. Sixty proof updates reused the exact analysis guide;
  the focused debug run measured approximately 13 ms total per platform.
- Native control fixture: RGBA16Float/EDR surface metadata; Float32 picker image
  reuse; actual AppKit pointer capture; cross-control drag isolation; Escape;
  arrow keys; undo/redo; invalid EV persistence. Light/dark proof captures cover
  128, 160, 226 and 400 logical pixels. A first-mount pixel-variation assertion
  verifies that the asynchronous illustration replaces its gray placeholder.
  A burst of 60 picker requests produces the exact final shared bitmap, and
  cancellation prevents a retired result from reappearing.
- Live print-proof coordinator: both Apple platform policies passed RGB/CMYK
  profile preparation, cancellation/retry, ICC preservation, history, save/reopen
  and scene pause/resume with the retained panel.
- Mac XCTest passed the actual HDR journey: precision conversion, Edit Color EV,
  Base/Adjusted swatches, proof drag, undo/redo/reset, Print/Off and PQ preview.
  The final `mac-color-commit.xcresult` passed both HDR and SDR journeys, including
  reopening Edit Color to verify 2 EV, exact preset names and dimensions, retained
  alpha, saved defaults and palette-popup removal.
  Screenshot review found and fixed a first-open Canvas invalidation bug; native
  accessibility now retains the dial, mode and Reset actions. The export-range
  binding no longer publishes during AppKit's control update. Xcode still reports
  an internal QoS warning during the color-editor interaction; no cause is
  attributed without a usable stack.

The physical M4 iPad (iPadOS 27.0) also passed the complete HDR XCTest journey,
including reopening Edit Color to verify 2 EV, in 110.31 seconds. The passing HDR
test is retained in `ipad-color-commit.xcresult`. The separate SDR journey passed
in `ipad-sdr-unmark.xcresult`, covering creation, preset/default persistence,
alpha retention and palette-popup removal. The dial, Edit Color, Print, export
and SDR captures were visually reviewed.

The earlier automation-initialization timeout was resolved when the user enabled
**Settings → Developer → Enable UI Automation**. The enabled-device run exposed
a test locator that tapped a picker’s static label instead of its native button;
both Apple color journeys now use the UIKit button query. SDR text entry now
scrolls fields above the keyboard, checks the target field’s keyboard focus, and
replaces existing text without assuming triple-tap selects it.

The full SDR journey also found a UIKit input bug: Create could save `Studio P3`
as `Studio P` while the last character remained marked text. Merely disabling
autocorrection or ending focus did not fix it. Create now explicitly commits
marked text, ends editing, and reads the bound form on the next main-queue turn.
The native test verifies the exact saved name by reopening its preset menu.
This UIKit form-commit fix is the only production change after the drawing
performance samples below; the measured rendering, picker and Rust code is
unchanged. Final Release builds and source/binary manifests include the fix.
The simulator build remains compilation coverage; unavailable Float32 texture
filtering prevents GPU qualification here.

## Release performance samples

The parent is an isolated build of `c1472101`, not a historical measurement.
Each case uses 10 seconds of warmup, 30 measured seconds and the workload's
postlude. The Mac display was queried at 90 Hz; the M4 iPad records 120 Hz.
Builds and UI automation were idle. Both machines ran their own workloads;
GPU timestamp instrumentation was disabled. Raw traces, launch PIDs, environment,
binary hashes and source hashes are retained with the artifacts.

| Device / SDR workload | CPU owner p95, parent → port (ms) | CPU owner p99 (ms) | Presentation p99 (ms) | Peak footprint (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Mac / sRGB U8 ink | 2.839 → 2.784 | 8.200 → 7.828 | 22.222 → 22.222 | 436.3 → 433.8 |
| iPad / sRGB U8 ink | 1.951 → 1.956 | 5.710 → 3.780 | 8.334 → 8.334 | 495.2 → 492.4 |
| Mac / ProPhoto U16, eight 4K layers | 3.714 → 3.574 | 4.367 → 4.771 | 11.111 → 11.111 | 1345.3 → 1321.4 |
| iPad / ProPhoto U16, eight 4K layers | 3.040 → 2.994 | 3.559 → 3.586 | 8.334 → 8.334 | 1358.8 → 1359.6 |

These samples show comparable SDR performance, with some tail variation. All
ten initial runs completed with zero rejected input and missing presentation
callbacks, and normal thermal state. They are short regression samples, not
the full ten-minute workload matrix or a physical input-latency qualification.

A separate optimized 400-pixel picker measurement found 17.31 ms for initial
HDR base generation, 14.63 ms per SDR-mapped update and 1.99 ms per EDR-mapped
update. That measurement led to moving HDR bitmap generation off the UI thread;
Rust color mapping is unchanged. The existing synchronous SDR field path is
unchanged (1.40 ms per complete field in the same microbenchmark).

The final worker implementation passed all three Release builds without compiler
warnings, the native component fixture, and the complete Mac UI journey. The
UI history helper now waits for Rust's asynchronous Undo/Redo publication.
The affected ink workloads were repeated on this exact build:

| Device / final workload | CPU owner p95 / p99 (ms) | Presentation p99 (ms) | Peak footprint (MiB) |
| --- | ---: | ---: | ---: |
| Mac / sRGB U8 | 2.779 / 3.305 | 22.222 | 420.1 |
| iPad / sRGB U8 | 2.070 / 2.487 | 8.334 | 491.1 |
| Mac / Display P3 F16 | 2.782 / 8.501 | 22.222 | 626.2 |
| iPad / Display P3 F16 | 1.982 / 2.418 | 8.334 | 647.8 |

All four repeats also completed with zero rejected input and missing callbacks,
and normal thermal state. Mac HDR presentation p99 varied from 11.111 ms in the
initial run to 22.222 ms in the repeat; these samples do not certify every frame
meeting 90 Hz. The iPad HDR p99 remained 8.334 ms. Parent and final binary/source
manifests are separate; the parent benchmark app was removed after capture.

## Qualification boundaries

The dial uses the shared GTK texture, geometry, gradients and icon vocabulary.
Text uses the Apple app’s established font rendering and menus use native Apple
controls. The web HDR panel had not landed in this checkout during initial
implementation; shared source agreement and Apple captures do not establish an
imperceptible cross-platform image difference.

Portable PQ PNG is the available HDR output route. Gain-map JPEG/AVIF entries
remain hidden when the shared codec reports unavailable; this work does not
claim qualification of Apple gain-map codecs, arbitrary HDR HEIF input, physical
Pencil sensor behavior, or calibrated display luminance. Synthetic drawing
traces measure host regressions, not physical input-to-photon latency.
