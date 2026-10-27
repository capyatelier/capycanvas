# Windows 120 Hz resize validation handoff

[Windows development](windows.md)

Validate the window-resize fix on the physical 120 Hz Windows computer. Establish
whether native panels and the canvas follow continuous resizing at 110–120 Hz,
and identify the remaining bottleneck if they do not. Start with measurement;
keep the immediate fix focused and defer the full cross-platform UI refactor.

## Starting point

Fetch current `origin/main` in the assigned checkout. It must contain the resize milestones:

```powershell
git fetch origin main
git rebase origin/main
git merge-base --is-ancestor d66f45ed9 HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing render-owner resize fix' }
git merge-base --is-ancestor fca8ddc06 HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing retained glass publication fix' }
git merge-base --is-ancestor 80973432e HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing frame-based workspace observation' }
git merge-base --is-ancestor 846aabea3 HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing background snapshot decoding' }
git merge-base --is-ancestor f09f45b0d HEAD
if ($LASTEXITCODE -ne 0) { throw 'Missing retained panel graphics and Navigator geometry fix' }
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

The fresh normal Release baseline at `33db3fe98` reaches 45.5–52.0 native size
changes/s with 89.0–93.2% UI CPU, or 17.1–20.5 ms UI CPU per native change.
These are two runs on the 60 Hz laptop, with Paint selected explicitly, private
profiles and UI tracing disabled. The six-second growth and reversal contacts
engage; the missed shrink contact is excluded. Renderer calls do not establish
native UI cadence or displayed FPS. Inspect the composed controls and canvas.
Earlier local filenames labelled `Sketch` actually used Paint; select and verify
the workspace explicitly on the receiving computer.

The matching-runtime minimal window needs 1.64–1.70 ms UI CPU per change and
reaches 169–175 native changes/s. Enabling extended content and Tall raises that
to 5.57–6.19 ms and reduces cadence to 97–105 changes/s. `CTitleBar` appears in
37% of owning-callback motion samples, versus zero in the plain control.
In Capy, extension off from construction, with the unsupported Tall setter
omitted, still retains `AppWindowTitleBarFeature` and `CTitleBar::UpdateSize`.
That control needs 17.2–18.5 ms per native change and does not measure removal of
the feature. Production chrome remains unchanged. A scratch frame using public
nonclient input regions passes mouse/pen/touch client clicks, caption drags and
diagonal resizing. The initial child-window transparency approach fails actual
dragging despite correct direct hit-test replies. Maximization needs the client
rectangle constrained to the work area and hit regions expressed in client
coordinates. Reliable repeated fullscreen restoration, caption visual states,
Snap, physical close and dark/inactive styling remain unverified. This is a
feasibility result, not a production replacement or a measured Capy speedup.
The same-content minimal timing pair needs 3.77–4.41 ms per native change with
custom regions, versus 1.70–2.33 ms with the default frame. `CTitleBar` leaves
the captured motion stacks, but native region publication remains prominent.
The client extents differ; see the full limits in the linked tier table.

The panel milestone moves unexpanded fills into retained compositor vector
geometry and caches blurred shadow patches. The normal XAML background host
does not resize. On the same frozen-Rust comparison, pooled UI CPU per native
change falls from 17.72 to 16.48 ms during growth and from 19.17 to 16.58 ms during
reversals. Two-run cadence ranges are 54.5–55.2 and 53.8–55.7 native changes/s;
these remain limited 60 Hz diagnostics. Light/dark
appearance, caption/header input, fullscreen restoration, retained controls,
panel acknowledgements, configuration expansion/resizing/dismissal and normal
close pass. Subpixel positions retain boundary antialiasing differences within
one physical pixel of the reference contour; uniform interior colors match
exactly. Hardware checks cover 150% scale, with window-free shadow math at
100%, 150% and 200%.

The pushed panel milestone is `f09f45b0d`. Its normal Release x64 rebuild with
current-main Rust and the focused native tests pass. The timings above retain
their frozen baseline Rust identity; they do not measure the newly rebuilt DLL.

Navigator now measures and arranges its image and fixed action row together in
its first native layout (`19d51c3c3`), including finite, minimum and fractional
allocations. A numeric geometry key suppresses unchanged global notifications.
Check the visible overview against the published slot intersected with its clip;
the tracked expansion journey waits for that agreement after resizing.

The subsequent Windows UI milestones must also be present:

```powershell
foreach ($milestone in @('04fd2e228','a9262291a','650d36a26','e27fda7a8','52e6dc961','7f3a7bfab','435c52fdf','19d51c3c3','b2375bd23')) {
    git merge-base --is-ancestor $milestone HEAD
    if ($LASTEXITCODE -ne 0) { throw "Missing Windows UI milestone $milestone" }
}
```

These repair contact integer serialization and render-owner liveness; admit
new tool bubbles only on canvas; retain model button fills during drawing;
correct drawer height-for-width and source corners; show a defined first image;
apply palette feedback to actual SDK dropdown/dialog buttons; reserve the whole
numeric component from slide resize; and fit Navigator without deferred sizing.
The current snapshot-bound bridge query also projects the measured layout rather
than a later live workspace. Use the normal Rust build: its query contract changed.

The focused bridge journey checks column open/close, receiver geometry/radii and GPU acknowledgement,
retained controls across resize, and workspace returns. Settled acknowledgements
do not prove complete intermediate frames. Whole-column/workspace/tab transitions
still have independent native-control and GPU-background adoption; this remains
an open structural bug. Physical release can also precede adopted workspace tint,
as it does in the current GTK sequence. Do not report either issue fixed from
these geometry assertions.

The final capture-free 60 Hz bridge bracket measures 15.77 ms of UI CPU per
observed native resize change versus 16.08 ms for main. The difference is within
drift and contact variance; this remains roughly twice the 8.33 ms budget.
A synchronous bridge
prototype increased this to 17.96 ms and was rejected. Shared split-UI rendering
and native drawing-surface probes are local experiments, not shipped application
transactions or 120 Hz evidence. Test the checked-in app and report fresh numbers.

The subsequent header change moves Capy styling to model/accepted-size updates,
reuses placed caption rectangles and removes a measurement JSON round trip.
Ninety focused light/dark native checks pass. Its same-DLL capture-free bracket is
15.03 ms/change versus pooled bridge baseline 15.46 ms, within 3.75% endpoint
drift and overlapping ranges: no distinct speedup is established. Workspace
compact-focus and live outgoing-overflow fade remain unverified. Use
`exercise-header-placement.ps1` through the private desktop wrapper for those
bounded header checks; it reports unmet fixture preconditions as coverage gaps.

The scroll-indicator fix removes the parent SDK indicator animations while
retaining native controllers and hover feedback. Actual routed/canvas pointer
events replace the Win32 hook, so pen/touch hide mouse-only bars reliably.
Use `exercise-scroll-indicators.ps1` with the Feedback fixture in both themes;
48 fixture and 54 normal-app assertions pass. Valid runtime template replacement
remains unverified. Its capture-free bracket costs 15.85 ms/native change versus
pooled Header 15.40 ms (+2.93%, within 3.23% endpoint drift and overlapping
ranges). No speedup or distinct regression is established; this does not qualify
120 Hz or fix the separate whole-column/workspace adoption issue.

This remains above the 8.33 ms CPU budget. Next work must adopt matching native
controls and rendered UI pixels together and reduce packet/layout work. Keep
the remaining native title-bar feature in the profile; changing
the extension flag alone does not eliminate it. Preserve the current chrome
until a custom-frame feasibility check and decision are complete.

Matching-background experiments also fail to recolor the exposed bands. A solid
magenta compositor backdrop, a custom `Window.SystemBackdrop`, and a native
window-class brush all leave black right/bottom strips in the minimal window.
The proper backdrop connection and persistent native brush are verified; the
setters succeeding does not establish coverage. No color mitigation is shipped.
Repeat only a materially different approach, and require a composed capture of
the newly exposed area before describing recoloring as working.

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

Run the focused bridge geometry journey in each theme before timing:

```powershell
foreach ($theme in @('light','dark')) {
    ./apps/layer-windows/scripts/test-desktop.ps1 `
        -Script ./apps/layer-windows/scripts/exercise-panel-bridge-layout.ps1 `
        -Parameters @{Executable=$exe;Theme=$theme} -TimeoutSeconds 300
}
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
