# Default workspaces

[Workspace and UI](README.md) · [Title bar](window-bar.md) · [Stacked columns](stacked-columns.md)

The included workspaces are **Sketch**, **Paint** and **Photo**. Their stable
IDs keep the older names: `builtin:workspace:painter` (Sketch),
`builtin:workspace:illustrator` (Paint) and `builtin:workspace:photographer`
(Photo). Workspaces are the only saved product item: there is no Save Layout or
Load Layout, and workspace changes save automatically.

## Design criteria

- **Familiar layouts.** Arrangements follow what artists know from established
  applications. [Procreate](https://help.procreate.com/procreate/handbook/interface-gestures/interface)
  and [Clip Studio Paint's Simple Mode](https://help.clip-studio.com/en-us/manual_en/090_tablet/Tablet_interface.htm)
  keep drawing tools, color, layers, size, opacity and undo close at hand, which
  Sketch follows. [Affinity Photo](https://affinity.help/photo2/English.lproj/pages/Workspace/interface.html)
  separates editing tools from settings panels, and
  [Photoshop](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/collapse-expand-icons.html)
  keeps frequent panels expanded and secondary ones as icons, which Photo follows.
- **No inert controls.** Every button and menu item performs a real operation.
  Do not add placeholders for missing features (clone, healing, crop, text,
  comic frames, line correction). A delivered command that is unavailable in the
  current state is disabled and publishes its reason.
- **Menus.** The application menus are **File, Edit, Layer, Select, Filter,
  View, Window, Help**, in that order, and fit beside a centered title in a
  1200px-wide window. File owns documents, windows, open, save and export; Edit
  owns document undo and editing; Layer projects the layer context menu's
  actions; Select owns selection commands; Filter has one submenu per filter
  category, including runtime-loaded filters; View owns canvas, navigation and
  Zen display; Window owns workspace, panel and toolbar management; Help owns
  shortcuts, links and About. Rust owns all menu copy, sections, availability
  and shortcut hints.
- **Tool keys** follow [Clip Studio Paint's families](https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Tool_Shortcuts.htm):
  P Pen/Pencil, B Brush/Airbrush/Decoration, E Eraser, J Blend/Liquify, M
  selection, W Auto select, F Fill, G Gradient, O Move, U Figure, Shift+U Ruler,
  H Hand and I Eyedropper. Repeating a family key cycles its tools in toolbar
  order. Space pans temporarily, Tab toggles Zen, and Primary+0 fits the canvas.
- **Tool Set and Tool** show only the active tool's groups and settings. Group
  buttons have an icon and a name, with exactly one selected; the list below
  shows the group's subtools with stroke previews, and each tool remembers its
  last subtool. Both panels fit three standard tiles at their minimum width;
  Layers keeps a six-tile minimum.
- **Dividers** are real toolbar items with stable identities, compact geometry,
  drag support and context menus, not disabled commands or blank tiles.
- **Approval.** A new design is implemented and validated on GTK and approved by
  the user before other hosts port it.

## Arrangements

[`layout_presets.rs`](../../crates/layer-ui/src/layout_presets.rs) defines every
arrangement and its initial working state, shared by all hosts.

| Workspace | Arrangement |
| --- | --- |
| Sketch | Title bar with Capy, Main Menu, Filters, Select and Scale/rotate on the left, the workspace switcher in the center, and Brush, Sculpt, Eraser, Layers and Color on the right (Web adds Full Screen). A compact toolbar centered on the left edge holds the brush size and opacity sliders, the color picker, Undo and Redo. No docked panels; Medium tiles; no zoom readout. Starts with Brush. |
| Paint | Tools toolbar on the left edge and Commands toolbar on top. An expanded left column holds Tool Set/Diagnostics, Tool/Brush size and Color/Palettes. The right column is a collapsed stack of Navigator/Proof, Properties/Filters and Layers, opened on load. |
| Photo | Commands toolbar outermost at the top, with Tool Options appended and without Clear, Fill Selection and Flip. Tools toolbar with Small tiles and the extra selection tools. A permanently expanded far-right column of Color/Palettes, Properties/Filters and Layers, and a collapsed strip beside it with Tool Set/Diagnostics, Tool/Brush size and Navigator/Proof, closed on load. Starts with Move. |

The Paint Tools toolbar holds Pen, Pencil, Brush, Eraser, Airbrush, Decoration,
Blend, Liquify; Lasso, Auto select, Fill, Gradient; Move, Figure, Ruler, Hand,
Eyedropper and the color selector. Its Commands toolbar holds New, Open, Save;
Undo, Redo; Clear, Fill selection, Scale/rotate; and Flip horizontal.

In Paint, the Color and Navigator groups take their content height: Color
follows its SDR or HDR wheel and footer, and Navigator follows the document
shape, from a 4:1 strip up to a square. Tool Set and Tool share the rest of the
left column evenly, and Properties and Layers keep their 30:45 split. Windows
keeps proportional columns.

Sketch's Brush button opens **Brushes → Tools → Tool**: Brushes lists the
drawing sets (Paint, Pencil, Pastel and the other media), Tools lists the chosen
set's tools with stroke previews, and Tool holds the settings. Brush restores
the most recently selected drawing tool; clicking it while selected opens or
closes its drawer, and choosing a set keeps the drawer open and restores that
set's last tool. **Sculpt** opens **Sculpting → Tools → Tool** with Blend and
Liquify, and Eraser has a two-panel **Tools → Tool** drawer. Brush and Sculpt
each remember their own selection across workspace switches and restarts.
Brushes, Sculpting and Tools are ordinary panels: Brushes starts 160 logical
pixels wide, shrinks to 104, and uses rows at least 44 pixels tall (48 dp on
Android). **Select** remembers the last selection tool and opens a two-panel
drawer of the selection tools; see [selection tools](selection-tools.md).
**Filters** opens **Filter Type → Filters → Properties**. Choosing a filter
inserts it above the selected layer, or replaces the selected filter while
keeping its identity, mask and clipping. Cancel deletes the selected filter and
closes the drawer, as one undoable step.

Photo adds the Crop tool after Operation in the Tools toolbar on every host (see the
[crop bar](canvas-action-bar.md#crop)).

## Workspace behavior

- Fresh installations open Paint. Startup resumes the saved workspace when
  available. If another window owns it, reuse an available built-in (Paint,
  Sketch, then Photo), then an existing user workspace; only when every
  workspace is in use does the window create a copy named after its source.
  Deleting the active workspace also reuses an available built-in.
- Built-in workspaces save tool and layout edits normally but cannot be renamed
  or deleted. `metadata.builtin` enforces this in SQLite and the browser store.
- Selecting a workspace restores its latest settings and arrangement; it never
  reapplies the shipped preset to a healthy workspace. If another window owns
  it, focus that window through the ownership path instead of taking it over.
- A workspace that cannot decode or validate is listed as unavailable and never
  replaced. Switching to it reports the error; startup falls back to the next
  available workspace.
- Seeding is idempotent. Upgrades keep existing workspaces and resume the active
  one. On a name collision the user's workspace keeps its name and the seeded
  one gets a numeric suffix. Untouched built-in layouts upgrade to the current
  default; customized ones keep their arrangement until Restore Starting Layout.
- Restore Starting Layout loads the latest shipped layout for built-ins and the
  saved starting arrangement for custom workspaces and copies. Its dialog
  previews exactly what it applies. It keeps working tool settings and document
  edits and adds one undoable layout change.
- Reset All Brushes resets every brush preset's settings in the current
  workspace, including inactive presets, through `UiSession::reset_workspace_brushes`.
  It keeps color, selected tool, arrangement, document edits and other
  workspaces, and adds no layout history.
- Ordinary opening and closing of a collapsed strip is transient and adds no
  layout history.

## Workspace manager

| Term | Meaning |
| --- | --- |
| Workspace | A named layout and its latest tool settings, saved automatically |
| Layout History | Earlier arrangements of the current workspace; excludes tool settings and document edits |
| Starting layout | The arrangement the workspace began with |

The Window menu starts with Customize Title Bar…, then Undo Layout Change and
Redo Layout Change, then the **Workspaces** and **Quick Access Toolbars**
submenus, then the panel rows. Workspaces lists the current workspace and up to
five others, then New Workspace… and Manage Workspaces…; Layout History… and
Restore Starting Layout…; and Reset All Brushes…. Captions come from
[`workspace_manager_ui.rs`](../../crates/layer-ui/src/workspace_manager_ui.rs).

- **Manage Workspaces** is a compact selectable list with a square **+** (New
  Workspace) at the top right and Cancel / Switch to Workspace below. Each row's
  **⋮** menu has Rename, Delete, **Show in top bar** and Move Up / Move Down.
  The current workspace starts selected with Switch disabled; a workspace owned
  by another window offers Switch to Window.
- Selecting a row, double-clicking or pressing Enter in the manager or Layout
  History only previews the arrangement in the editor behind the dialog. The
  confirming button commits. Cancel, Escape, back navigation and native
  dismissal restore the original arrangement. Dialogs are opaque, over a light
  enough backdrop to see the preview.
- **Layout History — name** lists versions with the affected panels, action and
  date. The current version starts selected with Restore disabled.
- **New Workspace** asks only for a name, copies the current settings and
  arrangement into an independent workspace, and pins it to the top bar.
- Failed saves offer Retry and Save as New Workspace, and a failed close keeps
  the option to leave the window open.

Hosts keep workspace decisions in shared Rust:

- Capture accepted edits through `capture_workspace` and `workspace_working_state`,
  and let `WorkspaceManager` handle saving, naming, loading, ownership and
  publication. Do not serialize widget state or keep a second workspace model.
- Switch only at a document and workspace idle boundary, through
  `PreparedWorkspace` and `adopt_workspace`. Keep the outgoing state if the
  incoming workspace fails to load.
- During a preview, hold the workspace transition, block editor edits and saves
  of the temporary arrangement, and renew ownership. Late load replies must not
  resurrect a cancelled preview.
- Native hosts use `layer-workspace` with its `native` feature and one
  `StoreWorker::shared` per installation directory, off the UI thread; Web uses
  its IndexedDB store. Liveness uses the kernel locks described in
  [workspace ownership](../internals/workspace-ownership.md), and wrapper
  transports forward `retire_owner` on final teardown.
- No database I/O or widget rebuilding on pointer-motion paths.

The shared pieces are `workspace_session.rs` (captures, previews), the
[`layer-workspace`](../../crates/layer-workspace/src/lib.rs) crate
(`WorkspaceManager`, `presentation.rs` lists and actions, `protocol.rs`,
`sqlite.rs`, `worker.rs`) and its `WorkspaceController` in `controller.rs` for
pages, forms, previews, autosave, ownership and close.

## Configurable switcher

The title-bar switcher is a rounded 36px track with 26px choices, vertically
centered at every title-bar size. It has no border and a recessed background
like a slider track, with a subtle accent for the active choice, and scrolls
horizontally when its choices overflow. By default it sits to the right of the
document title; Sketch centers it, and the title-bar editor can move it.

Manage Workspaces keeps a single ordered list. Every row has a narrow, dimmed
left grip; workspaces shown in the top bar also have a pin icon with the tooltip
**Shown in top bar**, and the current workspace keeps its checkmark. The top bar
follows the list order and skips unpinned rows. If the current workspace is
unpinned, it is shown first until the user switches away, without changing the
saved pins or order; dialog previews do not change this entry.

Rows reorder by the list-row and grip rules of the
[drag convention](drag-and-reorder.md), with an insertion line; Escape cancels.
Moving a row never changes its pin. Move Up / Move Down gives keyboard access.
Clicking still previews; menus, scrolling and dragging keep the selected row and
its preview. Pinning and ordering save immediately as app preferences shared by
all workspaces and windows; Cancel does not undo them.

The shared manager exposes `workspace_ids` (complete order), `switcher_ids`
(saved pins), `switcher_display_ids()` (what the bar shows), `refresh_switcher`
and `edit_switcher(SwitcherEdit::{Show, Move})`
([`manager_switcher.rs`](../../crates/layer-workspace/src/manager_switcher.rs)).
Hosts read preferences at startup, on focus and when refreshing the manager;
hosts with several windows notify the others after `switcher_revision` changes.
`WorkspaceController` queues accepted pin and order edits while preferences are
refreshing. Edits run in order, can complete during background autosave, and
drain before closing releases the workspace. Hosts wait for the published
switcher revision or error to acknowledge an edit.
`StoreRequest::Switcher` returns optional visible IDs: `None` means the three
defaults and `Some([])` hides the bar. Without a saved order, pins seed it and
new workspaces follow alphabetically. `UpdateSwitcher` and `UpdateWorkspaceOrder`
compare the previously read list before replacing it, need no workspace claim,
and change no workspace generation, settings, layout or history.

## Unreadable storage

Included workspaces are not repaired one at a time. If startup cannot read the
stored workspaces, it replaces the whole store, seeds fresh included workspaces
and shows a notice; see
[workspace startup](../internals/workspace-ownership.md#startup-always-adopts-a-workspace).

## Checks

```sh
bash tools/performance/workspace-motion.sh gtk --workspace-switcher
bash tools/performance/workspace-motion.sh gtk --workspace-menus
bash tools/performance/workspace-motion.sh gtk --native-test=native_paint_fitted_columns
bash tools/performance/workspace-motion.sh gtk --native-test=native_brush_drawer_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_unreadable_workspace_storage_input --native-storage
node apps/layer-web/test.mjs --workspace-switcher
node apps/layer-web/test.mjs --headless --stale-storage
node apps/layer-web/device.test.mjs --paint-columns
```

The browser store contract first needs its fixture:
`CAPY_STORE_CONTRACT_FIXTURE=/tmp/capy-workspace-store-contract.json cargo test --locked -p layer-workspace --features native browser_transactions_match_sqlite_contract`,
then `bash tools/performance/workspace-motion.sh web --workspace-store`. Android
covers the same journeys in `AndroidTitleBarTest`. These tests use isolated
stores and never touch the normal app workspaces.
