# Apple performance observations

The iPad and Mac use the same optional recorder, serial render owner and Rust
GPU timer. Ordinary launches leave recording and timestamp submissions disabled.
The native CAMetalLayer subclass observes the drawables acquired by wgpu and
uses Metal's `addPresentedHandler` and `presentedTime` to record actual display
presentation. A display-link tick or completed Rust call is not a presentation.
The iOS Simulator SDK does not expose drawable IDs or presentation callbacks;
simulator runs omit these events and cannot establish presentation acceptance.

One ten-minute 4K watercolor session is recorded on each physical platform below.
Complete workload-matrix results, physical input-to-pixel evidence and calibrated
instrumentation overhead remain required on both platforms. Following the user's
2026-09-11 clarification, current Mac validation targets **90 Hz (11.11 ms)**;
Mac 120 Hz presentation testing is deferred until suitable hardware is available
and does not block current Mac milestones. The iPad target remains **120 Hz
(8.33 ms)**. Keep failing workloads and unsupported measurements visible.

## Incremental workspace publication

Both Apple editors now consume the shared `workspace_update` contract. A full
model publication establishes the revision used by retained controls. Ordinary
tab/floating motion publishes absolute geometry, tab previews and drop hints;
native view placement moves hit areas, clipping and live Navigator allocations
together. Down, tear-off, release, cancellation and other model changes retain
their normal full publication. Every input phase still reaches Rust; history
and durable persistence remain shared behavior.

The paired C-ABI fixture gives separate compatibility/incremental owners the
same actions on both Apple presets. Complete snapshots match exactly after
removing the new `workspace_update` field. Across 32 floating moves, actual
serialized payload totals are:

| Preset | Compatibility bytes | Incremental bytes |
| --- | ---: | ---: |
| iPad | 2,675,515–2,675,520 | 5,403–5,440 |
| Mac | 2,681,947–2,681,952 | 5,403–5,440 |

The ranges cover cancellation and commit cases. This is about a 99.8% wire-size
reduction for this fixture; it is not a CPU/GPU timing or frame-rate result.
Geometry matches the compatibility layout on each move, intermediate updates
carry no durable persistence, and completion plus workspace Undo/Redo match.

The invisible AppKit workflow performs 24 floating moves per Apple preset,
checking real native drag/resize hit rectangles, tab bounds/clips and the live
Navigator allocation/image/clip while retaining its SwiftUI identity and the
panel models. A separate SwiftUI observation probe renders ten movements
without rebuilding unrelated command, panel, menu, layout, camera, other-group
or tab-visibility readers. Rejected revisions and camera-bearing or camera-less
updates have direct coverage. These checks exercise shared Apple code; UIKit
touch workflows and physical presentation remain separate acceptance evidence.

Reproduction commands are in the [Apple README](README.md). Raw measurements and
captures remain in ignored artifacts. Sustained 90 Hz Mac / 120 Hz iPad cadence,
isolated GPU timing, physical input latency and the remaining workload matrix
are still open. No hardware performance improvement is claimed from this
transport fixture alone.

## Repeatable native drawing workloads

Set `CAPY_WORKLOAD` to run a synthetic fixture through the same serial input
owner, display link, Rust renderer, editor panels, history and recovery writer
as ordinary drawing. Use a separate benchmark bundle identifier for device
installs, and a separate DerivedData directory. The opt-in workload additionally
uses a new private persistence root under `Caches/CapyPerformanceSessions` for
every editor instance. It never reads or replaces the artist's normal settings,
workspace or recovery copies. Ordinary launches have no workload timer.

| Profile | Document | Paint layers, excluding paper | Brush / diameter | Synthetic prediction |
| --- | --- | --- | --- | --- |
| `ink` | 2048×2048 | 1 | G-Pen / 24 px | Off |
| `ink-predicted` | 2048×2048 | 1 | G-Pen / 24 px | On |
| `wet-watercolor` | 2048×2048 | 1 | Wet Watercolor / 320 px | On |
| `layered-4k` | 4096×4096 | 8 | G-Pen / 24 px | On |
| `wet-watercolor-4k` | 4096×4096 | 8 | Wet Watercolor / 320 px | On |

The 4K cases retain seven translucent full-document underpaint layers and the
active paint layer. Setup creates the document through the shared project job
and uses ordinary UI actions for layers, fills, brush selection and size. The
editor stays in its full default workspace. No brush quality settings are
reduced. Ten seconds of drawing warm up the fixture before measurement.

The versioned trajectory is in `DrawingWorkloadPlan.swift`: deterministic curves
inside the document, pressure varying from 0.25 to 1, 240 samples/second, 1.5-second
strokes and 0.1-second lift gaps. A main-run-loop producer delivers coalesced
batches at 120 callbacks/second independently of render admission. Predicted
points use the same native prediction path and remain visual-only. A delayed
producer catches up; a backlog of one second aborts the run instead of silently
dropping samples or lowering the input rate. Interval maxima expose producer
lateness, which is synthetic scheduling delay, not a physical Pencil metric.

`CAPY_WORKLOAD_SECONDS` is the measured duration after warm-up (default 600;
allowed 1–1800). The run records separate setup, warm-up, measured, end and
postlude markers. A ten-second postlude observes pen-up, deferred GPU work,
recovery and idle transitions; its end does not prove that rendering drained.
Trace recording defaults to the requested measurement plus a 140-second allowance
for setup/warm-up/postlude, but normally finishes at the postlude. An explicit
`CAPY_TRACE_SECONDS` overrides that limit, and can therefore truncate a run.

Launch the built, isolated Mac app through Launch Services to foreground it;
an occluded window cannot supply presentation evidence:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
python3 apps/layer-apple/scripts/prepare.py
python3 apps/layer-apple/scripts/project.py
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -configuration Release -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceMac \
  PRODUCT_BUNDLE_IDENTIFIER=art.capycanvas.apple.mac.performance \
  CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES build
open -n --env CAPY_WORKLOAD=ink --env CAPY_WORKLOAD_SECONDS=600 \
  --env CAPY_TRACE_DIRECTORY="$PWD/artifacts/performance/mac-ink" \
  apps/layer-apple/DerivedData/PerformanceMac/Build/Products/Release/CapyCanvas-Mac.app \
  --args -ApplePersistenceIgnoreState YES
```

For an installed iPad benchmark bundle:

```sh
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-iPad -configuration Release -destination 'generic/platform=iOS' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceDevice \
  PRODUCT_BUNDLE_IDENTIFIER=art.capycanvas.apple.ipad.performance \
  DEVELOPMENT_TEAM="$CAPY_APPLE_TEAM" 'CODE_SIGN_IDENTITY=Apple Development' \
  -allowProvisioningUpdates build
xcrun devicectl device install app --device DEVICE_ID \
  apps/layer-apple/DerivedData/PerformanceDevice/Build/Products/Release-iphoneos/CapyCanvas-iPad.app
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_WORKLOAD":"ink","CAPY_WORKLOAD_SECONDS":"600"}' \
  art.capycanvas.apple.ipad.performance
```

Keep the benchmark window visible and leave its editor untouched. Only one
benchmark window should run on each device. Copy traces from the benchmark's
container using its bundle identifier. Trace metadata labels input as synthetic
and records the fixture specification; reports separate the measured interval
from startup and postlude. `measurement_completed` means a complete, non-aborted
interval was recorded. It does not imply performance acceptance, pixel inclusion,
physical input latency, or successful coverage of the other profiles. Review
rejected input, frame errors, missing presentations, readiness and all warnings.
The report retains the first and last presentation's distance from the measured
interval boundaries, denied display-link admissions, frames without viewport
submission and missing/zero-time completions. A completed input producer cannot
establish continuous rendering if its window becomes occluded.

## Physical ten-minute baseline: 2026-09-11

The existing Mac ten-minute trace was re-analyzed for the current 90 Hz target;
this is a new report over the original capture, not a new device run. CPU p99 is
6.083 ms, with eight owner-service samples above 11.11 ms. Continuous presentation
intervals have p50/p95 11.111 ms, p99 22.222 ms and maximum 77.778 ms; 746 of
48,890 continuous intervals exceed the 90 Hz period plus the existing 5%
cadence tolerance. These remaining gaps are visible at the current target and
are not waived by deferring 120 Hz. The report is stored locally under
`artifacts/performance/refresh-target-review/mac90-sustained.json`.

Both Release apps completed version 1 of `wet-watercolor-4k`: 4096×4096, eight
paint layers plus paper, Wet Watercolor at 320 px, synthetic pressure and
prediction. Each ran ten measured minutes after ten seconds of drawing warm-up,
followed by the ten-second postlude. The shared renderer and UI include incoming
changes through `d8a130b`. No other build or GPU test ran during measurement.
The iPad viewport was 2752×2064 physical pixels; Mac was 2400×1740 after window
layout settled. Separate benchmark bundles and private persistence roots were used.

| Measured interval | Physical iPad | Native Mac |
| --- | ---: | ---: |
| Duration, seconds | 600.000 | 600.009 |
| Display maximum, Hz | 120 | 90 |
| Nonpredicted samples delivered | 135,001 | 135,003 |
| Actual presentations | 65,407 | 49,266 |
| CPU owner service p50 / p95 / p99 / max, ms | 2.180 / 3.643 / 9.120 / 17.779 | 3.312 / 4.927 / 6.083 / 13.038 |
| CPU service over 8.33 ms | 1,092 | 28 |
| GPU queue span p50 / p95 / p99 / max, ms | 5.500 / 8.279 / 9.423 / 19.037 | 8.228 / 10.367 / 11.632 / 23.255 |
| Missing GPU samples | 204 | 221 |
| Presentation interval p50 / p95 / p99 / max, ms | 8.333 / 8.334 / 33.332 / 125.003 | 11.111 / 11.111 / 66.667 / 111.112 |
| Positive display-link target lateness p50 / p95 / p99 / max, ms | 8.338 / 8.352 / 16.670 / 28.032 | 22.297 / 33.411 / 33.419 / 66.741 |
| Display-link ticks denied admission / total | 1,132 / 66,914 | 241 / 49,882 |
| Producer interval-maximum lateness p50 / p95 / p99 / max, ms | 26.783 / 36.283 / 39.233 / 44.115 | 47.481 / 55.118 / 56.533 / 57.713 |
| Peak physical footprint, MiB | 1,798.05 | 1,910.91 |
| First-to-last measured footprint growth, MiB | +0.17 | +148.83 |
| Observed thermal states | Nominal | Nominal |

Presentation distributions retain the deliberate pen-up gaps; their high
percentiles must not all be called missed drawing frames. Every measured
presentation exceeded its display-link target by more than 1 ms. Neither target
lateness nor producer scheduling delay is physical input-to-pixel latency. GPU
queue spans include CPU submission gaps and the uncalibrated profiler; skipped
readbacks may bias their tails. They do not isolate GPU execution time.

Both measured intervals have zero rejected input batches, missing presentation
callbacks and zero-time presentations. The full traces have zero renderer errors
and recorder overflow. Each interval includes 375 admitted frames without a
viewport submission. The first/last actual presentations lie within 8 ms of
both interval boundaries on both hosts. The Mac's completed canvas was captured
after export and visibly contains the synthetic paint; no per-frame pixel oracle
or physical input latency assertion is inferred from that capture. Memory
includes document/history, ordinary recovery work and recorder storage; the Mac
growth remains to be characterized. Nominal thermal samples do not establish
the absence of clock-frequency changes.

These results leave the 8.33 ms tail budget and complete performance acceptance
open. The other four profiles still need ten-minute runs on both platforms,
along with physical input, overhead calibration and Mac 120 Hz presentation
evidence on a suitable display configuration. Artifacts stay local under
`artifacts/performance/{ipad,mac}-workload-sustained`.

A preceding twenty-second ink experiment reduced the Metal drawable count from
three to two. On iPad it increased median owner service from 1.113 to 9.482 ms,
with median drawable acquisition at 8.305 ms and median presentation intervals
at 16.667 ms. The Mac's median target lateness improved, but CPU budget
exceedances increased. The experiment was reverted on both targets; these final
runs retain three drawables. The retained scheduling change publishes native
canvas-readiness accessibility updates once per attached surface instead of
every submitted frame. The synthetic producer also leaves lift gaps asleep.
The pilot changed multiple factors, so it does not isolate this change's benefit.

## Large document replay safety

A direct Metal regression reproduced the startup device loss seen while
investigating frame scheduling: replaying seven filled 4096×4096 paint layers
in one frame attempted to create 4097 outstanding native command buffers.
The reproduction uses no native window or frame recorder. Incremental layer
fills succeeded, which explains why setup timing could hide the problem.

The shared renderer now records at most 512 render/compute passes per chunk,
then finishes and submits chunks in order after closing staging uploads.
Upload completion stays attached to the last chunk. The complete replay matches
every pixel of the incremental 4K image, with renderer telemetry off and on.
See the [renderer regression command](../../crates/layer-render-wgpu/README.md).
This fixes submission capacity; it does not close the sustained frame-time or
presentation requirements above. The published Apple scheduler is unchanged.

## Display scheduling comparison — 2026-09-11

The shared CAMetalDisplayLink experiment was **not adopted**. It supplied each
callback's drawable to the serial render owner and separated the CPU commit
deadline from the presentation target. The shorter CPU frame times did not
establish better presentation: the physical iPad repeatedly reported skipped
drawables with both requested rendering windows, including with GPU timing
disabled. The published hosts retain CADisplayLink and ordinary wgpu acquisition.

Each row below is a separate twenty-second measured `wet-watercolor-4k` run,
with ten-second warm-up and postlude. CPU times include the whole owner service;
admission-to-display times use actual nonzero Metal presentation callbacks.

| Host / requested Metal latency | GPU timer | Actual presentations | Zero-time presentations | CPU p99, ms | Admission-to-display p99, ms |
| --- | --- | ---: | ---: | ---: | ---: |
| iPad / 1 | On | 2,156 | 49 | 4.832 | 24.957 |
| iPad / 2 | On | 2,137 | 54 | 4.640 | 24.970 |
| iPad / 1 | Off | 2,154 | 57 | 4.387 | 24.916 |
| Mac / 1 | On | 1,642 | 0 | 6.371 | 33.380 |
| Mac / 2 | On | 1,655 | 1 | 5.875 | 33.376 |
| Mac / 1 | Off | 1,655 | 1 | 6.046 | 33.377 |

All six intervals completed with no renderer errors or missing presentation
callbacks. The Mac reports 90 Hz and the iPad 120 Hz. Changing the requested
latency did not change the observed target-to-deadline separation: approximately
22.222 ms on Mac and 8.333 ms on iPad. These observations do not demonstrate that
the requested latency is the actual end-to-end latency. Timer-disabled rows have
no GPU-duration samples; these short pairs do not calibrate all recorder overhead.

The experiment also exposed an unsafe explicit CATransaction commit on the
render queue: on iPad it invoked UIKit layout off the main thread and crashed
during attachment. That trial was removed before the six completed runs above.
A failed run produced no new trace; an older container file was rejected as
evidence. Collection must check file freshness and configuration against the
specific launch, not assume that the latest existing file belongs to it.

Local raw evidence remains under `artifacts/performance/{mac,ipad}-metal-link-`
`{safe1,safe2,no-gpu}`. The experiment is saved locally, not shipped in either
Apple target. Further scheduling work needs a new explanation and presentation
evidence; lower CPU measurements alone are insufficient.

After restoring CADisplayLink and integrating shared changes through `a7c048c`,
the same Release binaries were run once with GPU timing enabled and once with
it disabled. All four twenty-second intervals completed with zero rejected
input batches, missing callbacks or zero-time presentations:

| Stable host | GPU timer | Actual presentations | CPU p99, ms | Admission-to-display p99, ms |
| --- | --- | ---: | ---: | ---: |
| iPad | On | 2,190 | 9.077 | 25.965 |
| iPad | Off | 2,188 | 9.120 | 25.966 |
| Mac | On | 1,639 | 6.232 | 32.897 |
| Mac | Off | 1,655 | 5.873 | 32.928 |

The whole traces have no renderer errors or recorder overflow; startup/postlude
zero-time callbacks and omitted GPU samples remain in the reports. The iPad CPU
tail exceeds 8.33 ms with either instrumentation setting. Enabled GPU queue spans
have p99 9.389 ms on iPad and 10.363 ms on Mac and retain the overhead caveat.
This short pair does not establish a precise overhead correction or sustained
performance acceptance. The stable controls include subsequent shared layout
changes, so their comparison against the earlier Metal-link runs is not a
strictly identical-source scheduler-only experiment.
Reports are in ignored `artifacts/performance/{mac,ipad}-scheduler-control`
and corresponding `-no-gpu` directories. The Mac's completed synthetic painting
was captured and inspected; test apps were closed after collection.

## Snapshot transport

CPU sampling of the isolated 4K watercolor workload identified snapshot
construction, serialization and Foundation decoding in the owner publication
path. These profiling runs are diagnostic: sampling can interrupt the process,
so their frame timings are not used as an unsampled performance baseline.

Apple's snapshot request now uses `NativeHost::take_snapshot_bytes`, writing
UTF-8 directly from the shared models. Other native callers retain the value
API. Both paths use one schema and the same change detection, camera patches
and workspace-persistence policy. The writer preserves the value transport's
exact widening of `f32` numbers to `f64`; it does not shorten color or geometry
values. Failed serialization leaves the pending update unacknowledged.

Forty independent snapshots captured before the refactor match the new decoded
wire payloads exactly. They cover both Apple presets, fractional brush/color
values, camera changes, all five settings pages, collapsed drawers, workspace
history, Zen, surface errors and resize. The fixture emits the actual bytes
without an extra parse/re-encode step. Direct regressions also cover native
Android and Windows projections, unchanged-state suppression, mixed value/byte
consumers and serialization failure for full/camera/workspace updates.

After the final integration through `6eb0418`, all 40 current value/byte payload
pairs still match exactly. The original reference differs only by that incoming
change's removal of the theme-toggle item from the shared View menu. Every other
field and numeric value is preserved; the theme-toggle command remains in the
catalog. The original reference and the explicit difference report stay local.

```sh
mkdir -p artifacts
cargo run --release -p layer-host --example snapshot-transport \
  > artifacts/snapshot-value.json
cargo run --release -p layer-host --example snapshot-transport -- --stream \
  > artifacts/snapshot-stream.json
cargo run --release -p layer-host --example snapshot-transport -- --benchmark \
  > artifacts/snapshot-benchmark.json
```

On the development Mac, four alternating rounds of 200 full snapshots per mode
give these CPU transport measurements. Both rows run on Mac hardware with the
indicated platform's models; the iPad row is not physical iPad timing. Each
sample includes construction, encoding and destruction. The test excludes
Swift decoding, rendering and presentation and cannot establish a frame budget.

| Snapshot preset | Value path median / p99, ms | Direct path median / p99, ms |
| --- | --- | --- |
| iPad | 0.509 / 0.599 | 0.168 / 0.199 |
| Mac | 0.512 / 0.568 | 0.176 / 0.197 |

Matching twenty-second physical `wet-watercolor-4k` runs, with GPU timing
disabled and no CPU sampler, completed before and after the change. Each
retains warm-up, prediction, pen-up gaps and the full editor. These short pairs
are exploratory and do not establish sustained acceptance or precise overhead
calibration. Both builds use shared changes through `16c5886`; later integration
through `6eb0418` is outside these recorded hardware intervals.

| Host / transport | CPU owner p99 / max, ms | CPU frames over target | Time outside recorded Rust stages p99, ms | Actual presentations |
| --- | --- | --- | --- | --- |
| iPad / value | 7.713 / 10.925 | 22 | 2.415 | 2,220 |
| iPad / direct | 9.057 / 10.643 | 32 | 1.534 | 2,189 |
| Mac / value | 6.399 / 8.512 | 0 | 2.936 | 1,638 |
| Mac / direct | 5.508 / 9.057 | 0 | 1.844 | 1,641 |

The residual column subtracts all five measured Rust stages from owner service;
it is not an isolated publication timer. On iPad, drawable acquisition p99 rises
from 0.025 to 4.322 ms, and the overall CPU tail worsens despite the transport
improvement. Its continuous presentation interval p99 remains 16.667 ms and its
maximum rises from 33.332 to 50.001 ms. Mac retains continuous intervals up to
66.667 ms. None of these late frames are waived. All four measured intervals
have zero rejected input batches, renderer errors, recorder overflow, missing
presentation callbacks or zero-time presentations. Disabled GPU spans remain
unmeasured, and no physical input-to-pixel latency is inferred.

The detailed traces, stage distributions, memory/thermal observations and
comparison reports remain in ignored `artifacts/performance/*snapshot*` and
`artifacts/apple-snapshot-*` paths. This change reduces transport work; it does
not close the 90 Hz Mac / 120 Hz iPad performance gates. Further investigation
must include drawable waiting and presentation scheduling as well as the
remaining ten-minute workload matrix.

## Native UI lookup investigation

The shared Swift transport now reads Foundation-decoded dictionaries and arrays
in their original representation. Native Swift containers retain their Swift
lookup path. Previously, each decoded dictionary field access could bridge the
whole dictionary into Swift; an earlier iPad CPU profile attributes substantial
main-thread work to these lookups. This supporting change has not established
an overall drawing-performance improvement.

The recursive transport check covers 219,911 values, including all 40 captured
wire snapshots, plus native/Foundation containers, missing fields, array bounds,
Unicode, full-width unsigned integers, immutable replacements and round trips.
Both Release targets build, and the direct SwiftUI drawer/action check passes.
These checks establish transport compatibility, not complete UI workflow or
pixel parity.

A CPU-only command-field traversal of those 40 snapshots, repeated in four
alternating rounds of 100 traversals on the development Mac, averages 11.642 ms
with the old decoded-container lookup and 2.990 ms with the candidate. Fully
native Swift trees average 1.173 and 1.287 ms respectively; the representation
check has a small cost there. A discarded Foundation-only variant took 4.804 ms
for native trees, which is why the candidate preserves both lookup paths. These
are whole-traversal component timings, not single-frame or physical iPad times.

Forty-five-second physical `wet-watercolor-4k` runs before and after the lookup
change retain the full workspace, prediction, recovery, ten-second warm-up and
postlude. GPU timing is disabled. Source is `afa058a` plus the candidate for the
after runs. No CPU sampler or build runs during these measured intervals. The
pairs are exploratory; ordering, cache and thermal history are not calibrated.

| Host / lookup | CPU owner p99 / max, ms | CPU frames over target | Continuous interval p99 / max, ms | Long continuous intervals / total |
| --- | --- | --- | --- | --- |
| iPad / previous | 8.757 / 10.196 | 54 | 16.667 / 41.667 | 67 / 4,922 |
| iPad / candidate | 9.133 / 17.573 | 75 | 16.667 / 41.665 | 62 / 4,900 |
| Mac / previous | 5.395 / 12.076 | 1 | 22.222 / 66.667 | 47 / 3,696 |
| Mac / candidate | 5.499 / 7.348 | 0 | 22.222 / 55.556 | 47 / 3,709 |

All four measured intervals complete with zero rejected input batches, renderer
errors, recorder overflow, missing presentation callbacks and zero-time
presentations. Their full traces retain zero-time presentations outside the
measured interval: respectively 3, 1, 2 and 1 in table order. Continuous interval
counts use the existing target-period plus 5% tolerance. Mac remains evaluated
at 90 Hz and iPad at 120 Hz. The iPad CPU tail and both presentation gates remain
open; a faster lookup benchmark does not close them.

The ordinary before traces show many long presentation intervals with a long
gap between frame admissions and no intervening denied display-link tick.
The largest examples repeat shortly after stroke contact begins, with fast
drawable acquisition in the following frame. This motivates investigating main
run-loop/UI work as well as drawable waits; it does not by itself identify the
blocking function.

A headless hardware-backed state probe with both Apple presets confirms that
stroke down/up legitimately changes roughly 100 UI fields, mostly menu/command
enablement and toolbar/history controls; ordinary moves produce no new full
snapshot. Suppressing those boundary updates would lose behavior. A subsequent
25-second Mac Time Profiler run completes in a 27 MB bundle. In its segment after
15 seconds, the main thread has 718 ms of sampled weight; 437 ms includes
AttributeGraph updates, while JSON field access accounts for 48 ms inclusively.
These sampled categories overlap and are diagnostic, not frame-budget evidence.
This points the next investigation toward repeated UI graph work when applying
required updates. Both probes remain local under `artifacts/apple-ui-state-probe`
and `artifacts/apple-json-ui-profile-*`.

Separate 45-second Metal System Trace recordings generated about 35 GB of data.
Their analysis was stopped after prolonged CPU-heavy processing. They produced
no accepted isolated-GPU result and are excluded from the table. The completed
in-app 90-second traces recorded alongside them are diagnostic only. Partial
Instruments bundles, frame traces, captures, build logs and detailed comparison
reports remain in ignored `artifacts/apple-json-*`, `artifacts/apple-metal-*`
and `artifacts/performance/*json-lookup*` paths.

## Selective native UI observation

Both Apple editors now retain one canonical immutable JSON snapshot and expose
live, main-actor readers for fields and individual command, panel and menu
entries. Required changes in stroke-boundary enablement still reach the UI.
Unchanged readers no longer receive a whole-editor publication, and camera-only
patches preserve the command/panel/menu indexes. The in-app menu view also owns
its array reads, preventing menu enablement from invalidating the entire iPad
editor through the header. Document and lifecycle consumers explicitly take
immutable whole-state copies. Rust remains authoritative for values and actions.

All related projections are staged before any observation signals are sent.
Comparisons preserve key presence, nulls, Boolean/number distinctions, precise
integers, signed zero and array order. Only previously read fields need their
individual values compared; whole-object readers additionally detect changes
to unread fields. This avoids allocating observation nodes and recursively
comparing values for unused fields. Retained JSON values never become live
mutable state.

Standalone checks cover those semantics and all 54 local wire fixtures (40
deterministic transport snapshots plus 14 hardware-backed stroke snapshots).
An invisible SwiftUI hosting view verifies fresh rendered values, unchanged
unrelated bodies, camera patches and no-op snapshots, including a child that
retains a field reader without its parent rebuilding. Direct shared drawer,
window-presentation and document workflow checks also pass. These checks use
no system menu automation. They do not establish full visual or physical-input
acceptance.

An initial candidate compared every field. A completed 25-second Mac CPU profile
showed 577 ms of main-thread sampled weight in its segment after 15 seconds,
versus 718 ms in the preceding lookup-only profile. Inclusive AttributeGraph
update weight fell from 437 to 283 ms, while `EditorStore.receive` rose from 9
to 110 ms. These overlapping samples motivated the final restriction to read
fields; they do not measure the final version or establish a frame-rate gain.
The first candidate's clean 45-second pair likewise did not establish a cadence
improvement: Mac long continuous intervals increased from 47/3,709 to 55/3,721,
and iPad from 62/4,900 to 106/4,947. Raw profiles and intermediate comparisons
remain private in ignored `artifacts/apple-projection-*` and
`artifacts/performance/*ui-projection*` paths.

The final implementation incorporates the shared changes through `9019e23`.
All 307 relevant Rust checks pass (34 Apple bridge, 18 host, 255 UI; one existing
hardware-only host check remains ignored). The final 40 value/byte snapshot
pairs are identical, and their decoded values also match the preceding
checkpoint. Both Release targets build, and the signed iPad build installs and
launches. The command audit still covers all 62 commands on each host; inventory
coverage alone does not close their behavioral acceptance.

The final clean 45-second Mac `wet-watercolor-4k` run on the 90 Hz display
records 3,764 measured presentations. CPU owner service p50/p95/p99/max is
3.395/4.913/5.560/14.519 ms, with one frame above 11.11 ms. Continuous
presentation interval p50/p95/p99/max is 11.111/11.111/22.222/44.445 ms;
49 of 3,735 intervals exceed the target period plus 5% tolerance. The interval
has no rejected input, renderer errors, recorder overflow, missing callbacks
or zero-time presentations. There is one zero-time presentation outside it.
Measured footprint grows 15.25 MiB and thermal state stays nominal. This is a
short CPU/presentation observation with GPU timing disabled and does not
establish a cadence improvement, physical-input latency or sustained acceptance.

The final 45-second iPad run at 120 Hz records 4,995 measured presentations.
CPU owner service p50/p95/p99/max is 2.061/3.509/9.095/12.703 ms, with 87 frames
above 8.33 ms. Continuous interval p50/p95/p99/max is
8.333/8.334/16.667/33.334 ms; 83 of 4,966 intervals exceed the target plus 5%
tolerance. Its measured interval has no rejected input, renderer errors,
overflow, missing callbacks or zero-time presentations. The complete trace
contains six zero-time presentations outside that interval. Measured footprint
falls 7.41 MiB and thermal state stays nominal. GPU timing is disabled. Neither
the CPU tail nor presentation cadence meets the iPad acceptance target.

The preceding iPad launch produced no trace and subsequently disappeared from
the process list. Its short diagnostic CPU recording contained no samples;
the cause of termination was not established. That attempt contributes no
performance result. After confirming it had ended, the same installed binary
was launched with console logging. The successful run above logged preparation,
measurement completion and postlude before exporting a fresh trace. Console
logging was connected for that iPad run, with phase messages only; no profiler
or build ran during its measured interval. The final Mac run used no console
connection. This environmental difference and the short sample sizes limit
comparisons; neither host's results establish a frame-rate improvement.

The later Android presentation and independent workspace-storage changes through
`13d139c` are also integrated. They change no Apple source or existing dependency
lock entries; the new storage crate is not an Apple dependency yet. New shared
workspace-manager flows and tab-drag animation parity remain separate work.

The final delivery additionally integrates tab-drag and workspace-transition
changes through `f6c58a7`. Both Release builds, all 310 relevant Rust checks
(34 Apple, 18 host, 258 UI), the direct drawer workflow and the 54-fixture Swift
observation check pass. All 40 value/byte snapshot pairs still match the earlier
values exactly. Both final apps complete a five-second 4K drawing smoke interval
after warmup, followed by the normal postlude, without rejected input, renderer
errors or missing/zero-time presentations in that interval. This establishes
startup and drawing after integration; the 45-second timings above precede it.
The smoke intervals do not establish performance acceptance. All completed
benchmark processes are closed and the original Mac editor remains open.

## Capture locally

Build with `CAPY_CONFIGURATION=Release` for performance investigations. See
[README.md](README.md) for signing and build options. Debug captures are useful
for validating instrumentation but do not close performance gates.

Set `CAPY_TRACE_GPU=0` to retain CPU, input, memory and actual presentation
observations while disabling the GPU timestamp marker submissions and readback
polls. The default is enabled when tracing is requested; ordinary unrecorded
launches still create no frame timer. The trace header and report expose
`gpu_timing_requested`. Disabled GPU measurements remain null, with an explicit
warning; they must not be treated as zero GPU cost. This comparison isolates
the optional GPU timer, not the remaining recorder overhead. Use the same build,
workload, duration and display state for each pair.

```sh
CAPY_CONFIGURATION=Release bash apps/layer-apple/scripts/build.sh macos
open -n --env CAPY_TRACE_SECONDS=30 \
  --env CAPY_TRACE_DIRECTORY="$PWD/artifacts/performance/mac" \
  apps/layer-apple/DerivedData/Build/Products/Release/CapyCanvas-Mac.app
```

For iPad, build/install the Release app using the README commands and then:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_TRACE_SECONDS":"30"}' art.capycanvas.apple.ipad
```

Keep the app foregrounded through the interval and the two-second callback grace
period. Copy its local trace directory after export:

```sh
xcrun devicectl device copy from --device DEVICE_ID \
  --domain-type appDataContainer --domain-identifier art.capycanvas.apple.ipad \
  --source Documents/Performance --destination artifacts/performance/ipad
python3 tools/performance/apple_trace.py TRACE.jsonl --target-hz 90 \
  --output artifacts/performance/report.json
```

`CAPY_TRACE_SECONDS` must be finite, positive and no more than 3600. The default
output directory is `Documents/Performance` in the app container. The optional
`CAPY_TRACE_DIRECTORY` overrides it with a local writable directory. A finished
trace is a JSONL file; `.partial` means export did not finish. Recording starts
when the session owner is created, so startup is included. Shader/catalog/canvas
readiness and display-link activity transitions are recorded separately.

Artifacts are ignored by Git. Records contain timings, counts, memory, thermal
state and display dimensions; they contain no coordinates, artwork, document
names, account/team/device identifiers or input-device serials. Keep build,
signing, device-tool output and trace files local. Use placeholders in shared
commands and review staged source before pushing.

## Interpret the report

- CPU owner queue age measures the interval from display-link admission to
  serial-owner execution. Owner service includes the bridge and snapshot work.
  The five Rust stages are CPU preparation, drawable acquisition, viewport
  submission, present call and polling. They are not GPU execution durations.
- GPU timestamps bracket queue work across paint and viewport submissions,
  including CPU submission gaps. They measure a **GPU queue span**, not isolated
  GPU busy time. Three readback slots and a 256-result queue bound profiler work;
  full slots skip observations. The owner never waits for a readback. A bounded
  trailing poll drains the last result when the display link sleeps. Each marker
  pass performs a tiny storage write; empty passes produced zero counters on
  Metal. Counter resolution is deferred until the marker submission completes:
  resolving within that submission returned stale values on the tested Mac.
  Both marker work and extra submissions contribute profiler overhead. The older
  renderer telemetry also rejects zero counters; its empty-pass approach still
  needs migration and is not used for these Apple reports.
- Actual `presentedTime` values determine presentation intervals and lateness
  against the display link's target. Zero presentation time means skipped or
  unpresented, not zero latency. Missing callbacks are reported separately.
  Continuous cadence groups frames by display-link activity cycle, excluding
  intervals across recorded idle pauses. All intervals are also reported.
  The selected refresh-rate exceedance count uses a 5% cadence tolerance; target lateness over
  1 ms is a separate descriptive count. Neither is an acceptance waiver.
  Use `--target-hz 90` for current Mac validation and `--target-hz 120` for iPad
  (the default remains 120 for existing callers). Reports record the evaluation
  target and frame budget, and count CPU and continuous-cadence exceedances
  against that budget. Measured workloads retain their own continuous-cadence
  subset. The original `owner_service_over_8_33ms` and
  `continuous_intervals_over_120hz_budget` fields remain explicitly labeled
  diagnostics; they are not the current Mac 90 Hz acceptance thresholds.
  Refresh targets change report interpretation only, not app or input behavior.
  `frame_admission_to_present_ms` measures actual display time minus frame
  admission, independently of the scheduler's advertised target. It is software
  scheduling delay, not physical input-to-pixel latency. Comparing target
  lateness alone across different schedulers can conceal a changed target.
- Input enqueue/owner times measure transport queueing. The first presentation
  associated with each successfully received, nonpredicted batch is a **receipt
  proxy**. The renderer may defer that input or consume only part of its queue.
  This association does not establish that those pixels were included, nor
  physical Pencil input-to-pixel latency. Prediction remains visual-only.
  Corrections have separate batch counts, owner queue distributions and receipt
  proxies. Their sample timestamps remain the original observation times;
  correction delivery is measured from the new enqueue receipt.
- The report includes p50/p95/p99/max, missing/invalid/overflow counts, display
  capabilities, memory footprint and thermal states. Empty measurements are
null, not zero. Readiness requires canvas, shaders and bundled filter catalog.
  Startup memory growth and profiler storage are included in footprint; they
  must not be described as steady-state document growth.

The recorder reserves a capped array of fixed-size events (reported in the file
header), uses a short lock for concurrent callback appends, freezes once, and
streams JSONL on a utility queue. Memory sampling runs once per second. Timing
records and GPU timestamp submissions have overhead; run paired instrumentation
on/off investigations before drawing performance conclusions. Overflow or GPU
skips can bias distributions and must remain visible. Export keeps late callback
records for two seconds; missing completions at that boundary stay unverified.

## JSONL schema 1

The first line is metadata. Each remaining line is `[kind, a, b, ..., j]` with
unsigned integer fields; unused fields are zero. Times and durations use
nanoseconds in the CACurrentMediaTime monotonic clock domain. Frame IDs are
the admission timestamp. GPU timestamp differences are converted using the
queue's timestamp period, not compared as absolute CPU clock values.

| Kind | Fields in order, excluding trailing zeros |
| --- | --- |
| 0 tick | admission time, target time, admitted flag |
| 1 frame | ID, target, owner start, owner end, five CPU stage durations, latest nonpredicted receipt ID |
| 2 input | enqueue ID/time, owner start/end, oldest/newest sample time, count, kind (0 real, 1 predicted, 2 correction), original last phase, tool, accepted flag |
| 3 drawable | frame ID, acquire start/end, drawable ID, acquired flag |
| 4 presented | frame ID, actual presentation time, callback observation time, drawable ID |
| 5 memory | observation time, physical footprint bytes, resident bytes, thermal state, Mach status |
| 6 display | observation time, pixel width/height, scale multiplied by 1000, maximum refresh rate |
| 7 GPU | frame ID, GPU queue span, status (1 valid, 2 readback failure, 3 invalid timestamps) |
| 8 GPU status | observation time, support (0 uninitialized, 1 supported, 2 unavailable), requested/skipped/invalid/pending counts, poll-error flag |
| 9 state | observation time, frame ID, flags (1 canvas ready, 2 catalog loaded, 4 another frame needed, 8 shaders ready), frame-error flag |
| 10 activity | observation time, display-link awake flag |
| 11 workload | observation time, phase, profile ID, phase-dependent counters |

The analyzer also retains local scheduling experiment records: kind 12 contains
frame ID, CPU commit deadline, presentation target and drawable admission status
(0 ordinary acquisition, 1 supplied drawable accepted, 2 stale drawable rejected).
The optional sixth field of kind 6 records the requested Metal frame latency;
zero means unavailable. The published CADisplayLink hosts do not emit these
experimental fields. Owner completion includes polling and snapshot publication,
so lateness relative to the commit deadline is an upper bound, not a measured
Metal commit timestamp.

Workload phases: 0 configuration, 1 warm-up begins, 2 measurement begins,
3 measurement ends, 4 postlude ends, 5 failure, 6 producer sample. Phase 0's
remaining fields are width, height, paint-layer count, brush ID, diameter ×1000
and prediction flag. Phases 2/3/6 record cumulative nonpredicted sample and batch
counts, followed by the maximum producer lateness since its previous sample.
The metadata `workload` object includes the profile version, expected duration
and sample rate. These additions retain schema 1; older traces omit them.

## Fast checks

```sh
cargo test -p layer-render-wgpu --lib frame_timing::tests
python3 -m unittest discover -s tools/performance -v
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/FrameTrace.swift \
  apps/layer-apple/tests/frame-trace.swift -o /tmp/capy-frame-trace-tests
/tmp/capy-frame-trace-tests
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/DrawingWorkloadPlan.swift \
  apps/layer-apple/tests/drawing-workload-plan.swift -o /tmp/capy-workload-plan-tests
/tmp/capy-workload-plan-tests
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o /tmp/capy-frame-driver-tests
/tmp/capy-frame-driver-tests
```

The GPU test requires timestamp-capable hardware and checks frame identity,
strictly positive timestamps, bounded pending observations, completion and slot reuse. The Swift test checks
concurrent capacity, overflow, freeze and late presentation records. The Python
tests preserve active missed frames while excluding idle gaps, prevent missing
or invalid timings becoming zeros, deduplicate input receipt associations and
exclude shader startup from the ready subset. These tests use no UI automation.
Workload checks cover contact termination, lift gaps, pressure, coordinates and
invalid configuration. Report checks retain incomplete/failed measurements and
missing render observations even when the input producer completes.
