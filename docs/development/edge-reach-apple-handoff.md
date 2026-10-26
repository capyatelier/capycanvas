# Apple edge reach handoff

[Apple development](apple.md) · [Apple porting guide](../APPLE_PORTING_GUIDE.md) ·
[Edge reach](../ui/panel-customization.md#edge-reach)

Finish edge reach on iPadOS and validate it on an iPad, then fetch, rebase and
push to `origin/main`. macOS now forwards edge contacts through the shared owner;
UIKit still uses the thin resize handles. Delete this handoff in the commit
that completes the iPad port.

## What edge reach is

A one-finger, Pencil or pointer swipe that starts near a dock divider or floating
panel edge grabs that edge once it has moved about a third of a tile, and the
edge eases over to the contact. The reach, ownership, recognition, catch-up and
reset rules all live in shared Rust; read the
[Edge reach](../ui/panel-customization.md#edge-reach) section first, including
its shared-session paragraph. The host only classifies the control under a
contact, forwards it, and acts on the replies.

## Reference implementations

| Area | Source |
| --- | --- |
| Reach geometry and recognition | `crates/layer-ui/src/edge_grab.rs`, `edge_grab_tests.rs` |
| Session state, replies and catch-up | `crates/layer-ui/src/edge_contacts.rs`, `edge_contact_tests.rs`; `UiInput::EdgeContact`, `UiInput::EdgeDoublePress` and the `edge_*` fields of `InputReply` in `interaction.rs` |
| Owner model for divider drags | `crates/layer-ui/src/layout_columns.rs` (`divider_column`, `OwnerResize`) |
| GTK (authority) | `apps/layer-linux/src/workspace_edge_grab.rs`, tab deferral and `DragTarget::Edge` in `workspace.rs`, `swipe_control` in `input.rs`, journeys in `workspace_edge_grab_tests.rs` |
| Web | `apps/layer-web/edge-reach.js`, the edge handlers in `app.js`, `edge-reach.test.mjs` |
| Android (async session) | `DockInteraction` edge methods, `edgeSurface`/`edgeControl` markers and `followEdge` in `WorkspaceInput.kt`; `CanvasHost.edgeContact`; `AndroidInteractionTest.edgeReachGrabsColumnAndGroupEdges` |
| Windows (async session) | `WorkspaceGestures.cpp`; `capy_edge_reach` in `native/src/shared_controls.rs`; `scripts/exercise-edge-reach.ps1` |

Commits: shared model `1d7e8ab27`, Web `56988f820`, `edge_pending` and held
pickup fix `76ab3ddbd`, Android `e6c3a536c`, Windows `68897e259`.

## Apple integration

- `EditorStore.input` submits to `NativeOwner`, so replies arrive after the
  event, as on Android and Windows. Act on the latest reply. Where a decision
  must be made at press time (a scroll view or gesture recognizer that would
  otherwise start at its slop), expose a stateless bridge query like Windows'
  `capy_edge_reach`, which runs `ResolvedLayout::edge_grab_at` on the published
  layout. Do not reimplement reach rules in Swift.
- Workspace contacts go through `Shared/Bridge/WorkspaceReorderInteraction.swift`,
  `NativeReorderModel.swift`, `ReorderContact.swift` and each platform's
  `Platform/NativeReorderInput.swift`. The handles are `WorkspaceResizeHandle` in
  `Shared/Editor/WorkspacePanels.swift`, whose band double-click `reset` the
  reach's double press replaces. Canvas input is `macOS/Input/MacInput.swift`
  and `iOS/Input/PencilInput.swift`.
- Classify like the other hosts: swipe controls (sliders, number and text
  fields, navigator, color wheels, curves, palette tiles, layer rows, grips,
  held tiles) are `input`; tabs, immediate drag sources and horizontal
  scrollers are `pickup`; panel bodies, closed strips and the canvas are
  `free`. Send canvas contacts too; Rust gives only touch a canvas reach.
- Replies: withhold motion while `edge_hold`; defer a pressed tab's selection to
  release on `edge_select_on_release`; on `edge_cursor`, end the pending
  workspace gesture, cancel the canvas stroke and other pointer owners, take
  the contact and show the resize cursor for pointer and Pencil hover. Stop
  forwarding a contact once a reply has neither `edge_pending` nor a cursor.
  Cancel a pending edge when a hold arms a pickup or a workspace drag starts.
- Withholding motion must not cancel taps. Android learned this the hard way:
  hide only moves beyond the platform drag threshold, and never swallow a
  release unless the edge owns the contact.
- A double click or double tap in the reach sends `EdgeDoublePress`, except on
  buttons and controls.
- The catch-up runs in `UiSession::frame`, and a claim sets `canvas_wake`; keep
  `CanvasFrameDriver` presenting while the session asks for frames.

## Shared behaviour that changed under the existing Apple tests

A closed multi-column stack's edge is no longer fixed: dragging it opens the
member beside the contact. A column edge acts only on the column on its
screen-edge side, and toolbar-only columns stop at one lane instead of
collapsing. Rerun `tests/column-stacks.swift` and `tests/workspace-motion.swift`
and update any expectation that assumed the old rules, as Web's
`column-stacks.test.mjs` and Android's `stackedColumnsOpenAndResizeOrdinaryGroups`
were updated.

## Done when

- A Swift fixture (Mac, mouse and pen) and an iPad UI journey (touch and Pencil)
  cover, in light and dark: an outward swipe from inside a column grows it to
  the contact with the resize cursor; touch from the canvas grabs the edge
  while pen and mouse keep the canvas; a vertical swipe never grabs a column
  edge; a vertical swipe beside a group gap moves it; a vertical swipe from the
  tab strip under a gap moves the gap without selecting the tab; a swipe from a
  closed strip opens it; a double press in the reach resets the width; buttons
  and sliders in the reach still work.
- Existing workspace fixtures and UI tests pass, or fail identically on a build
  without the port (A/B on the same device).
- The implementation paragraph in
  [panel customization](../ui/panel-customization.md#edge-reach) describes
  Apple, its checks list names the new fixture and journey, and
  [Apple development](apple.md) lists them.

Use the reference Mac (M2 Pro Mac mini) and an iPad reserved through
`tools/devices/devices.py --platform apple` ([devices](devices.md#apple)).
