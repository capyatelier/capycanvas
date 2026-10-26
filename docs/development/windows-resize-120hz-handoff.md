# Windows 120 Hz resize validation handoff

[Windows development](windows.md)

Validate the window-resize fix on the physical 120 Hz Windows computer. Establish
whether native panels and the canvas follow continuous resizing at 110–120 Hz,
and identify the remaining bottleneck if they do not. Start with measurement;
keep the immediate fix focused and defer the full cross-platform UI refactor.

## Starting point

Fetch current `origin/main` in the assigned checkout. It must contain both fixes:

```powershell
git fetch origin main
git rebase origin/main
git merge-base --is-ancestor d66f45ed9 HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing render-owner resize fix' }
git merge-base --is-ancestor fca8ddc06 HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing retained glass publication fix' }
```

`d66f45ed9` retains shared layout content, moves swap-chain configuration and
resize off the UI thread, retains buffer capacity, and corrects source scaling
and frame deadlines. `fca8ddc06` removes full-snapshot invalidation when glass
region counts change, coalesces glass collection, and retains the latest GPU
panel acknowledgement independently of camera, search and layout packets.

The frozen 60 Hz diagnostic and its limits are in
[Windows window resize](../performance/top-tier.md#windows-window-resize).
Its 5.1–5.5 ms Paint publication mean excludes deferred glass and other WinUI
layout/composition work. It does not establish a 120 Hz frame budget.

The fast two-axis follow-up still reproduces grow-only black right/bottom regions
that clear on release. Retaining unchanged layout callbacks removes an 80% idle
UI feedback loop, but rapid resizing still uses 88–92% UI CPU with only 27–28
native window-size changes/s on the 60 Hz laptop. Treat the landed improvements
as partial. Subsequent frame-based workspace observation and direct caption
regions improve native size changes to about 43/s, with 90–95% UI CPU after
the latest rebase. Black regions
remain. A minimal WinUI window with a solid Grid and two corner markers also
shows grow-only black bands without Rust, a GPU swap chain or editor work;
Capy's heavier UI work greatly increases the lag. The earlier approximately 58
renderer calls/s did not measure native UI cadence. Prioritize the remaining
WinUI drawing/layout, native window regions, packet parsing and framework
resize synchronization. The last bounded UI stack sample shows the earlier
bridge/background and repeated stringification hot paths largely removed;
it does not identify another small unchanged-geometry cache fix. Inspect the
composed native controls as well as the canvas. Saved models confirm that
the local fixture's `Sketch` filename label actually used the default Paint
workspace; select and verify the workspace explicitly on the receiving computer.

Previous `artifacts/windows/resize-investigation/` files, binaries and temporary
UI timing probes are local and are **not delivered by Git**. Build on the new
computer and use the tracked helpers below. No temporary probes remain in main.

## Prepare the machine and build

Use a hardware GPU and the physical console. Record the computer, CPU, GPU,
driver, Windows build, monitor model, resolution, scale, HDR/VRR state and power
mode. Set the intended monitor to 120 Hz, put the test window wholly on it, and
verify the active mode rather than relying on the monitor's advertised maximum.
The fixture display flags select a real output; they do not create a 120 Hz
display. `CAPY_TEST_PRIMARY=1` selects the primary monitor, which may be the
laptop's slower built-in panel. Make the intended monitor primary or explicitly
place and verify the owned window on that output. Record any display changes and
restore them afterward.

Read [Windows setup](windows.md#build-and-run),
[environment](environment.md), and [measurement rules](../performance/measuring.md).
Build Release with the normal script, including Rust; do not copy the old laptop's
DLL or use `-SkipRust` after updating shared code.

```powershell
$results = Join-Path $PWD 'artifacts/windows/resize-120hz'
New-Item -ItemType Directory -Force $results | Out-Null
git rev-parse HEAD | Set-Content (Join-Path $results 'commit.txt')
./apps/layer-windows/scripts/probe-displays.ps1 |
    Set-Content (Join-Path $results 'displays.json')
./apps/layer-windows/scripts/build.ps1 -Configuration Release `
    -OutputDirectory (Join-Path $results 'build')
$exe = Join-Path $results 'build/CapyCanvas.exe'
Get-FileHash $exe, (Join-Path $results 'build/layer_windows.dll') |
    Format-List | Out-File (Join-Path $results 'binary-hashes.txt')
```

Stop only builds and test processes owned by this run before measuring. Use a
fresh private profile and the private test desktop for every injected journey;
never drive another session's window or real user documents.

## Focused resize workload

Create a scratch fixture under the ignored result directory using these tracked
helpers. Fixture construction is part of the receiving agent's task; there is
no checked-in `reproduce.ps1` resize benchmark to invoke.

| Need | Existing source |
| --- | --- |
| Private desktop and invocation | [`test-desktop.ps1`](../../apps/layer-windows/scripts/test-desktop.ps1) |
| Private-profile setup, owned window and UI Automation | [`CapyUia.ps1`](../../apps/layer-windows/scripts/CapyUia.ps1): `Enter-CapyEnvironment`, `Settings-File`, `Owned-DrawingWindow`, `Wait-Until`, `Find` |
| Guarded mouse input and private-desktop verification | [`RowPointerDriver.cs`](../../apps/layer-windows/scripts/RowPointerDriver.cs) |
| High-resolution producer pacing and QPC clock | [`WindowsPenMotion.cs`](../../tools/performance/WindowsPenMotion.cs) |
| Workspace switch, retained geometry and resize examples | [`exercise-workspace-layout.ps1`](../../apps/layer-windows/scripts/exercise-workspace-layout.ps1) |
| Existing ETW launch and bounded host trace setup | [`windows-pen-latency.ps1`](../../tools/performance/windows-pen-latency.ps1) |
| Closing the owned drawing | [`exercise-window.ps1`](../../apps/layer-windows/scripts/exercise-window.ps1) |

Keep the fixture limited to these steps:

1. Launch `$exe` with `CAPY_STORAGE_DIR` pointing at a fresh result subdirectory.
   Set the requested theme and English through the existing settings helper.
   Keep `CAPY_TRACE_UI` absent during timing. `CAPY_LATENCY_TRACE=1` enables the
   existing bounded host trace; it writes after close. Wait for the owned drawing,
   enabled canvas, ready renderer and settled workspace before warm-up.
2. Select Paint through `workspace-switch-builtin:workspace:illustrator`; Photo
   uses `workspace-switch-builtin:workspace:photographer`. Wait for an enabled
   visible switch. If it is overflowed, open `header-workspace-menu` and use its
   named menu item. Do not mistake startup delay for an overflowed header.
3. Start with the default blank drawing, default glass and Navigator visible.
   For the small-canvas comparison, use a 2005 × 1504 drawing and a 1700 × 1150
   physical-pixel window where the monitor permits; record actual canvas,
   window and DPI. Use per-monitor-v2 DPI awareness for OS coordinates. A different
   size is a separate diagnostic. Also run the
   ordinary working size on the 120 Hz monitor.
4. Prime one native edge drag, return to the starting bounds, and let shader and
   layout work settle. Record one second of idle UI-thread CPU before input.
   Measure three six-second drags, including bottom-right growth beyond the
   initial size and rapid two-axis reversals. On a sufficiently large display,
   include the 1100 × 750 to 2100 × 1350 range used in the black-region repro.
   Select workspaces before narrowing the window. Use the native non-client
   resize border and OS mouse input, retain
   foreground PID/HWND checks, and release the button in `finally`.
5. Pace requested positions at 240 Hz with actual QPC timestamps, no catch-up
   bursts, and no fixed 16 ms sleep. The old local runner used 16 ms producer
   steps; that limits changing input to about 62.5/s and cannot test sustained
   120 Hz geometry. Log achieved producer cadence and actual window bounds;
   fail the workload if injection is too slow or the window does not resize.
   Recheck the actual corner and foreground ownership before each contact;
   a missed resize handle invalidates that contact, even if input was delivered.
   Keep endpoints inside a range that preserves the same panel arrangement.
6. Separately measure programmatic resize over the same width range. Treat it as
   a diagnostic, not a replacement for interactive edge dragging. Record buffer
   growth separately from warm motion within retained capacity.
7. Repeat Paint in light and dark, then Photo with Navigator open. Start with one
   short diagnostic in each additional case; collect three gestures for every
   case reported as sustained. Capture arranged panel geometry, UI Automation
   runtime identities and screenshots before/after, outside the timing windows.
   Verify retained controls, readable content, correct viewport scale and settled
   shadows/glass. Confirm the window still paints and closes normally afterward.

Use a separate diagnostic capture pass during growth and reversals. Before/after
images miss transient black regions. Sample the interior inside the real window
border, compare native content bounds against it, and record maximum black area
and recovery after release. Keep capture overhead out of the qualification run.

Run the fixture through `test-desktop.ps1` using its `-Script`, `-Parameters`
hashtable and `-TimeoutSeconds` arguments. Reuse its fresh desktop rather than
injecting on the ordinary desktop. Save the fixture itself with the local raw
evidence so the measurements can be repeated.

## Measure presentation and explain misses

Capture actual presentation feedback for the owned process, scoped to the
moving windows in the workload log. The existing PresentMon 2.5.1 pattern below
belongs inside the fixture once `$review` is its owned process and `$run` is its
result directory. Set `$presentMon` to the installed executable. Start capture
after warm-up; wait for successful capture startup before the first gesture.

```powershell
$capture = Start-Process -FilePath $presentMon -ArgumentList @(
    '--process_id', $review.Id,
    '--timed', '40', '--terminate_after_timed', '--no_console_stats',
    '--no_track_input', '--v1_metrics', '--qpc_time_ms',
    '--session_name', "CapyResize-$($review.Id)",
    '--output_file', ('"' + (Join-Path $run 'presents.csv') + '"')
) -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $run 'presentmon.log') `
    -RedirectStandardError (Join-Path $run 'presentmon-error.log')
```

PresentMon may require administrator rights. If capture fails or lacks displayed
timestamps, retain the error and report presentation as unmeasured. Do not
substitute submitted FPS for displayed FPS. Check that private-desktop presents
are actually composed to the physical 120 Hz output before qualifying them.

The canvas is a composition swap chain without an HWND association. Use the
owned process's `presentation-probe` swap-chain addresses to identify it; do not
count unrelated WinUI chains together as extra frames. Conversely, smooth canvas
presents alone do not establish smooth native panel geometry. Correlate displayed
frames with changing window/panel bounds, using compositor tracing or a separate
visual capture when needed. Explain the available attribution and its limits.
The pen report's input analysis is pen-specific; do not feed resize logs into it
unchanged. Its Immediate-mode DXGI observation is an upper bound, not an exact
scanout timestamp.

For each gesture report moving-frame rate, p50/p95/p99 and longest presentation
gap, full-contact duration/count, onset delay, achieved input cadence and distinct
geometry updates. Exclude idle, settling and release intervals from moving FPS;
do not hide an onset stall by using only first-to-last presentation cadence.

If it misses, take a separate short CPU/ETW profile. Distinguish UI publication
from deferred glass collection, XAML measure/arrange/composition, render-owner
acquisition/pacing, GPU work and capacity growth. Keep full UI snapshot tracing,
per-frame screenshots and UI Automation polling out of timed qualification runs.
Any restored timing probes must buffer bounded records and be removed afterward.

Useful hot paths: `CanvasWindow::ApplyPending`, `PublishGlass` and the renderer
loop in [`CanvasWindow.cpp`](../../apps/layer-windows/CanvasWindow.cpp);
`backgrounds`, `measurePanelBridges` and `glass` in
[`WorkspaceView.cpp`](../../apps/layer-windows/WorkspaceView.cpp);
[`CanvasSnapshotMailbox.h`](../../apps/layer-windows/CanvasSnapshotMailbox.h);
`capy_snapshot` and surface sizing in
[`host.rs`](../../apps/layer-windows/native/src/host.rs);
shared layout publication in [`snapshot.rs`](../../crates/layer-host/src/snapshot.rs)
and `NativeHost::set_glass` in [`scene.rs`](../../crates/layer-host/src/scene.rs).

## Completion and next action

At an active 120 Hz output, the existing display-paced acceptance floor is
**114 changing presented frames/s**, with **p99 gap ≤16.67 ms**, in every measured
gesture. A steady 110/s is useful progress but misses that floor. One frame's
budget is 8.33 ms; a 5 ms publication mean alone cannot qualify it. On another
desktop these are local diagnostics, not reference-tier qualification.

Keep raw samples, hashes, monitor state, commands and captures in `artifacts/`.
Update the Windows resize entry in the [tier table](../performance/top-tier.md)
with the device, exact commit, workload and any measurement limitations. Report
what passed, what missed and whether the UI thread, renderer or compositor now
limits resizing. Include the next smallest measured fix if necessary.

Do not start broad suites or the cross-platform refactor. Run only checks needed
for any actual fix. Preserve shared Rust ownership of policy and layout, keep GPU
waits and I/O off UI/input threads, and commit complete stable improvements.
Fetch/rebase before pushing; never force-push main. If this immediate fix meets
the measured target and correctness checks, stop and hand back the evidence.
