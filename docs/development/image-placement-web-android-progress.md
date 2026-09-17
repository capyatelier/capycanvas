# Web and Android image placement qualification

This implements the [photo placement handoff](image-placement-web-android-handoff.md).
Qualification used the attached Wacom MovinkPad 14 (DTHA140, Qualcomm SM8635,
Adreno GPU), its Chrome browser, and headed desktop Chrome in an isolated
headless Mutter Wayland session. The tablet runs Android 15. The desktop has an
AMD Threadripper PRO 9995WX and NVIDIA RTX PRO 6000 Blackwell Max-Q, driver
610.57.4.0. Tablet Chrome 152 retained its existing desktop-site mode. The native
APK uses a debug Android host and a release Rust renderer.

## Delivered behavior

Web and Android Import/Paste now prepare an entire batch and start shared
interactive placement. Sources are centered and fitted to the current canvas.
Apply commits one history transaction; Cancel removes provisional layers.
Original Size (100%) restores native scale, including after native archive
save/reopen. Open retains the oriented source dimensions and fits the camera.
Both hosts expose placement controls independently of docked panels.

External canvas drops capture the document point before file access. Retained
layer rows use shared above/below/into validation and feedback. A failed,
cancelled or stale batch cannot partially insert or silently retarget. Captured
document epoch, artwork revision, selected target and GPU generation are checked
before adoption. Surface input stays in the host; placement, history, source
storage and destination rules remain in Rust. A touch on an active photo or
placement handle now manipulates that placement; other canvas touch gestures
retain their navigation behavior.

The Web controller retains original `File` bytes, decodes on the raster worker,
and checks cancellation/device loss across asynchronous boundaries. Android
supports multiple SAF selections, multi-item clipboard URIs and global URI drops.
It retains drag permissions through preparation, handles seekable descriptors
and bounded pipe spooling, and cancels provider/native work with its owner.
Both hosts apply one aggregate source allowance to a batch and a 512 MiB encoded
file limit. Source depth, profile, alpha and source-local painting use the shared
storage/export paths without a browser canvas or Android Bitmap conversion.

Filters come from the actual shared JPEG, PNG, TIFF, BMP/DIB, GIF and WebP
capabilities. First-frame naming disclosures are retained. **HEIC/AVIF are not
supported by these targets**; the optional native bridge remains Linux-only.

Implementation entry points:

- Web: [image-import.js](../../apps/layer-web/image-import.js),
  [image_import.rs](../../apps/layer-web/src/image_import.rs).
- Android: [ImageImport.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/ImageImport.kt),
  [ImageDropTarget.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/ImageDropTarget.kt),
  [image_import.rs](../../apps/layer-android/native/src/image_import.rs).
- Shared capture/validation: [art_layers.rs](../../crates/layer-ui/src/art_layers.rs).

## Runtime checks

The real-photo fixtures were the original public 61 MP LensTip `DSC02494.JPG`
(9504×6336) and CameraLabs A7 III JPEG (encoded 6000×4000, oriented 4000×6000).
URLs and SHA-256 digests are recorded in the
[GTK fixture provenance](image-placement-gtk-progress.md).
The test canvas was 2000×1500. Generated fixtures supplement these originals for
malformed input, alpha and 16-bit/ICC checks.

The checked-in [browser journey](../../apps/layer-web/image-placement.test.mjs)
checks real Chrome picker delivery, original-byte clipboard and external drops,
batch order/fit, Apply/Cancel, one-step Undo/Redo, Open, save/reopen, Original Size,
group/locked destinations, stale chooser context, delayed-read cancellation and
GPU replacement. Source identity compares resolved archive blob digests, so
changes in archive blob numbering cannot conceal or falsely suggest sample loss.
The controls are exercised in Zen mode at 360×640. Page errors fail the run.

The [tablet Chrome journey](../../apps/layer-web/image-placement-device.test.mjs)
fetches original JPEG bytes into `File` objects, then uses the production picker
controller and native CDP touch input. It verifies batch history, fit, exact
source retention, save/reopen and Original Size. This transport does not claim
to exercise Android DocumentsUI inside Chrome.

The [Android instrumented journey](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidRasterTest.kt)
checks real JNI/Vulkan batch placement, Compose controls, original source digests,
fit, history, reopen, Original Size, malformed second files, cancellation and
stale selection. Its optional motion run injects actual Android mouse, touch and
stylus events. Clipboard/profile qualification uses a 16-bit embedded-ICC PNG in
an 8-bit document. The system-picker/global-drag journey passed with both
originals on a 2000×1500 canvas: real DocumentsUI multiple selection, canvas point
capture, group insertion, large clipboard cancellation, activity/GPU recreation,
retired-batch rejection, and oriented photo Open. Its global drag uses test-owned
provider URIs; cross-app grant revocation and a stalled remote provider remain
unqualified.

Android canvas and row drops use one Compose target hierarchy. A native embedded
SurfaceView drop handler intercepted row drops during qualification; moving its
fallback into the Compose hierarchy fixed delivery.

The combined Android run passed all three journeys in 522.94 s, including the
complete motion workload below. A subsequent guard skips Vulkan buffer freeing
for empty lists; the final APK passed the real-photo batch regression again in
42.89 s. Android assembly, instrumentation assembly and lint passed. Desktop
Chrome passed both the source-tree and packaged-PWA journeys; tablet Chrome
passed its original-file journey and motion workload.

Shared validation passed 10 placement-filter tests and 12 source-filter tests.
The BMP/DIB, GIF and WebP decoder suite passed eight tests; two optional
external-reference/fixture-generation tests were skipped. Package dependency,
fingerprint and service-worker checks passed all 14 tests. Vulkan passed
12 `placed_photo` tests covering painting, mask/gradient/smudge/liquify source-local
geometry, display LOD, exact snapshots, off-canvas content and full-source
thumbnails, plus the oversized-preview admission regression. These exact GPU
checks ran on desktop Vulkan; they do not claim per-pixel readback coverage of
every operation on Adreno. Host journeys additionally verify unchanged source
digests and changed paint tiles with exact Undo/Redo.

## Measurements

Loading includes preparation and Apply, separately from warm motion. It follows
a cancelled rehearsal batch, so file/decoder/shader caches may be warm; tablet
Chrome fetches the originals before this timer. It excludes user picker delay
and network download time. Each warm
window contains the last 120 renderer updates from a 180-step stylus translation
or drawing contact. The preceding short mouse/touch contacts verify routing;
their short windows are not reported as warm measurements. These are renderer
timings, not physical pen-to-photon latency or guaranteed frame rates. GPU
timestamps include scheduling gaps and exclude presentation; browser timestamps
can be quantized. Native and Web frame submission/accounting differ. Desktop
Chrome used a 1600×1000 virtual monitor; tablet Chrome reported a 1646×908 CSS
viewport, and the native display is 2880×1800. These qualify each host at its
tested viewport, rather than a matched-resolution comparison.

Values are **p50 / p95 / maximum**, in milliseconds.

### Desktop Chrome

| Photo | Scale × fit | Translation GPU | Translation render CPU | Drawing GPU | Drawing render CPU |
| --- | ---: | --- | --- | --- | --- |
| 61 MP | 1.1 | 1.13 / 1.14 / 1.14 | 0.90 / 1.00 / 1.40 | 1.24 / 1.62 / 1.72 | 1.00 / 1.30 / 3.30 |
| 61 MP | 1.2 | 1.12 / 1.13 / 1.32 | 0.90 / 1.10 / 1.30 | 1.25 / 1.46 / 1.50 | 1.00 / 1.30 / 1.30 |
| 61 MP | 2 | 1.05 / 1.22 / 1.25 | 0.90 / 1.00 / 1.30 | 1.13 / 1.31 / 1.40 | 0.90 / 1.20 / 1.50 |
| 24 MP | 1.1 | 1.03 / 1.04 / 1.05 | 0.90 / 1.00 / 1.10 | 1.09 / 1.28 / 1.41 | 1.00 / 1.20 / 1.50 |
| 24 MP | 1.2 | 1.01 / 1.03 / 1.04 | 0.90 / 1.00 / 1.30 | 1.13 / 1.29 / 1.37 | 1.00 / 1.20 / 1.30 |
| 24 MP | 2 | 1.02 / 1.04 / 1.04 | 0.90 / 1.00 / 1.30 | 1.10 / 1.29 / 1.36 | 1.00 / 1.20 / 1.30 |
| Both visible | 2 | — | — | 1.88 / 2.00 / 2.09 | 1.20 / 1.50 / 1.70 |

Batch preparation + Apply: **4.68 s**.

Associated Chrome-process PSS during individual-photo drawing: 2910–2965 MiB; both visible: 3888 MiB. This includes the test harness's retained files/archive buffers.

Tracked canvas allocation with both visible: 514 MiB. This excludes imported assets and driver overhead and is not whole-process or total GPU memory.

### Tablet Chrome

| Photo | Scale × fit | Translation GPU | Translation render CPU | Drawing GPU | Drawing render CPU |
| --- | ---: | --- | --- | --- | --- |
| 61 MP | 1.1 | 16.71 / 27.79 / 28.05 | 1.50 / 2.10 / 8.80 | 15.79 / 16.65 / 17.24 | 6.60 / 7.60 / 12.40 |
| 61 MP | 1.2 | 16.71 / 17.17 / 17.43 | 1.40 / 1.80 / 6.20 | 14.02 / 14.55 / 15.07 | 6.60 / 7.50 / 15.30 |
| 61 MP | 2 | 16.84 / 17.50 / 17.63 | 1.50 / 2.00 / 6.60 | 9.31 / 9.76 / 10.16 | 6.90 / 7.70 / 8.30 |
| 24 MP | 1.1 | 15.99 / 16.71 / 17.10 | 1.50 / 1.70 / 6.40 | 8.91 / 9.50 / 9.63 | 6.90 / 7.90 / 9.30 |
| 24 MP | 1.2 | 15.66 / 16.32 / 16.65 | 1.50 / 1.80 / 6.00 | 8.78 / 9.18 / 9.70 | 6.70 / 7.90 / 8.90 |
| 24 MP | 2 | 11.86 / 16.19 / 16.71 | 1.50 / 1.90 / 8.50 | 8.59 / 9.11 / 9.90 | 6.70 / 7.50 / 8.40 |
| Both visible | 2 | — | — | 17.69 / 19.53 / 21.63 | 10.50 / 11.70 / 24.40 |

Batch preparation + Apply: **8.46 s**.

Associated Chrome-package PSS after the complete workload was **2859 MiB**. The first raw report sampled only the main process (~225 MiB); that value omits renderer/GPU processes and is not used as total memory here. The checked-in sampler now sums `dumpsys meminfo --package com.android.chrome`. Other open Chrome tabs contribute to package PSS.

Tracked canvas allocation with both visible: 505 MiB. This excludes imported assets and driver overhead and is not whole-process or total GPU memory.

### Native Android

| Photo | Scale × fit | Translation GPU | Translation render CPU | Drawing GPU | Drawing render CPU |
| --- | ---: | --- | --- | --- | --- |
| 61 MP | 1.1 | 29.93 / 31.89 / 33.83 | 50.89 / 83.27 / 89.93 | 30.28 / 31.10 / 31.76 | 54.11 / 89.36 / 91.15 |
| 61 MP | 1.2 | 33.29 / 34.80 / 40.04 | 56.18 / 85.68 / 106.78 | 25.39 / 26.49 / 26.61 | 54.91 / 87.17 / 90.71 |
| 61 MP | 2 | 24.60 / 25.04 / 26.47 | 52.79 / 85.68 / 109.16 | 25.33 / 26.98 / 27.11 | 51.49 / 83.66 / 84.65 |
| 24 MP | 1.1 | 24.03 / 24.49 / 25.51 | 52.09 / 84.42 / 86.10 | 24.75 / 26.09 / 26.40 | 52.44 / 84.42 / 86.93 |
| 24 MP | 1.2 | 23.38 / 23.76 / 25.30 | 53.43 / 84.88 / 102.43 | 24.25 / 25.64 / 25.84 | 54.62 / 87.25 / 89.14 |
| 24 MP | 2 | 22.58 / 22.84 / 24.71 | 52.11 / 85.36 / 116.18 | 23.49 / 23.77 / 24.73 | 51.43 / 83.78 / 84.81 |
| Both visible | 2 | — | — | 42.76 / 43.67 / 45.01 | 83.96 / 142.38 / 145.35 |

Batch preparation + Apply: **13.07 s**.

Individual-photo drawing process PSS: **837–937 MiB**; both visible:
**938 MiB**. Sampled mapping counts after reopening and throughout the six
cases stayed around 8,900–9,300; the initial preparation/history sequence reached
15,351. The recorded process RSS high-water mark was 1,648 MiB. These include
instrumentation and archive verification allocations.

Tracked canvas allocation with both visible: **522 MiB**. This excludes imported
assets and driver overhead; it overlaps process memory and must not be added to
PSS as a separate total.

Native drawing render CPU medians were 51–55 ms for an individual photo and
84 ms for both, with a 145 ms maximum in the two-photo window. These exceed a
60 Hz frame budget. The storage workaround completes the workload, with a
substantial interaction cost on this Adreno driver.

## Android memory finding

Repeated 61 MP + 24 MP preparation, save/reopen and editing exhausted host memory
mappings in the Adreno Vulkan driver while recording commands. One failing run
reached 64,039 mappings despite available RAM. Merely resetting Vulkan pools
retained driver storage. Freeing completed command buffers on every reuse was
stable, but drawing render CPU medians rose to roughly 75–85 ms in that diagnostic
run. Reclaiming every 64 or 16 resets preserved faster medians but still failed
when drawing with both edited photos visible. The 64-reset run reached 64,110
mappings as the second photo became visible.

Cold placement preview generation now submits and drains batches of 64 tiles,
using the existing renderer upload boundary. Android Vulkan pools free completed
buffers and release pool storage on every reset. This uses the existing
completion boundary. The final policy prioritizes completing the workload;
the native CPU cost and timing tails above are material limitations. Other
platforms keep their existing behavior.
This does not guarantee against arbitrary system memory pressure. See the
[pinned dependency patch](../../vendor/README.md#wgpu-platform-fixes).

## Reproduce and run

Use your own copies of the public JPEGs; they are not checked into the repository.
`artifacts/` evidence is local-only. The harness defaults to generated images
when real-file paths are not supplied.

```sh
# Build and run headed Chrome on an isolated headless Wayland compositor.
LAYER_WEB_PORT=4283 \
LAYER_PHOTO_FILES='["/path/61mp-DSC02494.JPG","/path/24mp-cameralabs-A7III.jpg"]' \
bash tools/performance/workspace-motion.sh web --image-placement

# Add LAYER_IMAGE_MOTION=1 and LAYER_TEST_ARTIFACTS=/path/results for timing JSON.
# --package --image-placement tests the packaged static PWA.
node apps/layer-web/package.mjs

cd apps/layer-android
./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug \
  -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.imageplacement \
  -PcapyAppLabel='Capy Image Placement Test'
```

Install the two generated debug APKs in the isolated application ID, copy the
JPEGs to that app's private files directory, then run:

```sh
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w \
  -e class art.capycanvas.AndroidRasterTest#imagePlacementBatchHistoryAndStaleRequests \
  -e imagePlacementPhotos placement-61mp.jpg,placement-24mp.jpg \
  -e imagePlacementMotion true \
  art.capycanvas.imageplacement.test/androidx.test.runner.AndroidJUnitRunner
```

The native report is in the test app's external files directory as
`image-placement-motion.json`. Run `imagePlacementSystemPickerAndExternalDrag`
and `retainedPlacePasteAndDocumentDetails` by substituting the method name.
The tablet Chrome runner accepts `--image-placement`, `LAYER_PHOTO_URLS` (two
original JPEG URLs), `LAYER_TEST_ARTIFACTS`, and optional `CAPY_ANDROID_SERIAL`/`ADB`
for associated Chrome-process PSS. See [device.test.mjs](../../apps/layer-web/device.test.mjs)
for its CDP connection and isolated-workspace setup.

The runnable static PWA is `dist/capycanvas`; serve it over localhost or HTTPS.
The native debug APK is
`apps/layer-android/app/build/outputs/apk/debug/app-debug.apk` and is installed on
the attached tablet as **Capy Image Placement Test**. The regular app and its
documents were not replaced. Browser tests use isolated workspaces/profiles and
remove their own test tab and temporary provider files.


## Moving-photo work before workflow centralization

The warm Android translation workload was rerun with the real host
Choreographer as the only frame producer, OS mouse/touch/stylus events, the
2880 × 1800 surface and SurfaceFlinger presentation timestamps. The shared
renderer now composes eligible cached retained images directly and clears a
full rebuild once. Ordinary independent tile draws can write directly into the
composite; effects and dependent blends continue through their existing path.
The pixel oracle compares fractional, rotated and mirrored affine edges with
alpha against tiled composition. A proposed extra fragment entry point was
removed: it conflicted with effect shader composition and had no measured
benefit. The existing scene shader owns the cached-image operation.

The native host publishes shared model diffs instead of repeatedly delivering a
roughly 124 KB full snapshot for each pose. This transport is in `layer-host`,
with Android currently its consumer. It preserves arrays, null values and
literal object keys; unchanged model data is reused by Compose.

Pre-refactor tablet report `/tmp/capy-moving-recovery-complete.json` (warm stylus,
120 diagnostic samples each):

| Source | Placement scale relative to fit | Render CPU median | GPU median |
| --- | --- | --- | --- |
| 9504 × 6336 | 1.1 | 4.36 ms | 9.30 ms |
| 9504 × 6336 | 1.2 | 4.45 ms | 8.67 ms |
| 9504 × 6336 | 2.0 | 4.63 ms | 5.62 ms |
| 4000 × 6000 | 1.1 | 3.78 ms | 6.31 ms |
| 4000 × 6000 | 1.2 | 4.65 ms | 6.59 ms |
| 4000 × 6000 | 2.0 | 4.45 ms | 4.86 ms |
| Both visible | Final movement | 4.63 ms | 7.36 ms |

These figures improve substantially on the earlier approximately 50 ms render
CPU path, but **do not qualify 120 Hz**. Host callbacks were around 11–12 ms and
most presentation intervals remained 16.67 ms. The 8.33 ms end-to-end frame budget
still requires work; fitting source data into GPU memory does not remove
composition bandwidth, synchronization or host publication cost. The report
predates the final shared-workflow changes and is a baseline, not acceptance of
those changes. Android large-canvas drawing/stability and then Web latency are
the next requested milestones.
