# Approved GTK workspace manager design

The compact manager was approved on 2026-09-12. Subsequent direction removes the
saved-layout library and introduces [three default workspaces](default-workspaces.md)
and Reset All Brushes. The [host handoff](workspace-manager-host-handoff.md) contains
the current contract. Earlier saved-layout screenshots are historical only.

## Menus

Window starts with Undo/Redo, followed by Workspaces, Quick Access Toolbars,
and the direct panel rows. Quick Access Toolbars sits directly below Workspaces
in the main Window menu. Its entries keep their short names.
Workspaces groups its commands by purpose:

- New Workspace / Manage Workspaces
- Layout History / Restore Starting Layout
- Reset All Brushes

Undo Layout Change and Redo Layout Change describe what the top-level actions undo.
**Layout History** contains earlier arrangements of the current workspace.

![Window menu](workspace-manager-gtk/window-menu.png)
![Workspaces submenu](workspace-manager-gtk/workspaces-menu.png)
![Quick Access Toolbars](workspace-manager-gtk/toolbars-menu.png)

## Manage Workspaces

The compact dialog has a selectable list, a square **+** button at the top right,
and Cancel / Switch to Workspace buttons below the list. Rename and Delete remain
in each row's options menu. Its explanation is:

> Workspaces save your tool settings and layout for different tasks.

Selecting or double-clicking a row previews its layout in the editor behind the
dialog. Only Switch to Workspace finalizes the selection. Cancel or closing the
dialog restores the original layout. Browsing does not switch the active workspace
or change either workspace's saved layout/history.

![Workspace list with preview](workspace-manager-gtk/workspaces.png)

## New Workspace and brush reset

New Workspace asks for a name and copies the current settings and arrangement.
All changes save automatically. There is no separate layout library or starting
layout dropdown. The three included workspaces can be edited and renamed, but
have no Delete action.

Reset All Brushes asks:

> Restore every brush’s settings in this workspace to their defaults.

Cancel keeps the settings; Reset Brushes applies the defaults to all presets in
this workspace, including inactive brushes. Current color and tool stay selected.

## Layout History

The scrollable list previews the actual editor layout. Cancel restores the layout
from before opening the dialog; Restore This Version commits one undoable change.
The title includes the current workspace name. There are no preview instructions.

Restore Starting Layout returns to the arrangement the workspace began with. It remains undoable. Its command and
confirmation replace the ambiguous Reset Layout label and sit beside Layout History.

![Layout History](workspace-manager-gtk/history.png)

## Try it

Close the older GTK instance and run from the repository root:

```sh
cargo run --locked --release -p layer-linux
```

Try the three header choices, then Window → Workspaces → Manage Workspaces.
Use **+** to copy a setup, move a panel, and preview an earlier arrangement in
Layout History. Check Cancel before restoring a version or switching workspaces.
Reset All Brushes restores the brush defaults in the current workspace.
