# Web and Android milestone 2 integration

Work in progress after the merged GTK checkpoint. This record does **not**
qualify either port as complete. Apple and Windows remain unported.

## Native color controls checkpoint — 2026-09-15

Android now requests Float32 filtering/blending and constructs native SDR
renderers for startup, project replacement and recovery. Renderer forwarding
reports the actual document representation and tiled-source capability. The
shared numeric color form powers Android and browser effect/gradient editors;
changing input models without editing retains the exact tagged color. Gradient
previews sample the same encoded-document interpolation used by the shader.

Photo project construction is shared with GTK. Android Open detects native
archives versus JPEG/PNG/TIFF, retains original source samples/profile/depth, and
clears the save location for an imported photograph. The missing-profile Ask
flow, host color settings, richer delivery dialogs and full browser native
storage are still pending.

Validation logs are under `artifacts/color-m2/web-android/`:

- `shared-color-form.log`: shared round-trip and invalid-draft test passed.
- `package-color-controls.log`: browser packaging tests passed.
- `web-contract-check.log`: actual Wasm target checked.
- `android-photo-check.log`: actual Android ARM64 target checked with cargo-ndk.
- `android-photo-apks.log`: isolated debug app and instrumentation APK built.
- `tablet-native-sdr-controls.log`: `AndroidHostTest#nativeSdrTaggedColorsAndGradientEditor`
  passed on physical Wacom DTHA140 (`5ll21u1002931`), Android 15, Adreno 735. It
  paints, undoes/redoes, edits a Display P3 gradient stop, switches the numeric
  model and confirms the untouched gradient definition is unchanged.

Install/test commands (SDK root `$ANDROID_HOME`):

```sh
CARGO_NET_OFFLINE=true apps/layer-android/gradlew -p apps/layer-android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.colorm2
$ANDROID_HOME/platform-tools/adb -s 5ll21u1002931 install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
$ANDROID_HOME/platform-tools/adb -s 5ll21u1002931 install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
$ANDROID_HOME/platform-tools/adb -s 5ll21u1002931 shell am instrument -w -e class art.capycanvas.AndroidHostTest#nativeSdrTaggedColorsAndGradientEditor art.capycanvas.colorm2.test/androidx.test.runner.AndroidJUnitRunner
```

The existing application was preserved; testing uses `art.capycanvas.colorm2`.
The requested 9504×6336 JPEG is in `/sdcard/Download/sony_a7r_v_29.jpg`. Host and
tablet SHA-256 match:
`3aac9c9b8b34c38a5e0121f16ad1ec806e92a19e15ee5e1128f36d987e888054`.

No tablet photo-navigation performance or browser precision claim is made at
this checkpoint. Final qualification requires actual 120 Hz navigation, native
save/reopen and profiled exports in both hosts, followed by the user's test.

## Native SDR storage and photo round-trip checkpoint — 2026-09-15

WebGPU now uses the same native U8/U16 tile backing, Float32 working storage,
source decoder and immutable capture queue as GTK and Android. Browser captures
map asynchronously and transfer bounded chunks to the raster worker for exact
compression. Source transport retains original channels, depth, profiles, image
resolution and shared tiles. The browser requires Float32 filtering and blending;
the attached tablet Chrome exposes both features. See the platform feature
introductions for [filtering](https://developer.chrome.com/blog/new-in-webgpu-119)
and [blending](https://developer.chrome.com/blog/new-in-webgpu-132).

Both hosts expose independent new-document space/depth/background choices,
named presets/defaults, shared numeric color editing, and palette management.
Picker fields, title-bar colors, saved swatches and effect controls derive their
sRGB presentation from tagged document colors instead of treating RGB numbers
as untagged CSS or Android colors.

Physical tablet validation:

- `tablet-web-native-raster.log`: exact raster save/reopen, undo/redo, invalid-file
  retention, GPU replacement and worker/IndexedDB recovery passed.
- `tablet-web-native-sdr.log`: Display P3 U16 creation/painting, unchanged numeric
  color model switches, exact tagged palette storage, embedded ProPhoto U16 PNG
  import and original-source native save/reopen passed. The source index and blob
  descriptors/digests are identical after reopening and saving.
- `tablet-native-raster.log`: all three `AndroidRasterTest` tests plus the tagged
  color/gradient test passed (4 tests). Includes exact ProPhoto U16 backing after
  drawing, save/reopen and forced GPU replacement.
- `shared-renderer-native-material.log`: five hardware-GPU native material and
  masked snapshot regression tests passed. The first sandboxed attempt had no
  hardware adapter; rerunning with GPU access passed all five.
- `web-picker-build.log`, `android-sdr-fixture-apks.log`, and
  `package-picker-palettes.log`: Wasm, ARM64 app/instrumentation and browser asset
  packaging builds/checks passed.

Reproduce the browser workflow with `checkRaster` in `raster.test.mjs` and
`checkSdrColor` in `color-m2.test.mjs`, using Chrome DevTools Protocol connected
to a task-owned tablet tab. The color test's PNG is generated by:

```sh
cargo run -p layer-color --release --offline --example photo_sources -- generate 513 257 artifacts/color-m2/web-android/prophoto16.png
```

Serve that file at the test's `photoUrl`. Disable the browser cache and reload
before qualification so the JS and Wasm belong to the same build. CDP input
coordinates must convert the physical canvas viewport to CSS pixels on this
scaled tablet. Tests now recognize the actual version-4 raster archive header.

Profile-assumption prompts, profiled output, inspection, document color edits,
source repair/placement, color preferences and final large-photo navigation
qualification remain pending. This is an intermediate integration checkpoint.

## Streaming profiled delivery and input policy checkpoint — 2026-09-15

The obsolete browser PNG readback/worker path and Android PNG-only export job
are replaced. Both ports offer PNG/TIFF/JPEG, builtin or imported ICC output
profiles, independent integer depth, alpha/matte choices, supported rendering
intents, optional 8-bit dither, proportional resizing and resolution metadata.
The editable project location is protected against accidental export overwrite.
BPC remains unavailable in the current shared CMM, as recorded in the main
integration assessment; the new dialogs do not expose an ineffective toggle.

Native export captures an immutable project on the owner and streams bounded
Float32 bands through the file worker. WebGPU maps one bounded band at a time,
then sends it to a worker-owned temporary OPFS file. The browser file worker
feeds synchronous bounded rows into the same shared CMM/resampler/codecs and
writes encoded output directly to OPFS. It does not retain a full Float32 frame
or encoded output in Wasm memory. Temporary output lifetimes are explicit; Web
Locks protect active jobs from cleanup after another worker/tab crashed. Exact
identity delivery bypasses composition and preserves original hidden RGB, sample
codes and embedded profile bytes. The 4 GiB temporary-file ceiling is a failure
bound, not a qualified memory or performance claim.

Preferences → Color now configures new-document defaults, photo editing depth
and untagged-image policy on both ports. Ask pauses preparation before adoption;
Cancel retains the live master. Builtin or custom ICC assumptions retain source
sample tiles and validate the decoder before adoption. Tagged inputs do not ask.

Validation:

- `tablet-web-profiled-export.log`: actual tablet WebGPU/DOM flow passed. PNG and
  TIFF output/reopen retain every original 16-bit source tile and ICC profile;
  P3 8-bit resized PNG reopens at 257×129 with resolution metadata; sRGB JPEG is
  produced from a ProPhoto16 master without altering it. The actual Ask dialog's
  Cancel retains the epoch; Adobe RGB assumption retains all original samples.
- `tablet-native-profiled-export.log`: all three Android raster integration tests
  passed with the new streaming exporter, including 16-bit PNG/TIFF identity and
  the existing save, undo, device/surface replacement and recovery checks.
- `tablet-native-profile-assumption.log`: the expanded 16-bit Android test passed,
  including a private preparation pause and explicit Adobe RGB assumption with
  identical original sample tiles.
- `shared-output-streaming-regressions.log`: all 13 hardware snapshot tests passed,
  including full-resolution composition, masked regions, exact hidden-RGB/gray
  identity, profile/matte/resize, dither, output preview and cancellation.
- `color-settings-policy.log`: future-document policy round-trip/validation passed.
- `web-delivery-final-check.log`, `android-profile-prompt-build.log`,
  `android-profile-prompt-tests-build.log`, `package-profiled-export.log`: Wasm
  checks, ARM64 app/test builds and browser asset packaging checks passed.

A test-only reload was blocked by Chrome's native confirmation. The task tab was
replaced; the user's existing tabs were retained. Subsequent qualification
suppresses that test tab's `beforeunload` handlers before an explicit uncached
reload. This was test transport/UI handling, not an observed renderer deadlock.

Still pending: host delivery comparison previews, user export presets and active
job cancellation/progress; profile-library persistence; Assign/Convert/depth and
source repair/rasterization/placement; document properties and full-resolution
histogram controls; final display, correctness and large-photo 120 Hz navigation
qualification. The apps are installed for integration testing, but are not yet
handed over as completed milestone-2 builds.

## Inspection and cancellable delivery checkpoint — 2026-09-15

Both ports expose the shared point/3×3/5×5 sampler and a nonmodal histogram
window. Inspection captures the complete committed composition at full
resolution, excludes zero-alpha pixels and display overlays, and labels RGB as
profile-encoded document coordinates and luminance as linear Y. RGB/luminance,
log scale, endpoint and out-of-range counts are available. One cancellable job
runs at a time; settled document revisions trigger replacement, stale results
are labeled, and animated effects show the captured time explicitly. Closing
the inspector or replacing its document cancels the private capture.

Export now has visible progress and cancellation before destination publication.
Cancellation handles are separate from worker-owned tasks; closing UI cannot
mutably alias or free a running job. Browser temporary output is retired after
publication, with cleanup failure reported separately from a successful export.

Physical tablet validation:

- `tablet-web-inspection-final.log`: complete SDR round trips and profiled output,
  exact histogram totals/transparent exclusion, actual nonmodal window, canceled
  histogram, and canceled export with no published file passed. The runner waits
  for `performance.timeOrigin` to change after reload, then startup completion;
  polling only the old page's startup state races navigation.
- `sdr-sample-area.log`: shared sampler stale-result cancellation and preservation
  of document/brush opacity passed for GTK, Web and Android.
- `web-inspection-final-build.log`, `android-inspection-final-build.log`,
  `shared-inspection-check.log`, and `package-inspection.log`: actual Wasm/ARM64
  builds, shared host check and browser packaging passed.

Remaining work still includes document color edits and comparisons, richer
source operations, named delivery presets/profile management and final tablet
memory/navigation qualification. No 120 Hz tablet claim is made yet.

`tablet-native-inspection-final.log`: all four `AndroidRasterTest` tests passed
on the physical tablet (37.405 s), including exact native SDR save/reopen and GPU
replacement, profiled PNG/TIFF identity, source interpretation, full histogram
counts/UI and canceled histogram/export with the master unchanged.

## Document color edits checkpoint — 2026-09-16

Web and Android now offer Assign Profile, Convert Color Space, and Change Bit
Depth, including full-composition Before/After comparisons. Preparation owns a
private converted project and fully rendered candidate on the existing GPU.
Apply checks the request, document revision/epoch and GPU generation before
publishing the shared color/history transaction. Failed preparation and Cancel
leave the master intact; undo/redo prepares the exact retained roots in the
previous/next mode instead of reconstructing strokes. Tool colors retain their
portable definitions across the change.

The shared `ColorCanvas` preparation shares immutable source cache ownership.
Browser conversion transfers only backing that can change: retained original
photographs stay shared in the owner and are reattached to the worker result.
Browser comparison consumes full-resolution Float32 bands into a bounded area
preview, numerically checked against the existing full-row resampler. This is
presentation reduction after composition; filters still receive native pixels.
Private browser snapshots/candidates now inherit the raster worker connection,
which is required when comparing/exporting an image with committed paint. The
private project shader catalog is explicitly completed after submission.

Validation:

- `tablet-native-color-edits.log`: physical Android test passed (14.664 s).
  Assignment retains every stored code; conversion changes coordinates; U16→U8
  changes native descriptors; undo/redo restores exact backing; save/reopen,
  canceled worker, and actual comparison UI/Cancel preserve the master.
- `tablet-web-color-edits.log`: complete prior SDR/input-policy/export/inspection
  suite plus actual Assign/Convert/depth dialogs, full-image comparisons,
  cancellation during preparation and after comparison, exact undo/redo and
  save/reopen passed. Retained source samples/profile remain unchanged. Includes
  histogram and PNG export of a photo with committed paint, exercising the
  private snapshot's browser raster-worker connection.
- `shared-conversion-regressions.log`: all eight conversion tests passed,
  including every SDR mode's lossless assignment, f64-reference conversion
  within one code, exact alpha, masks/wetness, cancellation and memory admission.
- `shared-color-transition-hosts.log`: exact shared color/history publication and
  picker/brush coordinate behavior passed for GTK, Web and Android.
- `color-preview-reduction.log`: asynchronous area preview matches the existing
  full-row resampler, including nonintegral ratios, alpha and extended RGB.
- `shared-color-preview-capture.log`: hardware-GPU profiled composite/preview,
  budget and cancellation regression passed.
- `web-color-edit-build.log`, `android-color-edit-tests-build.log`,
  `gtk-color-edit-check.log`, `package-color-edit.log`: actual Wasm/ARM64 builds,
  GTK check and browser packaging passed.

The browser workflow must wait for both GPU startup and
`JSON.parse(layerApp.app.workspace_view()).ready && !JSON.parse(layerApp.app.workspace_view()).busy`.
Workspace restoration temporarily blocks actions independently of GPU readiness.
Old pre-migration test-origin workspaces with untagged color arrays are explicitly
unavailable under the new schema; final deployment should use a fresh test origin.
No backward-compatibility migration is promised.

Still pending: flattened conversion copies, source Place/Paste/repair/rasterize,
document source details, persistent named export presets/profile library, output
comparison previews, broader adjustment/effect integration qualification, and the
final tablet memory/120 Hz navigation measurements. This checkpoint is not final
qualification or a user-test handoff.

## Retained placement and document details — 2026-09-16

Web and Android Place/Paste now use the same profiled PNG/JPEG/TIFF decoder and
source-retention policy as Open. They add a layer to the current master, keeping
its working space, editing depth, epoch and save location. Preparation captures
the selected target, document revision and GPU generation; stale adoption fails.
Missing-profile choices use the existing cancellable interpretation dialog.
Both layer-panel import buttons now use this path; their old browser Canvas2D /
Android Bitmap decoders, which reduced every import to RGBA8, have been removed.
The raw RGBA8 import APIs remain for synthetic renderer test fixtures only.

Document Properties inspects a small immutable metadata message on the file
worker. It reports canvas extent, document space/depth, resolution metadata and
each retained source's original channels, depth and profile, including explicit
assumptions. Inspecting properties does not transfer image tiles to a worker.

Validation on the physical tablet:

- `tablet-native-source-import.log`: Android import, exact source sample/profile
  preservation inside a P3 U8 master, undo/redo, reopen, actual properties dialog
  and real Android URI clipboard paste passed (8.148 s).
- `tablet-web-source-import-retry.log`: browser layer-panel import, undo/redo,
  reopen, properties and real Chromium custom-format clipboard paste passed.
  Every ProPhoto U16 source tile and ICC stayed identical inside a P3 U8 master.
  Chromium was visible but its page initially lacked focus; a real pointer click
  in the title bar fixed the clipboard test transport. The initial denial was
  explicit, and left the document intact. Clipboard permissions were granted
  only to the isolated localhost test origin through DevTools.
- `android-source-import-build.log`, `android-source-import-tests-build.log`,
  `web-source-import-build.log`: actual ARM64/Wasm builds passed.

Browser paste retains the bytes the browser exposes. Chromium's `web image/png`,
`web image/tiff` and `web image/jpeg` custom types are preferred over ordinary
PNG/JPEG, which may already have been sanitized by the clipboard producer or
browser. Original file import is the reliable interchange path with applications
that do not expose richer clipboard types. This follows Chromium's
[custom clipboard format contract](https://developer.chrome.com/blog/web-custom-formats-for-the-async-clipboard-api).
Android reads the clipboard's image URI directly without Bitmap decoding.

Still pending: source repair/rasterization, flattened conversion copies,
persistent named export presets/profile library, output comparisons, broader
adjustment/effect qualification and final tablet memory/navigation measurements.

## Retained-source corrections — 2026-09-16

Both tablet hosts now expose Repair Source Profile and Rasterize Source in the
shared commands and layer menu. Repair validates the selected builtin/custom ICC
against the actual source channels on the worker, retaining every original
sample. Rasterization converts at full source extent to the document's space and
integer depth. Both prepare the same complete-stack edit that Apply publishes,
with Before/After comparisons, explicit clipping/baked-edit explanations,
independent cancellation and stale document/request/GPU checks.

The existing shared edit code preserves paint, masks, adjustments and position.
Repair of a layer with baked pixel edits adds a corrected original separately;
the painted layer is untouched. One-step undo restores the exact prior source.
Web repair transfers only interpretation metadata to the CMM worker. Web
rasterization transfers only the selected source; the rest of the master stays
shared with its owner. Android conversion and comparisons run on IO. The host
comparison UI reuses the document-color job lifecycle and cancellation handling.

Validation:

- `tablet-native-source-edits.log`: physical Android extended placement/source
  test passed (14.388 s), including repair/rasterize comparisons, cancellation
  before conversion and after preview, exact undo/redo, baked-paint preservation,
  saved/reopened corrected sources and actual comparison UI cancellation.
- `tablet-web-source-edits.log` and `tablet-web-source-edits-retry.log`: the prior
  SDR/color/import suite passed; after fixing a quoted selector in the test,
  actual source dialogs passed repair/rasterize comparisons, exact undo/redo,
  preparation/preview cancellation, baked-paint preservation and save/reopen.
- `shared-source-repair-hosts.log`, `shared-source-rasterize-hosts.log`: shared
  tests now run for GTK, Web and Android, including source-sample identity,
  full off-canvas extent, paint/mask preservation and exact history.
- `android-source-edit-build.log`, `android-source-edit-tests-build.log`,
  `web-source-edit-build.log`: actual ARM64/Wasm builds passed.
- `package-source-edit.log`: all 13 browser packaging checks passed, including
  the new source comparison's rewritten ICC-picker module dependency.

Remaining: flattened conversion copies, output comparison previews, persistent
named export presets/profile library, broader correction/effect and display
qualification, and the final tablet navigation/memory benchmark and handoff.

## Output comparisons — 2026-09-16

Export now offers Artwork/Output comparisons on Web and Android. The output
preview consumes the actual encoded integer rows after output resizing, profile,
depth, matte and dither, interprets them using the actual encoder profile, then
reduces for the sRGB presentation. JPEG compression artifacts are explicitly
excluded in the dialog. The shared encoded-row preview consumer is also used by
GTK's existing snapshot preview, eliminating a separate tablet approximation.
The editable master is never changed. Preview dismissal drains its private job;
worker cancellation and temporary-file cleanup use the existing capture control.
Browser output comparison reuses the bounded Float32/OPFS output transport and
removes its temporary capture after the comparison, without producing an image
file. Android uses an immutable inspection snapshot on IO.

Validation:

- `tablet-native-output-preview.log`: real Android histogram/output-preview
  integration passed (11.046 s), including exact white output at the resized
  128×96 extent, cancelled preview, actual export comparison UI and unchanged
  document epoch; existing histogram and export-cancellation checks also passed.
- `tablet-web-output-preview.log`: full SDR/source-policy/export/inspection
  workflow passed with the actual resized 257×129 output comparison, explicit
  JPEG-preview limitation, cancellation while preparing, and exact PNG/TIFF
  U16 source/profile output unchanged.
- `shared-output-preview-serial.log`: all 13 GPU snapshot regressions passed
  with `cargo test -p layer-render-wgpu snapshot::tests:: -- --nocapture --test-threads=1`
  (60.91 s), including preview agreement with delivered samples, profile/matte/
  resize/dither, exact identity and hidden RGB, capture budgets and cancellation.
- `android-output-preview-build.log`, `android-output-preview-tests-build.log`,
  `web-output-preview-build.log`, `gtk-output-preview-check.log`,
  `package-output-preview.log`: ARM64/Wasm builds, GTK check and all 13 production
  browser packaging tests passed.

The initial concurrent GPU test run crashed with SIGSEGV. Its core dump identifies
`libvulkan.so.1` in `loader_get_icd_and_device` →
`terminator_SetDebugUtilsObjectNameEXT` → wgpu-hal command-buffer object naming.
The precise cause of that native loader failure is unknown; it is not reported
as a fixed application defect or a color assertion failure. The complete same
suite passed sequentially. Evidence is retained in
`shared-output-preview-regressions.log` and
`shared-output-preview-parallel-crash.log`; subsequent workstation GPU validation
uses the documented sequential invocation. Tablet Vulkan and Chrome WebGPU
integration runs did not exhibit this crash.

Remaining feature work: persistent named export presets/profile library and
flattened conversion copies, followed by broader workflow/display checks and the
final tablet navigation/memory benchmark and user-test handoff.

## Persistent delivery presets — 2026-09-16

Export offers Web / Share, Wide-color image, Further editing and Custom plus
named user presets. Save/Update/Delete and Reset Destination use the shared
bounded `ExportPresets` model. A successful delivery remembers its choices for
that destination; temporary edits to a named preset remember Custom, while the
named preset changes only through Update. Failed delivery does not remember it.
Reopening restores the complete recipe, including size, DPI and JPEG quality.

Android publishes preferences with `AtomicFile` on IO under an application
mutex. Web uses a strict IndexedDB transaction and an origin-wide Web Lock on
the file worker. Source documents and workspace snapshots do not own the library.
List operations transfer names only, without expanding all embedded ICC profiles
into UI state. Selected/saved recipes validate actual ICC channels and encoder
support before publication. Persistence errors after a successful image save are
reported as preference failures, without marking the image save as failed.

Validation:

- `tablet-native-export-presets.log`: persisted CRUD, every delivery choice,
  remembered Custom/reset and actual export-dialog reload passed (4.363 s).
  Instrumentation uses a separate preference directory for each test.
- `tablet-web-export-presets.log`, `tablet-web-export-presets-retry.log`: prior
  SDR/output-preview checks passed; after normalizing a BigInt in the DevTools
  transport, actual dialog Save/Update/Delete, reopen with all choices restored,
  successful delivery remembering Custom, and Reset Destination passed.
- `shared-export-presets.log`: all three preset tests passed, including profile
  interning/retirement, atomic rejection of invalid/oversized changes, protocol
  validation before mutation and metadata-only listing.
- `android-export-presets-tests-build.log`, `web-export-presets-build.log`,
  `gtk-export-presets-check.log`, `package-export-presets.log`: ARM64/Wasm builds,
  GTK check and all 13 Web packaging tests passed.

Remaining feature work: reusable ICC profile-library management and flattened
conversion copies. Broader correction/effect/display qualification and the final
tablet navigation/memory benchmark still precede the user-test handoff.
