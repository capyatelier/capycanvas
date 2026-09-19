# Android HDR display and footer

The reviewed Wacom MovinkPad Pro 14 (DTHA140, Android 15) uses a floating-point
BT.2100 PQ canvas. The user accepted its physical appearance on 2026-09-19.
Instrumented luminance/colorimetry and sustained thermal qualification remain
separate from that visual acceptance.

## Current implementation

For HDR artwork with Proof Off, Android presents PQ without applying the
shared display-headroom shoulder first. The shared Rust encoder converts
working primaries to BT.2020 with artwork RGB 1 at 203 cd/m². This viewing
output is bounded to PQ's 0–10,000 cd/m² signal range and BT.2020 channels;
it never changes the editable HDR master.

Android 15+, HDR10/HDR10+ display support and an exact float/PQ Vulkan pair are
required. The host also requires sRGB support for that float format so Proof
can switch encoding safely. `SurfaceView.setDesiredHdrHeadroom(0)` uses normal
Android HDR brightness policy. Separate mastering/MaxCLL metadata is not sent;
Android uses PQ's absolute encoding and its platform tone-mapping defaults.

Proof SDR, Print, gamut warnings, appearance drafts and SDR documents select
an sRGB surface and the shared SDR mapper, with headroom request 1. Display
capability loss also returns to SDR. Canvas and Navigator share the presenter
and surface. Document adoption and GPU recovery preserve the encoding.
GTK's linear/scRGB shoulder remains in use by GTK; it is not an obsolete
Android experiment and is retained in the shared renderer.

The left footer shows **HDR**, **SDR preview**, **Print proof**, or **Showing SDR**,
with the zoom/rotation bubble styling. Display Details explains the current view
and how to switch Proof. Color controls and layer thumbnails remain SDR previews.
Web remains mapped SDR.

## PQ display validation

The shared GPU oracle checks PQ against independent Float64 math for F16/F32,
sRGB/ProPhoto, alpha, negative/bright samples, different SDR recipes, retained
viewing captures, normal HDR shoulders and explicit proof.

On the Wacom, SurfaceFlinger confirms `BT2020_PQ` / `RGBA16161616F_UBWC` for
HDR and `V0_SRGB` for SDR proof. The final device regression checks above-white
canvas/Navigator pixels, an SDR display fallback and exact HDR restoration,
SDR/Print switching, idle tone-guide presentation, GPU recovery, footer geometry
and the concise HDR label. HDR editing/delivery/recovery and print portability
cover pen painting, touch/pen cancellation, undo, native save/reopen, EXR/PQ/SDR
delivery and Activity/GPU recovery. The real Chrome HDR workflow covers the
shared Web path.

The earlier PQ review captured canvas/Navigator maxima of 1.2783/1.53125 via
Android float PixelCopy. These captures convert PQ and do not measure luminance.
Android still reported 1.004× headroom, with a 400-nit white point and
`dimmingRatio=1` for both layers. Those reports do not cap the application’s PQ
pixels. The final code no longer reads, stores or republishes the unused ratio,
nor injects simulated headroom. It retains actual format, submitted HDR mode
and tone-generation diagnostics because they verify displayed frames.

Runnable final APKs, the Web package and raw evidence are under
`artifacts/android-hdr-final/review/`. The accepted PQ review is preserved under
`artifacts/android-hdr-compositor/review/`. Private drawing backups are outside
review bundles and Git.

```sh
ANDROID_HOME=/path/to/Android/Sdk apps/layer-android/gradlew -p apps/layer-android \
  :app:assembleDebug :app:assembleDebugAndroidTest -PcapyAbi=arm64-v8a \
  -PcapyApplicationId=art.capycanvas.hdr '-PcapyAppLabel=Capy Canvas HDR'
adb -s DEVICE shell am instrument -w -r -e requireHdr true \
  -e hdrFile /data/local/tmp/capy-hdr-pq.png \
  -e class art.capycanvas.AndroidRasterTest#hdrDisplayNegotiation \
  art.capycanvas.hdr.test/androidx.test.runner.AndroidJUnitRunner
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen \
  tools/performance/workspace-motion.sh web --hdr
```

## Canvas, Navigator and layer-thumbnail follow-up

Earlier investigation found missing redraw publication: Proof actions ignored
their shared `UiChange`, and completed tone analysis did not dirty an idle canvas.
Both publish presentation updates. Submitted tone-generation diagnostics prevent
a ready worker from being mistaken for a displayed result.

Web/Android thumbnails and filter previews now receive GTK's shared SDR recipe.
Painted and retained-photo previews use that mapping; masks remain neutral.
Appearance drafts invalidate thumbnail revisions and cancellation restores them.
Native cold-photo thumbnail preparation uses GTK's four-tile batches.

The earlier complete 24 MP workload recorded 6.856 s readiness, 678 MiB sampled
peak process PSS and 618 MiB tracked canvas residency. Histogram/export/Open
cancellation took 297/288/2 ms. The three pen/touch/pen runs and concurrent
save/reopen completed. These historical measurements are in
`artifacts/android-hdr-consistency/review/`; they are not a new sustained PQ
or physical-input/thermal qualification.

The superseded extended-linear experiment used a reported-headroom threshold
and synthetic ratio tests. That policy was replaced by PQ after review; its
raw evidence remains in the earlier artifact bundles, not as an active fallback.

References: [SurfaceView automatic headroom](https://developer.android.com/reference/android/view/SurfaceView#setDesiredHdrHeadroom(float)),
[Android mixed SDR/HDR composition](https://source.android.com/docs/core/display/mixed-sdr-hdr),
[Android tone mapping](https://source.android.com/docs/core/display/tone-mapping).
