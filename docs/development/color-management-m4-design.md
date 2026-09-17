# HDR phase 4: GTK implementation and qualification

Implementation/review contract, 2026-09-17. Base: origin/main `173f760f`. The base fails to
compile GTK after a shared workflow signature change; `1c0f531f` supplies the
missing source memory budget and is the runnable parent baseline.

## Current-code audit

- Document/source depth admits only U8/U16. Descriptors do not distinguish float
  from integer; native writeback clamps RGB to SDR before quantizing. HDR is not
  enabled by existing Float32 working textures.
- Native edit publication already has whole-batch validation, bounded 16-tile
  work, immutable backing shared by history/save, and recovery checkpoints.
- Shared Float32 compositing/effects preserve extended RGB in the native path.
  Individual operations and physical pass boundaries still need qualification.
- PNG PQ/HLG, gain-map JPEG and HDR HEIF/AVIF explicitly fail. Existing row-based
  PNG IO is bounded and supplies a practical interoperable HDR route.
- GTK describes only SDR P3/sRGB surfaces. ICC proof LUTs are bounded to SDR.
  Neither is an HDR presentation/mapping implementation.

## Implemented contracts

The implementation uses display-referred linear half-float RGB, fixed portable reference white of
203 cd/m² (RGB 1), with Float32 processing and premultiplied working surfaces.
Stored RGB is straight, finite, within [-65504,65504]; coverage is half-float
[0,1]. Zero coverage is canonical transparent black on edit publication. Retained
source samples may contain hidden RGB. Scalar masks/wetness stay UNORM16.
Round to nearest even only at the publication boundary. Overflow/nonfinite
results fail the whole edit; they must not silently saturate. Subnormal half
samples survive exact persistence. No full Float32 document mode.

Rename the shared sample-depth vocabulary to include F16, and explicitly tag
float descriptors. Existing SDR descriptors retain their serialized identity.
Reuse bounded source decoding and native publication, with exact undo/backing.

Initial HDR interchange: 16-bit PNG with full-range RGB cICP PQ, supported
sRGB/P3/BT.2020 primaries, normalized from absolute luminance to 203 cd/m².
Output is explicitly BT.2020 PQ with matching cICP, after validation of PQ's
nonnegative 0–10000 cd/m² range. Unsupported range requires deliberate mapping,
not hidden clipping. SDR delivery retains profiled PNG/JPEG/TIFF and applies the
saved rendition before output conversion. Gain maps and HLG remain explicit
unsupported inputs; no stale-map reuse. Reference: [PNG3](https://www.w3.org/TR/png-3/)
and [ITU BT.2408](https://www.itu.int/dms_pub/itu-r/opb/rep/R-REP-BT.2408-8-2024-PDF-E.pdf).

Saved SDR rendition and temporary SDR preview are separate. A shared deterministic
mapping feeds SDR presentation, mapped proof and SDR delivery. Native display
capabilities determine presentation; missing headroom never changes artwork.
HDR numeric entry, scene-linear exposure/curves, samples and stop-based histograms
must expose values above reference white and negatives.

## Numerical and qualification gates

- Persistence/recovery/undo: exact committed half bytes and metadata.
- Publication: nearest-even half reference (<= half an ULP, including subnormals),
  canonical Float32 cache equals decode(stored result). No partial overflow edit.
- Arithmetic/effect references: <= 2e-5 relative + 2e-6 absolute before final half
  quantization unless a stricter existing operation-specific tolerance applies.
- PQ: independent double-precision reference, <= 1 output code for 16-bit encoding;
  import additionally permits the declared half rounding error.
- CPU/GPU SDR mapping: <= 2e-5 relative + 2e-6 absolute; proof input equals the
  authored SDR export input before destination profile conversion.
- Preserve existing SDR tests and common-gate parent/fixed-baseline comparisons.
  Capture hardware/driver/build identities and native workflow/latency/memory.
  Device budgets and missing cross-platform/hardware qualification remain explicit;
  an absent measurement is not a pass.

RAW, layered PSD, native CMYK documents, OCIO/ACES, scene-based VFX and full
Float32 documents are excluded. No push before user feature review.

## Linux qualification budgets (declared before HDR timing)

Reference device: RTX PRO 6000 Blackwell Max-Q, 97,887 MiB VRAM, driver
610.57.04, 250 W power limit, AC workstation, Fedora 44. This is not constrained
or unified-memory hardware. Use an isolated 3840×2160 120 Hz Wayland output at
scale 2; no simultaneous compilation or benchmark GPU workload.

Retain the M2/M3 fixed baseline and the fresh runnable parent. Unchanged SDR
worker p95/p99 regressions above max(5%, 0.2 ms) require investigation/repeats.
HDR adds justified work: ordinary 24/45/60 MP navigation targets worker CPU/GPU
p99 <= 2 ms, request-to-present p99 <= 20 ms, first response <= 50 ms and <= 1%
missed 120 Hz slots over each 960-request run. Cold readiness <= 15 seconds.
Long-chain/physical-effect/concurrent-delivery stress targets p99 <= 33.4 ms,
<= 2% missed slots, and startup <= 30 seconds. Drawing targets worker CPU/GPU
p99 <= 3 ms and host-backed commit observation <= 35 ms. These are software
request/feedback observations, not physical input-to-photon guarantees.

60 MP ordinary HDR process peak <= 2 GiB, accounted renderer steady <= 2 GiB;
concurrent save/reopen/export peak <= 3 GiB and renderer <= 2 GiB. Sparse 4K
drawing with 32 layers and 12 retained strokes peak <= 1 GiB. Report process
RSS/high-water and actual driver GPU residency separately from renderer-owned
allocations; driver and compositor storage is not part of the Rust accounting.
Compare sustained repeated runs and recorded GPU temperatures/power/throttle
status; no laptop/mobile thermal qualification is inferred from this workstation.
Other-device budgets and HDR qualification remain outstanding, and their HDR
editor adoption is rejected explicitly until integrated and measured.

Results and remaining qualification: [GTK M4 validation](../history/color-management-gtk-m4-validation.md).
