# Android HDR display and footer follow-up — 2026-09-19

**Current policy:** the [Android-managed PQ follow-up](#android-managed-pq-follow-up)
supersedes the extended-linear headroom policy below. Earlier measurements are
retained as investigation history.

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
The ordinary Compose controls remain SDR. The follow-up below corrects the
near-SDR policy and verifies actual floating-point pixels.

Off presents HDR when available. SDR, Print, gamut warning and temporary SDR
appearance previews use one-times headroom and the existing shared mapping.
Opening a new document and replacing the GPU retain the negotiated encoding.
Display changes invalidate an idle frame without changing the artwork or its
history. JNI diagnostics distinguish selected format/encoding, current policy
and the headroom of the last submitted frame.

Web and Android place a compact display button on the left of the footer.
Padding, text and rounded background match the zoom/rotation readout on the
right. Clicking/tapping opens Display Details, following GTK. Android reports
**Showing SDR** below 1.05×, retaining the authored SDR appearance. Display
Details shows the actual reported ratio and the switching threshold.
Web still reports **Showing SDR** and explicitly explains its SDR canvas.

Older Android versions, missing headroom APIs and devices lacking the matching
Vulkan format/encoding use the existing mapped SDR path. Advertising an HDR
video decoder alone does not qualify the canvas. External-display changes,
physical luminance/colorimetry and sustained thermal behavior remain unqualified.

## Device-side limit

On the attached tablet, SurfaceFlinger confirms a 16-bit float canvas in
`V0_SCRGB_LINEAR`, and DisplayManager reports `mIsHdrLayerPresent=true`,
`mHdrVisible=true` and HDR high-brightness mode. The test artwork requests
**4.926108×** (the stored endpoint is 2.300448 stops), but Android grants only
**1.004000×**. The earlier integration mistakenly sent stops as a ratio. Both SDR and Print return
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


## Canvas, Navigator and layer-thumbnail follow-up

The reported picker/canvas mismatch was real, but the picker was an SDR
`ARGB_8888` bitmap. It did not establish physical HDR output. At 1.004×, the
canvas and Navigator abandoned the saved SDR rendition for the shared HDR
shoulder, with almost no additional brightness available. Android now retains
the saved SDR appearance below 1.05×. This is an explicit host presentation
policy, not a change to the shared GTK shoulder or saved rendition algorithms.
Useful reported headroom still selects the floating HDR path.

The deeper audit also found missing redraw publication: native Proof actions
ignored their `UiChange`, and completed tone analysis did not dirty an idle
canvas. Both now schedule presentation. Diagnostics include the tone-analysis
generation actually submitted to the surface, so a ready worker cannot be
mistaken for a displayed result.

Web and Android also omitted GTK's `set_ui_rendition` call for layer thumbnails
and filter previews. They now supply the shared SDR recipe before requesting
these byte previews. Painted and retained-photo thumbnails use that mapping;
masks remain neutral. A transient appearance draft invalidates thumbnail
revisions, and cancellation restores the original revision without editing the
master. Native cold-photo thumbnails use GTK's four-tile background batches
rather than scanning the whole photo on the render owner.

The float `PixelCopy` test uses `RGBA_F16`/linear extended sRGB. On the actual
1.004× feedback, canvas and Navigator contain no above-SDR samples. With
**simulated 4× feedback**, their surface buffers reach approximately **3.83×**
and **3.93×** respectively, including signed wide-gamut values. This verifies
above-white GPU presentation in both views; the simulated allowance does not
prove that the tablet emits those luminances. Real HDR brightness, the remaining
SDR Compose picker on a display with useful HDR headroom, and physical colorimetry
remain separate qualifications. Layer thumbnails intentionally match GTK's SDR
preview route, rather than claiming to be HDR surfaces.

Follow-up evidence and runnable builds are in
`artifacts/android-hdr-consistency/review/`. The renderer pixel oracle covers
painted and retained-photo thumbnails, changing/restoring the SDR recipe, and
unchanged master pixels. Host tests cover visible thumbnail updates through
undo/redo, tone-generation presentation, Proof, persistence and recovery. On this
limited display, float captures of Proof Off and SDR match exactly in both the
canvas and Navigator after the tone analysis has actually been presented.


The follow-up 24 MP native run completed all three injected contacts (pen,
touch, pen), concurrent save/reopen and the histogram/export/Open cancellation
checks. Readiness was 6.856 s; sampled peak process PSS was 711,104,512 bytes
(678 MiB), and tracked canvas residency was 647,978,412 bytes (618 MiB).
Cold UI heartbeat maximum was 148 ms; the later maximum was 76 ms. Histogram,
export and Open cancellation took 297, 288 and 2 ms. Save took 55 ms. Input CPU
p99 was 0.040 / 0.050 / 0.061 ms and render-owner queue p99 was 9.37 / 0.57 /
8.76 ms. These are injected host timings, not physical pen latency. GPU timer
samples were unavailable in this run. This successful limited-headroom run
does not establish why the earlier injection attempts failed or qualify
sustained physical HDR/thermal behavior.


## Android-managed PQ follow-up

At the user's request, HDR Off now sends the artwork to Android's tone mapper
without the app's headroom shoulder or automatic SDR fallback. The shared Rust
presenter explicitly selects compositor-managed HDR only on a PQ surface. The
existing linear/scRGB shoulder and Web/GTK SDR and print algorithms are unchanged.
The shared PQ encoder converts working primaries to BT.2020 with artwork RGB 1
at 203 cd/m². Output is bounded to PQ's 0–10,000 cd/m² signal range and BT.2020
channels; this display derivative never changes the editable HDR master.

Android 15+, HDR10/HDR10+ display support and an exact float/PQ Vulkan pair are
required. `SurfaceView.setDesiredHdrHeadroom(0)` restores Android's automatic
brightness policy. Missing ratio feedback is no longer an HDR capability veto.
Proof SDR, Print, gamut warnings, temporary appearance drafts and SDR documents
select an sRGB surface and the existing SDR mapper, with headroom request 1.
A display capability loss also returns to SDR. Canvas and Navigator use the
same presenter and surface. Float-format capability is retained across display
changes; document adoption and GPU recovery retain the chosen encoding.

The footer reads **HDR · Android managed** and explains possible brightness
changes, reference white, output bounds and the remaining SDR Color controls /
layer thumbnails. The reported ratio is diagnostic, not the app's pixel limit.
We do not send separate mastering/MaxCLL metadata; Android uses PQ's absolute
encoding and its platform tone-mapping defaults. This route intentionally allows
OS brightness/tone changes, rather than promising GTK's exact physical appearance.

The Wacom display regression passes with real `BT2020_PQ` / `RGBA16161616F_UBWC`
confirmed by SurfaceFlinger. The automatic request is 0 and reported headroom
remains 1.004×. Android's float PixelCopy captures contain above-white samples
in both canvas (maximum 1.2783) and Navigator (1.53125), with identical digests
under simulated 1× and 4× feedback. PixelCopy itself converts the PQ signal;
these samples are not a luminance measurement. SDR proof switches to `V0_SRGB`
and both captures are bounded to 1.0. GPU recovery returns to PQ.

In this capture SurfaceFlinger reports a 400-nit white point and dimmingRatio 1
for both layers; additional SDR UI dimming was not reported. This is not a
promise that Android will dim, nor proof of emitted brightness. The user is
reviewing physical appearance on the deployed tablet before this presentation
policy is considered visually qualified.

The hardware GPU oracle checks full-range PQ against independent Float64 math
for F16/F32, sRGB/ProPhoto, alpha, negative/bright samples, different SDR recipes,
retained viewing captures, normal HDR shoulders and explicit proof. Local review
APKs, raw captures and workflow logs are under `artifacts/android-hdr-compositor/review/`.

References: [SurfaceView automatic headroom](https://developer.android.com/reference/android/view/SurfaceView#setDesiredHdrHeadroom(float)),
[Android mixed SDR/HDR composition](https://source.android.com/docs/core/display/mixed-sdr-hdr),
[Android tone mapping](https://source.android.com/docs/core/display/tone-mapping).

The existing Android HDR editing/delivery/recovery and SDR/Print portability
workflows also pass, including pen painting, touch/pen cancellation, exact undo,
native save/reopen, EXR/PQ/SDR delivery and Activity/GPU recovery. The headed Chrome
HDR workflow passes after the shared shader change. This review does not rerun
the 24 MP sustained workload or establish a new thermal/colorimetry qualification.
