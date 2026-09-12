# GTK workspace manager redesign

Ready for user review, 2026-09-11. This implements the latest GTK feedback;
other hosts remain gated on approval.

## Window menu

Undo Workspace Change and Redo Workspace Change come first, followed by
Workspaces, direct panel visibility rows, and Quick Access Toolbars.
Panel and toolbar names omit their generated type suffixes. New Toolbar and
Manage Toolbars are inside Quick Access Toolbars.

![Window menu](workspace-manager-gtk/window-menu.png)
![Quick Access Toolbars](workspace-manager-gtk/toolbars-menu.png)

## Manage Workspaces

A single list provides New Workspace, Switch, and a per-workspace options menu
with Rename and Delete. The current workspace is marked. Search appears for
larger lists. Workspace Templates, Recently Deleted, and Backups and Storage
are available through More options. Storage maintenance and recovery tools are
under More storage options.

![Workspace list](workspace-manager-gtk/workspaces.png)

## Layout History

One modal contains a scrollable version list. Selecting a version previews the
actual editor layout behind the dialog. Cancel restores the layout present when
the dialog opened. Restore This Version commits one undoable change. Selecting
the current version disables Restore.

Previewing does not change autosave content, undo/redo, settings, or artwork.
The workspace keeps its ownership lease while the dialog is open. The history
modal uses lighter background shading to keep the preview visible.

New history entries describe the action and affected panel or toolbar, such as
“Moved Layers panel” and “Added Ink Tools toolbar.” Existing generic entries get
names where their predecessor is known from retained undo/redo. Old abandoned
branches without recorded ancestry appear as “Earlier layout”; their layouts
remain previewable and restorable.

![Live layout history preview](workspace-manager-gtk/history.png)

## Language

User-facing terminology consistently uses Workspace Template. The save dialog
introduces its benefit before asking for a name:

> A Workspace Template saves the exact layout of your tools and panels so you
> can load it again whenever you want. Give this layout a name, such as “Inking”.

Workspace, toolbar, deletion, backup, and recovery copy explains what the action
helps the user do. Internal storage mechanics and redundant brush-setting
assurances have been removed from normal flows.

## Try it

Close an older GTK instance and run from the repository root:

```sh
cargo run --locked --release -p layer-linux
```

Review Window → Workspaces → Manage Workspaces, Save Layout as Workspace
Template, and Layout History. Move a panel, preview an older version, cancel,
then restore it and undo the restoration. Also open Quick Access Toolbars and a
panel's right-click menu.

Validation: 264 shared UI and 30 native storage tests; GTK manager/history,
restart and independent windows, ownership takeover, unavailable-storage close
recovery, backup export/import, and real pointer-driven Window/File/panel menus.
The native history check verifies that selection changes the actual layout,
leaves SQLite content unchanged, renews ownership while open, cancels cleanly,
and restores exactly one undoable history event.
