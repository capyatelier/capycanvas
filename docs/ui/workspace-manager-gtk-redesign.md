# GTK workspace manager review

Updated for the latest user feedback, 2026-09-11. Ready for another GTK review
before adapting other hosts.

## Menus

Window starts with Undo/Redo, followed by Workspaces, Quick Access Toolbars,
and the direct panel rows. Quick Access Toolbars sits directly below Workspaces
in the main Window menu. Its entries keep their short names.
Workspaces also has a separate **Manage Workspace Templates…** entry.

![Window menu](workspace-manager-gtk/window-menu.png)
![Workspaces submenu](workspace-manager-gtk/workspaces-menu.png)
![Quick Access Toolbars](workspace-manager-gtk/toolbars-menu.png)

## Manage Workspaces

The compact dialog has a selectable list, a square **+** button at the top right,
and Cancel / Switch to Workspace buttons below the list. Rename and Delete remain
in each row's options menu. Its explanation is:

> Workspaces save your tool and panel layouts for different tasks.

Selecting or double-clicking a row previews its layout in the editor behind the
dialog. Only Switch to Workspace finalizes the selection. Cancel or closing the
dialog restores the original layout. Browsing does not switch the active workspace
or change either workspace's saved layout/history.

![Workspace list with preview](workspace-manager-gtk/workspaces.png)

## Manage Workspace Templates

The same compact design has a **+** button to save the current layout and Cancel /
Load Layout buttons below the list. Saved rows retain Rename and Delete. Its
introduction is:

> Workspace Templates save tool and panel layouts to reuse in any workspace.

Selecting a row previews the layout without applying it. Load Layout applies the
selection to the **current workspace** and closes the dialog, creating one undoable
layout change. Cancel or closing restores the original layout. The included
Default layout can also be selected and loaded.

Recently Deleted, backups, import/export, and version-management controls remain
absent from these managers.

![Workspace Template list with preview](workspace-manager-gtk/templates.png)

## Layout History

The scrollable list previews the actual editor layout. Cancel restores the layout
from before opening the dialog; Restore This Version commits one undoable change.
The explanatory “Select a version to preview…” text has been removed.

![Layout History](workspace-manager-gtk/history.png)

## Try it

Close the older GTK instance and run from the repository root:

```sh
cargo run --locked --release -p layer-linux
```

Review Window → Workspaces, both managers, and Layout History. Save a Workspace
Template using **+**, move a panel, preview that Workspace Template, then load it
and undo the change. Also preview another workspace and cancel before trying
Switch to Workspace.
