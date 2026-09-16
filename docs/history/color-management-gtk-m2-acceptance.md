# GTK color milestone 2 acceptance

**GTK milestone 2 is complete within the reference-system envelope below.**
This is the concise review entry point for the local GTK implementation. Detailed
numerical contracts, source references, failed attempts and reproducible commands
remain in the [validation record](color-management-gtk-m2-validation.md) and
[performance record](color-management-gtk-m2-performance.md). Historical milestone
proposals are not declarations of current support.

## Scope and implemented workflows

| Workflow | Implemented and validated behavior | Evidence in validation record |
| --- | --- | --- |
| New / Open / Place / Paste | Independent 8/16-bit depth and sRGB/P3/Adobe RGB/ProPhoto; retained originals; separate editable photo master; richer clipboard sources; explicit unsupported-input and missing-profile policies | New/photo master, retained Place/Paste, input policy and portable JPEG checkpoints |
| Edit and inspect | Float32 working values with exact integer backing; revisable exposure, white balance, Levels/Curves, hue/saturation and color balance; masks; full-resolution histograms, clipping and artwork sampling | Native paint/adjustment, physical-filter, exact query and GTK histogram checkpoints |
| Color controls and viewing | Tagged RGB/hex/OKLCH, saved palettes, brush opacity; managed canvas/picker/swatches/gradients and declared sRGB fallback | Numeric colors, managed viewing and display-only texture checkpoints |
| Assign / Convert / precision | Distinct semantics; complete preview and flattened copy; atomic undo/cancel; source repair and rasterization preserve baked/off-canvas edits | Document color transitions, source repair and rasterization checkpoints |
| Save / deliver | Exact native save/reopen, PNG/JPEG/TIFF sample/profile/metadata matching, resized preview, reusable recipes, DPI and explicit reductions | Snapshot/export/recipes/resolution and portable JPEG/CMM checkpoints |
| Existing editor reliability | Drawing, undo/redo, diagnostics, failed GPU recovery and save remain available; whole 60 MP color-plus-mask transforms publish exact backing | Recovery checks and full-resolution edit-publication checkpoint |

The initial milestone-1 audit found sRGB8-only boundaries, unbounded restoration
scratch and overlapping full-resolution source/composite/filter storage. The
replacement paths and their measurements are recorded in the validation history.
Apple/Windows lifecycle qualification was already incomplete and is not implied
by this GTK work.

## Acceptance envelope

Reference environment: Fedora 44, GTK 4.22.4, Mutter 50.4, NVIDIA RTX PRO 6000
Blackwell Max-Q, driver 610.57.04, Vulkan. Native navigation uses an isolated
3840×2160 120 Hz compositor with actual 200% scale, both maximized and a
2400×1800 canvas window. Synthetic input-to-presentation feedback is not a
physical finger/pen-to-photon measurement.

The user selected smooth 120 Hz unchanged-photo navigation with measured latency
documented. Full-resolution dirty-pixel, physical-filter and histogram regeneration
optimization is deferred. Full-resolution filter semantics are preserved; there
is no resolution-aware preview/editing pipeline.

The completed 61 MP Float32 display uses about 1.2 GiB. Admission can use one
quarter of driver-reported GPU headroom, allocating only the actual needed display.
Warm navigation performs no source decoding or image recomposition. Lower-priority
thumbnail preparation yields to interaction. The 24/45/60 MP and simultaneous-job
CPU/GPU measurements, fallback limits, fixed-baseline drawing comparisons and
precision-format comparisons are in the performance record.

Native immutable edit output is bounded separately from readback staging: up to
1 GiB per publication, at most 1.25 GiB pending plus active transfer, with the
existing 64 MiB spare pool separate. Whole 60 MP color and linked-mask transforms
pass exact samples, undo/redo, save/reopen and renderer replacement. These are
resource/correctness results, not a passed dirty-regeneration latency budget.

## Remaining platform and capability limits

- Other host integration (Web, Android, macOS, iPadOS and Windows) requires the
  user's approval. Shared-code compile checks do not qualify those hosts.
- Physical calibrated display accuracy, moving/spanning unlike monitors,
  on-screen wide-gamut Cairo and fallback changes during open dialogs remain
  unmeasured. Vulkan/OpenGL managed textures and Cairo sRGB snapshots are tested.
- Constrained GPUs using the bounded tile fallback are not qualified for 120 Hz
  61 MP/4K navigation when tiles must be refilled. Driver headroom is a snapshot,
  not a global resource reservation. Inactive-tab demotion/disk spill and unified
  memory pressure management remain future work.
- The portable CMM does not support black point compensation or absolute-intent
  conversion between non-matrix profiles with different media whites. UI and
  adapter capability checks disclose these limits. Print proofing is later scope.
- The Rust JPEG codec buffers whole images with headroom-based admission; it is
  not a streaming codec or a reservation across concurrent jobs.
- An unrelated existing layer-panel review assertion reports nested-row x=6
  versus x=0 on both parent and current implementations. Dedicated thumbnail,
  history and recovery checks pass; that broad fixture is not counted as passing.

## Final result

Seven final 61 MP native runs cover windowed/fullscreen 4K at 200% scale, input
phase variation and a full-resolution Gaussian blur. They present **5,376/5,376
later-phase requests** at sustained 120 Hz, with no camera source decoding or
recomposition. Each eight-second run loses 2–4 initial requests and reports
0–1 missed refresh slots. First response is **14.304–36.201 ms**; whole-run
request-to-present p99 is **6.295–11.519 ms**. CPU/GPU maxima across these runs
are **5.622/6.183 ms**. The timer stops after the gesture becomes idle. These
results qualify sustained navigation and explicitly retain the first-use limit.

The concrete latency findings are GTK allocation of unnecessarily large Float32
intermediate surfaces, timer restarts around a refresh cutoff, and stale display
phase/input timing during correction. Display-only half-float controls and a
paced camera burst with corrected presentation/input phase address those causes.
The cause of the older unmatched 8.226 versus 12–14 ms comparison is still
**unknown**; it is not attributed to a newly introduced drawing deadline.

Final correctness evidence includes 251 renderer and 77 core passes, both explicit
60 MP color/linked-mask publication cases, managed controls under Vulkan/GL/Cairo,
gradients, saving, portable paint/sampling, pen-up with concurrent backing,
diagnostics and SDR8/wide-color GPU recovery. Four report tests and four clock/
timer tests pass. The detailed records distinguish current code from earlier
captures, ignored optional cases and the unrelated known fixture failure.

The final navigation/correctness test executable is
`artifacts/color-m2/final-performance/navigation-input-anchor-qualified-gtk-tests`,
SHA-256 `76393f9a4de7065ad0c0868c505238a1d01fd555e62a86df6756d5acbde1dd64`.
Commands and complete environments are in the adjacent
`navigation-input-anchor-qualified-runs.json`; build with
`cargo test -p layer-linux --release --offline --no-run --message-format=json`
and use `bash tools/performance/gtk-raster.sh` as documented in the performance
record. The runnable application is built separately with
`cargo build -p layer-linux --release --offline`.
