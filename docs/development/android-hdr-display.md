# Android HDR display and footer follow-up — 2026-09-19

The Wacom MovinkPad Pro 14 (DTHA140, Android 15) advertises HDR10/HLG,
wide color and a desired maximum luminance of 1000 cd/m². The old Android
canvas nevertheless selected an 8-bit sRGB Vulkan surface and passed `1.0`
headroom to the shared presenter every frame. The “mapped SDR display” label
was an accurate description of that application path, not of the panel hardware.

## Implemented path

Android 15 or newer now negotiates the exact `Rgba16Float` /
`ExtendedSrgbLinear` surface pair when Vulkan offers it and Android exposes an
HDR display with an available HDR/SDR ratio. `SurfaceView.setDesiredHdrHeadroom`
requests the artwork's range on the canvas buffer layer. Android supplies the
current ratio; the existing shared Rust/GTK HDR presenter receives the lesser
of that ratio and the requested range. This Android encoding is SDR-relative
linear extended sRGB; it does not use Windows' fixed scRGB reference-white scale.
The ordinary Compose controls remain SDR.

Off presents HDR when available. SDR, Print, gamut warning and temporary SDR
appearance previews use one-times headroom and the existing shared mapping.
Opening a new document and replacing the GPU retain the negotiated encoding.
Display changes invalidate an idle frame without changing the artwork or its
history. JNI diagnostics distinguish selected format/encoding, current policy
and the headroom of the last submitted frame.

Web and Android place a compact display button on the left of the footer.
Padding, text and rounded background match the zoom/rotation readout on the
right. Clicking/tapping opens Display Details, following GTK. Android reports
**HDR (limited)** below 1.05× rather than implying useful highlight brightness.
Web still reports **Showing SDR** and explicitly explains its SDR canvas.

Older Android versions, missing headroom APIs and devices lacking the matching
Vulkan format/encoding use the existing mapped SDR path. Advertising an HDR
video decoder alone does not qualify the canvas. External-display changes,
physical luminance/colorimetry and sustained thermal behavior remain unqualified.

## Device-side limit

On the attached tablet, SurfaceFlinger confirms a 16-bit float canvas in
`V0_SCRGB_LINEAR`, and DisplayManager reports `mIsHdrLayerPresent=true`,
`mHdrVisible=true` and HDR high-brightness mode. The test artwork requests
**2.300448×**, but Android grants only **1.004000×**. Both SDR and Print return
the submitted headroom to 1.0 and clear the active HDR-layer state.

The read-only vendor configuration
`/vendor/etc/displayconfig/display_id_4630947011706244995.xml` explains the
observed limit: `sdrHdrRatioMap` is 1.0 through 398 nits and 1.004 at 400 nits.
The current SDR white is reported as 400 nits. A separate brightness map reaches
920 nits, and the advertised HDR maximum is 1000 nits; neither value establishes
that this firmware grants that brightness to this window. No vendor settings,
brightness override or system configuration is changed by this fix. These are
OS reports, not measured screen luminance. The app-side missing HDR path is fixed;
the firmware's available headroom remains very small.

Android's documented [HDR UI guidance](https://android-developers.googleblog.com/2025/09/hdr-and-user-interfaces.html)
explains independent SurfaceView headroom and SDR UI. The
[SurfaceView API](https://developer.android.com/reference/android/view/SurfaceView#setDesiredHdrHeadroom(float))
defines the Android 15 request; [Display](https://developer.android.com/reference/android/view/Display#getHdrSdrRatio())
provides the current HDR/SDR ratio.

## Reproduction and evidence

Local evidence/builds are under `artifacts/android-hdr-display/`; the `review/`
bundle provides runnable regular/isolated APKs, the static Web archive, source
and build checksums, screenshots and validation results. Private regular-app
recovery backups are excluded from that review bundle and from Git.

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

`hdrDisplayNegotiation` checks the actual surface, last submitted headroom,
idle headroom decrease/restoration, unchanged document revision, SDR/Print,
GPU recovery and footer position/style/details. It records Android display and
SurfaceFlinger state along with screenshots. Screenshots are SDR captures and
cannot prove physical HDR brightness. Existing real-codec HDR editing/delivery,
Print/persistence/recovery and large-document tests remain part of validation.

The HDR surface regression and existing editing/Print workflows pass on the
attached tablet; the headed Chrome HDR workflow also checks footer geometry,
computed styling and opening/closing details. The 24 MP HDR-surface benchmark
opened/analyzed the dense fixture and completed its first pen run, but Android
repeatedly rejected the subsequent injected touch sequence (`ACTION_OUTSIDE`).
Ending synthetic pen proximity did not resolve it. Those incomplete runs are
retained: observed readiness was 7.1–8.1 s and sampled peak process PSS was
0.64–0.75 GiB up to failure. They do **not** qualify the full mixed-input,
concurrent-save/cancellation or sustained HDR-surface performance workload.
The previous complete mapped-SDR workload results remain historical evidence.
