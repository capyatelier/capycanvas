# HDR master and output previews

Follow-up to the report that Export still displayed an SDR substitute for the
HDR master. Work is local to `~/code/capycanvas3`; no push is authorized.

## Corrected behavior

The previous comparison explicitly called the SDR document-preview path and
encoded every image into 8-bit SDR, hiding the master column for HDR delivery.
That implementation could not display HDR regardless of monitor capability.

Export now keeps the master visible beside the output. On a capable GTK GPU
renderer it shows **HDR master** and either **HDR output** or **SDR output**.
The master follows the canvas's HDR display shoulder and reference white, while
ignoring temporary canvas Preview SDR/proof toggles and saved SDR-appearance
settings. SDR delivery continues to use the saved rendition and ICC transform.

The HDR output preview simulates the actual resized PQ RGB and alpha codes
before reduction. Its range statistics gate strict delivery; only the explicit
clipping choice permits publishing out-of-range artwork. Preview clipping never
changes the master or enables strict export. The same encoder generates the
preview codes and the PNG writer's codes.

Linear composition and display mapping retain floating values. The final HDR
thumbnail is a tagged Rec.2100-linear half-float GTK texture, with RGB 1 =
203 cd/m², matching the canvas's display precision. GSK receives above-white
values instead of an 8-bit SDR image. Float32 processing and native artwork
storage are unchanged. The display derivative limits colors to PQ's signal
volume, as the canvas does; this is not an editing or export conversion.

When HDR is unavailable, labels explicitly say **SDR preview** and the saved
rendition is used. A bounded 250 ms capability check refreshes an open comparison;
closing it stops that check. One worker and one replaceable pending request
remain the limit. Cancellation is checked by the row provider, and the HDR
output simulation retains bounded rows, a 65,536-entry transfer table and the
thumbnail, rather than allocating an output-size image.

## Validation

Evidence is under `artifacts/color-m4/`, prefixed `hdr-preview-`.

- `shared-tests.log`: 168 core/color tests passed; seven pre-existing ignored
  cases were not run. The new codec test compares preview values with actual
  written PNG codes, checks alpha and area reduction, range counts and cancellation.
- `gpu.log`: four HDR GPU tests passed. The snapshot test verifies signed,
  above-white and translucent master values and independence from the saved SDR
  rendition. The CPU display mapping is checked against PQ/scRGB GPU presentation.
- `sdr.log`: native SDR-display fallback and master preservation passed.

- `desktop/test-qualified.log`: actual HDR desktop. The master texture contains
  linear sRGB `[4.470145, 1.470159, 0.720345, 1]`; the scaled GTK/GSK image
  contains `[4.444563, 1.444777, 0.694942, 1]`. The difference is interpolation
  of the transparency checker. Both retain above-white values. Wayland trace
  confirms the GTK parent surface uses BT.2020 PQ. SDR output stays below one;
  capability-loss/restoration injection refreshes both previews and labels.
- `native-results.json`: HDR edit/save/reopen/delivery, strict range gating,
  explicit clipping, export resizing and cancelled-dialog release, and mapped
  proof/history/save/reopen/export all passed.

- `60mp-final.*`: the retained 8192 × 7324 ProPhoto half-float document with
  the long effect chain completed its preview/range check in 8,959 ms. A 10 ms
  GTK heartbeat ran 834 times; the largest interval was 54.85 ms. Rapidly
  replacing 16 requests and cancelling completed in 152.34 ms, with the master
  revision unchanged. Peak RSS was 1352.9 MiB; per-process GPU allocation sampled
  about every 0.5 s peaked at 2082 MiB. Sampling can miss transient graphics
  peaks. This virtual 4K/120 Hz run measures the asynchronous preview workload,
  not HDR luminance or physical input latency. Full-size preview can take seconds;
  the heartbeat result does not prove uninterrupted 120 Hz responsiveness.
- `workspace-check-final.log`: all native workspace targets compile. Release
  app and test builds are recorded in `build-final.log` and
  `native-build-final.log`.

Initial native test assumptions were corrected: GTK needs a rendered frame
before widget readback, scaled checker colors may interpolate, and the nominal
60 MP fixture contains 59,998,208 pixels. The retained failed runs do not indicate
clipped HDR values; the successful runs above include the corrected assertions. Screenshots are SDR captures and cannot establish emitted
HDR luminance; floating texture readback and Wayland negotiation provide separate
transport evidence. Calibration and physical mixed-monitor movement remain
unqualified.

## Color picker assessment

The requested comparison with other editors and three interaction choices are
in [HDR picker options](../development/color-management-hdr-picker-options.md).
The current picker has not been changed while these alternatives await feedback.
