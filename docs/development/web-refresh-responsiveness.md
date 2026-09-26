# Web refresh responsiveness

Implemented and measured on 2026-09-25. The reported case was refreshing
<https://editor.capycanvas.art/> with a moderate image, seeing controls quickly,
then waiting tens of seconds for document commands and “Recover drawing?”.

The cause was both unnecessary computation and a readiness dependency. Startup
fetched the already embedded filter library and validated/warmed its entire
namespace. That transaction disabled document commands and blocked recovery,
while unused brush/effect compilation competed with drawing. Recovery discovery
also waited for a 15-second autosave tick. The image was decoded only **after**
accepting recovery; it did not explain the long pre-prompt delay.

Web now prepares dependencies of the current document and tool, and visible
previews when idle. It does not warm the unused catalog. Android and GTK now
share this policy through the [shared shader framework](shared-shader-readiness.md);
Apple/Windows host adoption is separate. No broad document-idle guard was weakened.

## Refresh sequence and changed dependencies

| Order | Work | Readiness and scheduling |
| --- | --- | --- |
| 1 | Load HTML/CSS, JS, icons and Wasm; instantiate the blank session and build controls. | Streaming compilation/cache and model/DOM work remain. `capy.startup.ui` records controls/layout. Recovery pixels are not loaded here. |
| 2 | Adopt workspace preferences/layout through the workspace worker and IndexedDB. | GPU initialization waits for workspace readiness, capped at one second. Recovery discovery starts at this boundary, independently of autosave. |
| 3 | Acquire the GPU device, create the renderer/presenter and submit paper. | `capy.startup.canvas` is host-observed submission, not scanout. The compiler awaits GPU completion of paper before admitting later work. |
| 4 | Prepare document dependencies, then the selected brush and physical eraser. | Required priorities 2/3; up to four pipeline promises per batch. Native writeback prepares only the document's bit depth. Selected procedural masks are generated once. |
| 5 | Offer recovery after listing old keys and reaching document parking readiness. | Requires the GPU, a visible page and no conflicting document operation/modal dialog. Listing/get/delete do not instantiate image-codec Wasm. The 15-second timer only checkpoints open drawings. |
| 6 | After Recover: read/decode the project, prepare its private renderer and actual effects, upload/validate tiles, adopt the drawing and checkpoint it. | Required dependencies progress independently of optional previews. Existing ownership locks and durable-checkpoint-before-origin-retirement policy remain. |
| 7 | Prepare visible filter previews and thumbnail readback dependencies. | Optional priority 5, one job per admission, after 200 ms without input. Host contacts, movement, keys/wheel, menus, dialogs, popovers and hidden pages defer admission. Rust also checks strokes, gestures, pending input and unfinished edits. |
| On demand | Select a previously unused brush/effect, start a transform, or import a package. | Readiness tracking continues after startup. Required dependencies prepare asynchronously before use. Runtime package validation remains atomic at priority 4. Cached effects survive normal revision changes and undo. |

The startup catalog fetch/finalizer and timer-driven recovery initialization
were removed. Identical Web library-only updates also avoid a validation
transaction; genuine package changes retain validation/publication rules.
Layer thumbnails wait for their own readback dependencies instead of global
startup completion. The existing compiler queue, dependency tracker and shared
preview-idle policy are reused; there is no second scheduler or renderer worker.
Net code growth is for continuing first-use readiness, host admission and
regression coverage, while obsolete startup paths are deleted.

Relevant implementation:
[Web scheduler](../../apps/layer-web/app.js),
[recovery](../../apps/layer-web/document-recovery.js),
[project preparation](../../apps/layer-web/src/documents.rs),
[shared requirements](../../crates/layer-render-wgpu/src/startup.rs),
[browser compiler](../../crates/layer-render-wgpu/src/startup_web.rs),
[preview preparation](../../crates/layer-render-wgpu/src/filter_previews.rs).

## Huion measurements

Physical Huion Kamvas Pad 12 (KP1202), Android 16, Chrome 143.0.7499.192,
1200×680 CSS viewport at DPR 2. The tab reports a desktop Linux user agent;
ADB and the browser endpoint identify the physical Android Chrome installation.
Thermal status was 0 at sampled checks. Optimized `dev-perf` builds were served
through USB at dedicated local origins; production-origin storage was untouched.

Normal HTTP and browser/driver caches were retained. These are local cached
refreshes, not production network or cold-driver measurements. The frozen
baseline and final Wasm hashes are retained in the
[compact measurement record](measurements/huion-refresh-2026-09-25.json).

| Milestone from navigation | Baseline, two passive refreshes | Final, two passive refreshes |
| --- | ---: | ---: |
| Controls/layout | 1.15 / 0.55 s | 0.56 / 0.51 s |
| Workspace ready | 1.53 / 0.73 s | 0.74 / 0.68 s |
| Paper submitted | 2.44 / 1.54 s | 1.07 / 0.95 s |
| Document dependencies ready | 3.35 / 2.36 s | 1.24 / 1.18 s |
| Current brush ready | 6.23 / 5.12 s | 1.61 / 1.59 s |
| Startup work complete | **47.19 / 48.06 s** | **1.70 / 1.67 s** |
| Pipeline API calls | **203** | **37** |
| Unused effect pipeline calls | **86** | **0** |
| Document command availability | Disabled during library validation | Enabled in every sampled state |

The 166 eliminated pipeline calls are an **82% reduction**, independent of
cache timing. Four of those are unused writeback bit-depth variants. A final
refresh bypassing HTTP cache completed in 2.21 s; this does not clear driver
caches. An earlier valid implementation with 41 pipelines took 5.75 s on its
first reload, then 1.86–1.88 s. Cache state matters, so the lowest elapsed times
must not be presented as a universal startup guarantee.
The frozen baseline was repeated after all final timing runs, with caches still
retained: it took **44.80 s**, created 203 pipelines and kept document commands
disabled during catalog validation. Its recovery prompt appeared at 44.90 s.

The recovery fixture was a 2048×1536 gradient/checker PNG (2,818,792 bytes), with
no effects. Its natural prompt moved from **51.94 s to 1.35 s**. Listing took
252 ms in the final capture and finished before document parking was ready.
Acceptance-to-adoption was 831 ms; acceptance-to-GPU-completion was **949 ms**.
The restored dimensions and image were checked. The final capture contained a
second fixture record from autosave; only the first restore was timed.

A separate final menu run opened/populated all 29 scripted probes, including
before brush readiness (two-animation-frame p95 21.2 ms, maximum 29.3 ms).
Command availability and DOM menu opening are distinct
from physical input-to-display latency.

### Drawing during startup

The same nominal 120 Hz CDP pen replay starts immediately after brush readiness
for seven seconds, then repeats for five seconds after startup. Each stroke is
undone and the blank drawing must return to unmodified state.

| Metric | Baseline early contact | Final early contact | Final steady contact |
| --- | ---: | ---: | ---: |
| Delivered moves/s | 32.87 | 45.85 | 45.30 |
| CPU frame duration, p95 | 10.5 ms | 6.0 ms | 6.0 ms |
| Latest consumed sample age at CPU submission, p95 | 61.2 ms | 34.0 ms | 33.0 ms |
| New pipeline calls during contact | 24 | **0** | **0** |

Final early drawing is within 10% of steady-state p95 sample age. CDP delivery
and acknowledgement limit event throughput; this is evidence about contention,
not physical stylus latency or display scanout. Statistics stop at pointer-up
and exclude samples predating the contact. No thermally throttled-device claim
is made.

## Acceptance checks and limits

Acceptance targets are dependency and responsiveness properties, rather than a
single millisecond promise across all hardware: no unused catalog/brush GPU
warmup, no catalog lock on initial document commands, recovery discovery without
timer delay, asynchronous readiness on first use, no optional admission during
contact, and early drawing within 10% of steady-state p95 sample age.

Validation covers the optimized Wasm build; 655 shared UI tests; shared host
compilation; eight startup, 26 preview, ten native writeback and two effect
shader tests; package loading; and ten Git guard tests. Four existing hardware
benchmark cases remained ignored by their test annotations. GPU tests ran
serially with hardware access after an initial sandbox driver crash.
On the Huion, staged startup/first-use/reuse, error/restart and package rejection,
all 23 brush presets with visible marks and exact undo/redo, queued preview
pause/resume during a held contact, real preview pixels and cache lifecycle,
and automatic multi-drawing recovery passed. Raw logs and
screenshots stay under ignored `artifacts/web-refresh/huion/`.

Remaining limits matter as the catalog grows:

- Embedded UI metadata and WGSL still increase bundle/parse/memory cost. This
  change removes redundant loading and unused GPU compilation, not source bytes.
- First use of a new brush can still generate its selected procedural texture
  on the main thread. Prebaking immutable masks or moving those recipes to a
  worker is a separate optimization if first-selection stalls remain material.
- Runtime package parsing/link validation and shader source preparation still
  consume CPU. Used pipeline caches are not given a new global memory bound.
- A running GPU/compiler job cannot be preempted. Admission pauses subsequent
  optional jobs; required tool/document work can still take time.
- Export, selection and other operation-specific kernels retain existing
  first-use paths. This change does not make every renderer operation worker
  owned or eliminate every synchronous pipeline outside startup/brush/effects.

## Earlier Wacom investigation

The initial investigation used a Wacom MovinkPad 14, Android 15, Chrome
153.0.8010.52. Baseline workspace readiness was 0.41–0.44 s and brush readiness
2.60–2.75 s, but document commands waited until 19.70–20.28 s. An experimental
catalog bypass plus early discovery moved recovery from 20.06 s to 1.27 s.
Pausing optional compilation reduced startup p95 sample age from 21.1–24.5 ms
to 6.9–7.1 ms. These were diagnostic ablations, not final implementation results;
see [Wacom measurements](measurements/wacom-refresh-2026-09-25.json).

A separate Chrome timeline found GPU-process tasks of 306 ms during initial
setup and 51–55 ms during later warming, while the main-thread Long Tasks
observer saw only one 58 ms task. CPU sampling also identified procedural
contact-paper generation. Main-thread Long Tasks alone miss relevant stalls.

The deployed startup/recovery JS was compared on 2026-09-25: service-worker
version `c2ef37df1bb687fd1e96c2765ae726ca79f92bbd00d40b473b4100b41aab8ac4`.
Recovery matched the investigation checkout byte-for-byte; startup/scheduling
matched after asset fingerprint normalization. This did not establish identical
Wasm binaries or the user's exact cached version.

## Reproducing

Use a dedicated origin and [Web development](web.md) USB forwarding. Never use
an artist's working tab for fixture setup. The runner reloads only its matching
URL; it does not accept/discard recovery or clear storage.

```sh
LAYER_DEVICE_CDP=http://127.0.0.1:9278 \
LAYER_WEB_URL=http://127.0.0.1:4297/ \
LAYER_REFRESH_MS=15000 LAYER_REFRESH_RUNS=2 \
LAYER_TEST_ARTIFACTS=artifacts/web-refresh/passive \
node tools/performance/web-refresh.mjs
```

Use `--bypass-http-cache` once after rebuilding to avoid mixing new JS with old
Wasm. It does not clear shader caches. Repeat separately with `--probes` for
menus; use `--strokes` on an empty dedicated drawing and at least 30 seconds for
the final build (75 seconds for the old baseline). `--timeline --profile` adds
Chrome GPU/main-thread attribution and profiling overhead. Software/GPU-disabled
desktop runs are harness checks only.

For recovery, open a known image and let its checkpoint finish, then refresh.
Accept Recover during capture to retain both initial and restored renderer
milestones in `markEvents`. Record prompt, accept, adoption, GPU completion and
durable replacement separately. Run the device harness's `--staged-startup`,
`--filter-previews`, `--contact-brushes` and `--drawing-tabs-recovery` cases for
correctness; elapsed-time improvements alone are insufficient.
