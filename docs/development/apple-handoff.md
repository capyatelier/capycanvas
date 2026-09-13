# Apple port handoff

## Goal

Ship complete, visually consistent native iPadOS and macOS apps with fast,
readable workflows, shared maintainable code, and validated drawing performance.

Full acceptance remains in [the Apple goal tracker](../history/apple-acceptance.md#goal):
all exposed features, menus and actions; shared main-editor geometry and canvas
behind the header; iPad 120 Hz and current Mac 90 Hz performance targets. Mac
120 Hz testing is deferred. The overall goal is **incomplete**.

## Latest editor milestone

- Branch `main`. The shared flat menus, attached column panels and icon integration
  are grouped into one major milestone. Preserve any later working-tree changes.
- The Edit trigger is fixed with a full label hit shape. `ViewThatFits` is retained.
  Popup content now inherits the root's current theme; copying the invoking
  control's complete environment had made dark-menu text unreadable.
- The embedded UIKit context source also transfers only appearance and enabled
  state. This restores Zen accessibility on the connected iPad. Existing UIKit
  hold/pan recognizers retain the original contact. `EditorActionMenu`,
  `EditorMenuButton` and `editorPopover` share vertical, opaque presentation.
  No native context-menu/drag-session handoff or extra popup window remains.
- Menus reuse native keyboard capture, with shared navigation/actions and UIKit
  responder restoration. macOS retains its OS application menu bar and native
  secondary-click adapter. Its Select All action forwards to the focused text
  editor before canvas dispatch. Toolbar grip labels include their toolbar names.

## Verified progress and limits

- Connected iPad: both-theme main menu/Undo; layer/mask/footer anchors/actions;
  upward layer dragging with exact Undo/Redo; submenus, arrow navigation and
  command shortcuts; keyboard routing after dismissal; workspace menu actions
  and held dragging in both directions. Five tests pass together. Zen passes
  separately after its accessibility fix, including hold/tap suppression and
  the resulting Total Zen action. No skips; the earlier failure stays recorded.
- Mac: an isolated editor passes both blend choices and Undo/Redo. Actual native
  events in a separate fixture verify submenu/back navigation, disabled rows,
  Return, Escape and shifted shortcuts. Neither test addresses system-menu coordinates.
- Both hosts pass workspace pin/order persistence, toolbar styles/Zen and full
  toolbar creation/rename/duplicate/delete. Final customization follow-ups verify
  Mac Command-A routing and accessible toolbar names; both tests use native
  Select All text replacement, avoiding caret-dependent backspace counts.
- UIKit callback checks pass touch/pen/mouse classification, edge scrolling,
  hold/lift retention, immediate grips and cancellation. These are not a new
  physical Pencil pass. Attached-column and icon evidence is preserved below.
- Both signed Debug hosts build; 41 Apple bridge, 25 host and 306 UI tests pass,
  with one existing hardware-only host test ignored.
- iPad Escape/Return remain unverified. A first-responder simulator probe received
  a printable key but no XCTest Escape press or key-command callback; the physical
  Escape check also failed, and simulator Return did not execute a menu action.
  Do not add a product workaround just to accept missing synthetic input.
- Exact results and deployment state: `artifacts/apple-flat-menus-v1/checkpoint.json`.
  Earlier column/icon evidence: `artifacts/apple-column-panels-v1/` and
  `artifacts/apple-icons-checkpoint.json`. Raw evidence stays out of Git.

## Continue toward full acceptance

Use the goal tracker's complete inventory and remaining gates. Current performance
still fails sustained presentation cadence: Mac has residual long intervals and
iPad retains drawable-acquisition stalls. The other drawing workloads, calibrated
CPU/GPU/input-latency measurements, complete feature/lifecycle workflows and the
full visual matrix remain open. See [performance evidence](../../apps/layer-apple/PERFORMANCE.md)
and [Apple testing instructions](../../apps/layer-apple/README.md).

Read `AGENTS.md` and the shared drag convention. Keep vertical menus, opaque
shared colors, simple native adapters and regular artist app data. Do not ask
for another speculative physical retry. Keep automation focused and never
coordinate-test the Mac system menu bar; check editor effects directly.

Fetch/integrate other ports periodically and before publishing. Commit and push
only completed major milestones to `main`, using author and committer
**Zack Drach <zackdrach@gmail.com>**. Public HTTPS fetching works; the verified
repository SSH-agent socket is recorded only in the ignored local checkpoint
folder. Publish source/docs only, without device/account identifiers, signing
details, raw logs or captures. No overall-goal completion is claimed here.
