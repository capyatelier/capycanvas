# Approved GTK workspace manager design

Approved by the user on 2026-09-12, including the final Load Layout menu label
and the default saved-layout name. This is the visual reference for other hosts;
use the [host implementation handoff](workspace-manager-host-handoff.md) for scope,
integration responsibilities, and acceptance checks.

## Menus

Window starts with Undo/Redo, followed by Workspaces, Quick Access Toolbars,
and the direct panel rows. Quick Access Toolbars sits directly below Workspaces
in the main Window menu. Its entries keep their short names.
Workspaces groups its commands by purpose:

- New Workspace / Manage Workspaces
- Save Layout / Load Layout
- Layout History / Restore Starting Layout

Undo Layout Change and Redo Layout Change describe what the top-level actions undo.
**Saved Layouts** are named arrangements that can be reused in any workspace;
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

## Saved Layouts

The same compact design has a **+** button to save the current layout and Cancel /
Load Layout buttons below the list. Saved rows retain Rename and Delete. Its
introduction is:

> Layouts save tool and panel arrangements to reuse in any workspace.

The menu entry is **Load Layout…**; the dialog title is **Saved Layouts**. Save
Layout suggests **<workspace name> Layout**, for example **Painting Layout**.
The name remains editable; existing-name conflicts use the normal naming dialog.

Selecting a row previews the layout without applying it. Load Layout applies the
selection to the **current workspace** and closes the dialog, creating one undoable
layout change. Cancel or closing restores the original layout. The included
Default layout can also be selected and loaded.

Recently Deleted, backups, import/export, and version-management controls remain
absent from these managers.

![Saved layout list with preview](workspace-manager-gtk/layouts.png)

![Save Layout name](workspace-manager-gtk/save-layout.png)

## Layout History

The scrollable list previews the actual editor layout. Cancel restores the layout
from before opening the dialog; Restore This Version commits one undoable change.
The title includes the current workspace name. There are no preview instructions.

Restore Starting Layout returns to the layout the workspace began with, even after
other saved layouts have been loaded. It remains undoable. Its command and
confirmation replace the ambiguous Reset Layout label and sit beside Layout History.

![Layout History](workspace-manager-gtk/history.png)

## Try it

Close the older GTK instance and run from the repository root:

```sh
cargo run --locked --release -p layer-linux
```

Check Window → Workspaces, both managers, and Layout History. Save a layout
using **+**, move a panel, preview that saved layout, then load it
and undo the change. Also preview another workspace and cancel before trying
Switch to Workspace.
