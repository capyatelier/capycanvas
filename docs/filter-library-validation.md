# Forty-filter validation

2026-09-10. The original ten plus thirty additional original WGSL filters are
registered in the shared core and validated in GTK. Web/Android already consume
the shared catalog/property model, but their categorized preview-picker ports
and expanded device tests remain outstanding at this milestone.

## Correctness and UI

- 48 GPU tests pass; four release-mode benchmarks are opt-in. All forty
  algorithms pass WGSL validation and pixel tests with transparency, zero
  opacity, zero masks, clipping, frozen time and animation where applicable.
- Every filter's local paint update equals a genuinely uncached full rebuild,
  not the existing result cache. A five-expensive-filter chain also matches at
  document corners and a paint-tile boundary.
- Edits above a filter, below its clipping base, and outside its isolated group
  do not update that filter's input or result. Tests compare the untouched cache
  with a forced rebuild afterward.
- Every numerical parameter of the thirty additions is tested at both limits;
  changing values never recompiles its shader. The core validates footprints.
- All forty picker previews match their corresponding full-resolution canvas
  pixels within one output byte, including multi-pass and document remapping.
- The GTK integration inserts all forty filters, edits their generated controls,
  exercises category/search and Stats, and captures screenshots. Inspected the
  rendered catalog contact sheet and representative GTK picker/property pages.
  Short percentage ranges retain slider controls instead of count spin buttons.
- Shared core/engine/UI tests pass, native clippy is clean and wasm compiles.
  These are not yet expanded web/Android UI or end-to-end refresh-rate claims.

Generated images, not checked into git:
`artifacts/filter-library/contact-sheet.png` (five columns, eight rows; names in
`order.txt`), individual named PNGs alongside it, and GTK screenshots in
`artifacts/ui/adjustments-gtk/`.

## Work avoided and overhead

Pointwise programs stay fused in dirty scene tiles. Neighborhood programs keep
GPU-resident input/result checkpoints and redraw a contiguous dirty rectangle
expanded by current parameter-derived support, including every downstream pass.
Shared intermediate images populate the additional input halo required by later
passes. This deliberately uses a few region draws, not a dispatch or mask per
pixel/tile. It may conservatively include untouched pixels inside the rectangle.
Global remapping declares whole-document dependency; changes of filter parameters
or time invalidate its output, not unchanged source pixels.

Layer/group/clipping dependency indices rebuild only on structural edits. No
readback, bitmap scan or allocation of image-sized memory occurs during warmed
updates. This is shared renderer behavior, not GTK-specific policy.

Broad-edit measurements exposed redundant tile render passes and copies while
capturing filter inputs. The compositor now redirects write-only tile output
into the resident input image, batching adjacent draws into one render pass.
Read/modify/write tiles retain the existing composition path. This is a generic
job optimization, not a separate algorithm for particular filters.

For the same five-filter stress stack, completion medians before → after:
full **7.31 → 4.01 ms**, local **0.330 → 0.217 ms**, broad **9.04 → 5.68 ms**.
Incremental filtering shades 71,296 pass-pixels versus 100,663,296 for a full
update (about **0.071%**). Gaussian alone shades 10,244 versus 33,554,432.
Frozen cached frames perform zero image passes. Tracking is included in the
measurements; no speculative tile subdivision or GPU work-compaction pass is added.

## Workstation measurements

Release Vulkan, NVIDIA RTX PRO 6000 Blackwell Max-Q. Resident 4096×4096 RGBA8
artwork. Twenty-four warm-ups and ninety-six measured updates per case. Entries
are **median / p95 / p99 milliseconds**. Completion includes CPU preparation,
submission and an explicit GPU wait **only in the benchmark**; it is not display
presentation. GPU timestamps bracket commands. Occasional CPU-side wall-time
outliers are retained, not discarded or attributed to a shader without evidence.
Vignette's local run contains one 21.20 ms CPU-side outlier despite a 0.020 ms
GPU p99; this is not a guarantee that every interactive frame finishes in 8.33 ms.
Cold compilation/setup is excluded here
and is reported separately in the raw CSV.

Local: 24px dab with 26×26 damage crossing tile boundaries. Full: identical dab,
forcing whole-document damage while retaining allocations/pipelines. Broad:
3600px dab, including its rasterization cost. Thus local/full isolate region
tracking, while broad measures a realistic heavy brush plus filtering.

| Filter | Local completion | Full completion | Full GPU | Broad completion |
| --- | ---: | ---: | ---: | ---: |
| Baseline | 0.085 / 0.097 / 0.123 | 0.249 / 0.256 / 0.741 | 0.094 / 0.095 / 0.096 | 1.635 / 1.738 / 1.829 |
| Curves | 0.103 / 0.118 / 0.180 | 0.954 / 1.373 / 2.881 | 0.230 / 0.239 / 0.246 | 2.400 / 3.560 / 4.051 |
| Levels | 0.106 / 0.116 / 0.294 | 0.910 / 1.386 / 3.014 | 0.185 / 0.186 / 0.197 | 2.334 / 2.527 / 2.631 |
| Brightness / Contrast | 0.110 / 0.122 / 0.329 | 0.836 / 0.930 / 2.778 | 0.154 / 0.155 / 0.171 | 2.363 / 3.439 / 3.762 |
| Hue / Saturation | 0.103 / 0.126 / 0.189 | 0.892 / 1.024 / 2.866 | 0.196 / 0.197 / 0.201 | 2.408 / 3.554 / 3.641 |
| Color Balance | 0.110 / 0.128 / 2.104 | 0.881 / 0.966 / 1.162 | 0.200 / 0.202 / 0.208 | 2.439 / 3.608 / 5.336 |
| Exposure | 0.103 / 0.168 / 2.060 | 0.813 / 0.883 / 0.969 | 0.130 / 0.133 / 0.145 | 2.445 / 3.282 / 3.510 |
| Vibrance | 0.103 / 0.110 / 2.044 | 0.882 / 0.899 / 1.061 | 0.200 / 0.201 / 0.211 | 2.491 / 3.538 / 3.888 |
| Black & White | 0.102 / 0.121 / 2.153 | 0.863 / 0.932 / 0.998 | 0.177 / 0.178 / 0.190 | 2.470 / 3.687 / 4.422 |
| Gradient Map | 0.102 / 0.107 / 0.318 | 0.854 / 0.877 / 1.473 | 0.171 / 0.174 / 0.183 | 2.456 / 3.670 / 4.212 |
| Posterize | 0.106 / 0.130 / 0.302 | 0.854 / 1.040 / 2.068 | 0.161 / 0.162 / 0.178 | 2.476 / 3.281 / 4.757 |
| Gaussian Blur | 0.134 / 0.163 / 0.169 | 1.272 / 1.349 / 1.460 | 0.584 / 0.591 / 0.594 | 2.863 / 4.463 / 4.760 |
| Unsharp Mask | 0.134 / 0.145 / 0.223 | 1.180 / 1.291 / 1.730 | 0.478 / 0.481 / 0.482 | 2.920 / 4.185 / 4.562 |
| High Pass | 0.156 / 0.317 / 0.474 | 1.396 / 1.546 / 1.814 | 0.687 / 0.695 / 0.695 | 3.152 / 4.259 / 4.524 |
| Motion Blur | 0.127 / 0.137 / 2.060 | 1.134 / 1.280 / 1.491 | 0.442 / 0.443 / 0.444 | 2.591 / 4.164 / 5.284 |
| Denoise | 0.130 / 0.201 / 2.088 | 1.540 / 1.704 / 2.147 | 0.854 / 0.860 / 0.863 | 2.971 / 3.647 / 4.063 |
| Edge Detect | 0.129 / 0.140 / 2.452 | 1.124 / 1.236 / 1.674 | 0.422 / 0.423 / 0.423 | 2.532 / 2.798 / 3.225 |
| White Balance | 0.107 / 0.150 / 2.164 | 0.819 / 0.908 / 0.922 | 0.130 / 0.131 / 0.148 | 2.363 / 2.540 / 2.659 |
| Split Tone | 0.105 / 0.111 / 2.044 | 0.885 / 1.119 / 1.447 | 0.189 / 0.190 / 0.202 | 2.360 / 3.443 / 3.652 |
| Vignette | 0.106 / 0.170 / 21.196 | 0.818 / 0.905 / 1.242 | 0.132 / 0.132 / 0.136 | 2.393 / 3.687 / 4.877 |
| Film Grain | 0.125 / 0.182 / 2.027 | 0.974 / 1.054 / 1.088 | 0.292 / 0.293 / 0.294 | 2.671 / 4.142 / 4.601 |
| Bloom | 0.147 / 0.155 / 0.309 | 1.671 / 1.831 / 2.632 | 0.951 / 0.959 / 0.961 | 3.191 / 3.684 / 3.770 |
| Soft Focus | 0.139 / 0.151 / 0.158 | 1.540 / 1.745 / 3.001 | 0.797 / 0.813 / 0.817 | 3.057 / 4.672 / 4.853 |
| Halftone | 0.125 / 0.134 / 0.204 | 0.994 / 1.094 / 2.709 | 0.295 / 0.296 / 0.296 | 2.505 / 2.897 / 3.476 |
| Crosshatch | 0.104 / 0.110 / 0.127 | 0.977 / 1.057 / 1.772 | 0.288 / 0.289 / 0.299 | 2.506 / 2.767 / 2.872 |
| Emboss | 0.126 / 0.134 / 0.143 | 0.996 / 1.103 / 3.328 | 0.283 / 0.284 / 0.284 | 2.492 / 2.675 / 3.114 |
| Pixel Mosaic | 0.127 / 0.142 / 0.149 | 0.949 / 1.016 / 1.064 | 0.230 / 0.231 / 0.231 | 2.438 / 2.852 / 2.980 |
| Chromatic Aberration | 0.130 / 0.161 / 0.184 | 0.971 / 1.135 / 1.739 | 0.257 / 0.258 / 0.258 | 2.505 / 2.800 / 3.490 |
| Painterly | 0.133 / 0.142 / 0.200 | 1.827 / 2.077 / 2.364 | 1.070 / 1.078 / 1.083 | 4.159 / 5.671 / 6.580 |
| Solarize | 0.127 / 0.151 / 0.265 | 1.104 / 1.182 / 1.317 | 0.150 / 0.152 / 0.168 | 2.419 / 3.552 / 4.014 |
| Pencil | 0.135 / 0.265 / 0.662 | 1.197 / 1.274 / 1.430 | 0.509 / 0.511 / 0.512 | 2.866 / 3.565 / 3.623 |
| Kaleidoscope | 0.241 / 0.262 / 0.410 | 0.957 / 1.035 / 1.092 | 0.261 / 0.262 / 0.263 | 2.578 / 3.828 / 4.453 |
| Swirl | 0.242 / 0.261 / 0.327 | 0.937 / 1.013 / 1.074 | 0.248 / 0.249 / 0.250 | 2.723 / 3.646 / 3.981 |
| Ripple | 0.123 / 0.142 / 0.408 | 0.939 / 1.085 / 1.436 | 0.246 / 0.248 / 0.248 | 2.594 / 3.990 / 4.615 |
| Glass | 0.126 / 0.140 / 2.081 | 1.039 / 1.144 / 1.180 | 0.337 / 0.338 / 0.339 | 2.685 / 3.689 / 3.831 |
| Rainy Glass | 0.155 / 0.239 / 2.181 | 1.217 / 1.300 / 1.420 | 0.272 / 0.275 / 0.279 | 3.242 / 4.492 / 4.967 |
| VHS | 0.155 / 0.192 / 0.260 | 1.243 / 1.726 / 3.037 | 0.276 / 0.282 / 0.283 | 2.550 / 2.930 / 4.140 |
| CRT | 0.270 / 0.346 / 0.444 | 0.981 / 1.168 / 1.303 | 0.271 / 0.272 / 0.275 | 2.520 / 2.895 / 3.548 |
| Heat Haze | 0.132 / 0.161 / 2.267 | 1.055 / 1.131 / 1.280 | 0.350 / 0.351 / 0.353 | 2.597 / 3.267 / 4.300 |
| Iridescence | 0.152 / 0.160 / 0.169 | 1.323 / 1.504 / 3.501 | 0.329 / 0.332 / 0.334 | 2.544 / 2.804 / 3.059 |
| Domain Warp | 0.127 / 0.133 / 0.138 | 1.512 / 1.763 / 2.023 | 0.747 / 0.751 / 0.753 | 3.073 / 4.354 / 4.554 |
| Five expensive | 0.217 / 0.224 / 0.234 | 4.015 / 4.518 / 4.766 | 3.026 / 3.082 / 3.095 | 5.681 / 7.590 / 7.891 |

The stress stack, bottom to top, is Motion Blur → Gaussian Blur → Domain Warp →
Painterly → Denoise. All full/broad p99 values in this final run fit 8.33 ms;
this workstation result does not promise every frame on every device. Nonlocal
filters have real execution and memory costs and are not described as free.

### Animation

Only time changes, at 1/120-second intervals. In the stress stack only Domain
Warp animates; the two stages beneath it stay cached. No source recapture.

| Filter / stack | Completion | GPU |
| --- | ---: | ---: |
| Film Grain | 0.230 / 0.239 / 0.304 | 0.160 / 0.161 / 0.161 |
| Ripple | 0.177 / 0.200 / 0.248 | 0.110 / 0.110 / 0.110 |
| Rainy Glass | 0.220 / 0.274 / 0.590 | 0.135 / 0.135 / 0.136 |
| VHS | 0.211 / 0.237 / 1.992 | 0.140 / 0.141 / 0.142 |
| CRT | 0.202 / 0.214 / 0.267 | 0.133 / 0.134 / 0.134 |
| Heat Haze | 0.293 / 0.309 / 0.377 | 0.208 / 0.208 / 0.209 |
| Iridescence | 0.258 / 0.277 / 0.489 | 0.188 / 0.189 / 0.189 |
| Domain Warp | 0.680 / 0.741 / 2.103 | 0.599 / 0.605 / 0.608 |
| Five expensive | 2.543 / 2.855 / 3.177 | 2.355 / 2.610 / 2.619 |

### Memory

At 4096² each RGBA8 image is 64 MiB. An isolated one-pass image filter needs an
input and output (128 MiB); a two-pass effect adds one reusable intermediate
(192 MiB). Scratch is high-water reused across effects while image stages exist.
Adjacent compatible stages alias outputs as inputs. The five-filter stack uses
448 MiB for these caches/scratch, rather than separate input/output pairs for
every filter. Masks add R8 images only when needed. Preview/cache and paint
storage are additional, reported by Stats. Mobile memory and device performance
still need the expanded platform validation; this does not introduce per-frame
allocation of those images.

### Reproduce

```sh
cargo test -p layer-render-wgpu --lib -- --test-threads=1
cargo test --release -p layer-render-wgpu filter_library_latency -- --ignored --nocapture --test-threads=1
```

Run benchmarks alone. Raw output is generated/ignored at
`artifacts/benchmarks/filter-library.csv` (CPU, completion and GPU percentiles,
image pass-pixels, cache bytes and cold setup).
