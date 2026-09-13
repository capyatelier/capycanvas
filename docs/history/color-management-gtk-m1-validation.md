# GTK raster foundation validation

GTK milestone 1 qualification completed on 2026-09-13 in the local worktree.
Other hosts require the user's approval before work continues. This report records
the implemented contracts and measured limits; the proposed milestone document
was evaluated against the code, numerical references and hardware results.

The final source is the commit containing this report, following `e746d6e`.
Significant intermediate commits were `3fe5e76` (contracts and baseline),
`abc523a` (immutable tiles and indexed container), `290c760` (production GTK
activation and predecessor removal), and `e746d6e` (bounded capture and retained
sessions). Rendering comparisons use the fixed pre-change baseline `ebafa44`.

## Result and scope

GTK now edits encoded sRGB8 raster data, restores exact stored revisions for
undo/redo, saves indexed lossless tiles and recovers host-backed checkpoints.
Historical stroke persistence/reconstruction, the old archive codec and linear8
paint targets were deleted. Layers, masks, immutable sources and embedded live
adjustments survive reopening and remain editable. The exposed document mode is
sRGB8; integer16, ICC workflows and other host ports are outside this checkpoint.

Two complete final 25-scenario runs, three repetitions per scenario, measured
**21,840 frames with zero move or pen-up frames above 8.33 ms**. Across these runs,
the largest CPU move p95/p99 was **2.609/3.155 ms**; the largest completed move p99
was **4.859 ms**, and pen-up p99 was **4.010 ms**. No unchanged drawing path
reproducibly crossed the previously declared regression trigger. Pen-up now does
additional useful capture work, so its cost is reported against the absolute
budget separately from the old replay implementation.

Dense 24/45/60 MP cases and two simultaneous 24 MP documents completed drawing,
concurrent saves, archive verification and explicit memory accounting. Native
GTK New/Open/Save/Export/autosave/surface lifecycle and seven pacing workloads
passed on the physical Vulkan GPU under isolated Mutter. This is workstation
qualification, not a mobile, physical-tablet or physical-display latency claim.

## Independently checked decisions

Paint uses `Rgba8UnormSrgb`, storing `sRGB_encode(linear_RGB × alpha)`; alpha is
unencoded coverage. Hardware decodes before filtering and encodes after blending.
Shaders retain linear-premultiplied Float32 arithmetic. Straight sRGB source
uploads use their separate descriptor and decode/associate at the source boundary.
Masks and persistent wetness are linear R8. Existing nonlinear effect domains
remain explicit and independent of the storage transfer function.

The shared color tests reproduce only nine distinct linear8 values for sRGB codes
0–50 and verify that all 256 opaque sRGB codes survive the standard transfer pair.
Low-alpha references propagate the stored encoding/association quantization
interval instead of requiring impossible exact straight-color recovery. Exact
project preservation means exact committed tile bytes, including alpha and masks;
it does not imply lossless conversion to every other pixel representation.

[wgpu 30.0.1 texture formats](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html)
specifies normalized integer loads and sRGB storage/linear shader conversion.
[WebGPU texture formats](https://gpuweb.github.io/gpuweb/#texture-format-caps)
defines capability boundaries; the working-format experiment checked native
extensions explicitly. [W3C compositing](https://www.w3.org/TR/compositing-1/)
separates straight-color blend functions from premultiplied composition.

Integer16 and FP16 had similar measured kernel costs, but an exhaustive half-float
round trip preserved only 7,169 of 65,536 UNORM16 codes exactly, with up to 16 codes
error. Consequently the schema does not impose an FP16 intermediate on future
integer16 data. Exact integer backing, Float32 arithmetic and bounded working
tiles are the selected direction. Full-image Float32 blending was substantially
slower in the independent kernel experiment; its complete results appear below.

The runtime filter fixture was generated from pre-migration renderer
`7719e6b0ffa69e9aca1bf19acfedef9584d2fdad` with only storage-boundary corrections.
The production result matches within one code over 1,966,080 channels; see the
[fixture provenance](../../crates/layer-render-wgpu/tests/fixtures/README.md).
Scalar transfer, blend and spatial-sampling references test the relevant numeric
boundaries separately. No artistic filter algorithm was changed to match a fixture.

## Backing, history and lifecycle bounds

The [project format reference](../reference/project-format.md) defines the final
container, validation limits and durability boundaries. Immutable 256² tiles
share unchanged backing among the current document, history and save snapshots.
Only affected physical pages and persistent wet-state planes are captured.
A selected-fill GPU test also checks reuse of untouched tile handles and exact
undo pixels. Committed operation recipes are cleared after submission.

Capture admission allows at most 16 jobs and reserves room for the largest next
256 MiB frame within a 512 MiB pending-staging ceiling. Buffers are at most
16 MiB, reusable spares at most 64 MiB, and cached CPU readback scratch at most
64 MiB. At most four compression jobs run per capture. The native frame mailbox
remains bounded to two frames. Capture pressure defers commit/correction/operation
boundaries while ordinary move frames continue. Waiting pen-up does not advance
continuous ink or prediction. No compression or GPU readback wait runs on GTK's
input owner.

The capture telemetry conservatively adds outstanding staging reservations,
spare buffers and active cached scratch. It can count a returned allocation both
in its outstanding reservation and spare pool until the capture completes. The
maximum sampled value in the 4K runs was 184.2 MiB; it excludes immutable source
and history data and driver allocations. The older canvas-residency counter alone
is not a total-memory measurement.

History retains at most 256 edits within a conservative 512 MiB backing/metadata
budget excluding current-document ownership. Pending tiles/roots have explicit
reservations, and unchanged backing identities are deduplicated. Active contact
storage is limited to 131,072 points with 32 predicted points. Late correction is
limited to the latest completed contact for two seconds; new contacts, metadata
edits and undo/redo close that window. Corrections amend the current checkpoint
without mutating earlier snapshots or adding a history step.

GPU completion, host-backed revisions and durably published files are separate
states. Failed/abandoned capture tickets publish errors. GTK's immutable file
worker, 15-second autosave and atomic publication preserve the previous recovery
copy on failure/cancellation. Autosave does not acknowledge the manual save
checkpoint. A surface unrealize/re-realize replaces the renderer while retaining
the session, dirty state, location and history. The device-destruction test restores
a previously backed checkpoint on a new device; uncaptured samples are not claimed
recoverable.

## Correctness checks

All final release tests passed; the renderer examples and benchmark also pass
release compilation checks:

| suite | result | local record |
|---|---:|---|
| core | 45 passed | `/tmp/color-m1-shared-tests.txt` |
| engine | 45 passed | same |
| UI/session | 322 passed | same |
| GPU renderer library | 123 passed; 18 separately ignored | `artifacts/color-m1/final-gpu-tests.txt` |
| GPU project integration | 3 passed | `artifacts/color-m1/final-project-tests.txt` |
| C ABI | 11 passed | `artifacts/color-m1/final-ffi-tests.txt` |
| GTK recovery failure/cancellation | 2 passed | `artifacts/color-m1/recovery-failures.txt` |
| native GTK file/surface workflow | 1 passed | `artifacts/color-m1/gtk-final-files.txt` |
| native GTK pacing, seven workloads | 1 passed | `artifacts/color-m1/gtk-frame-pacing.txt` |

The GPU checks cover brushes/erase, wetness/smudge, fill/gradient/figures, masks,
transforms, selection, layer/group/clipping composition, built-in/custom filters,
and fused versus physical passes. Project checks round-trip the committed state
and continue wet painting after reopening. Shared checks exercise malformed and
old archives, source/metadata validation, stable snapshots, history limits,
correction expiry, active-contact limits and renderer replacement. The 18 ignored
renderer tests are separate hardware workloads, not part of the 123-test count.

The native file workflow checks New/Open/Save/Export/cancellation, autosave dirty
state, recovery-file removal after explicit save, and actual hide/unrealize/show
with identical pixels and retained undo identity. Final G-Pen and watercolor PNGs
were also visually inspected for coherent output, missing strokes and seams.

## Final drawing comparison

Hardware is the reference workstation described with the baseline below, with
AMD Ryzen Threadripper PRO 9995WX (96 cores). Release builds and hardware GPU runs
were serialized; clocks and ordinary desktop scheduling were not locked.

Final local records are `artifacts/color-m1/qualified-v2.md` and
`artifacts/color-m1/qualified-repeat.md`, each with its `.log` and PNG gallery.
Each run includes 10,920 measured frames. Ranges below show the two independent
run results, not confidence intervals; all time columns are milliseconds.
The baseline CPU column is its nonblocking submit p95. Final CPU timings count
input submission and frame creation separately from GPU/capture-capacity waits.
Completed timings include those waits and the queue-ordered capture copies;
asynchronous compression completion is a later boundary.

The predeclared investigation trigger was a reproducible increase above
`max(5% of baseline, 0.2 ms)` for unchanged move p95/p99 and CPU submit p95.
The comparison flags a case when **both** final runs exceed **both** unchanged
baselines by that margin: `min(final) > max(baseline) + max(0.05 × max(baseline),
0.2 ms)`. None did. Some individual results are slower, as the ranges show;
this is not a claim of identical per-frame timing. Drawing and pen-up completed
p99 also meet the independent 8.33 ms absolute gate.

| scenario | baseline CPU p95 | final CPU p95 | baseline completed p95 | final completed p95 | baseline completed p99 | final completed p99 | final pen-up p99 maximum | capture peak MiB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| gpen_inking | 0.074–0.110 | 0.073–0.105 | 0.118–0.161 | 0.117–0.158 | 0.162–0.344 | 0.247–0.319 | 1.180 | 128.0 |
| pencil_shading | 0.056–0.069 | 0.075–0.079 | 0.108–0.120 | 0.122–0.127 | 0.289–0.316 | 0.247–0.250 | 0.797 | 80.0 |
| large_eraser | 0.091–0.134 | 0.118–0.119 | 0.167–0.206 | 0.185–0.197 | 0.316–0.336 | 0.299–0.317 | 1.115 | 96.0 |
| large_paintbrush | 0.160–0.218 | 0.196–0.222 | 0.279–0.352 | 0.313–0.349 | 0.439–0.544 | 0.613–0.828 | 1.727 | 104.0 |
| soft_airbrush | 0.147–0.159 | 0.166–0.191 | 0.243–0.245 | 0.264–0.293 | 1.895–3.430 | 1.339–1.420 | 1.172 | 136.0 |
| anchored_grain_chalk | 0.105–0.112 | 0.142–0.143 | 0.168–0.178 | 0.206–0.213 | 0.430–0.883 | 0.412–0.797 | 0.893 | 96.0 |
| flat_marker | 0.154–0.183 | 0.279–0.376 | 0.253–0.288 | 0.387–0.480 | 1.950–3.732 | 1.625–1.765 | 1.191 | 136.0 |
| scatter_spray | 0.158–0.200 | 0.360–0.404 | 0.249–0.323 | 0.461–0.518 | 1.748–2.984 | 1.494–1.522 | 1.212 | 128.0 |
| dual_texture | 0.107–0.110 | 0.132–0.186 | 0.174–0.177 | 0.220–0.276 | 1.321–2.597 | 1.411–1.450 | 0.683 | 96.0 |
| multiply_glaze | 0.219–0.291 | 0.305–0.308 | 0.472–0.528 | 0.523–0.542 | 2.261–2.287 | 0.579–0.645 | 1.247 | 64.0 |
| smudge_pickup | 0.510–0.515 | 0.612–0.629 | 0.901–0.980 | 0.875–0.932 | 2.683–2.708 | 1.312–1.317 | 1.312 | 96.0 |
| wet_round_oklab | 1.393–1.592 | 1.484–1.683 | 2.320–2.621 | 2.534–2.598 | 3.126–3.485 | 2.876–2.927 | 3.766 | 64.0 |
| liquify_push | 0.626–0.686 | 0.427–0.568 | 0.959–0.970 | 0.739–0.839 | 2.473–2.577 | 1.119–1.139 | 0.950 | 64.0 |
| liquify_twirl | 0.904–1.052 | 0.742–0.945 | 1.368–1.480 | 1.200–1.496 | 3.370–4.642 | 1.675–1.910 | 1.649 | 72.0 |
| layered_composite | 0.142–0.162 | 0.149–0.187 | 0.246–0.277 | 0.249–0.285 | 0.577–0.599 | 0.792–1.029 | 1.707 | 184.2 |
| textured_flat_filbert | 0.089–0.102 | 0.133–0.139 | 0.155–0.186 | 0.209–0.214 | 0.313–0.430 | 0.291–0.321 | 1.112 | 108.0 |
| dry_scumble | 0.362–0.424 | 0.422–0.434 | 0.609–0.692 | 0.700–0.702 | 1.636–2.680 | 0.975–1.023 | 1.164 | 116.0 |
| pastel_block | 0.087–0.110 | 0.137–0.165 | 0.164–0.185 | 0.206–0.255 | 0.230–0.367 | 0.293–0.360 | 1.436 | 124.0 |
| transparent_glaze | 0.190–0.208 | 0.189–0.210 | 0.335–0.337 | 0.305–0.330 | 2.369–2.396 | 0.488–0.508 | 1.646 | 162.0 |
| opaque_gouache | 1.766–1.781 | 1.701–2.020 | 3.054–3.345 | 3.118–3.535 | 3.924–4.045 | 4.138–4.859 | 3.712 | 128.0 |
| watercolor_wash_edge | 1.981–2.393 | 1.800–2.609 | 3.248–3.857 | 3.026–3.923 | 4.475–4.539 | 4.040–4.722 | 3.759 | 128.0 |
| wet_watercolor | 1.824–2.184 | 1.880–2.072 | 3.097–3.472 | 3.132–3.311 | 3.719–4.041 | 3.595–3.806 | 3.370 | 128.0 |
| loaded_oil_mixer | 1.703–2.011 | 1.841–1.984 | 3.240–3.422 | 3.264–3.617 | 3.877–4.088 | 3.885–4.339 | 3.443 | 128.0 |
| palette_knife | 1.152–1.320 | 1.513–1.562 | 2.086–2.271 | 2.387–2.523 | 2.523–2.840 | 2.737–3.350 | 4.010 | 176.0 |
| natural_blender | 1.513–1.906 | 1.765–1.975 | 2.707–3.055 | 2.832–3.225 | 3.539–3.636 | 3.521–3.900 | 3.315 | 112.0 |

## Dense images and simultaneous documents

The committed `raster_workloads` example uses opaque gradients plus deterministic
low-amplitude noise, 32 paint layers, a 1024×768 viewport and G-Pen contacts.
These are dense SDR8 storage/interaction workloads, not photographic CMM tests.
Each single-document case measures four contacts × 64 frames while saving an
immutable snapshot on another thread. The two-document case alternates complete
contacts, 512 measured frames total, while one document saves.

The archive is reopened and all saved tile SHA-256 digests compared. An unchanged
second save reuses compressed backing. Undo/redo is GPU-completed and full-image
export checks the restored result's checksum. Timing includes a flushed and
file-synced save, but does not model GTK's chooser or atomic rename. CPU drawing
here measures engine frame creation; completion includes input queueing and any
capacity/GPU waits. Record: `artifacts/color-m1/dense-workloads.txt`.

| case | CPU p50 / p95 / p99 ms | completed p50 / p95 / p99 ms | concurrent save ms | archive MiB |
|---|---:|---:|---:|---:|
| 24 MP, 6000×4000 | 0.070 / 0.141 / 0.446 | 0.121 / 0.233 / 0.669 | 194.40 | 156.45 |
| 45 MP, 8192×5504 | 0.066 / 0.104 / 0.502 | 0.121 / 0.175 / 0.833 | 280.08 | 281.65 |
| 60 MP, 8192×7324 | 0.083 / 0.134 / 0.616 | 0.135 / 0.212 / 0.945 | 445.92 | 366.55 |
| two 24 MP documents | 0.056 / 0.104 / 0.358 | 0.108 / 0.204 / 0.640 | 166.27 | 156.87 |

Cold operations are outside drawing timing:

| case | source generation + device + initial submission ms | initial host-backed ms | reopen + digest comparison ms | unchanged save ms | undo / redo ms | export + checksum ms |
|---|---:|---:|---:|---:|---:|---:|
| 24 MP | 3680.35 | 3764.17 | 247.71 | 174.78 | 13.10 / 11.17 | 65.05 |
| 45 MP | 812.25 | 904.97 | 372.94 | 331.60 | 20.66 / 19.32 | 127.45 |
| 60 MP | 1049.99 | 1224.23 | 561.16 | 384.21 | 15.59 / 13.74 | 131.50 |

The first case includes process/device/cache warmup. Thus these initial timings
are actual observed cold costs, not a claim that cost decreases with image size.
The simultaneous documents initialized in 635.33/456.15 ms and became host-backed
in 711.95/517.89 ms respectively.

| case | renderer allocated/reserved MiB | current compressed tiles MiB | immutable source MiB | process RSS KiB | process high-water KiB |
|---|---:|---:|---:|---:|---:|
| 24 MP | 251.58 | 62.96 | 91.55 | 698628 | 928196 |
| 45 MP | 412.03 | 107.56 | 172.00 | 921092 | 1259536 |
| 60 MP | 524.91 | 135.79 | 228.88 | 1199032 | 1646144 |
| two 24 MP documents | 251.58 each | 63.41 each | 91.55 each | 1289596 total | 1646144 total |

Process RSS/high-water include driver allocations and qualification snapshots,
reopening and export temporaries. High-water is cumulative across the cases in
one process. Renderer counters and process values are different accounting views,
not disjoint quantities to sum. Larger source/composite residency redesign and
streaming whole-image export remain milestone 2 work. These results do not enable
unbounded image sizes or establish budgets for constrained devices.

## Native GTK presentation

The native test ran the production app-owned Wayland Vulkan subsurface and
`GskVulkanRenderer` on an isolated Mutter virtual monitor, 1600×1000 at 120 Hz.
Canvas viewport was 1200×900, brush size 384, navigator and cursor enabled,
with six seconds per workload and synthetic input at 480.1–486.9 events/second.
It recorded 5,055 worker frames and GPU timestamp intervals, with 5,054 presented
and one discarded feedback. Record: `artifacts/color-m1/gtk-frame-pacing.json`.

All table times are milliseconds, using the empirical nearest-rank percentile.
Worker elapsed includes acquire/configure/render/present; worker thread CPU
excludes descheduling. The GPU timestamp interval brackets rendering and can
include gaps between queue submissions. Enqueue-to-present joins each frame's
monotonic queue timestamp to compositor presentation feedback. It does **not**
measure device sampling, input arrival before enqueue or physical scanout.

| workload | frames / discarded | frame-handler p99 | worker elapsed p99 | worker thread CPU p99 | GPU interval p99 | enqueue-to-present p95 / p99 | presentation spacing p99 / max |
|---|---:|---:|---:|---:|---:|---:|---:|
| GPen | 722 / 0 | 0.061 | 0.702 | 0.666 | 0.459 | 6.323 / 6.524 | 8.679 / 20.165 |
| NaturalBlender | 722 / 0 | 0.068 | 1.496 | 1.467 | 1.598 | 6.354 / 6.522 | 8.639 / 10.477 |
| WetRound | 723 / 0 | 0.056 | 1.069 | 1.034 | 1.073 | 6.259 / 6.330 | 8.605 / 10.484 |
| WatercolorWash | 722 / 1 | 0.058 | 2.379 | 2.352 | 2.934 | 6.039 / 6.096 | 8.526 / 8.929 |
| Pan | 722 / 0 | 0.037 | 0.472 | 0.470 | 0.281 | 6.347 / 6.461 | 8.599 / 10.631 |
| Hand | 722 / 0 | 0.033 | 0.441 | 0.440 | 0.200 | 6.372 / 6.470 | 8.563 / 10.413 |
| Transform | 722 / 0 | 0.286 | 2.238 | 2.218 | 2.339 | 6.340 / 6.459 | 8.613 / 10.376 |

The native run verifies sustained scheduling and presentation with low frame
creation costs. Presentation spacing varies around the nominal 8.333 ms interval;
the G-Pen maximum was 20.165 ms, and one watercolor frame was discarded. These
are retained in the report rather than treating the passing workflow assertion
as proof that every native display deadline was met. No physical tablet/display
input-to-photon or other GPU/driver/backend qualification is claimed.

## Optimizations and failed intermediate measurements

Initial after runs exposed 58–286 ms outliers. A targeted trace isolated deferred
warm-up undo spilling into the first measured drawing frame: 145.7 ms CPU and
276.8 ms including capacity waits. The harness now waits for an actual restoration
frame and drains deferred input before recording completion. Empty deferred frames
cannot be counted as cheap completed drawing frames. Undo remains measured
separately in the dense example.

Capture damage now uses operation selection/transformed footprints and only the
current contact's terminal-edge coverage. Blanket capture backpressure was split
from the native frame mailbox so ordinary moves continue while backing finishes.
Worker-prepared reusable buffers avoid repeated pinned allocation at pen-up.
Copying mapped GPU memory once into cached CPU scratch and using up to four
compression lanes reduced representative 45–56 MiB palette-knife capture jobs
to roughly 36–44 ms off the input owner. Earlier `after*.md` and `qualified.md`
records are failed/intermediate runs; `qualified-v2.md` and `qualified-repeat.md`
are the final complete measurements used above.

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

## Reproducing the checks

Run hardware tests serially without overlapping benchmark processes or builds.
`LAYER_GPU_INDEX=0` selects the reference physical GPU for offscreen tools.

```bash
cargo test --release -p layer-core -p layer-engine -p layer-ui
LAYER_GPU_INDEX=0 cargo test --release -p layer-render-wgpu --lib -- --test-threads=1
LAYER_GPU_INDEX=0 cargo test --release -p layer-render-wgpu --test project -- --test-threads=1
LAYER_GPU_INDEX=0 cargo test --release -p layer-ffi --lib -- --test-threads=1
LAYER_GPU_INDEX=0 cargo run --release -p layer-render-wgpu --example working_formats
LAYER_GPU_INDEX=0 cargo run --release -p layer-render-wgpu --example raster_workloads -- all
LAYER_GPU_INDEX=0 cargo run --release -p layer-bench -- \
  --scenario all --repeats 3 --output-dir artifacts/color-m1/recheck \
  --report artifacts/color-m1/recheck.md
```

Repeat the last command with another output/report name. Build the native tests
with `cargo test --release -p layer-linux --no-run`. From `apps/layer-linux`, run
that test binary selecting `tests::native_document_files` and
`tests::native_frame_pacing` with `--ignored --test-threads=1`, on isolated
D-Bus/Wayland sessions. Use a private
`XDG_RUNTIME_DIR`, `LAYER_SETTINGS_FILE`, `CAPY_WORKSPACE_DIR` and
`CAPY_RECOVERY_DIR`, with `GDK_BACKEND=wayland`, `GSK_RENDERER=vulkan`,
`G_DEBUG=fatal-criticals`, and Mutter's
`--headless --wayland --no-x11 --virtual-monitor=1600x1000@120`.
Set `LAYER_PACING_REPORT` to preserve native JSON. The two recovery unit tests
are selected by `recovery::tests` without `--ignored`.

Paths under `artifacts/` and `/tmp` name local outputs, not shipped files. Key
measurements are retained in this tracked report; executable workloads and numeric
references are committed with the implementation.

## Original rendering baseline

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

### GPU raster benchmark — 4096×4096

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

## Original unchanged repeat

Same executable and hardware, repeated before production changes. Differences
between these two runs are host/driver scheduling variability, not code changes.

### GPU raster benchmark — 4096×4096

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

## Original working-format experiment

`cargo run --release -p layer-render-wgpu --example working_formats` uses the
same Float32 bilinear sampling and source-over arithmetic for all formats, with
50 warmups and 300 measured submissions per case, 16 physical passes each.
The two 256² textures are bounded working tiles; 4096² deliberately measures
full-image bandwidth pressure. CPU submission and serialized GPU-completed wall
time are separate. These kernels do not establish photographic editing accuracy.

Adapter: AdapterInfo { name: "NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition", vendor: 4318, device: 11188, device_type: DiscreteGpu, device_pci_bus_id: "0000:f1:00.0", driver: "NVIDIA", driver_info: "610.57.04", backend: Vulkan, subgroup_min_size: 32, subgroup_max_size: 32, transient_saves_memory: Some(false), limit_bucket: None }
Features: Features { features_wgpu: FeaturesWGPU(TEXTURE_FORMAT_16BIT_NORM | TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES), features_webgpu: FeaturesWebGPU(FLOAT32_FILTERABLE | FLOAT32_BLENDABLE) }
| Format | pixels | kernel | bytes (two textures) | submit p50/p95/p99 ms | completed p50/p95/p99 ms |
|---|---:|---|---:|---:|---:|
| Rgba8Unorm | 256² | sample | 524288 | 0.0476/0.0549/0.0621 | 0.1078/0.1174/0.1287 |
| Rgba8Unorm | 256² | sample + source-over | 524288 | 0.0476/0.0549/0.0636 | 0.1093/0.1216/0.3633 |
| Rgba8UnormSrgb | 256² | sample | 524288 | 0.0478/0.0523/0.0621 | 0.1077/0.1180/0.3437 |
| Rgba8UnormSrgb | 256² | sample + source-over | 524288 | 0.0533/0.0610/0.0808 | 0.1188/0.1379/0.1731 |
| Rgba16Unorm | 256² | sample | 1048576 | 0.0540/0.0854/0.2898 | 0.1182/0.1701/0.4011 |
| Rgba16Unorm | 256² | sample + source-over | 1048576 | 0.0547/0.0630/0.0957 | 0.1199/0.1530/0.3847 |
| Rgba16Float | 256² | sample | 1048576 | 0.0483/0.0668/0.1073 | 0.1109/0.2547/0.3259 |
| Rgba16Float | 256² | sample + source-over | 1048576 | 0.0481/0.0838/0.1849 | 0.1125/0.2808/0.3612 |
| Rgba32Float | 256² | sample | 2097152 | 0.0481/0.0643/0.1272 | 0.1205/0.2809/0.3699 |
| Rgba32Float | 256² | sample + source-over | 2097152 | 0.0543/0.0619/0.1184 | 0.1610/0.2630/0.3596 |
| Rgba8Unorm | 4096² | sample | 134217728 | 0.0553/0.0877/0.2744 | 0.9298/1.1221/1.2970 |
| Rgba8Unorm | 4096² | sample + source-over | 134217728 | 0.0498/0.0885/0.1723 | 1.0014/1.2270/1.3420 |
| Rgba8UnormSrgb | 4096² | sample | 134217728 | 0.0530/0.0920/0.2109 | 0.9606/1.1590/1.2933 |
| Rgba8UnormSrgb | 4096² | sample + source-over | 134217728 | 0.0548/0.1024/0.1626 | 1.4556/1.7015/1.7701 |
| Rgba16Unorm | 4096² | sample | 268435456 | 0.0624/0.1313/0.2572 | 1.8191/2.0915/2.1831 |
| Rgba16Unorm | 4096² | sample + source-over | 268435456 | 0.0738/0.1146/0.1720 | 1.8489/2.0935/2.2102 |
| Rgba16Float | 4096² | sample | 268435456 | 0.0669/0.1085/0.1630 | 1.7897/2.0528/2.1236 |
| Rgba16Float | 4096² | sample + source-over | 268435456 | 0.0619/0.1140/0.2196 | 1.8042/2.0670/2.1551 |
| Rgba32Float | 4096² | sample | 536870912 | 0.0889/0.1679/0.2105 | 4.0072/4.2971/4.6991 |
| Rgba32Float | 4096² | sample + source-over | 536870912 | 0.1163/0.2025/0.3591 | 10.4686/11.3619/12.1449 |

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
production comparison above supplies the drawing qualification; this isolated
experiment alone does not establish production performance.
