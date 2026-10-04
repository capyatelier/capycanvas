# M5–M6: execution and acceptance

[Specification](photo-editing-m5-m6.md)

Status: **proposed for review**. Baseline: `origin/main`, `da5b399ada27f7d97927456e5975fb9298308d32`. This is an assignment plan, not a validation report. No implementation, build, runtime journey or benchmark was performed for this specification.

## Implementation rules

Each implementation packet identifies the relevant specification sections, baseline commit, affected files, dependency commits, old path to remove, observable acceptance cases and required checks. Private helper names may change, but formulas, limits, undo behavior, persisted meaning, color semantics and host scope must follow the specification. If a required contract cannot be implemented within the existing architecture, return a concrete failing case and the smallest proposed revision. Do not invent a framework to finish a packet.

Work in an owned worktree under `/home`, install hooks, and preserve other sessions' files, builds and devices. Before each packet, check the current code against its dependency commits; this inventory describes the audit baseline. Keep unrelated formatting and cleanup out. Never run workspace-wide rustfmt or workspace tests. No new dependency is planned.

A packet is a scope boundary, not automatically a commit. Integrate only a complete milestone that builds and passes its checks. Keep changes to `effects.rs`, `session.rs`, `manifest.json`, shared shader includes, render request types and serialization coherent within that milestone.

## Implementation checkpoints

| Gate | Executable work and evidence | Consequence |
| --- | --- | --- |
| **G0 scope and contract** | Apply D1–D12 and the explicit Liquify deferral. Use the stated geometry, calibration, preset, filter and export contracts as implementation defaults. Recheck integration points against the implementation baseline. | No further product decision is required to begin. Implement this contract; incompatible evidence triggers a concrete revision rather than an invented alternative. |
| **G1 wet-material order** | Prototype complete geometry mapping of raw pigment and wetness before the existing appearance evaluation. Prove bounded neighbor halos, style-coordinate consistency and retained→bake pixel parity for nonuniform mesh/scale with independent wet-edge fixtures. | Freeze the material placement/bake interface before extending retained transforms to wet paint. A color-only flatten is insufficient. |
| **G2 photographic algorithms** | Implement the proposed Selective Color, Shadows/Highlights, Clarity and Dehaze math in the existing GPU harness, with independent numerical references and a small photographic comparison sheet. Include portrait, landscape, white wall/snow, sky, saturated edge, transparent edge and HDR highlight. | Review halos, cast removal, constant-field behavior and neutral identity before host work. Change the formula in one place if quality fails. |
| **G3 resource and query review** | Review typed source identity, cancellation, bounded chunks, one auxiliary effect binding and complete accounting/eviction visitors. Prove nested/clipped effect input and two stacked source-aware effects. | Approve interfaces before splitting core/renderer/host packets. No generic job or resource graph. |
| **G4 GTK UI review** | Build and walk the concrete GTK controls, including both themes and narrow layouts, then present them for review. | Follow [UI rules](../ui/README.md#rules-for-ui-changes): “Build and validate a UI change on GTK first, then get the user's approval before porting it.” Web and Android remain required first-delivery work after this checkpoint. |

The material, photographic and resource checkpoints are engineering work within implementation, scoped to their dependent packets. Independent packets can start immediately. Photographic review needs concrete output; the specification alone cannot demonstrate quality. GTK approval remains the existing repository checkpoint before UI ports.

## Integration order

1. **M0: shared contracts and prototypes.** Apply G0, begin the material spike and resource/query interface review, and implement schema validation plus typed request/view skeletons. Run photographic prototypes as their filter/guide dependencies become available. Keep all host consumers compiling.
2. **M1: geometry vertical slice.** Pixel-tight bounds → retained placement → paint extent/bake/lifecycle → multi-target/pivot/snapping/Warp. Preserve baked Liquify through the shared pixel-write admission and explicit bake path. Finish GTK journeys and review before porting its UI.
3. **M2: color vertical slice.** Properties pages/gestures → exact measurements → Levels/Curves/WB/histograms → pointwise effects/resources/local guides/gradients. Independent leaf shaders can proceed alongside geometry once shared-file ownership is clear.
4. **M3: workflow vertical slice.** Comparison/samplers, effect library and export complete against the stable query/resource/draft contracts; follow the same GTK→Web→Android sequence.
5. **M4: first-delivery qualification.** Cross-feature journeys, all relevant checks, real hardware measurements, deletion audit and current guide updates. Apple/Windows follow-up stays explicit.

M1–M3 may overlap at independent file boundaries. Each integrated slice includes shared core, renderer and its functioning host route; do not merge a menu entry whose backend is unfinished. Format and ABI changes have a single owner and update every reader/writer/fixture in the slice. Adding a current version requires no compatibility reader or deferred Liquify payload.

## Bounded implementation packets

Paths below are repository-relative. Abbreviated `ui/`, `core/`, `wgpu/` and `engine/` mean `crates/layer-ui/src/`, `crates/layer-core/src/`, `crates/layer-render-wgpu/src/` and `crates/layer-engine/src/`. New files/types may be placed beside these owners; the named paths are existing integration points.

### Geometry

| ID and dependencies | Owned work and reuse | Observable acceptance and deletion |
| --- | --- | --- |
| **A1 — bounds; G0/G3** | `core/content_bounds.rs`, `ui/image_geometry.rs`, renderer bounds requests; reuse thumbnail GPU measure and the current cache/request lifecycle. | Alpha-tight source+override, erasure, mask products, selection, hidden content, empty results at every depth; stale cancellation; bounds only readback. Remove runtime CPU tile decoding/scans and rectangle-only shortcuts, including Trim/Reveal callers. |
| **A2 — durable maps; G0/G1** | `core/layers.rs`, `affine.rs`, `projective.rs`, `warp.rs`; exact placement APIs, mask relationships, geometry identity and validation. Use an optional mesh directly; no future Liquify variant. | Outer composition exact; no generic nonlinear inverse; pole/singular/nonfinite refusal; affine fast path; locked/multi-target validation before preview. Model round trips preserve exact control values. |
| **A3 — placed evaluator; A1/A2** | `wgpu/scene/placement.rs`, `paint_transform/`, `scene/scale.rs`; reuse source-position sampler, tessellation/binning, LOD and region admission. | Identity/affine/projective/mesh/folds, every interpolation, clipped/linked/unlinked masks, exact export versus independent oracle; source pages unchanged; G1 material parity; remove affine-only sampling assumptions. |
| **A4 — extent and bake; A3** | `core/canvas_geometry.rs`, `merge.rs`, `engine/bake_steps.rs`, `ui/source_edit.rs`, image-command plans. Reuse scheduling but map raw planes, not color-flattened merge output. | Paint after downscale can reach canvas; hidden tiles preserved; atomic explicit bake/undo/cancel and resource admission; source repair, Revert, Rasterize Original, crop/resize policies match geometry spec. |
| **A5 — transform targets/session; A1–A4** | `ui/operation.rs`, `operation/placement.rs`, `canvas_bar.rs`, existing layer selection. | Whole paint/photo retained, pixel selection and mask path preserved, supported group roots deduplicated, unsupported selection refused as a whole; Apply one undo, Cancel/no-op none, Reset entry pose. Remove obsolete placed Distort/Warp refusals. |
| **A6 — pivot/snap/nudge/repeat; A5** | Existing transform controls, ruler projection, keymap/command catalog, `UiInput` repeat/key-up routing. | Absolute reference X/Y; 6/10 DIP snap; 1/10 doc-pixel nudge; focus/cancel/keyup correct; only retained outer deltas repeat; shared menu command unbound by default. |
| **A7 — Warp refinement; A2/A5** | `core/warp.rs`, transform transaction/node selection, existing mesh controls and shader buffers. | Nonuniform row/column breakpoints, exact de Casteljau splits, 32-cell cap, multi-point drag and touch selection, shape unchanged by split/reopen; remove implicit retained mesh fitting. |

Liquify implementation packets and the retained-field prototype are removed. Keep its existing baked path, presets and undo behavior. Add regression coverage only where shared placement/material/write admission changes touch that path; live Liquify and Reconstruct remain deferred.

### Properties, effects and analysis

| ID and dependencies | Owned work and reuse | Observable acceptance and deletion |
| --- | --- | --- |
| **B1 — metadata/pages/gestures; G0** | `core/effects.rs`, `ui/effects.rs`, `numeric.rs`, manifest metadata and published host views. | ≤16 pages, bounded visibility/mapping metadata, common/page controls, active page transient, hidden values retained; one undo per numeric drag and cancel preserves redo; all consumers compile. |
| **B2 — exact query sources; G3/A3** | `layer-render` request types; `wgpu/artwork.rs`, `scene/stack.rs`, `scene_images.rs`, current source admission and callbacks. | Actual insertion-point input in clipped/isolated/Pass Through/fused/spatial stacks; placed layer content; frozen frame/time; no cold decode/wait on UI; late success/error never edits another owner. |
| **B3 — GPU statistics; B2** | Existing tonal reductions, bounds/continuations, `core/color/histogram.rs` semantics, small result transport. | Defined Preview/Exact bins/counts, HDR tails, alpha>0 count once, selection>0, exact two-pass Auto summaries, bounded preview/chunks; remove production full-image CPU histogram. |
| **B4 — Levels; B1/B3** | Manifest, common WGSL scalar helper, `native_effects/tone.rs`, shared Auto/calibration actions. | 22 parameters, channels then master, clamps, signed/HDR math, fixed quantiles/error bound, inverse-master calibration, atomic invalid/constant/stale refusal. |
| **B5 — picker/Curves/WB; B1/B2** | `ui/color_picker_session.rs`, `eyedropper.rs`, `effects.rs`, existing curves and numeric controls. | Fixed pre-effect sample, exact curve inverse/point rules, physical fields/EV, targeted drag, WB neutral solve, touch loupe, one undo, pending release and tool-switch cancellation. Remove host point math. |
| **B6 — hue/pointwise filters; B1** | Manifest and shared WGSL owned by one integrator; existing catalog discovery and filter tests. | Hue42/Selective37/Mixer17 schemas, Oklab membership, Invert/Threshold/Desaturate/Photo Filter; exact neutral, finite extended/alpha/profile behavior. Selective Color waits for G2. |
| **B7 — resource ABI; G3/B1** | `core/effects.rs`, project binary payloads, `wgpu/effects.rs`, generated interfaces/cache keys, NativeHost bridges. | ABI4 exact validation, one typed auxiliary binding per separate stage, bounded dedup/upload/lifetime/accounting; pointwise resources retain tile batching and ordinary fusion is unchanged; update every consumer with no ABI3 adapter. |
| **B8 — Color Lookup; B7** | Shared bounded .cube parser, effect resource property and existing host file services; proof tetrahedral helper conventions. | All six tetrahedra/ties, domains/encoded spaces/residuals, malformed/oversize refusal, same pixels after source-file deletion/reopen/paste, no LUT reupload on intensity drag. |
| **B9 — source-aware guides; B2/B7** | Factor `wgpu/snapshot/tone.rs` region reduction and current local-tone guide ownership. | Stack-ordered frozen input; at-most-768 edge; stale cancellation; missing/pending/error states; parameters that only consume guide do not rebuild it; exact export awaits matching guide. |
| **B10 — local effects; B9/G2** | Manifest and guide consumers; bounded Dehaze RGB/transmission passes. | Stated Shadows/Highlights/Clarity/Dehaze equations, independent oracles and accepted quality sheet; halos/transparent/HDR/constant fixtures; no per-frame full-canvas guide. |
| **B11 — Gaussian range; B1** | Six manifests, `gaussian-prepare.wgsl`, table sizes and all consumers/halos. | σ0..85, radius255, 256 lanes/129 records, preparation dependencies, reduced scale, window seams and Frequency Separation; remove old cap consistently, measure large reach. |
| **B12 — unified gradients; B1/A3** | Core definition, tool operation/mask/effect shaders and existing shared gradient editor actions. | 2–32 stops, three shapes/mix spaces, tagged alpha-weighted interpolation, deterministic integer-depth dither, one drag undo; delete duplicate two-color/radial/transparent models and shader math. |

### Workflow and delivery

| ID and dependencies | Owned work and reuse | Observable acceptance and deletion |
| --- | --- | --- |
| **C1 — Histogram/Info views; B1–B3** | `ui/layout.rs`, `layout_presets.rs`, panel registry/customization, shared statuses/readouts/visibility subscriptions. | Normal dock/tab/float/drawer lifecycle; Histogram/RGB Waveform above Properties in Photo, Info secondary; Window panel access; no custom-layout migration; bounded updates preserve Properties focus; old Histogram command becomes Show/Focus Panel. |
| **C2 — samplers; B2/C1/A4** | Small core guide-like records/edits, shared hit-test/bar and image-geometry transforms. | Ten stable IDs max, project dirty without image revision, one drag undo, outside-canvas kept, current/before values transient, no reads solely because markers exist. |
| **C3 — comparison; B2** | Shared view state and momentary actions; `ViewportPresenter`, existing proof/SDR and alternate composition cache. | Adjustment-only bypass retains generators; shared animation time; exact view restore; split motion presents cached pair; common HDR output; no export/history/sample contamination. Default shortcut unbound. |
| **C4 — effect values/library; B7** | Exact `EffectSettings`, bounded preset CRUD/storage format modeled on export/profile libraries. | Stable64-entry/64MiB limits, embedded resources/programs, source-space provenance, failed mutation atomic, no settings/catalog mutation. |
| **C5 — effect apply/preview; C4/B1** | Factor current insertion policy; renderer namespace admission, window clipboard, existing eight-item preview pipeline. | Same-kind Paste exact values, cross-document lifetime, unequal conflicting WGSL refusal, saved picker category, late/locked target refusal, one edit; no rebind/default substitutions. |
| **C6 — export draft; G0** | `ui/export.rs`, `export_presets.rs`, shared numeric metadata/actions and codec limits. | Original/Fit/Long/Short/Percent/MP rounding, resolved dimensions, PPI metadata only, recipe stores mode, Off-default sharpening; remove host recipe reconstruction. |
| **C7 — delivery rows; C6** | `layer-color/src/resize.rs`, `output_rows.rs`, NativeHost/Web HDR paths. | One seven-row sharpening wrapper after resize, independent alpha/HDR oracle, exact Off/pass-through gates, before rendition split, provider cancel and checked memory; remove duplicate setup. |
| **C8 — frozen export/artifact; C7** | `layer-host/src/export.rs`, snapshot output, existing GTK/Android temporary files and Web OPFS worker token. | One frame per dialog, actual encoded byte count, one job/latest pending recipe, prepared byte reuse, stale cleanup, bounded codec admission, no target opened during calculation. |
| **C9 — Export Again; C8** | Shared document-file session state/command and native opaque target-token services. | Per-document successful recipe/target, fresh snapshot, tab-safe, failure/cancel retain prior state, revoked target opens picker, Web download is another download, project checkpoint unchanged. |

### Hosts and integration ownership

Apply H1–H3 to each accepted vertical slice.

| Packet | Integration surfaces | Completion |
| --- | --- | --- |
| **H1 GTK** | `apps/layer-linux/src/effects.rs`, `number_control.rs`, canvas bar/toolbars/workspace panel factories, `files/export.rs` and library workers. | Generic shared page/control projection replaces local Curves grouping; new panel/loupe/gradient/resource/export routes; accessible labels follow visible labels; private-display journeys in both themes; G4 review. |
| **H2 Web; H1/G4** | `apps/layer-web/effects.js`, `numeric.js`, `canvas-bar.js`, panel factories, export controls/documents/output/worker. | Match GTK state and controls; asynchronous source/storage preparation, current cancellation/OPFS bounds, real headed WebGPU; remove local `readRecipe` and legacy histogram window/endpoint. |
| **H3 Android; H2** | Existing `Effects.kt`, `NumberControl.kt`, `CanvasBar.kt`, `Panels.kt`, `ExportDialog.kt`, `OutputPreview.kt`, `ColorPreferencesStore.kt`, JNI views. | Same shared semantics on native render owner; retained controls and Activity/surface recreation; one export task; reserved-device private-ID journeys; remove local recipe assembly and HistogramWindow. |
| **I1 integration** | All serialization, accounting, source-repair, color-conversion, effect-resource and plane visitors; each slice's tests/guides. | Combined fixtures below; project/effect format rejects unsupported input; exact resources accounted once; no unowned compatibility or superseded runtime path. |

Host presentation choices remain native. Labels, precision, validation, eligibility, page contents, sample math, source scope, export dimensions and history boundaries remain shared Rust.

## Apple and Windows follow-up

These are required follow-up packets, not part of GTK/Web/Android acceptance. Shared Rust/ABI consumers must compile in every earlier slice. Missing presentation is capability-gated with an accurate shared reason; a host must never open a new project while silently discarding geometry or resources.

| Packet | Concrete work | Required evidence |
| --- | --- | --- |
| **F1 Apple geometry/controls** | Shared bar/tool controls and gestures, pages in `PropertyControls.swift`, precise curves, gradient editor, file import and document epoch ownership. Test macOS and iPadOS input separately. | Swift/bridge fixtures, command audit, both-theme mouse/keyboard and physical iPad pen/touch, retained save/reopen/bake journeys and existing baked-Liquify regression where affected. |
| **F2 Apple inspection/delivery** | Histogram/Info panels and factories, comparison output branch, samplers, preset storage/clipboard, authoritative export draft/frozen task/target token; include shared metadata row. | Metal/HDR/proof and cancellation/output tests, source-resource persistence, export and quick-export journeys. Delete `HistogramController.swift`/`HistogramPresentation.swift` legacy route. |
| **F3 Windows geometry/controls** | `EffectView.cpp`, `EffectControls.h`, `CurveView.cpp`, `GradientView.cpp`, existing action bar, workspace factories and project file lists. Remove local .01 curve-nudge math. | Rust/bridge and Windows fixture checks, native capture/focus/accessibility, hardware D3D12 retained geometry/material and affected baked-Liquify regression. Existing notices/checkable bars already work; extend them rather than reimplementing. |
| **F4 Windows inspection/delivery** | Histogram/Info/comparison, sampler actions, library/file services, `ExportForm.h`, frozen output/target-token ownership and metadata row. | Both themes, proof/HDR/device removal, save/reopen/LUT/preset/export journeys; delete DocumentView legacy histogram dialog/task route. |
| **F5 shared legacy retirement; F2/F4** | Remove final old histogram host request/task methods once no consumers remain; delete temporary capability exclusions. | Every host consumes one query system; no full-resolution CPU histogram route. All affected host tests and physical GPU journeys pass. |

Do not include unrelated M2–M4 port gaps as completed work. Apple preset-count and Windows notice/menu issues listed in old roadmap text are already addressed at the audited baseline.

## Regression and real-user acceptance

The feature specifications own detailed numerical cases. Add these cross-feature fixtures to existing harnesses, not a parallel test application:

1. Placed source with transparent alpha and erased overrides → exact bounds → Distort/Warp → linked mask → sampled Curves and local guide → comparison → export. Incremental pixels match full recomposition and exact output; source identity stays unchanged.
2. Mixed hidden/visible photo and paint descendants in isolated and Pass Through groups, clipped adjustments and group/unlinked masks. Whole transform applies once per owner; a locked/unsupported descendant refuses all targets without history loss.
3. Watercolor pigment/wetness with edge style under nonuniform Warp and downscale → explicit bake → undo/reopen. Verify both appearance and underlying planes, then resume painting.
4. Existing baked Liquify on an affine-editable layer → undo/redo/save/reopen. On retained projective/Warp content, verify shared write refusal → explicit Apply Transform to Pixels → baked Liquify. No silent geometry bake, retained field or new reconstruction session.
5. Source-profile correction, Rasterize Original, Revert, color/depth conversion and canvas commands on deformed photos. No lost mesh, changed interpolation, moved masks or silently discarded hidden pixels.
6. Animated lower filter with exact Auto, ten samplers, live histogram and comparison. Both comparison halves share time; correction uses one captured time; time advancement alone cannot starve it or accept changed source content.
7. Copy custom Curves/LUT preset across separate document sessions, close source, change destination profile/depth, then save/reopen. Preserve exact definitions or refuse atomically; test both program-ID and WGSL-symbol conflicts.
8. Open export with animated/local-tone/deformed HDR image → preview → change recipe → calculate bytes → export → edit master → Export Again. Prepared output and final bytes match one frozen input; project save checkpoint never moves.
9. At every async boundary: delete/lock target, switch document/page/tool, undo, cancel, close/recreate renderer, fail upload/storage/codec. No stale result edits a new owner, no preview enters Save, and errors preserve accepted artwork.
10. Every new drag/cancel action in light/dark, narrow/wide, docked/drawer/floating context. Numeric focus survives sample/bin updates; touch/pen capture leaves canvas and returns; native blur releases held modes.

For first delivery, walk those journeys on GTK, Web and Android. Mouse/pen tap and touch loupe are separate picker tests. Transform/pivot/mesh/sampler/split handles drag immediately. New panel charts have accessible equivalent numeric text; button accessible names track their visible labels. Existing test names are regression entry points, not evidence that the new behavior is tested.

## Checks

Follow [testing](testing.md) and each host guide for current prerequisites. These commands exist at the audited baseline; choose changed crates and matching renderer filters, then run complete changed-crate suites when closing an integration milestone.

```sh
cargo test --locked -p layer-core
cargo test --locked -p layer-engine
cargo test --locked -p layer-ui
cargo test --locked -p layer-render
cargo test --locked -p layer-color
cargo test --locked -p layer-workspace
cargo test --locked -p layer-host -- --test-threads=1
cargo check --locked -p layer-linux --tests
cargo check --locked -p layer-web --target wasm32-unknown-unknown
cargo test --locked -p layer-render-wgpu transform -- --test-threads=1
cargo test --locked -p layer-render-wgpu placement -- --test-threads=1
cargo test --locked -p layer-render-wgpu liquify -- --test-threads=1
cargo test --locked -p layer-render-wgpu native_effects -- --test-threads=1
cargo test --locked -p layer-render-wgpu adjustments -- --test-threads=1
cargo test --locked -p layer-render-wgpu live_windows -- --test-threads=1
cargo test --locked -p layer-render-wgpu image_windows -- --test-threads=1
cargo test --locked -p layer-render-wgpu view_color -- --test-threads=1
python3 tools/build/test_shader_generation.py
```

Run renderer and NativeHost tests on hardware GPU; use a single test thread for device-heavy filters. The explicit software-adapter test opt-in proves numerical behavior only. Run the shader-generation check when its inputs change. Extend `transform_oracle_tests.rs`, `placement_tests.rs`, `liquify_tests.rs`, `tests/native_effects/`, session Recorder and current export provider-failure fixtures.

GTK uses its private compositor runner; Web uses a rebuilt client and headed WebGPU:

```sh
cargo test --locked -p layer-linux
bash tools/performance/workspace-motion.sh gtk --native-test=native_canvas_bar_modes
bash tools/performance/workspace-motion.sh gtk --native-test=native_effect_colors_gradients_and_retained_controls
bash tools/performance/workspace-motion.sh gtk --native-test=native_composite_histogram_updates_without_changing_the_drawing
bash tools/performance/workspace-motion.sh gtk --native-test=native_export_sizes_preserve_master_and_release_cancelled_dialogs
bash apps/layer-web/build.sh
node --test apps/layer-web/{run,package,frame,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog}.test.mjs
bash tools/performance/workspace-motion.sh web --image-placement
bash tools/performance/workspace-motion.sh web --adjustments
bash tools/performance/workspace-motion.sh web --photo-edit
bash tools/performance/workspace-motion.sh web --portable-photo
bash tools/performance/workspace-motion.sh web --export-metadata
```

Run each journey separately, using owned ports/profiles/displays. Add assertions to these harnesses and register new routes before documenting new executable flags. Headless screenshots alone may omit WebGPU pixels.

Android: in `apps/layer-android`, run `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a`. For installation, first build with the private application ID supplied by `tools/devices/devices.py appid`, then reserve the intended tablet and route **every** device command through `tools/devices/devices.py run`. Follow [device tests](android.md#device-tests), extend `AndroidInteractionTest`, `AndroidRasterTest`, `AndroidHostTest` and the canvas-bar benchmark, and read instrumentation's `OK`/`FAILURES!!!` and skips. Exit status alone is insufficient. Never install over or clear `art.capycanvas`.

Apple/Windows shared checks and later physical tests follow [Apple](apple.md#tests) and [Windows](windows.md). A Linux bridge build is not Metal, D3D12 or native UI qualification.

## Performance and resource qualification

Use [targets](../PERFORMANCE_TARGETS.md) and [measurement rules](../performance/measuring.md). Measure the modified path on its reference hardware and canvas; historical tables do not qualify a new build.

| Tier/reference | Canvas | Motion target and minimum presentation | Moving p99 gap |
| --- | --- | --- | --- |
| Low: TCL TAB 11 Gen 2 | 4248×2832 | 60 Hz; ≥57 presented fps | ≤33.3 ms |
| Mid: Wacom MovinkPad 11 | 6000×4000 | 90 Hz; ≥85.5 presented fps | ≤22.2 ms |
| Top: Wacom MovinkPad Pro 14 | 9504×6336 | 120 Hz; ≥114 presented fps | ≤16.7 ms |

Measure ≥3 warm 5–10-second gestures with the tier photo, release/benchmark build, default workspace/glass, thermal status 0, exact pipelines warmed and one undone priming gesture. Keep Fit/100%/high zoom, Navigator on/off and diagnostic tracing runs separate. Record commit and executable hashes, device, input path, photo hash, color/depth/blending, camera, brush size and active panels. Keep raw results in artifacts and current headline values in the tier tables.

Affected rows: photo and paint transform body/handles/pivot/nudge/Warp, multi-target movement, Curves/Levels/hue/local-filter sliders, Gaussian reach, gradient geometry/stops, sampler drag, comparison divider and pan/zoom with panels. Measure Histogram/Info plus ten points, cold-source picker101, Auto latency, source-guide preparation, export wall time/cancel and full resource lifetime separately. Measure existing baked Liquify only when changed shared code affects its frame path, under the existing brush targets; deferred live Liquify adds no qualification row.

Transform press→first motion, filter change→preview p95, and ordinary tool/menu response must be ≤100 ms. Report CPU submission, GPU completion, fresh input consumption and actual presentation separately. For any affected brush regression, cached/refinement frames do not count as fresh input updates. A background analysis must yield to new contact. Full-screen effects use display-resolution motion evaluation or a justified bounded approximation; full native canvas processing every frame is not the default.

The existing offscreen 24MP transform latency tests use fixed 120Hz assertions and cannot qualify tablet presentation. Android `effects` and `spatial-effects` benchmark scenarios must run separately; selecting both chooses only spatial work. Collect each invocation's output before the next because the harness clears its output directory. Existing `photo` measurements may exercise destructive transforms: add explicit retained-placement scenarios.

No operation may OOM. Reserve mesh, guide, dual-view, LUT, staging, history and export bytes before admission and test allocator ceilings. Preserve existing bake baselines: the documented 24MP/6GB XP-Pen journey aims for <~1.5GB settled PSS, <~2.5GB preview→Apply and ≥1GB system available; GTK Frequency Separation preview adds <200MiB and Apply its new result pages plus ~300MiB. These are existing scenario bounds, not blanket guarantees for new retained resources. Do not sum allocator, GPU and PSS numbers that overlap.

Existing baseline failures remain visible. An unchanged but below-target result proves only no measured regression; it does not pass the target. A reference device/output path limited to 60Hz cannot qualify 90Hz presentation. Record such a blocker and investigate the relevant route; do not substitute a faster desktop result or treat historical measurements as current qualification.

## Completion and documentation

Each packet returns changed files, behavior, tests with results, measured evidence, removed paths, remaining risks and unverified hosts. Check required behavior against code and tests, not handoff prose.

Before closing first delivery:

- All scope IDs in the overview have acceptance evidence on GTK/Web/Android.
- Format visitors, cancellation and cross-feature fixtures pass; Apple/Windows consumers compile and F1–F5 remain tracked.
- No competing page navigation, gradient model, host sizing policy or first-wave CPU Histogram survives. The existing baked Liquify production route remains supported.
- Current document/rendering/runtime-filter/project-format/UI/export guides describe the actual behavior; the remaining roadmap reflects only delivered work.
- Current tier tables contain measured results or explicit open gates, never projected rates.
- Move durable contracts into their owning guides and delete completed implementation plans. Keep reports, inventories and handoffs in `artifacts/` or `*.local.md`.

No push, deployment or device use is part of preparing this specification.
