# Workspace switcher options plan

[Developer guide](README.md)

## Goal and scope

Make workspace visibility easy to find and change from the title-bar switcher,
with minimal visual weight and implementation work. This document plans the
feature; it does not implement it.

Add a vertical **⋮** inside the switcher well, a visibility checklist, and a
**Manage Workspaces…** entry that opens the existing full editor. Keep all
reordering in that editor. Defer workspace icons.

## Interaction

Clicking a workspace continues to switch immediately. Clicking **⋮** opens:

```text
Show in top bar
  ☑ Sketch
  ☑ Paint
  ☐ Photo
  ☐ My workspace
────────────────────
Manage Workspaces…
```

- **Show in top bar** labels the checklist. List every workspace in the existing
  saved order, including hidden workspaces. Checked means persistently pinned;
  it does not mean active.
- Selecting a checkbox toggles only that workspace's visibility and saves
  immediately through the existing preference path. Close the menu normally
  after activation; no Apply, Cancel, or custom stay-open behavior.
- Visibility changes do not switch or preview a workspace, change its layout,
  alter its order, or add document/layout history.
- Preserve the current behavior: an unpinned active workspace remains visible
  until switching away. Its checkbox remains unchecked. Clearing all pins
  retains access to the current workspace and the options button.
- **Manage Workspaces…** is the final entry after a separator. Close the menu
  before opening the existing editor, with its existing selection and preview
  behavior. Rename the editor command in Window → Workspaces to the same label.
- Right-click anywhere inside the switcher always opens the same visibility
  menu: the active workspace, any inactive workspace, the unused well, or the
  dots button. It never switches workspaces or opens the title-bar customization
  menu. No commands depend on which workspace was clicked; the dots are only
  the visible left-click/tap entry point.
- Touch/pen hold and keyboard context-menu input open the same menu. Short
  taps still switch; a recognized hold must not also switch on release.
  Escape dismisses the menu and returns focus to its invoking control.
- In the compact dropdown, keep existing workspace-switching entries. Add a
  **Show in top bar** submenu containing the same checklist, then
  **Manage Workspaces…** as the final entry. Do not add another dots button.
  This separates switching from pinning while reusing the same shared data.

Reordering, renaming, deletion, and creation remain in the full editor.
There are no Move Earlier/Later commands, drag handles, or drag reordering in
the switcher or its options menu.

## Visual treatment

Place **⋮** at the trailing edge inside the existing rounded well:

```text
[ Sketch   Paint   Photo   ⋮ ]
```

Use the existing shared
[more icon](../../apps/layer-web/icons/layer-more-symbolic.svg).
Keep the current 36px well and 26px choice treatment. Give the dots a compact
visual footprint with a hit area spanning the well's height; reserve enough
width for reliable input without widening the workspace choices.

Share the recessed background. Add no separate border, circle, permanent
divider, badge, or instructional text. Use theme colors with readable muted
contrast and existing hover/pressed feedback. Reserve the selected-workspace
accent for the active choice. Use **Workspace options** for the tooltip and
accessible name.

Keep the button visible and fixed while the workspace choices scroll. Include
its width in title-bar measurement and compact-layout decisions. Do not reveal
it only on hover. Use existing native menu scrolling for long workspace lists.

## Implementation

1. Extend the shared workspace presentation with the complete ordered set of
   workspace identities, localized display names, saved visibility, and action
   availability. The current switcher publishes pinned/displayed rows; the
   manager's full rows are absent when its dialog is closed. Build the checklist
   from existing manager state without opening the editor or starting a preview.
2. Route checkbox activation through the existing
   `WorkspaceInput::EditSwitcher` and `SwitcherEdit::Show` path. Reuse its
   asynchronous persistence, revision updates, cross-window refresh, pending
   state, and failure handling. Publish confirmed state; surface failures through
   the existing host error presentation. Add no storage schema, settings,
   dependencies, or second preference model.
3. Keep menu text, checked state, and availability in shared Rust. Hosts present
   native controls and forward input. Reuse the existing menu infrastructure;
   add checkbox semantics only where needed rather than building a popup editor.
   Update every registered Fluent catalog for new or changed copy.
4. Add the button and context-menu routing on GTK first. Secondary input from
   every child of the switcher must reach its visibility menu and stop before
   the enclosing title-bar context handler. This includes the rendered switcher
   during Customize Title Bar; preserve header placement and dragging gestures.
   Apply the same routing to the compact workspace selector.
5. After GTK visual and interaction review, follow the existing
   [host rollout](../ui/README.md#rules-for-ui-changes): obtain approval before
   porting, then Web, then Android, Apple, and Windows. Reuse the shared
   presentation and actions on each host.
6. Update [Default workspaces](../ui/default-workspaces.md) when behavior lands.
   Remove this plan once implementation and validation are complete.

Code starting points:

- [Shared switcher preferences](../../crates/layer-workspace/src/manager_switcher.rs)
- [Shared controller and published view](../../crates/layer-workspace/src/controller.rs)
- [Workspace menu commands](../../crates/layer-ui/src/workspace_manager_ui.rs)
- [GTK switcher](../../apps/layer-linux/src/workspace_switcher.rs)
- [GTK header and compact presentation](../../apps/layer-linux/src/workspace_header.rs)

## Validation and completion

Extend existing shared and host switcher tests; use the checks in
[Testing](testing.md) and [Localization](../ui/localization.md#adding-ui-text).
Follow the [input convention](../ui/drag-and-reorder.md) for holds, cancellation,
and suppression of the release click.

Shared regressions cover complete ordered choices while the editor is closed,
checked state independent of the active workspace, toggles preserving order and
workspace contents, the active-unpinned case, persistence across restart,
cross-window updates, pending edits, and save failures. Reuse existing coverage
where it already verifies these behaviors.

Walk the real journey on each implemented host in light and dark themes:

- Find the visible dots, hide and restore a workspace, and open the full editor.
- Reorder in the full editor and verify that the switcher and checklist follow.
- Right-click the active workspace, an inactive workspace, the dots, and unused
  well space separately. Each must show the same visibility checklist, including
  hidden workspaces, without changing the active workspace. Also check the
  compact selector and the switcher in Customize Title Bar.
- Exercise normal switching, touch/pen hold, keyboard activation, Escape,
  outside dismissal, and focus return.
- Check narrow windows, compact menus, scrolling choices, long/localized names,
  a long workspace list, and no saved pins.
- Confirm no accidental switching, competing title-bar menus, or change to
  Customize Title Bar placement and dragging gestures.

Assess header sizing and scrolling changes against the
[performance targets](../PERFORMANCE_TARGETS.md). Measure affected motion rows
by the [measurement rules](../performance/measuring.md) on the reference
hardware and record results where required. Report unverified hosts, input
devices, failures, and performance targets explicitly.

The feature is complete when visibility is discoverable from the switcher,
changes persist through the existing preference path, the full editor remains
the sole reordering surface, and the applicable checks and host journeys pass.

## Deferred

Workspace icons, custom icon metadata and pickers, icon-only display, custom
colors, direct switcher reordering, and draggable popup lists are outside this
feature.
