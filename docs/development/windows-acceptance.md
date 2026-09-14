# Windows acceptance status

Checkpoint: 2026-09-14. The native Windows app and reproducible portable ZIP and
unsigned MSIX are available. The full Windows goal remains open: physical input,
complete visual/accessibility review, deployment lifecycle and actual 120 Hz
painting have not all passed.

This is the current acceptance index. The [host notes](../../apps/layer-windows/README.md)
and [implementation history](../history/windows-implementation.md) retain older
checkpoints; their earlier lists of missing features are not the current backlog.

## Validated application and packages

The reviewed Release application and both packages contain `46a294f`, including
shared header surfaces, native layer keyboard actions, contact-brush mask painting,
Preferences accessibility, curve transactions and the shared stationary-lasso fix
from `dd98ed3`. Portable assembly and extracted runtime checks pass; unsigned MSIX
assembly and archive guards pass. Both packages use the clean published source.

The integrated build
passes 399 shared UI and 110 Windows tests (11 hardware/manual tests ignored),
and the expanded native canvas journey passes all 22 checks. A stationary lasso
tap preserves the existing selection, full PNG export and pending Redo after
Undoing either a scale or rotation. Native mouse and injected pen both pass.

Native curve drags now preview in one shared transaction. Release commits one
Undo step; Escape, capture loss and hiding Properties restore the original curve.
Unchanged clicks keep Redo and preserve the grab offset. At `457174e`, the full native effects
fixture passes with OS-delivered mouse, pen and touch: completed drags, unchanged
clicks, source hiding, Escape and insertion rollback all preserve exact history.
Pen device removal and canceled touch input also restore the preview. The existing
editor journey passes, including numeric-draft Escape, retained fields, themes,
Zen and normal shutdown. The shared effect-gesture regression now includes Windows
and passes across all five tested platforms. These are synthetic input checks,
not physical digitizer or 120 Hz acceptance.

The preceding integrated release (`ae0cf85`) passed 398 shared UI and 110 Windows
tests; 11 hardware/manual tests remained ignored. Its full native Preferences
fixture and actual Narrator journey also passed after merging upstream effect history.

The layer change (`483ad16`) passes the complete native layer journey: native
Toggle states, Space activation, alpha-lock Undo/Redo, Menu and Shift+F10 on names,
content and masks, F2 rename, retained rows, masks, grouping, virtualization,
theme/document replacement and normal shutdown. The integrated source also passes
52 engine tests and five hardware D3D12 project tests. All twelve contact presets
paint both content and hidden masks, with exact save/reopen and Undo/Redo pixels.
The mask case covers the upstream fix that retains contact geometry while removing
paper material from coverage brushes. These are functional checks, not a complete
screen-reader or physical-input review.

At `f20611a`, the full native header catalog/Preferences journey, 26 menu
checks across mouse, injected pen/touch and keyboard, three hardware D3D12 contact
brush tests, Release compilation and strict Windows Clippy passed. Its Web GPU
raster journey passes painting, exact save/reopen/history, corrupt-file retention,
GPU replacement and IndexedDB reload recovery. Explicit grouping fixes the contact
shader's browser WGSL validation failure; Navigator reflow now defers during GPU
suspension, with a regression check in that recovery journey.

Strict Web Clippy on Rust 1.98 reports two `arc_with_non_send_sync` warnings in
unchanged renderer code. An initial browser raster attempt timed out after archive
export; the final run passed with the existing 30-second gates and better wait
error context. No timeout relaxation or production timeout fix is claimed.

At `f051e61`, the selected integrated Rust suites passed 632 ordinary tests, with
12 hardware/manual cases ignored, plus seven D3D12 functional tests covering
contact brushes and project/painting recovery. Those broader suites were not all
rerun for this header change. None of these results establish painting performance.

| Area | Recorded result | Remaining limit |
| --- | --- | --- |
| Workspace/header/docking | Shared title-bar customization, tools picker, drawers, floating panels, workspace history and persistence pass their native journeys. Collapsed columns have the corrected footer grip, blank-space menu, double-click/tap expansion and drag-out resize (`f99507a`); docking targets follow Web/Android (`d4877e3`). | Physical device acceptance and complete visual review remain open. The previously qualified synthetic cross-canvas capture failure is not a reason to change production capture without new evidence. |
| Matched workspace design | Paint, Sketch and Photo each have twelve native/Web scene pairs: three widths, both themes, fitted and paper-under-header cameras. Camera values match. Sketch's measured header passes; Paint Tool Set/Layers and Photo Layers measurements pass. Disabled header contrast is corrected (`9529ec2`). At `f20611a`, eight fresh Photo pairs cover both themes, two widths and both cameras. Five sampled light header backgrounds match Web exactly: menu/title/settings RGB 219, switcher RGB 222. | Paint/Photo full comparisons still report a 21-physical-pixel header width difference and compact control-set differences. Full images are preserved; geometry passes do not mean whole-image identity. Color-wheel polish is closed under the imperceptible-difference/clean-code requirement. |
| Tools and canvas editing | Seventeen-tool projection, expressions, retained drafts/controls, subtools and cancellation pass. With the shared `dd98ed3` lasso fix, mouse and injected pen each pass figure/lasso, move/scale/rotation preview and Cancel, and applied translation, 1.5x scale and 90-degree rotation: twenty-two checks. Scale/rotation Undo/Redo restores exact full 2048×1536 PNG exports; native export preserves the checkpoint, and stationary lasso taps preserve selection and Redo. | Translation checks sample three 16×16 artwork interiors. Complete exports verify scale/rotation history for these cases; arbitrary affine combinations, cross-platform image equivalence and physical devices remain separate. |
| Keyboard and text | Canvas shortcuts work after toolbar clicks; fields, sliders, menus, Tab and button activation retain native behavior (`509833e`). Actual Microsoft Japanese IME composition, conversion, Enter/Escape arbitration and layer-name Undo/Redo pass on `f051e61`. Native layer keyboard actions and exposed toggle states pass at `483ad16`. | UIA names, values, patterns and focus are exercised. The bounded Narrator journey below passes, including theme focus and selected values. Complete screen-reader review, other IMEs and candidate-popup geometry remain unverified. |
| Touch and pen routing | Eighteen native injected-contact checks pass on `f051e61`: pan/pinch/rotation anchoring, third-contact pause, contact replacement, cancellation/restart, single-finger Hand and a later pen stroke with independent Undo. | Physical pressure/history, tilt, eraser, hover, cancellation and comprehensive physical touch are still required. Synthetic input and renderer pressure/tilt tests do not establish digitizer behavior. |
| Documents, storage and windows | Import/export, Unicode project paths, embedded assets, save/replacement/close decisions and two GPU reconstructions pass the document journey (`575345b`). Preference failure/retry and multiwindow isolation pass. Snap, minimize/restore, maximize/restore and painting history pass on the available 60 Hz display (`05cbdb5`). | Mixed-display/DPI and system suspend/resume remain unverified. Forced GPU reconstruction is separate from those transitions. |
| Filters | The pinned `50e3acd` renderer matches independent pre-migration algorithms on the same Windows GPU: D3D12 within one byte, Vulkan exact, across 160 sampled cases. | The original Linux PNG reference still fails for both implementations. This qualifies the sampled migration comparison; it does not establish cross-platform perceptual equivalence or newly retest filters after brush integration. |
| Portable ZIP | `46a294f`: repeated assembly produces identical bytes. Extracted payload inventory/hashes, app-local runtime origins, paths with spaces, unrelated working directory, filters, drawing/history, pan/resize and zero-exit close pass. | The exercise ran on the development host. Clean-machine acceptance remains open; deterministic archive assembly does not imply identical compiler output across machines. |
| MSIX | `46a294f`: repeated unsigned assembly, complete inventory, MakeAppx unpack, activation metadata, logos and archive validation pass. Lock retry and signature/invalid-input guards pass. | Distribution signing and installed launch/update/uninstall remain open. No signing or installation is implied by these archive checks. |
| 120 Hz painting | Input/presentation instrumentation and analysis tools exist. | Sustained actual ≥120 Hz painting and input-to-present p99 <8.33 ms are unmeasured. The available panel is 60 Hz; offscreen timings cannot close this gate. |

See the [title-bar record](title-bar-windows-acceptance.md),
[independent filter qualification](windows-filter-qualification.md), and
[build/package guide](windows.md) for the corresponding commands and limits.
Native gesture fixtures run serially with disposable profiles. Raw captures,
results, machine details and packages remain in ignored `artifacts/windows`.

## Japanese IME check

This bounded check used OS-delivered Roman letter keys through the already
installed Microsoft Japanese IME, native ValuePattern readback and shared state.
No Unicode injection or ValuePattern.SetValue supplied the composed text.
The full native preedit and committed-name captures are retained unmasked.

To reproduce in an isolated review profile:

1. Begin renaming a layer and select its existing name. Enable Japanese input,
   type `NIHONGO`, then Space: `にほんご` converts to `日本語`.
2. Press Enter once. Composition ends, the rename field keeps focus, and the
   committed layer name/document remain unchanged. Press Enter again to commit.
3. Begin rename again, select the name, type `TESUTO`, then Space to get `テスト`.
   The first Escape returns to `てすと`; the second clears composition while
   leaving rename active. The third cancels rename, preserving `日本語`.
4. One Undo restores the original name and clean document; one Redo restores
   `日本語`. Undo once more, restore the review's prior input locale, and close.

All steps passed in the same owned application, with normal zero exit within
five seconds and empty stderr. No production input change was needed. The
legacy IMM context query was unavailable for this native text focus; the real
conversion and key-arbitration results establish the scoped IME result.

## Work still needed

Prioritize observed missing behavior and the open acceptance gates above.
Completed color-wheel, column, touch-navigation and transform journeys do not
need further polish or larger fixture matrices without a new defect or relevant
source change. The remaining visual differences need a whole-editor usability
review before adding font/layout complexity just to satisfy a pixel comparator.

The Windows Home host has no existing supported clean Windows environment.
Windows Sandbox [does not support Home](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-overview).
No feature installation, edition change or MSIX installation was attempted.

Portable clean-machine acceptance comes before installed MSIX lifecycle. Real
pen/touch, screen-reader, suspend/resume and mixed-display checks must retain
their own evidence rather than inheriting a pass from automation of other routes.

When the high-refresh display becomes available, fix the hardware, workload,
duration and missed-refresh limits before measuring. The original matrix includes
4K/32-layer documents, G-Pen, large eraser, Natural Blender, Watercolor Wash,
pen-up, pan/zoom and controls animating during painting. Use repeated warm runs
and a sustained thermal run; retain p95/p99/max and missed-refresh counts.
Measure actual presentation cadence separately from input-to-present latency,
and keep readback/captures outside the measured gestures. Until then the 120 Hz
requirement remains open.

## Narrator check

Actual Windows Narrator was exercised with OS-delivered keys in disposable
profiles. On the `35131be` baseline it read tool names, numeric brush-size labels
and values, Alpha lock off/on/Undo, other layer flags, F2 rename and its Undo,
and content/mask keyboard menus. UIA enters a region and inspects state; this
does not establish complete keyboard discoverability.

Changing theme could move focus to Fit canvas, and a collapsed styled selector
could omit its selected value. Preferences now retain native controls and
accessibility identities while refreshing colors and icons. The window preserves
its focused Preferences control across theme updates. Named native choice items
expose the collapsed selected value.

The final Release passes the complete Preferences fixture, extended with rapid
keyboard Light/Dark/System selection, stable native identity, retained focus and
readable selected values. Validation, field retention, icon selection, search,
dependent controls, dialog reopening and shortcut editing also pass. Narrator
reads Color theme with Light or System after selection; Tab reaches the next
field and Close is readable and actionable. Both owned processes closed normally;
app exit was zero and stderr was empty.

To reproduce: open Preferences with Narrator running, focus Color theme, open it
with Alt+Down or F4, select Light and press Enter. Check its spoken value and focus.
Restore System, Tab to the next field, then focus and activate the dialog's Close.

Speech evidence used Read item and Copy last spoken phrase, restoring the original
clipboard. Initial UIA focus entry used Move Narrator cursor to system cursor.
See the [official Narrator commands](https://support.microsoft.com/en-us/accessibility/windows/narrator/appendix-b-narrator-keyboard-commands-and-touch-gestures).
This is scoped assistive-technology evidence, not full accessibility certification
or physical input/performance acceptance.
