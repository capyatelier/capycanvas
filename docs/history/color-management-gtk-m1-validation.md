# GTK raster foundation validation

Work in progress. GTK is the only host being qualified; other host work requires
user approval. This record distinguishes measurements from design expectations.

## Baseline

Rendering baseline: `ebafa44` (2026-09-13), before changing production rendering.
Linux 7.1.10-200.fc44.x86_64, NVIDIA driver 610.57.04, Vulkan, RTX PRO 6000
Blackwell Max-Q Workstation Edition, PCI 0000:f1:00.0 (`LAYER_GPU_INDEX=0`).
GPU memory 97,887 MiB; configured power limit 250 W. Mains workstation; clocks
and concurrent desktop load are not locked. Offscreen measurements have no display
refresh or input-to-present result. A hardware presentation check is separate.

`cargo run --release -p layer-bench -- --scenario all --repeats 3` measures 25
4096² scenarios, at least 32 layers, eight coalesced samples/frame, with startup
excluded. Exact sample counts, submission p95, completion p50/p95/p99, pen-up p99,
work counts and renderer resident bytes follow. The existing harness labels its
120 Hz gate PASS despite several isolated >8.33 ms frames; this is a percentile
gate, not a claim that every deadline was met. Its canvas allocation counter does
not include all process, driver, source, history or staging memory.

# GPU raster benchmark — 4096×4096

Adapter: `NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition`; backend code 1; device type code 2.

Release build with debug symbols. Each frame submits eight simulated coalesced pen samples through the public C ABI, calls the ABI frame function, then waits for that submission to complete. Every scenario has at least 32 visible paint layers. Each repetition creates a fresh canvas, warms the exact scenario pipeline, undoes the warm-up stroke, and contributes every measured frame to the reported distribution. Setup, shader/pipeline creation, canvas allocation, scenario warm-up/undo, brush selection, layer creation, and PNG export are outside the timing window. Submit latency is the production non-blocking path; completed-work latency serializes each measured frame to isolate its GPU work. Concurrent system/GPU load is not controlled, so these are reproducible workload references rather than cross-machine scores.

| scenario | state features | repeats | frames | move completed p50 ms | move completed p95 ms | move completed p99 ms | pen-up completed p99 ms | max move ms | submit p95 ms | move/pen-up frames > 8.33 ms | dabs | conservative contact Mpx | composite visits Mpx | paint pages | coverage pages | material pages | preview pages | resident canvas MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| gpen_inking | existing path | 3 | 864 | 0.074 | 0.118 | 0.162 | 0.149 | 2.196 | 0.074 | 0/0 | 86856 | 22.0 | 12.5 | 125 | 0 | 0 | 0 | 95.3 |
| pencil_shading | existing path | 3 | 1620 | 0.070 | 0.108 | 0.289 | 0.457 | 4.298 | 0.056 | 0/0 | 33480 | 113.3 | 34.8 | 222 | 0 | 0 | 0 | 119.5 |
| large_eraser | existing path | 3 | 420 | 0.097 | 0.167 | 0.316 | 0.157 | 0.454 | 0.091 | 0/0 | 2676 | 303.9 | 166.7 | 224 | 0 | 0 | 0 | 120.0 |
| large_paintbrush | existing path | 3 | 594 | 0.114 | 0.279 | 0.439 | 0.158 | 3.377 | 0.160 | 0/0 | 918 | 309.8 | 423.1 | 181 | 0 | 0 | 0 | 109.3 |
| soft_airbrush | existing path | 3 | 195 | 0.140 | 0.245 | 3.430 | 0.151 | 6.777 | 0.159 | 0/0 | 4407 | 293.6 | 86.0 | 143 | 0 | 0 | 0 | 99.8 |
| anchored_grain_chalk | existing path | 3 | 339 | 0.092 | 0.168 | 0.430 | 0.131 | 7.672 | 0.105 | 0/0 | 10158 | 46.9 | 33.5 | 127 | 0 | 0 | 0 | 95.8 |
| flat_marker | existing path | 3 | 180 | 0.135 | 0.288 | 3.732 | 0.346 | 4.807 | 0.183 | 0/0 | 7956 | 484.5 | 85.5 | 144 | 0 | 0 | 0 | 100.0 |
| scatter_spray | existing path | 3 | 159 | 0.141 | 0.249 | 2.984 | 0.093 | 3.386 | 0.158 | 0/0 | 36189 | 29.1 | 78.5 | 125 | 0 | 0 | 0 | 95.3 |
| dual_texture | existing path | 3 | 180 | 0.099 | 0.177 | 2.597 | 0.125 | 4.096 | 0.110 | 0/0 | 468 | 31.2 | 40.4 | 59 | 0 | 0 | 0 | 78.8 |
| multiply_glaze | existing path | 3 | 135 | 0.314 | 0.528 | 2.287 | 0.358 | 2.441 | 0.291 | 0/0 | 3765 | 147.0 | 53.0 | 224 | 0 | 0 | 0 | 146.0 |
| smudge_pickup | smudge advection | 3 | 135 | 0.321 | 0.901 | 2.708 | 0.848 | 3.038 | 0.510 | 0/0 | 2361 | 68.4 | 18.1 | 224 | 0 | 39 | 0 | 132.2 |
| wet_round_oklab | reservoir + Oklab mixing | 3 | 135 | 1.533 | 2.320 | 3.126 | 1.554 | 3.606 | 1.393 | 0/0 | 5520 | 189.6 | 49.5 | 224 | 0 | 88 | 0 | 147.5 |
| liquify_push | bilinear deformation | 3 | 114 | 0.483 | 0.959 | 2.473 | 0.672 | 2.986 | 0.626 | 0/0 | 492 | 54.6 | 44.0 | 224 | 0 | 0 | 0 | 131.8 |
| liquify_twirl | bilinear deformation | 3 | 90 | 0.789 | 1.480 | 3.370 | 0.955 | 3.831 | 1.052 | 0/0 | 423 | 82.1 | 87.5 | 224 | 0 | 0 | 0 | 143.3 |
| layered_composite | existing path | 3 | 1380 | 0.096 | 0.277 | 0.577 | 0.231 | 21.676 | 0.162 | 3/0 | 85599 | 293.9 | 278.8 | 395 | 0 | 0 | 0 | 162.8 |
| textured_flat_filbert | advanced dry | 3 | 438 | 0.111 | 0.186 | 0.313 | 0.359 | 4.408 | 0.102 | 0/0 | 5352 | 214.6 | 170.2 | 192 | 0 | 0 | 0 | 112.0 |
| dry_scumble | coverage | 3 | 438 | 0.328 | 0.692 | 2.680 | 0.377 | 10.813 | 0.424 | 1/0 | 3147 | 160.5 | 184.9 | 192 | 132 | 0 | 0 | 161.5 |
| pastel_block | advanced dry | 3 | 438 | 0.105 | 0.164 | 0.230 | 0.149 | 3.990 | 0.087 | 0/0 | 7557 | 217.4 | 142.6 | 190 | 0 | 0 | 0 | 111.5 |
| transparent_glaze | wetness | 3 | 438 | 0.164 | 0.335 | 2.396 | 0.196 | 3.896 | 0.190 | 0/0 | 7251 | 789.6 | 355.8 | 201 | 0 | 152 | 0 | 123.8 |
| opaque_gouache | reservoir + wetness | 3 | 438 | 0.972 | 3.054 | 3.924 | 2.001 | 5.328 | 1.766 | 0/0 | 14979 | 698.1 | 199.4 | 192 | 0 | 126 | 0 | 151.4 |
| watercolor_wash_edge | coverage + R8 wetness + event-driven capillary transport + live edge | 3 | 438 | 1.979 | 3.857 | 4.475 | 2.927 | 32.239 | 2.393 | 2/0 | 7341 | 514.4 | 428.8 | 201 | 146 | 146 | 0 | 187.3 |
| wet_watercolor | coverage + R8 wetness + event-driven capillary transport + live edge | 3 | 438 | 1.688 | 3.472 | 4.041 | 3.076 | 21.885 | 2.184 | 2/0 | 7251 | 418.7 | 386.8 | 200 | 140 | 140 | 0 | 184.0 |
| loaded_oil_mixer | reservoir + wetness | 3 | 438 | 0.983 | 3.240 | 3.877 | 2.296 | 4.441 | 1.703 | 0/0 | 11673 | 941.5 | 290.6 | 199 | 0 | 144 | 0 | 158.8 |
| palette_knife | reservoir + wetness | 3 | 438 | 1.106 | 2.271 | 2.840 | 1.900 | 3.499 | 1.320 | 0/0 | 2790 | 1193.9 | 959.9 | 211 | 0 | 198 | 0 | 178.7 |
| natural_blender | smudge advection | 3 | 438 | 0.902 | 2.707 | 3.539 | 3.591 | 6.446 | 1.513 | 0/0 | 17712 | 1076.3 | 241.2 | 194 | 0 | 0 | 0 | 145.5 |

The 120 Hz budget is 8.33 ms for both move and pen-up work. These offscreen completed-work results exclude surface acquisition and presentation scheduling; target-device acceptance still requires input-to-present traces. Conservative contact pixels sum rotated contact bounding rectangles.

120 Hz completed-work gate: **PASS**.

## Working-format experiment

`cargo run --release -p layer-render-wgpu --example working_formats` uses the
same Float32 bilinear sampling and source-over arithmetic for all formats, with
50 warmups and 300 measured submissions per case, 16 physical passes each.
The two 256² textures are bounded working tiles; 4096² deliberately measures
full-image bandwidth pressure. CPU submission and serialized GPU-completed wall
time are separate. These kernels do not establish photographic editing accuracy.

Adapter: AdapterInfo { name: "NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition", vendor: 4318, device: 11188, device_type: DiscreteGpu, device_pci_bus_id: "0000:f1:00.0", driver: "NVIDIA", driver_info: "610.57.04", backend: Vulkan, subgroup_min_size: 32, subgroup_max_size: 32, transient_saves_memory: Some(false), limit_bucket: None }
Features: Features { features_wgpu: FeaturesWGPU(TEXTURE_FORMAT_16BIT_NORM | TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES), features_webgpu: FeaturesWebGPU(FLOAT32_FILTERABLE | FLOAT32_BLENDABLE) }
Format | pixels | kernel | bytes (two textures) | submit p50/p95/p99 ms | completed p50/p95/p99 ms
Rgba8Unorm | 256² | sample | 524288 | 0.0476/0.0549/0.0621 | 0.1078/0.1174/0.1287
Rgba8Unorm | 256² | sample + source-over | 524288 | 0.0476/0.0549/0.0636 | 0.1093/0.1216/0.3633
Rgba8UnormSrgb | 256² | sample | 524288 | 0.0478/0.0523/0.0621 | 0.1077/0.1180/0.3437
Rgba8UnormSrgb | 256² | sample + source-over | 524288 | 0.0533/0.0610/0.0808 | 0.1188/0.1379/0.1731
Rgba16Unorm | 256² | sample | 1048576 | 0.0540/0.0854/0.2898 | 0.1182/0.1701/0.4011
Rgba16Unorm | 256² | sample + source-over | 1048576 | 0.0547/0.0630/0.0957 | 0.1199/0.1530/0.3847
Rgba16Float | 256² | sample | 1048576 | 0.0483/0.0668/0.1073 | 0.1109/0.2547/0.3259
Rgba16Float | 256² | sample + source-over | 1048576 | 0.0481/0.0838/0.1849 | 0.1125/0.2808/0.3612
Rgba32Float | 256² | sample | 2097152 | 0.0481/0.0643/0.1272 | 0.1205/0.2809/0.3699
Rgba32Float | 256² | sample + source-over | 2097152 | 0.0543/0.0619/0.1184 | 0.1610/0.2630/0.3596
Rgba8Unorm | 4096² | sample | 134217728 | 0.0553/0.0877/0.2744 | 0.9298/1.1221/1.2970
Rgba8Unorm | 4096² | sample + source-over | 134217728 | 0.0498/0.0885/0.1723 | 1.0014/1.2270/1.3420
Rgba8UnormSrgb | 4096² | sample | 134217728 | 0.0530/0.0920/0.2109 | 0.9606/1.1590/1.2933
Rgba8UnormSrgb | 4096² | sample + source-over | 134217728 | 0.0548/0.1024/0.1626 | 1.4556/1.7015/1.7701
Rgba16Unorm | 4096² | sample | 268435456 | 0.0624/0.1313/0.2572 | 1.8191/2.0915/2.1831
Rgba16Unorm | 4096² | sample + source-over | 268435456 | 0.0738/0.1146/0.1720 | 1.8489/2.0935/2.2102
Rgba16Float | 4096² | sample | 268435456 | 0.0669/0.1085/0.1630 | 1.7897/2.0528/2.1236
Rgba16Float | 4096² | sample + source-over | 268435456 | 0.0619/0.1140/0.2196 | 1.8042/2.0670/2.1551
Rgba32Float | 4096² | sample | 536870912 | 0.0889/0.1679/0.2105 | 4.0072/4.2971/4.6991
Rgba32Float | 4096² | sample + source-over | 536870912 | 0.1163/0.2025/0.3591 | 10.4686/11.3619/12.1449

Integer16 and FP16 have similar measured kernel costs here. Their precision
contracts differ: an exhaustive IEEE half-float round trip of all 65,536 UNORM16
codes preserves only 7,169 codes exactly, with up to 16 codes error. Equal-size
half-float storage therefore cannot guarantee integer16 preservation. Float32
full-image blending is materially more expensive in this experiment.

Working strategy: encoded sRGB8 storage with linear-premultiplied Float32 shader
values; hardware sRGB decode before filtering and encode after blending. Keep
linear-light brush, resampling and composition math, and the existing declared
nonlinear effect domains. Alpha remains unencoded coverage. Future integer16
uses exact integer backing and normalized Float32 loads where supported; it must
not pass through mandatory FP16. Float32 scratch is bounded to tiles when a
materialized intermediate needs its precision. Future host capability checks
must select a supported integer texture path, rather than assume native UNORM16
render-attachment support everywhere.

The sRGB blend kernel has a measurable bandwidth/format cost on this GPU. The
production before/after drawing suite must determine whether it meets its gates;
this isolated experiment is not a production no-regression claim.

## Independently checked contracts

The shared color tests reproduce nine distinct linear8 values for sRGB codes
0–50 and verify all 256 opaque sRGB codes survive the standard transfer pair.
They specify decode-before-unassociation and alpha-zero handling, with the
quantization interval at each alpha defining the reference tolerance.

[wgpu 30.0.1 texture formats](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html)
specifies normalized integer loads and the sRGB storage/linear shader conversion.
[WebGPU texture formats](https://gpuweb.github.io/gpuweb/#texture-format-caps)
defines the capability boundaries; native wgpu extensions are checked explicitly.
[W3C compositing](https://www.w3.org/TR/compositing-1/)
separates straight-color blend functions from premultiplied composition.

## Gates fixed before implementation

For this reference workstation, investigate a reproducible unchanged-path p95/p99
increase above max(5% of baseline, 0.2 ms), comparing to both unchanged baseline
runs. Drawing completed p99 must remain below 8.33 ms for the existing 4K suite.
Record CPU frame creation separately from completed GPU work. Save/undo costs
must be reported separately; old replay save latency is not an equal-work baseline
for raster readback. No synchronous input-owner GPU wait is allowed.

The remaining GTK raster save/undo/recovery, history/staging budgets, dense-image
and multiple-document measurements, performance comparisons, and native workflow
qualification are outstanding. No cross-platform completion is claimed.

## Unchanged repeat

Same executable and hardware, repeated before production changes. Differences
between these two runs are host/driver scheduling variability, not code changes.

# GPU raster benchmark — 4096×4096

Adapter: `NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition`; backend code 1; device type code 2.

Release build with debug symbols. Each frame submits eight simulated coalesced pen samples through the public C ABI, calls the ABI frame function, then waits for that submission to complete. Every scenario has at least 32 visible paint layers. Each repetition creates a fresh canvas, warms the exact scenario pipeline, undoes the warm-up stroke, and contributes every measured frame to the reported distribution. Setup, shader/pipeline creation, canvas allocation, scenario warm-up/undo, brush selection, layer creation, and PNG export are outside the timing window. Submit latency is the production non-blocking path; completed-work latency serializes each measured frame to isolate its GPU work. Concurrent system/GPU load is not controlled, so these are reproducible workload references rather than cross-machine scores.

| scenario | state features | repeats | frames | move completed p50 ms | move completed p95 ms | move completed p99 ms | pen-up completed p99 ms | max move ms | submit p95 ms | move/pen-up frames > 8.33 ms | dabs | conservative contact Mpx | composite visits Mpx | paint pages | coverage pages | material pages | preview pages | resident canvas MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| gpen_inking | existing path | 3 | 864 | 0.094 | 0.161 | 0.344 | 0.194 | 16.721 | 0.110 | 1/0 | 86856 | 22.0 | 12.5 | 125 | 0 | 0 | 0 | 95.3 |
| pencil_shading | existing path | 3 | 1620 | 0.072 | 0.120 | 0.316 | 0.401 | 14.876 | 0.069 | 2/0 | 33480 | 113.3 | 34.8 | 222 | 0 | 0 | 0 | 119.5 |
| large_eraser | existing path | 3 | 420 | 0.116 | 0.206 | 0.336 | 0.184 | 0.438 | 0.134 | 0/0 | 2676 | 303.9 | 166.7 | 224 | 0 | 0 | 0 | 120.0 |
| large_paintbrush | existing path | 3 | 594 | 0.131 | 0.352 | 0.544 | 0.326 | 2.280 | 0.218 | 0/0 | 918 | 309.8 | 423.1 | 181 | 0 | 0 | 0 | 109.3 |
| soft_airbrush | existing path | 3 | 195 | 0.147 | 0.243 | 1.895 | 0.151 | 16.076 | 0.147 | 2/0 | 4407 | 293.6 | 86.0 | 143 | 0 | 0 | 0 | 99.8 |
| anchored_grain_chalk | existing path | 3 | 339 | 0.089 | 0.178 | 0.883 | 0.145 | 2.283 | 0.112 | 0/0 | 10158 | 46.9 | 33.5 | 127 | 0 | 0 | 0 | 95.8 |
| flat_marker | existing path | 3 | 180 | 0.136 | 0.253 | 1.950 | 0.155 | 2.228 | 0.154 | 0/0 | 7956 | 484.5 | 85.5 | 144 | 0 | 0 | 0 | 100.0 |
| scatter_spray | existing path | 3 | 159 | 0.182 | 0.323 | 1.748 | 0.103 | 2.187 | 0.200 | 0/0 | 36189 | 29.1 | 78.5 | 125 | 0 | 0 | 0 | 95.3 |
| dual_texture | existing path | 3 | 180 | 0.097 | 0.174 | 1.321 | 0.115 | 2.014 | 0.107 | 0/0 | 468 | 31.2 | 40.4 | 59 | 0 | 0 | 0 | 78.8 |
| multiply_glaze | existing path | 3 | 135 | 0.304 | 0.472 | 2.261 | 0.297 | 2.565 | 0.219 | 0/0 | 3765 | 147.0 | 53.0 | 224 | 0 | 0 | 0 | 146.0 |
| smudge_pickup | smudge advection | 3 | 135 | 0.363 | 0.980 | 2.683 | 0.828 | 2.769 | 0.515 | 0/0 | 2361 | 68.4 | 18.1 | 224 | 0 | 39 | 0 | 132.2 |
| wet_round_oklab | reservoir + Oklab mixing | 3 | 135 | 1.667 | 2.621 | 3.485 | 1.880 | 3.620 | 1.592 | 0/0 | 5520 | 189.6 | 49.5 | 224 | 0 | 88 | 0 | 147.5 |
| liquify_push | bilinear deformation | 3 | 114 | 0.506 | 0.970 | 2.577 | 0.515 | 3.934 | 0.686 | 0/0 | 492 | 54.6 | 44.0 | 224 | 0 | 0 | 0 | 131.8 |
| liquify_twirl | bilinear deformation | 3 | 90 | 0.748 | 1.368 | 4.642 | 1.441 | 11.805 | 0.904 | 1/0 | 423 | 82.1 | 87.5 | 224 | 0 | 0 | 0 | 143.3 |
| layered_composite | existing path | 3 | 1380 | 0.086 | 0.246 | 0.599 | 0.234 | 37.550 | 0.142 | 2/0 | 85599 | 293.9 | 278.8 | 395 | 0 | 0 | 0 | 162.8 |
| textured_flat_filbert | advanced dry | 3 | 438 | 0.104 | 0.155 | 0.430 | 0.123 | 1.936 | 0.089 | 0/0 | 5352 | 214.6 | 170.2 | 192 | 0 | 0 | 0 | 112.0 |
| dry_scumble | coverage | 3 | 438 | 0.317 | 0.609 | 1.636 | 0.445 | 3.101 | 0.362 | 0/0 | 3147 | 160.5 | 184.9 | 192 | 132 | 0 | 0 | 161.5 |
| pastel_block | advanced dry | 3 | 438 | 0.127 | 0.185 | 0.367 | 0.177 | 2.142 | 0.110 | 0/0 | 7557 | 217.4 | 142.6 | 190 | 0 | 0 | 0 | 111.5 |
| transparent_glaze | wetness | 3 | 438 | 0.170 | 0.337 | 2.369 | 0.336 | 2.680 | 0.208 | 0/0 | 7251 | 789.6 | 355.8 | 201 | 0 | 152 | 0 | 123.8 |
| opaque_gouache | reservoir + wetness | 3 | 438 | 1.009 | 3.345 | 4.045 | 2.925 | 5.201 | 1.781 | 0/0 | 14979 | 698.1 | 199.4 | 192 | 0 | 126 | 0 | 151.4 |
| watercolor_wash_edge | coverage + R8 wetness + event-driven capillary transport + live edge | 3 | 438 | 1.694 | 3.248 | 4.539 | 2.641 | 10.356 | 1.981 | 1/0 | 7341 | 514.4 | 428.8 | 201 | 146 | 146 | 0 | 187.3 |
| wet_watercolor | coverage + R8 wetness + event-driven capillary transport + live edge | 3 | 438 | 1.464 | 3.097 | 3.719 | 2.631 | 8.349 | 1.824 | 1/0 | 7251 | 418.7 | 386.8 | 200 | 140 | 140 | 0 | 184.0 |
| loaded_oil_mixer | reservoir + wetness | 3 | 438 | 1.038 | 3.422 | 4.088 | 2.974 | 7.092 | 2.011 | 0/0 | 11673 | 941.5 | 290.6 | 199 | 0 | 144 | 0 | 158.8 |
| palette_knife | reservoir + wetness | 3 | 438 | 1.022 | 2.086 | 2.523 | 1.831 | 4.784 | 1.152 | 0/0 | 2790 | 1193.9 | 959.9 | 211 | 0 | 198 | 0 | 178.7 |
| natural_blender | smudge advection | 3 | 438 | 0.935 | 3.055 | 3.636 | 2.197 | 4.014 | 1.906 | 0/0 | 17712 | 1076.3 | 241.2 | 194 | 0 | 0 | 0 | 145.5 |

The 120 Hz budget is 8.33 ms for both move and pen-up work. These offscreen completed-work results exclude surface acquisition and presentation scheduling; target-device acceptance still requires input-to-present traces. Conservative contact pixels sum rotated contact bounding rectangles.

120 Hz completed-work gate: **PASS**.

## Raster activation checkpoint (qualification in progress)

The production shared/GTK path now owns immutable raster revisions, stores indexed
lossless tiles, and restores those revisions for undo/reopen. Historical stroke
storage, replay persistence, the old archive reader and linear8 paint targets are
removed. Only bounded current-contact and late-correction inputs remain. The GTK
file worker takes immutable snapshots; recovery publication does not clear dirty.

Local release checks at this intermediate checkpoint: 43 core, 41 engine, 321 UI
unit tests; 122 physical-GPU renderer tests (18 separately ignored); both project
GPU integration tests; GTK native New/Open/Save/Export/cancellation workflow on
isolated Mutter, 1600×1000 at 120 Hz. All passed. The filter fixture was generated
independently from pre-migration renderer `7719e6b0ffa69e9aca1bf19acfedef9584d2fdad`
with only storage-boundary corrections; see the fixture README for provenance.

Performance is **not yet qualified**. The first after run exposed 58–286 ms
outliers in dense drawing scenarios. A targeted trace isolated deferred warm-up
undo spilling into the first measured frame: 145.7 ms frame creation and 276.8 ms
including capture-capacity waits. The harness now waits for the actual undo frame
before starting drawing measurements. Capture damage for terminal brush edges
has also been narrowed to the contact's coverage pages. Compression/backpressure,
separate undo costs, native pacing and background save contention remain under
investigation. These changes require fresh complete measurements before acceptance.

## Capture scheduling and codec measurements

The CPU is an AMD Ryzen Threadripper PRO 9995WX (96 cores). A release microbenchmark
ran 300 independent encode/hash and decode/verify operations on 256² RGBA tiles:
constant, gradient, gradient with low-amplitude deterministic noise, and full-range
xorshift noise. Each decoded result was compared exactly. Representative p95
encode/hash and decode/verify milliseconds for the textured tile:

| codec | compressed bytes | encode/hash p95 ms | decode/verify p95 ms |
|---|---:|---:|---:|
| miniz zlib level 1 | 175019 | 2.490 | 1.619 |
| zlib-rs level 1 | 190045 | 1.679 | 1.133 |
| Zstandard level 1 | 184677 | 0.935 | 0.400 |
| Zstandard fast -3 | 206640 | 0.733 | 0.297 |
| Zstandard fast -20 | 252689 | 0.256 | 0.180 |

The chosen native codec is Zstandard fast -20, explicitly named in the manifest.
It trades compressed size for bounded capture latency; precision is unchanged.
Constant tiles remain tiny (72 bytes). Full-range noise occupies 262159 bytes,
within the bounded tile envelope. History budgets charge actual compressed bytes.
The zlib dependency and decoder were removed from the raster implementation.

A second bottleneck was repeated compressor access to mapped GPU staging memory.
Copying each mapped chunk once into cached CPU memory, returning the unmapped
staging allocation promptly, and compressing at most four chunks concurrently
reduced the traced 45–56 MiB palette-knife backing jobs to roughly 36–44 ms. CPU
scratch is bounded to four 16 MiB chunks; the staging pool retains at most 64 MiB.
No per-frame full-document capture was introduced: these are large contact-end
captures, whose affected tiles cover much of the benchmark canvas.

The three-repeat targeted palette-knife run then measured move p50/p95/p99
1.151/2.417/2.926 ms, pen-up p99 3.384 ms and CPU move p95 1.399 ms, with zero
missed 8.33 ms frames out of 438. Warm-up undo took approximately 44 ms and is
reported as restoration work, outside drawing timing. The full suite must still
be repeated after this optimization; a single targeted result is not acceptance.

The latest shared checks pass 45 core, 44 engine and 322 UI tests. Added coverage
checks metadata/source validation, prediction/contact budgets, late-correction
expiry, renderer replacement preserving history and save identity, failed recovery
publication retaining the previous file, and close during an in-flight autosave.
GTK's file/surface lifecycle test passes: actual unrealize/re-realize retains
identical exported pixels, location, dirty state and undo/redo.

After the bounded compression change, all 123 physical-GPU library tests pass
(18 separately ignored), including explicit device destruction with an abandoned
capture and restoration of the previous backed checkpoint on a new device. Both
GPU project integration tests pass. GTK's native autosave/file/surface lifecycle
passes, and both recovery failure/cancellation unit tests pass. The native check
also verifies that autosave leaves dirty set and that a successful explicit save
removes its recovery copy. Final repeated performance qualification remains open.
