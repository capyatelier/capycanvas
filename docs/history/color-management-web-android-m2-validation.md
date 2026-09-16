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

## Reusable ICC profile library — 2026-09-16

Export and source-profile controls offer saved profiles as well as file import;
Preferences → Color exposes library management. Imports store exact app-owned ICC
copies under SHA-256 identities, deduplicate repeated imports, enforce 128-entry /
64 MiB aggregate and 16 MiB per-profile limits, and verify stored bytes before use.
Unavailable/corrupt entries show an explicit issue and can be removed or reimported.
Deleting an entry affects neither original files nor profiles embedded in documents
or export presets. Android uses atomic private files; Web uses a dedicated
IndexedDB object store with strict transactions and an origin-wide lock. Listing
returns profile metadata, and all ICC parsing is on the file worker. Web's former
synchronous owner-side ICC inspection was removed.

Validation:

- `tablet-native-profile-library.log`: exact bytes/dedup, storage corruption
  detection and reimport, independent preset ownership, real export profile
  picker and Preferences library removal passed (6.206 s).
- `tablet-web-profile-library.log`: prior SDR/output-preview/preset workflows plus
  exact ICC bytes/dedup, corruption rejection/repair, independent preset
  ownership, saved-profile picker and Preferences management passed.
- The Web test found that the first worker handoff detached the caller's input
  buffer. The public API now transfers a private copy, and repeated import of
  the same caller-owned bytes passes. This was fixed before the passing run.
- `android-profile-library-build.log`, `android-profile-library-tests-build.log`,
  `web-profile-library-build.log`, `package-profile-library.log`: ARM64/Wasm builds
  and all 13 production Web packaging tests passed.

Flattened conversion copies remain the final feature gap. Broader workflows and
viewing agreement, then the fresh tablet navigation/memory benchmark, still need
qualification before final deployment and user testing.

## Flattened color-conversion copies — 2026-09-16

Convert Color Space now offers Editable layers or Save flattened copy on Web and
Android. Both compare the complete native-resolution composition before saving.
A copy contains one rasterized layer in the destination space at the original
integer precision, canvas extent and physical resolution. The original master,
its save location, dirty checkpoint, layers and history are unchanged. The copy
requires a separate `.capy` destination and rejects the known master handle/URI.
Android encodes into a private temporary file before opening the destination;
Web prepares native output on the file worker. Cancellation drains preparation
and publishes no image. Publication disables cancellation once writing starts.

The native row-to-project builder is shared by GTK, Android and Web, replacing
GTK's duplicate copy builder. This also preserves physical resolution in GTK
copies, which the previous builder omitted. Web reuses bounded full-resolution
output bands rather than allocating a full Float32 export frame.

Validation:

- `shared-flatten-copy.log`: the masked/effect composition matches native GPU
  rendering within one U16 code; full extent, 300 PPI, exact copy save/reopen and
  cancellation passed (3.03 s).
- `tablet-native-flatten-copy.log`: existing exact color-history/depth checks,
  full-copy preview/write/reopen, rejected adoption over the master, unchanged
  epoch/revision/location/dirty state and actual copy-dialog cancellation passed
  (13.568 s).
- `tablet-web-flatten-copy.log`: the SDR/output-preview, color/history/depth and
  copy workflows passed, including actual copy-picker cancellation, unchanged
  master checkpoints, full extent/precision and exact native copy reopen.
- `android-flatten-copy-build.log`, `android-flatten-copy-tests-build.log`,
  `web-flatten-copy-build.log`, `gtk-flatten-copy-check.log`,
  `package-flatten-copy.log`: ARM64/Wasm builds, GTK check and all 13 Web packaging
  tests passed.

Remaining: broader correction/effect/display qualification, followed by fresh
matching tablet navigation/memory measurements and final deployment for user
confirmation. No tablet 120 Hz performance claim is made at this checkpoint.

## Correction and recovery qualification — 2026-09-16

The six SDR photo controls were checked through ProPhoto U16 source import,
retained masks, native save/reopen and repeated edits/reset. The full-resolution
histogram changes with each correction and returns exactly to its prior result
when the control is restored; original source/profile bytes remain intact.

- `tablet-web-photo-corrections-retry2.log`: all six corrections and mask
  inversion/undo passed. Earlier attempts found test-expression mistakes and an
  identity-export harness assumption about remembered resize settings; the
  harness now explicitly selects Original size and no dither.
- `tablet-native-final-workflows.log`: diagnostics and wide-U16 GPU replacement
  passed; broader recovery and correction reopening found two real host defects.
  Embedded effects were waiting for validation while the native shader compiler
  was still unstarted. Startup now precedes the combined validation/readiness
  wait. An action rejected while the GPU was failed could also leave a stale
  error after successful restart. Errors associated with that failure now retire
  when the canvas is restored; unrelated action errors remain visible.
- `tablet-native-final-workflows-retry.log`: both affected workflows pass
  (25.636 s combined), including exact snapshots, active-contact save,
  undo/redo, GPU replacement, corrupt-file retention, stale-candidate rejection,
  Activity recreation and production recovery offer/adoption.
- `shared-final-photo-effects.log`: nine shared GPU correction tests pass
  (104.59 s), covering both integer depths/all four spaces, low-alpha and extended
  RGB, long fused/physical chains, analytic curves, gradients, masks and exact
  save/reopen. `shared-final-display-color.log`: four GPU display tests pass
  (44.24 s), including canvas/export/navigator/thumbnail/sample color coordinates
  and explicit SDR surface conversions without changing stored pixels.
- `shared-final-color-codecs.log`: 60 shared color/codec tests pass, four optional
  external-fixture tests skipped in that invocation. The separate external run
  passes installed working profiles and RGB/gray/CMYK/YCCK/progressive/oriented
  JPEG references. The first CMYK reference invocation selected raw ink bytes
  instead of the ICC fixture; `shared-final-cmyk-reference.log` passes with the
  recorded `/usr/share/color/icc/krita/cmyk.icm` profile and independent LittleCMS
  reference from `artifacts/color-m2/final-performance/portable-icc-reference`.

Web and Android currently use the explicit sRGB SDR viewing fallback. Their
native U8/U16 master and profiled delivery retain wide-gamut data. These checks
do not claim a measured physical panel color match or native wide-gamut surface
presentation on either host.


## Full-size browser photo admission — 2026-09-16

`tablet-web-large-open-before.log` confirms that the 9504×6336 Sony JPEG was
rejected by the codec's unmeasured-host fallback, before rendering. The Web file
worker now uses the bounded, capacity-based allowance documented in
[portable-color.md](../development/portable-color.md), preserving the smaller
fallback when the browser supplies no capacity hint. This deliberately does not
claim device capacity is free memory. Codec work remains serialized; compression
uses its independent worker. File jobs have a 180 s watchdog, while interactive
compression retains 30 s. Only needed buffers are allocated.

The first trial exposed 32-bit Wasm saturation when converting an 8 GiB hint to
`usize` before division; the policy now divides in `u64`. In
`tablet-web-large-open-after2.log`, the original 61 MP file opens as native SDR8
in 5.51 s with the exact full extent and no import error. The file's hash and
copied tablet path remain those recorded above. This is import qualification;
frame cadence, memory residency and navigation still need the final benchmark.
`web-large-photo-admission-build2.log` and `package-final-workflows.log` record
the Wasm build and 13 passing package tests.

## Tablet navigation and resource checkpoint — 2026-09-16

Feature workflows above are complete; the performance/user-acceptance gate is
still open. Tests use the isolated `art.capycanvas.colorm2` package and a separate
browser origin. The source JPEG remains `/sdcard/Download/sony_a7r_v_29.jpg`.
No browser flags, user tabs, density or global refresh preferences were changed.
The tablet currently reports physical density 280, 2880×1800 landscape, 120 Hz;
Chrome 152.0.7977.82 uses DPR 1.75. Thermal status was 0, with battery/GPU about
28/34 °C (`tablet-performance-{battery,thermal}.txt`).

The 61 MP native import initially failed admission. The shared policy incorrectly
applied an unlimited root cgroup's free pages over `MemAvailable`, discarding
reclaimable headroom. Only an actual smaller cgroup limit now caps availability.
The decoder gets one third of remaining memory, with source + decode still below
one half. Unknown-host fallback follows the same fractions. The full Sony photo
then opened in 5.27 s; zero headroom remains zero and restricted cgroups are tested.

The first navigation measurements identified two independent costs:

- Android's Adreno driver does not expose `VK_EXT_memory_budget`. Requiring that
  extension left it in the 691 MiB bounded display path even with available RAM.
  Android now admits a complete display pyramid from half the smaller driver/system
  headroom, or measured system headroom alone for a Vulkan **integrated** device
  that exposes host-visible device-local memory. This follows the
  [Vulkan UMA memory model](https://docs.vulkan.org/guide/latest/memory_allocation.html).
  Unknown/discrete devices keep the bounded fallback. GTK's quarter-driver-budget
  policy is unchanged. A ceiling allocates only this document's actual pyramid.
- Minified display used sixteen bilinear samples per screen pixel. On Chrome,
  submission plus a queue-completion wait took about 30–54 ms at minified zooms,
  versus about 10–13 ms when magnified (`web-photo-gpu-wait.json`; this includes
  browser/IPC wakeup latency, not GPU timestamp duration). Contiguous display
  levels now use hardware filtering and adjacent completed mips use trilinear
  display sampling. The tiled fallback retains cross-page gathers. Pixels outside
  the document skip artwork sampling. Editing/filter inputs/export remain full
  resolution and native integer/Float32; display reductions never feed them.

The Web file worker retires an idle Wasm heap over 256 MiB after five seconds,
only with no pending jobs or open output lease. Another file action recreates it;
interactive capture compression uses a separate worker. Browser complete-cache
admission uses the documented capacity-based ceiling, not a fabricated free-RAM
measurement. This is not a whole-editor reservation manager or automatic tab offload.

Measured 9504×6336 navigation, full 2880×1800 viewport, synthetic zoom/pan/rotation:

| Path | CPU frame median / p95 / p99 | Callback cadence median / p95 / p99 |
| --- | --- | --- |
| Android bounded, before fixes (warm run 1) | 1.24 / 42.96 / 80.95 ms | frequent missed refreshes; 190 frames / 361 input ticks |
| Android complete + adjacent mips (warm run 1) | 1.21 / 2.30 / 5.18 ms | 8.334 / 8.334 / 16.667 ms; 348 frames / 361 input ticks |
| Web complete, before adjacent mips (warm) | about 1.8 / 2.5 / 2.8 ms | p95 about 66.7 ms, p99 83–100 ms |
| Web complete + adjacent mips (three runs) | 1.4–1.8 / 2.4–2.5 / 2.6–2.8 ms | 16.7 / 16.7–16.8 / 16.8 ms |

Raw evidence is `native-photo-navigation-{refined,trilinear}-*.json` and
`web-photo-navigation-{complete,filter,trilinear}.json` under
`artifacts/color-m2/web-android`. Native tracked canvas storage is 1309.2 MiB;
Web 1265.0 MiB. These exclude source/CPU/driver/process overhead and are **not**
whole-device peak memory. Input is injected through the ordinary native owner
queue or Web camera/frame scheduler; these are not hardware input-to-photon
measurements. First runs are retained separately. Chrome's idle RAF is also
16.7 ms: the remaining Web ceiling is separate from the resolved GPU stalls.
Its `throttle-main-thread-to-60hz` flag exists and is set to Default. Disabling
it/restarting the whole browser awaits user approval; its causal role is not
proven merely by the flag's existence. Web 120 Hz is not yet qualified.

Fresh frame-creation baselines were rebuilt locally, on this tablet:

- Android: merged pre-port `00d5a85b`, same three-run 24 MP concurrent-save harness.
  Baseline paint p95 2.71–2.84 ms and p99 3.21–3.86 ms; current 5.19–6.06 ms
  and 8.54–9.53 ms. This exceeds the proposed regression trigger. The baseline
  constructs the legacy renderer; current constructs native integer backing with
  Float32 processing. The comparison includes that precision/commit-work change;
  the exact share of the added cost has not been isolated. Dirty-frame/commit
  optimization is explicitly deferred by the user. Do not call this an unchanged
  hot-path pass or confuse it with the qualified unchanged-photo navigation.
- Web: `00d5a85b` cannot start its unported UI (`rgba.slice is not a function`).
  The clean `origin/main` commit `dff06311` is the runnable fresh baseline, without
  patching its app. On the same 6000×4000 SDR8 canvas and 2880×1800 viewport,
  warm baseline p95/p99 are 1.0 / 1.2–1.3 ms; current 1.2 / 1.4–1.6 ms.
  A 0.3 ms p99 increase appears in one run; others are at the 0.2 ms noise trigger.
  Initial contact/save values remain in the raw reports rather than being omitted.
  Current native SDR8 was explicitly queried after the benchmark.

`native-{fresh-baseline,current}-frames.json` and
`web-{main-baseline,current}-frames.json` contain three independent runs each.
Both use existing frame-creation harnesses; Web's dimension selector now targets
only the two numeric extent fields so new preset/name controls do not corrupt setup.
This is not an equal-processing FP16-versus-U16 microbenchmark.

Validation after the display changes: all 15 shared display-cache GPU tests pass
serially (`shared-display-trilinear-tests.log`), covering bounded storage, missing
mips, edits, native stroke undo/redo, device replacement and source effects/masks.
Contiguous-vs-atlas interpolation permits at most one encoded SDR display code;
exact backing/export tests retain their original tolerances. The two memory policy
tests pass (`shared-memory-final.log`). Wasm and ARM64 builds pass. The real Node
packaging invocation reports **13 tests** (`package-final-real.log`). Earlier
sandboxed packaging invocations that reported only one file-level pass did not
execute those individual cases; this final unrestricted run supersedes their
incorrectly summarized test counts above.

Still required before final handoff: resolve/document the Chrome 120 Hz ceiling,
finish whole-process memory and latency evidence, verify file-worker retirement
followed by delivery, deploy the final named Android/PWA builds, and receive the
user's hands-on confirmation. Apple/Windows ports and print proofing remain out
of scope; the main-integration platform warnings still apply.

### Final delivery checks and measured limits

`native-memory-samples.json` samples Android `dumpsys meminfo` and system
`/proc/meminfo` approximately once per second during a separate 61 MP import and
navigation run. Observed process peaks were 841.8 MiB PSS / 994.2 MiB RSS; steady
PSS was about 765 MiB. Android's graphics accounting reports only about 169 MiB,
less than the renderer's 1309 MiB tracked textures, so it cannot stand in for total
GPU residency. System available memory fell from 4466.9 MiB to a sampled minimum
1044.0 MiB and settled near 1430–1447 MiB. The system delta includes driver/cache
and other processes; it is not exclusively this app. These are sampled peaks,
not allocator-enforced maxima. The separate memory run passes without device
loss; its instrumentation timings are not mixed into performance results.

`software-latency-estimates.json` associates each submitted frame with the latest
already-processed synthetic input by host timestamps. Android's warm request to
CPU submission is median / p95 / p99 **10.22 / 11.56 / 14.52 ms**. Choreographer's
**expected**, not observed, presentation adds roughly 22 ms, for a 32.33 ms
request-to-expected-present estimate. Web's three runs give p95 17.5–17.8 ms and
p99 18.0–19.5 ms to CPU submission at its 60 Hz RAF cadence. These omit coalesced
requests and lack per-present camera confirmation; they are not measured
input-to-presentation or input-to-photon percentiles. Actual panel latency remains
unmeasured. The identified navigation regression was minification GPU cost plus
Android's missing complete-cache admission; additional compositor latency has
not been causally isolated. No claim is made that cadence proves input latency.

`web-worker-retirement-save.json` records creation/request/termination events:
a 61 MP import's oversized idle file worker terminates, the next Save As creates
a new worker, and a 146,745,637-byte native master is delivered successfully.
`tablet-web-handoff-workflows-retry.log` then passes native paint/photo save and
reopen, exact U16 PNG/TIFF samples/profile content, resized P3 delivery, sRGB JPEG,
nonmodal histograms, cancellation and explicit untagged interpretation. Its first
invocation exposed a test error: it compared profile **archive offsets**, which
may change with workspace metadata. The corrected assertion preserves profile
content digest/length checks; native readers independently verify profile bytes.
`tablet-native-handoff-workflows.log` passes all nine affected Android workflow
tests in 80.028 s, including retained effects/masks, colors/history, exact copies,
saving, profile/preset ownership, diagnostics and GPU/recovery paths.

The production Web package now includes exact codec license notices. Bilevel
fax TIFF was already rejected by the supported unsigned 8/16-bit matrix; disabling
that unused decoder removes `fax`/`fax_derive`. Supported Deflate/LZW/JPEG TIFF
routes remain enabled. TIFF's old zune JPEG/core archives omit notices; the
original Zlib alternative is pinned to their publication revisions. zune-core
also omits repository metadata, so its exact complete notice is checked into
`apps/layer-web/licenses` and the packager verifies both version and checksum.
`shared-final-tiff-feature-tests.log`: 61 shared color/codec tests pass, four
external-fixture tests skipped (their earlier separate results remain above).
`package-handoff-notices.log`: all 14 real package tests pass, including refusal
to reuse a notice after dependency version drift. The static PWA builds with
275 precached files (`web-handoff-package-final.log`). Android's test name is
provided through `-PcapyAppLabel='Capy Canvas Color M2'`; normal builds retain
`Capy Canvas`.

`tablet-web-final-package-workflows.log` passes the same SDR journey on the
actual fingerprinted PWA at `http://127.0.0.1:8128/`, with a valid standalone
manifest and service worker. `android-final-package-build.log` builds the final
ARM64 APK after the unsupported TIFF feature removal; it is installed as
`art.capycanvas.colorm2`, label **Capy Canvas Color M2**, alongside the normal app.
The Downloads JPEG's SHA-256 was rechecked after deployment and remains unchanged.

### Final installed-build measurements

`native-final-package-navigation-{0,1,2}.json` retains every run from the final
APK (no memory sampler running). Actual drawing submissions number 346 / 345 /
345 for 361 input ticks each; two additional callbacks per run do no paint work.
CPU render/present p95 is 2.05 / 2.68 / 2.31 ms; p99 5.12 / 5.91 / 6.34 ms.
Submission cadence median and p95 are approximately 8.334 ms, p99 16.667 ms.
Each run contains 16–18 intervals over 12 ms (about 5%); do not describe this as
zero missed frames. These gaps still need the user's smoothness assessment.
`web-final-package-navigation.json` uses the committed CLI against the actual
packaged app: all three runs submit 361 frames, CPU p95 2.4–2.5 ms, p99 2.6–2.8 ms;
RAF median 16.7 ms and p99 16.8 ms. The Chrome 120 Hz question remains pending.

`web-memory-samples.json` samples all Chrome processes associated with its
package; other user tabs remain open and contribute to these totals. Browser PSS
rises from 1674 MiB before the 61 MP import to an observed 2692 MiB peak, then
settles at 1991 MiB after the idle worker releases about 600 MiB. Total reported
RSS peaks at 3032 MiB. System available RAM starts at 4070 MiB, bottoms at 1725 MiB
and settles at 2371 MiB. Driver/unmapped GPU allocations are not fully included in
process PSS. The figures are whole-browser observations, not exact per-tab
allocation attribution. The separate final navigation run follows this memory
sampling and has no dumpsys polling overhead.

Installed APK SHA-256:
`eef27f502a9efa11fad6de18c8980ba8e285a5876494f0b3a173f30aa80ed5da`.
Packaged service worker SHA-256:
`aa5cdc5ddb95bd3656c2cd75f4847634f544705f42b52eed59bc7f7262f3c8da`.

`web-final-origin-unavailable.log` proves a new PWA navigation reaches full GPU
startup while its own ADB port is removed and an uncached URL fails to fetch.
The app's connection is restored afterward; other tabs' connections are untouched.
Chrome's `navigator.onLine` still reflects the tablet's general connectivity, so
it was not used as proof that this origin was unavailable. The earlier emulated
network attempt alone was insufficient evidence. Temporary served test images
were removed from the distribution after testing; the original remains in the
tablet's Downloads folder.

Handoff status: both installed/deployed builds are ready for the user's test.
Native label: **Capy Canvas Color M2**. Web: the tablet Chrome tab at
`http://127.0.0.1:8128/` (cached PWA). Test image:
`/sdcard/Download/sony_a7r_v_29.jpg`. Test hosts were measured separately; retaining
both large-photo instances competes for the same system RAM and may select the
bounded fallback. Chrome flag changes require the pending user choice because
they affect the whole browser and require restarting other tabs. No 120 Hz Web
pass or actual input-to-photon claim is made. User confirmation is still required;
this checkpoint does not declare the entire cross-platform milestone complete.

## User-test corrections — 2026-09-16

The user found that G-Pen strokes removed the original photograph from complete
paint tiles in Web. A 4353×769 opaque fixture (more than the sixteen decoded
source slots) reproduced 188,283 transparent pixels after a single stroke in
`web-gpen-before.json/png`. With ordered uploads, the same fixture has zero
transparent pixels (`web-gpen-after.json/png`).

The Web-only `Queue::write_buffer` shortcut was not equivalent to the native
staging path: cold neighborhood queries reuse scene uniform offsets while earlier
source-to-paint initialization still refers to them. Queue writes execute before
the submitted drawing commands, so later uniforms replaced the earlier values.
The shared reusable staging belt now encodes copies at each actual point of use
on all hosts. This follows [wgpu's queue ordering contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_buffer).
It changes neither integer backing nor Float32 processing. The controlled Web
workflow passes exact undo/redo, native save/reopen, and GPU replacement
(`web-gpen-complete-workflow.log`). The native equivalent checks U8/U16 and both
in-place/candidate publication with cold source tiles (`native-gpen-regression.log`).
The committed browser regression is `photo-paint.test.mjs`, also available via
`node apps/layer-web/test.mjs --photo-paint`; its optional photo URL runs the same
checks on the 61 MP JPEG.

The Android report was from `art.capycanvas`, updated September 13, while the
phase 2 build had been installed separately as `art.capycanvas.colorm2`. The normal
package is now updated with `adb install -r`, preserving its existing data. No
JPEG reader fallback or archive compatibility path was added: current Open
already detects native archives versus photo signatures. The original JPEG in
Downloads remains unchanged.

The expanded Android test passed 61 MP import, G-Pen, opacity preservation,
undo/redo and save/reopen, then crashed during forced GPU recovery. The symbolized
trace enters Adreno render-pass construction from the first staged paper frame;
Scudo reports a mapping allocation failure. Source upload submissions already
bound cold-photo command memory, but paper/resident composition had no such
boundary. Large display composition now drains every sixteen completed tiles,
including frames with no source uploads. Normal camera-only frames do no
composition and do not enter this path. The failing trace is retained in
`android-61mp-recovery-crash.log` and the original run in
`android-61mp-gpen-before-recovery-fix.log`. The same test passes after the fix
in 52.665 seconds (`android-61mp-gpen-workflow.log`): no transparent pixels,
matching histograms after undo/redo/reopen/recovery, and identical native blob
manifests after reopening. All fifteen shared live-display GPU regressions pass
(`display-recovery-bound-tests.log`).

Chrome passes the full-size photo through save/reopen but still encounters
memory pressure after replacing the GPU, with the composition bound alone
(`web-61mp-gpen-before-device-retirement.log`). Browser recovery qualification
remains open at this checkpoint.

The user approved disabling Chrome's browser-wide
`throttle-main-thread-to-60hz` flag. It was changed from Default to Disabled using
`chrome://flags`, then activated with Chrome's Relaunch button. The Disabled
selection persisted and all pre-existing user tabs remained present. Flag state
is recorded in `chrome-throttle-disabled-after-relaunch.log`.

The normal Android package also passes its complete SDR raster suite in 88.147 s
(`android-corrections-sdr-suite.log`, nine ordinary tests plus the opt-in large
photo test reported as skipped). The large photo test is run separately with
`-e photoWorkflow true`, as above. To reproduce on the installed normal package:

```sh
adb shell am instrument -w -e class art.capycanvas.AndroidRasterTest \
  art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
adb shell am instrument -w -e photoWorkflow true \
  -e class art.capycanvas.AndroidRasterTest#largeJpegGpenPreservesPhotoThroughSaveAndRecovery \
  art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

Web suspension now explicitly destroys the retired GPU device after preserving
the CPU document. Ordinary document adoption shares the live device and does
not destroy it. This follows the [WebGPU lifetime contract](https://gpuweb.github.io/gpuweb/#resource-lifetime):
dropping JavaScript handles does not promise timely release of GPU allocations.
wgpu 30.0.1's WebDevice drop is a no-op. Explicit retirement prevents recovery
from relying on garbage collection to release the previous device's resources;
the end-to-end test now passes (`web-61mp-gpen-workflow.log`). It imports the
9504×6336 JPEG, draws G-Pen, verifies zero transparent pixels and changed image
content, exact undo/redo histograms, native save/reopen blob manifests, and an
identical histogram after GPU replacement. No host or browser console error is
reported. Available system memory was sampled at 4823492 KiB during recovery
and 2642636 KiB after its final full-image histogram; the composition-only run
fell to 800892 KiB and then lost the tab. These spot readings support the resource
lifetime diagnosis but are not peak-memory measurements or per-tab accounting.

`web-corrections-navigation.json/log` measures the corrected PWA at 2880×1800,
DPR 1.75, after the complete 61 MP workflow and GPU replacement. With the approved
Chrome throttle disabled, all three runs submit 361 frames; RAF median/p95 is
8.3/8.4 ms. CPU frame creation p95 is 1.9/1.8/1.9 ms and p99 is 2.0/1.9/2.1 ms.
Intervals above 12 ms number 5/2/2 out of 360, so this is smooth 120 Hz navigation
with occasional missed intervals, not a zero-drop guarantee. Tracked canvas
storage is 1272 MiB including the painted photo's tiles. The synthetic latest
request-to-CPU-submit estimates are p95 8.8/8.7/8.9 ms and p99 12.6/9.2/10.1 ms
(`web-corrections-software-latency.json`). The prior Default-flag measurements
were 60 Hz and p95 17.5–17.8 ms. Removing the browser throttle explains the
cadence ceiling and approximately one refresh interval of submission latency.
These runs also include the correctness fixes, so they do not isolate the exact
cause of every CPU timing difference. Actual presentation latency remains
unmeasured; the earlier unexplained compositor delay is not claimed resolved.

The final PWA also passes every exported SDR workflow check in
`apps/layer-web/color-m2.test.mjs`: tagged color/palettes, U16 profiled delivery,
document color/history, retained imports and real custom-format clipboard paste,
source repair/rasterization, export presets, ICC library, flattened copy and all
six correction layers/masks. Evidence is split across
`web-corrections-all-sdr-workflows.log` (first two checks),
`web-corrections-remaining-sdr-workflows.log` (imports/source edits/presets),
`web-corrections-final-sdr-workflows.log` (profile library), and
`web-corrections-copy-filters.log` (copy and corrections).

Those logs retain harness failures rather than hiding them. Setup on an existing
tablet origin must finish recovery prompts with **Keep for Later**, show the
Color/Layers panels, and accept Chrome's clipboard permission prompt. The
profile test now waits for the actual close event, and the copy test selects
Color space in the open dialog instead of the retained Preferences control.
Neither failure required a production color change. The original workspace was
restored after testing; no recovery copies were deleted.

The final normal Android package passes the three-run 61 MP navigation harness
(`android-corrections-navigation.log`, 18.963 s). Warm runs submit 346/342 frames
for 361 input ticks, with CPU p95 2.08/2.26 ms and p99 5.90/4.65 ms. Input cadence
median/p95 is 8.334 ms, with 4/2 intervals above 12 ms. The first immediate
post-import run has a 2.15 s delay before its queued inputs are processed and only
102 submissions; it is **not** a smooth-navigation pass. The raw timestamps
establish an owner-queue delay, but do not identify the intervening operation.
Its cause is not known from this capture. Warm latest-input-to-submit p95 is
11.14/11.59 ms; this remains a software estimate, not observed presentation.
The raw runs and summary are `android-corrections-navigation-{0,1,2}.json` and
`android-corrections-navigation-summary.json`. Cold import/startup and dirty
regeneration remain outside the warm navigation result.

Final Android delivery uses **Capy Canvas / `art.capycanvas`**, APK SHA-256
`fcdff15a316c68a9bdee04f69f55a4212a555218732e0ed73a3e0170b0c930fc`.
Opening `Downloads/sony_a7r_v_29.jpg` through that app's real system picker
succeeds and displays the 9504×6336 photograph (`android-normal-package-open61mp.png`).
Its SHA-256 remains
`3aac9c9b8b34c38a5e0121f16ad1ec806e92a19e15ee5e1128f36d987e888054`.
The normal app's existing recovery entries were kept for later. The separately
installed earlier `art.capycanvas.colorm2` is not the corrected handoff package.

The final Web package uses Wasm fingerprint `43511c2096db122e6018` at
`http://127.0.0.1:8128/` on the USB-connected tablet. The earlier public editor
tabs are a different build. Service-worker activation was verified explicitly;
the final reload removes test picker/clipboard instrumentation. User hands-on
confirmation remains the final acceptance step. No Apple/Windows port or merge
to the main branch is included in this checkpoint.
