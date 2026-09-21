# Windows pen taps and Tool Set follow-up — 2026-09-20

A physical Wacom at 120 Hz exposed a duplicate native pointer over the canvas and missed toolbar taps. These fixes retain the existing independent input thread and presentation path. They add no smoothing, input delay, or frame work.

## Findings and fixes

- `CanvasWindow::StartInput` had no native cursor policy. Set the canvas `InputPointerSource.Cursor` to null; XAML chrome retains its own cursor. Microsoft documents this property for both mouse and pen pointers: [InputPointerSource.Cursor](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.input.inputpointersource.cursor).
- `WorkspaceGestures` treated movement beyond native drag slop as a consumed gesture before a hold had started. That set `ignoreClick`, swallowing ordinary native Button clicks even when the pen stayed inside the button. Early motion now disarms reorder pickup while letting the Button own its click and capture. Holds, actual drags, scrolling, focus loss, and cancellation still suppress clicks where appropriate.
- Windows Tool Set used 82×32 previews beside labels. The [web reference](../../apps/layer-web/style.css) uses full-width, 40-pixel-high previews above an icon and right-aligned name. Match that layout, the 2-pixel subtool gap, 3×6 padding, and group sizing. Theme changes also invalidate preview assets.

## Evidence

The focused OS-injected pen probe alternated Pen/Pencil choices with 0, 2, 4, 6, and 8 physical pixels of movement, four contacts per distance, both with and without in-range hover between contacts.

| Build | Successful taps | 6–8 px drift |
| --- | ---: | ---: |
| Before gesture fix | 24/40 | 0/16 |
| After gesture fix | 40/40 | 16/16 |

Aggregated counts and document revisions are in [the evidence data](pen-input-windows-20260920.json). Raw isolated probe evidence is local under `artifacts/windows/workspace-pickup/6fd9d85691754d66a4ddd35fed2ee12d` and `e1a9335b5b9b42a48d93c95b6caec525`. No private profiles or raw UI snapshots are committed.

The direct draw-then-Clear regression passed 6/6 single taps: three stationary, three with 8-pixel drift. Each stroke created editable content; one Clear tap advanced document revision and removed transformable content. The pen remained in range through each transition. Evidence: `artifacts/windows/pen-buttons/2da0700f613f46349a764e9a63dc50fd`.

The first version of this test tapped while startup filter-library validation still disabled document edits. Waiting for initial Clear availability resolved that test setup issue; it was not evidence of another input defect. The broad fixture also contained a retired partial-Zen expectation, although shared snapshots now always publish `partial_zen=false`; update the fixture to current chrome hiding/restoration. Drawer scenarios explicitly select Open individual panels, accept already-collapsed columns, and keep mouse hover away from UIA-opened flyouts.

Release build and native input/work-buffer tests pass. The complete workspace pickup fixture passes for pen, mouse, and touch: drifting taps on docked/floating/drawer tiles, pre-hold motion, same-contact hold/drag, menus, disabled commands, immediate grips, cancellation, one-step undo/redo, collapsed columns, nested drawers, and Zen hide/restore. The cursor regression uses Windows `GetCursorInfo` to confirm canvas hiding and toolbar restoration. Tool Set was visually checked in the rebuilt native app against web CSS and the shared preview assets. The complete Tool Set functional fixture passes, including all 17 primary tools, subtool projection, numeric edits, retained fields/buttons/scrolling, stale-draft protection, and transform cancellation. Its controlled stroke now waits for brush readiness and dismisses any tool drawer. Final visual/functional evidence: `artifacts/windows/tool-style/92c88f3f83544811a5142a7c003f8c7c`.

These are synthetic OS-input and native UI checks, not a physical Wacom driver or digitizer qualification. Retesting the new build on that Wacom is still needed.

## Reproduction

```powershell
./apps/layer-windows/scripts/build.ps1 -Configuration Release -SkipRestore -OutputDirectory artifacts/windows/wacom-input-20260920
./apps/layer-windows/scripts/exercise-pen-buttons.ps1 -Executable ./artifacts/windows/wacom-input-20260920/CapyCanvas.exe
foreach ($device in 'pen','mouse','touch') {
    ./apps/layer-windows/scripts/exercise-workspace-pickup.ps1 -Executable ./artifacts/windows/wacom-input-20260920/CapyCanvas.exe -Device $device
}
```
