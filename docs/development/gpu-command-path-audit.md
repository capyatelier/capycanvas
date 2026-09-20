# GPU command-path audit and cached restore milestone — 2026-09-20

## Outcome and scope

Starting point: `origin/main` at `8bd5509ea523e401404aa3940649bc7936c4dcbe`.
The swept-brush research branch is preserved, not merged by this milestone.
Before pushing, the milestone was rebased onto concurrently advanced main at
`599a2ee6`. Those intervening commits change Apple/UI/host code and one core
test, not the measured renderer, production core code, or GPU dependencies.
Host and Web compile checks were repeated after integration.

The shared renderer now groups **already-decoded native color tile restores**
into the existing batches of at most sixteen. Cold decodes and scalar
(mask/material) restoration keep their original early submissions. No brush
algorithm, shader, sample precision, prediction, cache capacity, GPU lifetime
rule, or upload limit changes.

On the physical Wacom, restoring 64 cached tiles completes in **3.64–3.70 ms**
instead of **8.07–8.10 ms**: approximately **2.2×** faster for this stage.
Linux Vulkan completes the same stage approximately **2.4×** faster.
This is not a measurement of whole-app undo, brush throughput, or input-to-present
latency, and it does not establish a fix for the persistent frozen canvas.

## Audit: shared work, Android amplification, and a separate presentation fault

The earlier live primary-app investigation used `art.capycanvas` at
`5f71d7ec`, with the user's 9504 × 6336 photo and separate paint layer. Two
consistent callbacks took 1435.12 and 1197.42 ms. Composition occupied 1338.13
and 1121.39 ms, while committed brush work took 5.65 ms and approximately zero.
These were exploratory live interactions, not a paired benchmark; no action
marker proves either row was an undo.

A 55-second CPU profile of that session attributed 28.26% of canvas-owner
samples inclusively to command finalization, 10.38% to command-buffer freeing,
and 6.17% to allocation. These percentages overlap and must not be added.
JSON object parsing occupied 9.09%, with the query stack occupying 8.86%
(also overlapping). The layer-thumbnail transport includes pixel arrays in JSON;
not every JSON sample can be uniquely attributed to thumbnails.

The important relationships are:

| Work | Shared cost / platform difference | Implication |
| --- | --- | --- |
| Source decode/upload, composition, display mips | Shared Rust/wgpu renderer; backend costs differ | Command construction and synchronization can dominate even with almost no brush work |
| Native history restoration | Exact raster revisions, not replayed brush dabs | One-tile submissions can be removed where source pixels are already cached |
| Bounded submission waits | Shared resource policy; native owner can wait synchronously | Callback wall time includes earlier GPU work and driver work, not just current shader execution |
| Vulkan completed-command-buffer reclamation | Android-specific workaround for prior Adreno mapping/allocation failures | Android amplifies command churn; removing the workaround is not a qualified fix |
| Thumbnail JSON parsing | Measured Android host transport path | Avoidable owner-thread allocation outside brush evaluation; not changed here |

See [scene submission](../../crates/layer-render-wgpu/src/scene.rs),
[source cache](../../crates/layer-render-wgpu/src/scene/sources.rs),
[history restore](../../crates/layer-render-wgpu/src/raster.rs),
[Android command-buffer policy](../../vendor/wgpu-hal/src/vulkan/command.rs),
and the [earlier wide-brush investigation](android-wide-brush-performance.md).

Android input, dispatch and drawing share the canvas Looper. Long owner work
therefore delays the cursor and canvas interaction. The shared renderer costs
also exist on other platforms, but this profile does not measure their host
responsiveness or backend overhead.

The persistent stale canvas is a **separate unresolved fault**: thumbnails and
camera readouts changed, while the displayed artwork did not follow zoom/rotate.
Later inspection found the owner idle, 386 additional camera-only publications,
matching surface generations, and no captured device-loss/native exception.
Those observations do not identify which presentation link failed. Faster
restoration cannot be presented as a demonstrated repair.

Local raw interaction evidence remains in
`artifacts/swept-brush/interaction-diagnosis/`, including its README,
`interaction.perf.data`, symbolized reports, telemetry, and screenshots.
Those artifacts are ignored by Git; the findings and limitations are recorded
here so the milestone is reviewable without them.

## Why this particular simplification

Previously `restore_raster` called the existing native restore helper once per
color tile even when the helper could encode sixteen independent copies.
A cached run now shares command creation/finalization/submission across up to
sixteen copies. The code uses the existing source-cache validity lookup and
restore API; it adds no scheduler, shader variant, or second cache.

A pending cached prefix is flushed **before** any cold decode can evict/reuse
its source slots. Actual restoration still refreshes cache recency. New working
textures remain private candidates until the entire restore succeeds, preserving
late-failure atomicity and reuse of unchanged pages. Request metadata uses a
fixed stack array; a small pending handle vector is allocated only when a cache
hit occurs. No additional decoded pixel buffer is retained.

This reduces executed command-management work, not overall source line count:
a small bounded batching policy and regression coverage are added. It is an
incremental improvement, not a replacement of the composition architecture.

Blindly batching every restore was rejected. On Wacom, 128 cold tiles took
about **45.5 ms**, versus **17.9–18.9 ms** for early one-tile submission. Four
extra source-upload ceiling drains occurred per iteration. Even batches of four
were slower in a pilot without those extra drains. Fewer command buffers alone
is not a reliable performance objective: upload/compute overlap and bounded
resource pressure matter.

A separate display-mip worklist prototype was also rejected for this milestone.
It removed many repeated per-tile bindings/dispatches and helped Linux, but
Wacom variants were inconsistent or regressed, especially small dirty sets.
No mip shader or production display-mip code from that experiment is shipped.
The prototype patch and raw variant measurements remain under the local ignored
`artifacts/gpu-command-batching/` directory.

## Paired measurements

Physical hardware:

- Wacom DTHA140, serial `5ll21u1002931`, Android 15, Adreno 735 Vulkan.
  Clip Studio Paint was force-stopped with explicit user permission after its
  background CPU usage confounded earlier pilots. The primary Capy Canvas app
  remained running; no app reinstall, restart or document edit was performed.
  The user left the tablet idle for the final paired benchmark.
- Linux Vulkan, NVIDIA RTX PRO 6000 Blackwell Max-Q. This exercises the shared
  renderer, not the GTK GUI.

The release-mode ignored test
`raster::restore_tests::native_restore_submission_latency` runs both policies
against the same renderer, U16 sRGB blobs, and preallocated 256 × 256 Float32
targets. Document metadata is 9504 × 6336 (60,217,344 pixels), but it does **not**
render/composite that whole canvas. Solid-color input tiles are highly
compressible; arbitrary photo decode costs are not represented.

Each case has 30 warmups and 120 measured iterations, with two rounds reversing
policy order. Sizes 1, 4 and 64 settle into the 64-slot decoded-source cache;
128 tiles deliberately exceed it and miss continually. Each iteration waits for
completion after the API calls. The old policy is `batch=1`, the shipping
cache-aware policy is `batch=0`, and `batch=16` is the rejected unconditional
comparison. Both shipping and baseline paths still perform the same pixel copies.

Times below are milliseconds, ranges across the two rounds. `cpu_ms` in the
raw log means **API wall time**, including any driver work or blocking; it is
not active CPU time. `completed_ms` is elapsed wall time from the same start
through the final completion wait, not a separate duration to add, nor a GPU
timestamp.

| Device / work | Final submissions, old → new | API median, old → new | Completion median, old → new | Completion p99, old → new |
| --- | ---: | ---: | ---: | ---: |
| Wacom, 1 cached tile | 1 → 1 | 0.271–0.277 → 0.258–0.274 | 1.084–1.093 → 0.978–1.096 | 1.603–1.938 → 1.378–1.539 |
| Wacom, 4 cached tiles | 4 → 1 | 0.781–0.825 → 0.314–0.324 | 1.360–1.381 → 1.225–1.232 | 1.999–2.650 → 2.297–2.458 |
| Wacom, 64 cached tiles | 64 → 4 | 6.010–6.027 → 2.020–2.059 | 8.066–8.095 → 3.645–3.696 | 10.658–11.100 → 5.241–5.341 |
| Wacom, 128 cold tiles | 128 → 128 | 17.575–18.468 → 17.647–18.527 | 17.915–18.943 → 18.025–19.039 | 22.101–22.241 → 22.366–22.519 |
| Linux, 64 cached tiles | 64 → 4 | 0.428–0.504 → 0.083–0.101 | 0.601–0.606 → 0.252–0.257 | 2.753–3.004 → 0.553–0.564 |
| Linux, 128 cold tiles | 128 → 128 | 5.013–5.570 → 5.049–5.323 | 5.078–5.613 → 5.090–5.380 | 8.298–8.307 → 8.310–8.480 |

Cold Wacom medians straddle one another by round; its p99 is slightly higher.
Small-case p99 is mixed. These short paired runs support a cached-restore benefit,
not a claim of zero overhead or uniformly improved tails. Final-policy steady
iterations have zero source-upload drains. The 15.5 MiB cumulative upload peak
in later log rows includes the rejected policy and remains below the unchanged
16 MiB limit.

Tracked raw outputs:
[Wacom](measurements/cached-restore-wacom.txt) and
[Linux](measurements/cached-restore-linux.txt).

## Regression scope

Three new tests cover exact U8 sRGB/U16 P3 restored pixels, undo/redo, unchanged
page reuse, removal, late corruption without partial publication, retry, mixed
cached/cold source eviction ordering, exact U16 mask restoration, submission
counts, and the existing upload bound.

Linux hardware Vulkan passed the broader raster-filtered suite: **31 passed,
5 ignored**. This includes existing canonical paint/undo/save/reopen/device
replacement coverage and G-Pen edge/original-photo checks. The ignored 60 MP
workloads were not run by that command. After removing a redundant wrapper
around the existing cache lookup, all three new restore tests passed again on
Linux hardware and on Wacom (**3 passed, 1 benchmark ignored** on each).
The final Wacom binary also passed both existing native G-Pen regressions:
2048 px batch-edge preservation in compute/fragment paths and original-photo
pixel preservation in candidate/in-place paths, each at U8 and U16.

Wacom's broader raster-filtered run completed 14 existing tests successfully
before it was stopped after 10 minutes 25 seconds in
`native_engine_paint_undo_save_reopen_and_device_replacement_share_canonical_samples`.
That all-profile workflow test was still using CPU; this was an intentionally
incomplete run, not a suite pass or proof of a hang. The completed checks included
exact raster capture/undo/redo, continued paint, mask coverage, late restore
failure, and device-loss handling. The broader Wacom suite is not qualified.

The Wacom selection test
`affine_raster_selection_matches_linear_reference_in_fill_brush_and_mask`
fails at `layer_tests.rs:977`, translation `[30.5, 41.25]`, pixel `(32,41)`.
A clean build of unchanged `8bd5509e` reproduces the identical failure; the
final binary also fails there. This is a pre-existing selection issue, not
fixed or suppressed by this milestone.

Tracked regression outputs: [Wacom restore tests](measurements/cached-restore-wacom-tests.txt),
[Wacom G-Pen tests](measurements/cached-restore-wacom-gpen-tests.txt),
[incomplete Wacom raster run](measurements/cached-restore-wacom-partial-raster-tests.txt),
[Linux raster suite](measurements/cached-restore-linux-raster-tests.txt),
[Linux final restore tests](measurements/cached-restore-linux-tests.txt),
[baseline selection failure](measurements/cached-restore-wacom-baseline-selection.txt),
and [final selection failure](measurements/cached-restore-wacom-final-selection.txt).

`cargo check -p layer-render-wgpu --target wasm32-unknown-unknown` and
`cargo check -p layer-host` passed. Apple, browser runtime, GTK GUI and Windows
were **not tested**. Shared code reach is not a measured speedup on those hosts.

## Reproduction

On a machine with a supported physical GPU:

```sh
cargo test -p layer-render-wgpu --release --lib native_restore_submission_latency -- --ignored --nocapture --test-threads=1
cargo test -p layer-render-wgpu --release --lib raster -- --test-threads=1
```

Android uses NDK 29.0.14206865, cargo-ndk and API 29. Build the test executable,
then use its actual path printed/reported by the build (Cargo's hash can differ):

```sh
ANDROID_NDK_HOME=/path/to/android-ndk cargo ndk -t arm64-v8a --platform 29 test --release -p layer-render-wgpu --lib --no-run
/path/to/android-ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip -o /tmp/capy-restore-tests target/aarch64-linux-android/release/deps/layer_render_wgpu-HASH
adb -s 5ll21u1002931 push /tmp/capy-restore-tests /data/local/tmp/capy-restore-tests
adb -s 5ll21u1002931 shell chmod 755 /data/local/tmp/capy-restore-tests
adb -s 5ll21u1002931 shell /data/local/tmp/capy-restore-tests native_restore_submission_latency --ignored --nocapture --test-threads=1
adb -s 5ll21u1002931 shell /data/local/tmp/capy-restore-tests restore_tests --test-threads=1
adb -s 5ll21u1002931 shell rm /data/local/tmp/capy-restore-tests
```

This runs a headless test executable beside the primary app; it does not install
another application or exercise pen input/presentation. Record background load
and thermal state and do not compare concurrent GPU workloads. Stop other
applications only with the owner's permission.

## Remaining work, in priority order

1. Trace presented-image freshness across edit, undo, camera-only update and
   Android surface presentation to isolate the persistent stale-canvas fault.
   Successful renderer submissions alone are insufficient.
2. Attribute ordinary composition more finely: source misses/decode/upload,
   mip updates, command finalization, and bounded waits, with dirty-tile counts
   and action-tagged app measurements. This is the dominant measured long-stall
   region; restore batching alone cannot remove it.
3. Simplify shared source/composition work where it removes both CPU command
   overhead and GPU work, retaining existing ordering and memory bounds.
   Re-qualify large **and** small dirty sets on multiple physical backends.
4. Remove avoidable thumbnail pixel serialization from the measured Android
   owner path without inventing a separate host-specific renderer.
5. Revisit asynchronous chunk progression only if bounded, reduced work still
   blocks input too long. That is a larger responsiveness change, not something
   proved necessary by these microbenchmarks.

Restoration still must decode uncached backing and copy each changed pixel;
large dirty areas still incur composition and display-update bandwidth.
These are remaining costs, not a measured absolute speed limit. This milestone
does not change brush appearance or brush algorithms.
