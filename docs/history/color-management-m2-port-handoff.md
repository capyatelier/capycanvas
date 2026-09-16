# Phase 2 integration handoff and next-step assessment

2026-09-16. The user requested platform handoffs and publication so the Apple
agent can proceed. This supersedes the **current-status** claims in the older
[main integration assessment](color-management-m2-main-integration.md), without
rewriting its historical measurements.

Read the proposed [phase 2 criteria and common gates](color-management-milestones.md),
[technical design](color-management-research.md), and
[benchmark guide](../development/gpu-raster-benchmarks.md) alongside this status
and current code. Later user decisions recorded below take precedence.

## Status

- **GTK:** qualified within the [accepted reference envelope](color-management-gtk-m2-acceptance.md).
- **Web/Android:** complete SDR workflow checks and the 61 MP G-Pen, exact
  undo/redo, save/reopen and GPU recovery regressions pass on the tablet. Warm
  navigation meets the agreed 120 Hz target with occasional missed intervals.
  The [validation record](color-management-web-android-m2-validation.md#user-test-corrections--2026-09-16)
  records an unexplained 2.15 s immediate-post-import Android queue delay, latency
  limits, and pending hands-on confirmation of the latest corrected builds.
- **macOS/iPadOS and Windows:** phase 2 ports remain outstanding. Current source
  still reads effect/gradient colors as arrays and creates attachment-based
  renderers. Existing control and cross-host project workflows can therefore
  regress with the shared phase 2 contracts. Publication is an authorized
  integration handoff, **not an all-platform release-readiness claim**.

Start with the short [Apple handoff](../development/color-management-m2-apple-handoff.md)
or [Windows handoff](../development/color-management-m2-windows-handoff.md).
The latest upstream Apple changes through `9b7c4eb7` are integrated; they must
survive the native SDR transition.

## Contracts and accepted scope

`layer-core` owns exact encoded U8/U16 backing, color definitions and retained
sources. `layer-color` owns conversion/interchange using `moxcms` and
`libjpeg-turbo-rs`; do not restore the replaced C CMM/JPEG paths. `layer-ui` owns
shared workflow/history policy. Native bridges own asynchronous platform I/O,
GPU lifetime and atomic publication. Display mips may reduce viewing cost but
must never supply edits, exact sampling or export.

Preserve existing drawing, saving, diagnostics, prediction and recovery. Record
fresh platform baselines and real backend capabilities. Use enough RAM/VRAM to
meet smooth navigation; keep fallback behavior explicit. The user deferred
full-resolution dirty regeneration optimization and a resolution-aware preview
pipeline. Filter comparison may use justified imperceptible differences; exact
backing/identity export and U16 precision requirements remain unchanged. The
accepted target is smooth 120 Hz navigation with latency documented, not an
unconditional sub-8.33 ms input-to-photon claim. Preserve Apple's separately
recorded current Mac 90 Hz hardware qualification limit.

## Recommendation

**Finish phase 2 on Apple and Windows before proceeding to phase 3 implementation.**
The first priority is the existing color-control/project contract regression,
then complete host workflows and native device qualification. Obtain the latest
Web/Android user retest and investigate the Android cold queue delay separately
from the already-passing warm navigation result. Missing calibrated-display,
memory-pressure and platform hardware evidence must remain explicit limits.
Do not reopen the deferred dirty-renderer redesign to delay the ports.

Phase 3 can then begin with numerical proof-transform work and GTK proof UI.
Current `crates/layer-color/src/icc.rs` explicitly rejects black point
compensation and absolute conversion between two non-matrix profiles with
different media whites. Proof/delivery profile separation, paper/ink simulation,
gamut warnings and independent reference-CMM checks still need implementation.
Those are phase 3 tasks, not unfinished ordinary SDR functionality.

## Integration validation

Merge `ed978185` combines phase 2 `a8b3cd76` with upstream `9b7c4eb7`.
The user explicitly selected publication to `origin/main` (there is no remote
`master`). Merge resolutions preserve both native color/display publication and
upstream queue-aware GPU timing, plus receiving-window prediction during document
adoption. Two incoming stroke replay tests now pass the document's working space;
the host snapshot test now expects upstream's unclamped Apple floating preview.

On Linux, the following passed on the merged source. Logs are under local
`artifacts/color-m2/platform-handoff/`; these are regression checks, not new
performance qualification or Apple/Windows native build evidence.

```sh
cargo test --locked --offline -p layer-core -p layer-color -p layer-engine -p layer-ui -p layer-host --lib
cargo check --workspace --all-targets --locked --offline
cargo check -p layer-web --target wasm32-unknown-unknown --locked --offline
ANDROID_NDK_HOME=/home/babymastodon/Android/Sdk/ndk/29.0.14206865 cargo ndk -t arm64-v8a --platform 29 check -p layer-android --locked --offline
cargo test -p layer-render-wgpu --locked --offline --lib frame_timing::tests -- --test-threads=1
cargo test -p layer-render-wgpu --locked --offline --lib -- live_display::tests native_gpen_keeps_original_photo_pixels_in_touched_tiles native_engine_paint_undo_save_reopen_and_device_replacement_share_canonical_samples --test-threads=1
```

Shared tests: **658 passed, 5 ignored**. Targeted GPU tests: **19 passed**.
Workspace/all-targets and both real cross-target checks passed; local handoff links
resolve and `git diff --check` passed. Native Swift/Metal and WinUI/D3D12 checks
require their platform environments. The tablet validation linked above predates
this upstream merge; these compile checks do not replace a final device retest.
