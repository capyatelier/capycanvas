# Windows pen latency qualification — 2026-09-20

## Scope and changes

Updated from `0a1acaf4` to `5b9de3f3`, then incorporated `f458d13e`
(Android's required retained front-buffer presentation). Before publishing, also
integrated `484e6973` (shared GPU cursor cleanup). Windows already shares
Android's Rust brush, tile execution, retained display cache, and composition
optimizations. The Windows work adds these fixes:

* Admit display/source cache memory using the rendering adapter's DXGI
  `Budget - CurrentUsage`, capped by available system memory, with the existing
  desktop one-quarter headroom policy. Unknown or exhausted budgets retain the
  bounded fallback. This is an admission snapshot, not a memory reservation.
* Use whole-tile source admission within the same one-eighth allowance and
  256 MiB ceiling. Power-of-two tiers discarded usable headroom and caused
  avoidable eviction in the wide-brush replay.
* Prefer DXGI immediate presentation when supported, with FIFO fallback. Retain
  the frame-latency waitable object, acquire-before-input ordering and maximum
  frame latency of one. Immediate presentation permits tearing; latency is
  prioritized, matching the requested tradeoff.
* Sanitize only optional Windows predictions. `PointerPredictor` extrapolates
  pressure beyond [0,1]; this previously made Rust reject the entire batch with
  `Pointer axes are not normalized`, stopping rendering. Predicted pressure and
  distance are bounded and nonfinite predictions are discarded. Real samples
  remain untouched, ordered, and subject to normal ingress validation.

Added opt-in bounded timing capture, a guarded OS pen injector, a DXGI statistics
analyzer, and a deterministic large-image fixture. No timing files are written
per input event or frame. Tracing and experimental presentation controls require
`CAPY_LATENCY_TRACE`; normal application startup does not enable them.

## Test system and workloads

Intel Iris Xe, D3D12, driver 32.0.101.6737; Windows 11 build 26200; approximately
16 GiB RAM. The display is 2256×1504 at approximately 60 Hz; the measured DXGI
refresh interval is 16.6667 ms. Native canvas viewport: 1418×988 physical pixels,
150% UI scaling, scRGB `Rgba16Float` presentation.

The input project is a deterministic synthetic 9504×6336 (60,217,344 pixels,
nominal 61 MP camera dimensions) sRGB U8 image plus an empty paint layer. It is
not a real camera photograph. Fixture SHA-256:
`2D480BADC27D63DE91A0D13235EA65B5932662BB73C9563027DADD2FB3DD31B6`.

* **2000 px G-Pen rendering:** 2752×2064 offscreen scRGB target, 20% zoom, 180
  stroke frames, eight 240 Hz samples per frame, circular path, full pressure,
  then 360 zoom frames. Completed-frame timing includes GPU queue completion;
  it is not display latency. Setup and initial image loading are excluded.
  Every run checks exact native undo/redo roots. D3D12 is explicitly selected
  with `LAYER_GPU_INDEX=1` on this machine (the default headless adapter was
  Vulkan, which is not the Windows application's backend).
* **18 px G-Pen input/presentation:** real WinUI window with the same project,
  canvas fit, isolated settings, 240 Hz OS-injected pen, 10 seconds, 2,401
  actual points, one circle/second with 200×120 screen-pixel radii. The injector
  uses a high-resolution waitable timer and verifies foreground/window ownership
  before every event. Timing capture checks that every real point was consumed.

The input time origin is QPC immediately before each OS injection call, recorded
independently by the injector. Complete, ordered injected and delivered histories
must have identical counts; each WinUI timestamp must fall within its matching
injection interval plus a 2 ms tolerance. Windows' generated pointer timestamps
are quantized to milliseconds and sometimes appear slightly ahead of receipt.
The analysis uses the recorded QPC rather than fitting a clock offset or changing
raw pen data. Injection call overhead/scheduling is included; physical digitizer
time is excluded. The raw timestamp offset range is retained in the JSON.

DXGI measurements correlate each consumed point with its own frame's present ID,
then with `GetFrameStatistics`' displayed present count. For FIFO, refresh counts
and QPC provide the refresh timestamp. For unsynchronized modes, a flip can occur
mid-refresh: the refresh-start timestamp can precede the input. The analyzer
therefore reports **no exact display timestamp** for immediate/mailbox. Instead,
it reports a conservative presentation-observation bound using the next frame's
acquisition-start timestamp, which occurs after the statistics query. This
includes intervening host work and is later than the observation itself.
Unobserved/dropped presents are reported as unmatched, never assigned the time of
a later present. Overflow, incomplete consumption and inconsistent clocks reject
a run. These are **software input-to-DXGI-reported-display measurements**, not
physical input-to-photon measurements: digitizer latency, scanout location and
panel response are excluded. PresentMon's ETW session was unavailable without
OS administrator privileges; direct DXGI statistics worked without that session.

## Measured results

The compact [measurement data](pen-latency-windows-20260920.json) records each
run, percentile distributions, frame counts, memory ceilings, CSV hashes and
artwork hashes. Raw timing and capture files remain in the local artifact folder.

### 61 MP, 2000 px G-Pen

| Native policy / run | Completed frame p50 | Completed frame p95 | CPU frame p50 | Source misses p50 | Zoom frame p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Main fallback, `native-before-1` | 111.29 ms | 135.33 ms | 106.80 ms | 97 | 84.54 ms |
| Main fallback, `native-before-2` | 100.32 ms | 128.91 ms | 96.82 ms | 97 | 83.59 ms |
| DXGI admission, `native-after-1` | 84.80 ms | 132.56 ms | 38.84 ms | 54 | 7.50 ms |
| DXGI admission, `native-after-2` | 85.84 ms | 107.92 ms | 40.13 ms | 54 | 7.23 ms |
| Admission control, `granular-control-1` | 82.89 ms | 103.72 ms | 37.83 ms | 54 | 7.27 ms |
| Admission control, `granular-control-2` | 86.49 ms | 106.58 ms | 38.45 ms | 54 | 7.29 ms |
| Final, `granular-after-1` | 77.79 ms | 122.83 ms | 34.52 ms | 42 | 7.52 ms |
| Final, `granular-after-2` | 82.49 ms | 106.18 ms | 36.17 ms | 42 | 7.38 ms |

Order was before/after/after/before for native admission, then
final/control/control/final for tile-granular admission. The final policy admits
229 MiB of decoded sources and 57.25 MiB of uploads on this machine, within the
same approximately 1.8 GiB display admission snapshot. The retained display
itself occupies approximately 1.2 GiB. The previous tiered admission allowed
128 MiB of sources; original Windows main allowed 64 MiB.

Median display-composition batches fall from 17 to 1; composition CPU time falls
from 82.79–92.47 ms to 21.33–21.52 ms. Zooming the completed image has zero source
misses in admitted-cache runs. Tile-granular admission improves median completed
time by 4.6–6.2% against its paired admitted-cache controls; its p95 varies and
is not consistently better. The remaining CPU-to-GPU-completion gap is material;
these results do not make the 2000 px workload fit a 16.7 ms frame budget.

All eight native-policy runs produce the same artwork-root SHA-256:
`acf57bb009053ed67616d0d49bc21451ec277ad8fdf59a42fb634e6872aabba7`.
Every replay independently verifies exact undo and redo roots.

A separate pull comparison held the display override at 768 MiB: pre-pull
`0a1acaf4` completed-frame medians were 124.82/127.88 ms, versus 107.45/107.06 ms
on `5b9de3f3` (approximately 14–16% improvement from shared changes). The native
policy measurements above then expose the additional Windows memory-admission
gap. Brush changes between those historical revisions can change stroke pixels;
exact cross-revision artwork equality is claimed only for the native-policy runs.

### 61 MP, 18 px G-Pen

These are the corrected-harness runs from the same final release binary, in
immediate/FIFO/FIFO/immediate order. Every run has maximum frame latency one,
acquisition waiting enabled, and all **2,401/2,401** real inputs consumed.

| Run / mode | Host frame p50 | Owner queue p50 | Injection-to-frame-return p50 | Presentation observation bound p50 / p95 | Matched inputs / 2,401 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `final-default-2`, immediate | 2.35 ms | 1.21 ms | 4.77 ms | 5.36 / 12.57 ms | 2,162 |
| `final-fifo-1`, FIFO | 3.48 ms | 10.56 ms | 13.29 ms | 46.88 / 54.28 ms | 2,307 |
| `final-fifo-2`, FIFO | 3.14 ms | 9.23 ms | 11.66 ms | 45.28 / 53.73 ms | 2,227 |
| `final-default-3`, immediate | 2.18 ms | 2.06 ms | 5.27 ms | 27.92 / 35.18 ms | 516 |

Host frame time is the native frame call through presentation return; unlike the
2000 px replay it does not explicitly wait for GPU completion. The final FIFO
refresh-based display medians are 41.45/40.18 ms. Immediate has no comparable
exact refresh timestamp, so the observation-bound column is used for both modes.
Receipt medians are 0.72/0.65 ms for FIFO and 1.01/0.92 ms for immediate.

Immediate lowers the host queue and submission delay in both repeats. Its two
runs have substantially different presentation-observation latency and coverage.
That is consistent with different Windows composition behavior, but the actual
composition mode was not established by ETW; this is an inference, not a measured
Independent Flip claim. Only 21.5% of input-associated presents were observed in
the second immediate run, versus 90.0% in the first. Do not generalize the 5.36 ms
best-run median to every point, display, window state or machine. The bound can
also include an idle service interval after pen-up, inflating its maximum without
establishing a corresponding photon delay.

Earlier same-binary experiments found mailbox behaving near FIFO. Disabling the
acquisition wait did not improve the immediate median and worsened tails in the
completed experiment; another launch timed out. The production choice is therefore
supported immediate presentation **with** acquisition waiting, maximum latency
one, and FIFO fallback. This allows tearing. No swap-chain front buffer is mutated
in place, and no brush precision, real input samples or user stabilization settings
are removed.

### Verification after the last upstream update

Before pushing, rebased onto `484e6973`, rebuilt the Rust DLL, WinUI application
and replay, and ran both requested workloads again. That upstream commit removes
the unused SVG cursor fields/argument; Windows continues using the same retained
GPU segments. The renderer/cache logic measured above is unchanged.

* `post-rebase-wide`: completed-frame p50 **81.11 ms**, p95 **120.74 ms**;
  zoom p50 3.53 ms, p95 7.27 ms. Exact artwork-root hash remains identical and
  undo/redo verification passes.
* `post-rebase-default`: G-Pen 18 px, immediate, acquisition wait enabled,
  maximum latency one; **2,401/2,401** real inputs consumed. Host frame p50
  **2.04 ms**, queue p50 **1.42 ms**, injection-to-frame-return p50 4.60 ms.
  Presentation-observation bound p50/p95 is **28.17/38.84 ms**, with only
  **405/2,401** input-associated presents observed. This confirms the slower
  presentation-observation regime seen in the second immediate repeat; it does
  not establish uniform 5 ms display or photon latency.

These checks and their new binary hashes are included in the JSON. The 19 GPU
regression tests above precede the cursor-only integration; the release rebuild
and final drawing captures verify its Windows host integration.

## Input and presentation API assessment

The existing independent `InputPointerSource` is appropriate for WinUI ink:
input dispatch is separate from the XAML UI thread and every chronological
intermediate point is retained. In Windows App SDK, `PointerPoint.Position` is
unpredicted; `RawPosition` was removed because it duplicated it. Prediction is
explicitly separate through `PointerPredictor` ([Microsoft release notes](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-1-0)).
The G-Pen defaults have zero streamline, pressure smoothing, stabilization and
motion filtering. The brush's swept geometry remains part of its rendering
model; real input is not temporally smoothed or delayed. Optional future-only
prediction does not postpone real points. Existing user-selected stabilization
settings remain available.

The host waits for the DXGI frame-latency object **before** draining input,
then processes the newest queued samples. Maximum frame latency is one and the
swap chain uses flip discard with two buffers. Active input wakes the owner
immediately; the idle service timeout is not an input polling interval.
The 16 ms retry applies to unavailable surfaces, not successful acquisitions.
This follows Microsoft's [waitable swap-chain guidance](https://learn.microsoft.com/en-us/windows/uwp/gaming/reduce-latency-with-dxgi-1-3-swap-chains).

Android's single retained shared-present buffer cannot be copied directly onto
Windows' discard back buffers. DXGI's relevant options are flip presentation,
a one-frame queue, and unsynchronized/tearing presentation where supported;
[flip-model guidance](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)
explains the role of Independent Flip/MPO. Promotion is driver/compositor and
window dependent. No claim that this window entered Independent Flip is made.
Windows also offers [delegated ink trails](https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nn-dcomp-idcompositiondelegatedinktrail),
but their compositor trail representation is not a replacement for the full
color-managed G-Pen renderer, layer composition and masks. A separate compatible
preview integration would require its own visual and latency qualification.

Memory querying follows [QueryVideoMemoryInfo](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/nf-dxgi1_4-idxgiadapter3-queryvideomemoryinfo);
display timestamps follow [DXGI_FRAME_STATISTICS](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/ns-dxgi-dxgi_frame_statistics).

## Reproduction

Build release with `apps/layer-windows/scripts/build.ps1 -Configuration Release`.
Build examples with `cargo build --locked --release -p layer-render-wgpu
--example photo_fixture --example photo_interaction` in a VS x64 developer shell.
Generate a fixture with `photo_fixture.exe INPUT.capy`. Run the rendering replay:

```powershell
$env:LAYER_GPU_INDEX='1' # verify the printed adapter/backend on your machine
photo_interaction.exe INPUT.capy OUTPUT.csv 8 18446744073709551615 180 2000 circles
```

The cache argument `18446744073709551615` selects native admission; `0` forces
only the display fallback and does not override source cache admission.
For the historical native fallback comparison use the frozen baseline binary.

```powershell
& tools/performance/windows-pen-latency.ps1 `
  -Executable ABSOLUTE_PATH_TO_CapyCanvas.exe -Project ABSOLUTE_PATH_TO_INPUT.capy `
  -OutputDirectory FRESH_RESULT_DIRECTORY -Diameter 18 -Seconds 10 -SkipPresentMon
node tools/performance/windows-pen-report.mjs FRESH_RESULT_DIRECTORY
```

Inspect `capture.json` for the actual selected mode rather than assuming
support. Raw local evidence lives in
`artifacts/windows/pen-latency-20260920/`; compact measured results accompany this
report. Runs must be isolated from compiles and other GPU workloads.

## Rejected trials and interpretation limits

* The initial default headless replay selected Vulkan; it is excluded from the
  D3D12 comparison. Explicit adapter selection was used for reported runs.
* The unfixed native host delivered 2,401 real inputs but consumed only 10 before
  predicted pressure invalidated a batch. A diagnostic replay reproduced the
  exact normalization error. Such stopped-renderer runs are not latency samples.
* An early immediate-mode trace overflowed during startup/idle frames. Capture
  now starts after real pen consumption and ends 500 ms after the last consumed
  frame; overflow still rejects the run.
* Applying the FIFO refresh timestamp formula to immediate flips produced
  negative values. That analysis was rejected. The retained raw trace is usable
  for host timing and presentation-observation bounds, using the documented
  corrected method; no negative latency is presented as a speedup.
* `integrated-immediate-repeat-2` actually had acquisition waiting disabled:
  an empty environment variable still counted as present. Its metadata records
  `no_vsync_wait: true`; it is classified by that captured setting, not its name.
  `final-default-1` exposed the same issue with PowerShell coercing a null
  argument to the .NET setter into an empty string. It is also a wait-disabled
  experiment, not a production-default result. Explicit environment-provider
  removal fixes this; subsequent runs verify the captured mode and wait policy.
  The next wait-disabled launch never reached ready within the 90-second
  metadata timeout and was closed normally. Production retains the wait object.
* Per-frame matching intentionally excludes presentations not seen in DXGI
  statistics. This matters especially in immediate mode. Coverage is reported
  alongside timing; measurements of only observed frames can be selection biased.
  Every accepted trial separately verifies that all 2,401 real inputs reached
  the render owner. Missing presentation observations are not lost input.

The remaining 2000 px completed-frame time is not a claim of 60 Hz rendering on
this integrated GPU. The replay performs full-precision brush/layer composition,
updates display mips, and waits for GPU completion; no artwork precision or
brush quality was reduced to improve the benchmark. Physical pen-to-photon
qualification still needs a physical pen plus high-speed camera/photodiode,
including scanout position and panel response, on each target display.

## Correctness checks

Release Rust and WinUI builds passed. The native C++ input/work-buffer suite
passed with `/W4 /WX`, including prediction overshoot, nonfinite predictions,
unchanged real records, ordered boundaries and bounded queue ownership. All eight
JavaScript analyzer tests passed, including missing presentations, stopped
renderers, invalid clocks, immediate-mode observation bounds and input without
a new present, plus injection count and timestamp correspondence.

Nineteen focused D3D12 Rust tests passed: memory admission (2), source cache
ownership/decoding/admission (6), display mips (4), complete display versus bounded
and scratch reference (2), repeated wide composition (1), native G-Pen source
pixel preservation and batch edges (2), and presenter resources/geometry (2).
The separate ignored hardware source-decode benchmark was not run. A broader
retained-display sweep was stopped to focus on these affected cases; it is not
reported as a completed suite.

In a VS x64 developer shell, select the verified D3D12 adapter and run:

```powershell
$env:LAYER_GPU_INDEX='1'
$filters=@('display_memory','scene::sources::tests','display_mips::tests',
  'live_display::tests::complete_display_',
  'live_display::tests::repeated_wide_composition','native_gpen','present::tests')
foreach($filter in $filters) {
  cargo test --locked --release -p layer-render-wgpu --lib $filter -- --test-threads=1
  if($LASTEXITCODE -ne 0) { throw "Failed: $filter" }
}
& apps/layer-windows/scripts/test-input.ps1
node --test tools/performance/windows-pen-report.test.mjs
```
