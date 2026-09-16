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
