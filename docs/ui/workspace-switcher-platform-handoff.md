# Goal

Implement the configurable workspace switcher on your platform, matching GTK/Web.
In Manage Workspaces, add Show in top bar, a pin indicator, narrow left grips on
all rows, and Move Up / Move Down. Style the switcher without a border, using the
theme background mixed with 20% black for a recessed slider-track look, and a
subtle blue active choice. All rows reorder; the top bar follows that order and
skips hidden entries. Temporarily prepend the current workspace if it is hidden,
until the user switches away; keep saved pins/order and dialog previews unchanged.
Right-click or touch/pen hold opens the row menu;
mouse holds never open menus. Dragging with the same held contact closes the menu.
Handles drag immediately on every device.
Preserve scrolling, preview/Cancel behavior, workspace contents, and persistence.
Test mouse, touch/pen where supported, keyboard, cancellation, and restart.

# Pointers

- Shared behavior/storage: `crates/layer-workspace/src/manager_switcher.rs`.
  Render `switcher_display_ids()`; `switcher_ids()` remains the saved pin selection.
- Shared controller: `controller.rs` in that directory; `WorkspaceInput::EditSwitcher`
  and `RefreshSwitcher`, plus `WorkspaceView.switcher_display` for header choices,
  `switcher` for pin controls, `order`, and `switcher_busy`.
  Refresh on focus and notify other windows after `switcher_revision` changes.
- GTK reference: `apps/layer-linux/src/workspace_switcher_dialog.rs`.
- Web reference/tests: `apps/layer-web/workspace-switcher.js` and `workspace-switcher.test.mjs`.
- Interaction rules: [drag-and-reorder.md](drag-and-reorder.md).
- Detailed behavior/screenshots: [default-workspaces.md](default-workspaces.md#configurable-switcher).
