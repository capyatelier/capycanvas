# Windows WinUI motion and ink plan

[Windows development](windows.md)

Make the existing WinUI client keep panel parts together while they move, spend
less UI-thread time per motion frame, and show pen ink sooner. Measure every
change and report the gains. Background research and the options already ruled
out are in the [Windows UI plan](https://claude.ai/artifact/CCCXZRuxCwJRTuDDtnX2rn).

Out of scope: drawing the workspace in the canvas swap chain, a Win32 or system
DirectComposition host, delegated ink trails, WebView2 and other toolkits.

## Ink is the guardrail

Time from pen contact to displayed ink must not get worse than today's build
or the fastest native path Windows offers, and should improve on today's.

- Measure ink before the first change and again after every change that
  touches the render thread, the canvas swap chain, presentation or the glass.
  Revert a change that makes ink worse beyond run-to-run spread.
- Keep UI work off the pen input thread and the render thread. Nothing in this
  plan may delay a stroke frame for a panel.

## Starting point

Rebase onto `origin/main`. The facts below were read from the code at
`5c33a47f6`; check them before building on them.

### Why panels drift

During a divider drag, tab slide or column open/close, parts of one panel come
from three producers:

| Part | Producer | When it shows the new layout |
| --- | --- | --- |
| Text, buttons, tab labels | XAML layout | Frame N |
| Panel fills, tab-strip cutouts, shadows | `presentBackgrounds()` | `root.LayoutUpdated` → `queueMeasurements()` → the next `CompositionTarget::Rendering`, which reads arranged bounds with `allocatedBounds` |
| Glass and bridge-group fills (`group.gpu`) | `glass()` → `CanvasWindow::PublishGlass` → render thread → canvas swap chain | One or more frames later |

All three are in [`WorkspaceView.cpp`](../../apps/layer-windows/WorkspaceView.cpp).
During native window sizing, `nativeResize()` already switches to opaque XAML
fills and contours and suppresses glass; other workspace motion does not.
This diagnosis comes from reading the code. Reproduce it before fixing it.

### Ink path today

- Pen input arrives on a core independent input source with full history and
  `PointerPredictor` (`CanvasWindow::StartInput` in
  [`CanvasWindow.cpp`](../../apps/layer-windows/CanvasWindow.cpp)).
- The render loop acquires the next image (`capy_acquire`), then drains input
  and calls `capy_frame`.
- [`host.rs`](../../apps/layer-windows/native/src/host.rs) sets
  `desired_maximum_frame_latency = 2`, Immediate presentation when supported
  (otherwise Fifo), and scRGB `Rgba16Float` when supported.
  `d66f45ed9` raised the latency from 1 alongside its resize work without a
  recorded reason. The 2026-10-18 Pencil series in the
  [top-tier table](../performance/top-tier.md#windows-pencil-input-2026-10-18)
  ran at latency 1.
- [`frame_pacing.rs`](../../apps/layer-windows/native/src/frame_pacing.rs)
  separately allows two GPU submissions in flight.
- Nobody has recorded whether the canvas is composed by DWM or flipped
  directly. WinUI draws a `SwapChainPanel` below the XAML content, with
  translucent panels on top.

## Measure first

Record each baseline on the same build and device before changing code.

1. **Ink.** Run the Pencil and G-Pen series from
   [measuring](../performance/measuring.md) (`windows-pen-latency.ps1`, then
   `windows-pen-report.mjs`). Capture PresentMon's `PresentMode` for the
   canvas swap chain:
   - with default glass panels beside the stroke;
   - with panels over the stroke;
   - in Zen.

   Build a bare reference window as a local probe under `artifacts/`, not
   committed. It should be a Win32 window with a D3D12 flip-model swap chain,
   latency 1 and tearing, drawing a dot at the newest pointer position. Drive it
   with the same pen driver. Its numbers are the fastest native path, the
   guardrail's lower bound. Software timestamps exclude the digitizer and
   panel. If someone can film the pen tip with a 240 fps or faster camera on
   the Surface Pro, ask for that; otherwise report physical latency as
   unmeasured.
2. **Drift.** Capture the composed window during a vertical divider drag
   between stacked groups, a tab slide and a column open/close, in both themes.
   [`PanelBridgeCapture.cs`](../../apps/layer-windows/scripts/PanelBridgeCapture.cs)
   shows the capture pattern. Report:
   - the largest offset in physical pixels between a fill edge and its XAML
     frame;
   - the share of frames with a tab label outside its tab.
3. **UI speed.** For divider drags, measure changing displayed frames/s, p99
   gap and UI CPU per change, using the column-resize workload in
   [the top-tier table](../performance/top-tier.md#windows-gpu-vm-column-resize).
   For window resize, use the
   [resize handoff workload](windows-resize-120hz-handoff.md#focused-resize-workload).
4. **Profile.** Record one divider drag and one window resize with WPR CPU
   sampling and open them in WPA. Split UI-thread time into:
   - snapshot JSON decoding and `Stringify` keys;
   - XAML measure and arrange;
   - composition updates and `CTitleBar`;
   - `measurePanels` offscreen bodies;
   - glass collection.

## Work

Each step is its own milestone with its regression check. Push each one as
soon as its checks pass. Steps 1, 2 and 6–8 are required. Do steps 3–5 in the
order the profile ranks them, and skip any that it shows won't pay off.

1. **Draw backgrounds in the same pass as the XAML.** Set fills, strip
   cutouts, shadows and borders from the shared layout bounds in the same pass
   that places the frames (`applyLayout`, `present`, `resize`). Stop reading
   arranged bounds a frame later. Keep the deferred `Rendering` pass only for
   measurements sent back to shared Rust: tab widths, content heights and
   scroll metrics. Extend the per-`Rendering` trace that `9aecbdc9d` added so
   a fixture fails when a fill and its frame disagree in any frame.
2. **Move glass with its panel.** Glass content may be a frame or two old, but
   its shape must move with the XAML. First check whether the renderer's
   blurred texture can feed a WinUI composition surface without a GPU wait on
   the UI thread, through a shared handle or D3D11On12. If it can, paint each
   glass panel and bridge-group fill through a composition surface brush, and
   delete the swap-chain glass path. Refresh the blur less often than the canvas,
   and never on a stroke frame's critical path. If it can't, reuse the
   `nativeResize()` opaque fills and contours for all workspace motion and
   bring glass back when motion settles. Measure ink after this step.
3. **Resolve divider-drag layout on the UI thread.** Window resize already
   resolves shared layout synchronously from the layout input (`resolveLayout`).
   Do the same for divider drags through a stateless shared-Rust call, as
   `capy_drawer_frame` does, instead of waiting for a snapshot round trip. Shared
   Rust keeps owning the result.
4. **Replace JSON keys with numbers.** `presentBackgrounds`, `measurePanels`
   and `glass` build a `Stringify` key per group on every pass. Compare plain
   values instead, as the Navigator geometry key does.
5. **Move panels instead of relaying them out.** Shift panels whose size
   doesn't change with `Translation` or a render transform, as `placeMotion`
   already does for floating drags.
6. **Frame latency 1.** Set `desired_maximum_frame_latency` to 1 and
   `FramePacing::IN_FLIGHT` to 1, then measure ink, divider drags and window
   resize. If resize or stroke throughput drops, find the stall. Don't keep
   two latency settings, and don't reconfigure the swap chain at pen-down.
7. **Sample late.** Measure the time from input drain to submit in the render
   loop. If it exceeds about a millisecond, drain the newest pen samples again
   just before encoding the stroke tip.
8. **Find the present mode.** If strokes are `Composed: Flip`, find what blocks
   independent flip or an overlay plane. Check, one at a time:
   - the scRGB format;
   - source-size scaling on the panel;
   - the alpha mode;
   - translucent XAML above the canvas.

   Report which conditions reach `Hardware: Independent Flip` or
   `Hardware Composed: Independent Flip` on each GPU. Change the UI for this
   only if the change is small; otherwise report the finding.

Then consider replacing the WinUI title bar with a custom frame. It is the
largest platform cost in a bare window. The
[resize handoff](windows-resize-120hz-handoff.md) records the feasibility work
and the checks still open: Snap, maximize, fullscreen, caption states and
close. Do it only if the profile shows `CTitleBar` is still a large share once
steps 1–5 are done. Leave the black strips at the window edges alone: they
appear in a bare WinUI window too (microsoft-ui-xaml #5148). Don't ship private
resize-sync APIs.

## Devices

Claim frame rates only from an output whose active mode is verified as 120 Hz.
The Surface Pro, 11th Edition (Adreno X1-85) has a real pen. The Iris Xe laptop
runs at 60 Hz, so its numbers are labelled diagnostics. GPU VMs have no display
output, so use them only for functional checks.
[Devices](devices.md) covers reservation and private desktops.

## Done

Every step has landed, or has been rejected with the measurement that rejected
it. Walk divider drags, tab slides, column open/close, window resize and strokes
in light and dark themes. Run the checks in [testing](testing.md) for each
change.

Report back with one table per device and commit:

| Metric | Before | After |
| --- | --- | --- |
| Drift: largest fill-to-frame offset; frames with a label outside its tab | | |
| Divider drag: changing displayed frames/s, p99 gap, UI CPU per change | | |
| Window resize: changing displayed frames/s, p99 gap, UI CPU per change | | |
| Ink: input to present p50/p95/p99, pen-down to first frame, against the bare reference | | |
| Present mode while drawing: beside panels, under panels, in Zen | | |
| GPU time per stroke frame | | |

Say what limits each motion now: the UI thread, the renderer or the compositor.
Record results in the tier tables and keep raw data in
`artifacts/windows/winui-motion/`. When the work lands, move lasting decisions
into [Windows development](windows.md) and delete this plan.
