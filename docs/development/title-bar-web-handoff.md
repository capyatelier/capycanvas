# Port the reviewed GTK title bar to Web

Implement the current GTK behavior on Web using the existing shared Rust model.
This is a projection/input port, not another layout engine or a toolbar-docking
prototype. Earlier entries in `workspace-header-progress.md` describe superseded
designs; the current GTK code and this checklist are the reference.

## Shared implementation and native reference

- `crates/layer-ui/src/header.rs`: stable item IDs, left/center/right regions,
  sizes, defaults, geometry/overflow, actions and context menus.
- `crates/layer-ui/src/header_drag.rs`: grab-time geometry, live neighbor slide,
  detach/re-entry, destination validation and final edit.
- `crates/layer-ui/src/customization.rs` and `session.rs`: editor baseline,
  Done/Cancel, shared tool picker, tool activation, drawers and durable captures.
- `apps/layer-linux/src/workspace_header{,_editor,_drag}.rs`: GTK projection.
- `apps/layer-linux/src/workspace_header_tests.rs`: acceptance journeys.
- Read `docs/development/web.md`, `web-packaging.md`, and the required
  `docs/ui/drag-and-reorder.md` before implementation.

## Current behavior to port

- The title bar contains individual components/tools, **not docked toolbars**.
  Render plain icons with shared tool semantics; no surrounding toolbar shell.
  Preserve the full canvas behind the overlay and native/browser window behavior.
- Label the entry point **Customize Title Bar…**. The inline editor is a compact
  horizontal component bank plus Small/Medium/Large, Show footer, Cancel and
  Done, wrapping/scrolling sensibly in small windows. No editor heading, size
  label, help block, reset button, region-add buttons or extra reorder rows.
- While editing, the **entire item/chip drags immediately after movement slop**,
  with mouse, touch and pen. Decorative grips are not the only drag target.
  Bank chips are inert: no click/tap/keyboard-to-add. Add Tools is a bank chip;
  its valid drop opens the **existing toolbar tool picker** at that destination.
  Do not embed the whole picker in the editor or recreate tool-selection logic.
- Reordering slides the item and neighbors in real time. Detachment keeps the
  original grab offset; re-entry restores the appropriate preview. Drop outside
  removes an existing item; singleton components return to the bank, tools go
  away. Dropping a bank item outside cancels. No vertical blue insertion cursor.
- Keep an easy-to-hit empty center zone, native-control exclusions, overflow,
  cancellation/capture loss/source invalidation and stable IDs. Use shared Rust
  drag policy; do not infer insertion positions from animated DOM neighbors.
- Existing-item selection supports Delete/Backspace and Left/Right, including
  crossing region boundaries. Keep appropriate shared context menus. Native
  text editing and tool-picker keyboard input must not be intercepted.
- Done publishes the preview; Cancel restores it. Save, workspace switching,
  reload and closing during customization must not persist a temporary preview.
- One bar size scales all items consistently. Retain selector background,
  rounded hover, centered grips, shared spacing and equally padded controls.
  Open drawer tiles have square lower corners. Clicking other title-bar/toolbar
  space dismisses drawers. Action buttons use grey pressed feedback; blue means
  a selectable tool is selected.
- Menu-label visibility belongs under Window/customization, not a redundant
  context-menu checkbox. Footer visibility is workspace-owned; zoom/rotation
  stay bottom-right. Zen means full Zen only; no partial-Zen setting.

## Defaults and platform distinctions

The latest requested names are **Sketch** (minimal Painter preset), **Paint**
(panel-heavy Illustrator preset), and **Photo**. Do not swap the stable built-in
IDs or their saved layouts. Sketch's bar is Capy/Menu/filters/lasso/transform on
the left, workspace switcher centered, brush/blend/erase/layers/color on the
right; settings stays trailing. Use the shared platform defaults, not copied
arrays. Fullscreen is Web-only. Clock/battery appear only in fullscreen (tablet
presentation counts as fullscreen), although their saved positions are retained.

Native ownership now uses kernel locks plus SQLite fencing. **Do not port file
locks/PIDs to Web.** Keep browser ownership and IndexedDB transactions separate;
preserve fencing and interrupted-save recovery. Browser timeout behavior remains
covered by `browser_leases_still_expire_and_fence_stale_writers`.

## Acceptance

Build Web and exercise real browser mouse/touch/pen journeys, not just serialized
state. Cover bank/body pickup and inert clicks, drop-only Tools and picker Cancel,
reorder/detach/remove/re-entry, keyboard/context menus, all drawers and action
feedback, Done/Cancel and save/switch/reload, small windows/overflow/empty-center,
both themes, all sizes, true 2× scale, fullscreen/status, footer and full Zen.
Keep ordinary toolbar hold-drag behavior unchanged outside this explicit editor.
Run shared tests and existing Web regressions. Record captures/results and any
device limitations before handing it back for review.
