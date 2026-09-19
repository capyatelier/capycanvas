# Contrast × Scale — SDR Proof review

September 2026. Replaces the Strength/Balance mapping in the
[circular Proof control](color-management-proof-dial.md). GTK retains the same
circle, brightness/color arcs, small curved values, keyboard behavior and reset.
The center is now **100% contrast, balanced macro/micro**, with no labels or Auto
button. Reset centers the control; each gesture remains one document Undo.

## Why the mapping changed

The old Strength coordinate increased local illumination compression while also
reducing the range used by the final SDR shoulder. Those two changes competed;
perceived contrast could peak partway through the control. Centered Balance did
not increase texture at all. The new contrast control changes neither the
baseline compression nor its output shoulder.

## One shared formula

Let `x` be horizontal balance and `y` vertical log contrast, both in `[-1,1]`:

```text
C = 2^y                         # 50% bottom, 100% center, 200% top
macro_gain = C × 2^(−x/2)
micro_gain = C × 2^( x/2)
```

The existing bounded, document-space fast local-Laplacian analysis separates HDR
log luminance `L` into broad illumination `B_hdr` and residual `L − B_hdr`.
A fixed baseline compresses broad illumination by 0.6 around linear 18% gray:

```text
P = log2(0.18)
broad = P + 0.4 × (B_hdr − P)
H_base = max(0, 0.4 × saved_HDR_range + 0.6 × P)
Y0 = BT2390(2^(broad + L − B_hdr), H_base)
B = logit2(BT2390(2^broad, H_base))
u = logit2(Y0)
D = u − B
u_out = logit2(0.18) + macro_gain × (B − logit2(0.18))
        + micro_gain × D + Brightness
Y_out = 2^u_out / (1 + 2^u_out)
```

`B` is the existing edge-preserving HDR illumination transformed through the
fixed baseline, rather than a second analysis of the rendered SDR thumbnail.
This approximation reuses the same full-document guide, has no additional image
scan or guide storage, and reconstructs the baseline exactly at the center.
The default baseline retains the previously reviewed local default treatment;
the visual default remains subject to user feedback.

Black and white have explicit endpoint handling. Only the macro guide's logit
input is bounded away from endpoints (2^-24); this is derived analysis, not
master clipping. Intermediate/output calculations remain Float32. The output
luminance scales straight RGB together before destination-gamut mapping; alpha
remains coverage. Color intensity and print ICC preparation use the existing
shared gamut policy. Gain maps are regenerated from the actual delivered SDR
base and the unchanged edited HDR master, including JPEG base quantization.

At any fixed balance, vertical movement scales log-odds distance from the same
pivot. This is a precise contrast-direction guarantee before gamut mapping;
it is not a claim that every perceptual contrast metric increases identically
on every scene. Horizontal movement changes the ratio of macro/micro gain while
preserving their geometric mean. Extreme micro contrast can emphasize noise or
edge artifacts; the deliberately limited 0.5–2 overall range and balance gains
of 0.71–1.41 keep this bounded.

## Simplification requested for prerelease

There is one saved SDR recipe: Brightness, Contrast, HDR range, Color intensity
and Balance. Removed the old method enum and Browser/Photographic/global mapper
branches, Tone/Detail fields, Highlights field, Knee migration, version-preserving
renderer paths, and GTK custom-recipe/dash fallback. The native document uses the
current default when no SDR recipe exists. Unknown JSON fields follow normal
Serde handling; there is no old-algorithm restoration. Old prerelease SDR
appearances can change. Old out-of-range contrast values are rejected rather
than silently clamped. Working master samples, layers and alpha are unaffected.

Rust owns recipe validation, pad coordinates, gains, geometry and CPU mapping.
The Float32 WGSL counterpart is validated against it. Native delivery, ICC print
preparation, actual gain-map bases and all snapshot previews call the same
spatial mapper. Isolated swatches, which lack a neighborhood, use its homogeneous
illumination estimate. Ordinary SDR documents retain their existing pipeline.

## Validation and limitations

Evidence: `artifacts/color-m4/contrast-dial/`. The prior build and measurements
remain under `proof-dial/` for comparison.

- Shared tests: 93 core, 84 color and 477 UI tests pass; 7 tests requiring external
  ICC fixtures remain explicitly ignored. Control mapping sweeps all 40,401
  snapped combinations; checks baseline identity, reciprocal scale gains,
  monotonic vertical response, alpha, serialization and working-space invariance.
- The BT.2390 stage retains an independent Float64 reference test. GPU tests
  compare both point and spatial mapping against CPU for multiple primaries,
  surface formats, HDR headroom and mapped print proofing; master pixels remain
  unchanged. Endpoint-adjacent contrast assertions compare linear output, since
  inverse log odds magnifies Float32 quantization near white.
- Native workflows exercise all five real HDR photos, cardinal circle positions,
  both arcs, compact/floating layouts, one-step Undo and Escape cancellation,
  exact guide reuse, save/reopen, SDR/HDR delivery and device failure/recovery.
- Actual JPEG and alpha AVIF gain maps reconstruct the HDR master from both
  low/macro and high/micro SDR bases. Maximum sampled absolute linear RGB error:
  JPEG 0.056251, AVIF 0.001563. These are small-fixture measurements, not natural-
  image worst-case error bounds.

On Fedora 44, NVIDIA RTX PRO 6000 Blackwell Max-Q / 610.57.04, bundled GTK 4.22.4,
and isolated Mutter 1600×1000 at 120 Hz, the retained 8192×7324 ProPhoto F16 / 20
effects document produced these single-run results:

| Workload | Time | Largest 10 ms heartbeat gap | Peak sampled process-tree RSS | Peak GPU residency |
|---|---:|---:|---:|---:|
| 60 control updates + event pumping | 1,185 ms | 26.23 ms | 1,368,680 KiB | 1,889 MiB |
| SDR export preview at 200% contrast / micro +100% | 14,454 ms | 38.62 ms | 1,457,560 KiB | 2,149 MiB |

Cancellation after rapid export-choice changes took 143.05 ms. Guide analysis was
reused throughout control adjustment, Undo restored the recipe, and the master
layers stayed unchanged. Sampling at 0.5 s can miss brief allocation peaks;
these are not input-to-present, p95/p99, or final-file publication measurements.
No codec staging was observed in these workloads. Workspace check and the GTK
release build pass; existing unused-code warnings in Apple/Windows are retained.

Evidence: `shared-tests.log`, `gpu-local-final.log`, `gpu-point.log`,
`photos-final.log`, `hdr.log`, `gainmap.log`, `recovery.log`, `layout-final.log`,
`codec-contrast.log`, `60mp-controls-current.*`, `60mp-export.*`.
The old 60 MP fixture and original **HDR review.capy** contain the removed Knee
setting, so the first large run failed to read it. One-off review
copies replace only SDR settings; metadata outside that recipe and complete
HDR payload bytes are unchanged. Source/payload hashes and the reproducible
fixture script are in `fixture-record.json` and `refresh-review-fixtures.py`.
The default launcher opens **HDR review - Contrast.capy**; the original is kept.
This fixture preparation is not a restored production migration path.

The guide is still limited to a 768-pixel edge; the circle preview is a 128-edge
SDR crop. Full-resolution exports and canvas pixels remain Float32 processing of
the F16 master. No additional master precision reduction was introduced. Native
hardware qualification is limited to the recorded Linux/GPU environment.
Physical touch/pen, emitted HDR calibration, mixed monitors, mobile/low-memory
hardware and browser workflows remain unqualified by this GTK review. Earlier
browser workflow failures are not claimed fixed. Broader phase-4 gates remain
open; this review does not declare phase 4 complete.
