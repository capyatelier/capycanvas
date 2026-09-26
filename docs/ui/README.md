# Workspace and UI

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The UI is shared at the level of behavior and layout, rather than as a collection
of rendered widgets. Rust describes the editor state and the controls it needs;
each frontend presents them using its native toolkit or the browser DOM.

This lets tool commands, numeric rules and workspace configuration stay consistent
while file dialogs, focus and accessibility follow the platform.

[Command search](command-search.md) describes the shared catalog, native command
bar, context-aware invocation and validation checks.

## Different workflows, shared tools

The interface needs to serve digital painters, photographers and comic artists.
Their priorities differ even when they use the same layers and masks. A comic
artist may keep reference-layer and lasso-fill controls close at hand. A painter
may devote that space to brush settings. A photographer may need effect chains, which apply filters in order, and properties for the selected
layer or filter.

Existing habits matter as much as the choice of tools. Artists build muscle
memory around panel positions, shortcuts and repeated gestures, and that is hard
to relearn. The aim is an interface flexible enough to accommodate the designs
people already know. Configurable layouts, controls and shortcuts let the app
adapt to those habits while keeping the underlying commands consistent.

These workflows should be arrangements of a common editor rather than separate
applications with duplicated rendering code. The existing layout and panel
models let users choose which controls are visible and where they belong. Tool
Settings follows the active tool, and Properties exposes the relevant effect
parameters. The [Painter, Illustrator, and Photographer defaults](default-workspaces.md)
provide initial arrangements for these tasks and remain editable workspaces.

The proposed [color-management user journeys](color-management.md) describe
creation, photo import and adjustments, color entry, export, print proofing and HDR.
The [current SDR handoff](../history/color-management-m2-port-handoff.md) records
delivered workflows, qualification limits and the remaining platform ports.

The [image Open, Import and drop assessment](image-open-import-proposal.md)
distinguishes upstream photo workflows from local GTK placement work and sets
out the remaining editing, external-drop, format and native performance gates.

The [paintable-selection specification](paintable-selection-proposal.md) defines
Paint selection, Quick Mask, and persistent Selection Layer workflows, including
gestures, soft coverage, display, shared implementation, and acceptance checks.
The [Selection Layer design](saved-selections-assessment.md) records save/edit/load
behavior, grouping, persistence, and proposed further Quick Mask capabilities.
The [selection command inventory](selection-command-inventory.md) maps operations
to menus, canvas actions, tools, and layer rows, with implementation status and
recommended delivery priorities.

## Session, actions and views

[`UiSession`](../../crates/layer-ui/src/session.rs) coordinates the engine and
editor state. A widget sends a typed `UiAction` instead of directly modifying a
layer or a renderer resource. The session validates and executes the action, then
reports which parts of the UI changed.

For example, selecting a different layer changes the active edit target and the
relevant controls. Moving the camera changes viewport state, without requiring
the host to rebuild the layer list. A *view* here is a description of UI state, such as layer rows and available
actions, rather than rendered pixels. Hosts cache these descriptions and update
affected controls;
this also avoids replacing a widget while a user is dragging or editing it.

Commands have stable identities and shared availability rules. A button, menu item
and shortcut should invoke the same action and agree on whether it is enabled.
Native text fields still handle their own editing keys.

## Layout and customization

The [layout model](../../crates/layer-ui/src/layout.rs) describes docked bands,
splits, tab groups, floating panels and collapsed columns. These are semantic
relationships: Rust knows where a panel belongs, while the frontend creates and
sizes the actual widgets.

The [customization model](../../crates/layer-ui/src/customization.rs) describes
panel contents, including tool and command tiles and configurable controls. Users
can rearrange panels and toolbars and change their contents. Shared code can
serialize and restore the workspace; automatic storage depends on the host.
The detailed [panel contract](panel-customization.md) covers drag targets, menus,
resize behavior and configuration.

The drawing interface uses [squircle corners](squircle-corners.md): tiles,
toolbars, panels, tabs and drawers share one radius scale on every host, while
settings, menus and popovers keep native styling.

The [canvas action bar](canvas-action-bar.md) shows the next steps for a
selection, transform or placed image beside it, as a glass panel that stays out
of the way of canvas contacts.

GTK [toolbar components](toolbar-components.md) provide inline brush size/opacity
sliders and a contextual Tool Options bar, with shared sizing, overflow and
workspace behavior.

GTK also has a [workspace window-bar builder](window-bar.md) for arranging
individual application controls and tools in left, center and right regions,
with workspace-owned size and canvas-info visibility. It does not dock toolbar
containers into native chrome.

The [drag and reorder convention](drag-and-reorder.md) specifies required pickup
behavior across devices: tiles require a hold for everyone; list bodies require
a hold for touch and pen; handles and title/tab bars drag without a hold. The
[source inventory](drag-inventory.md) tracks the implementation changes needed.

[`WorkspaceState`](../../crates/layer-ui/src/workspace.rs) is the durable layout
value. Transient menus, native widgets and unfinished gestures are not serialized.
GTK now uses `layer-workspace` for named workspaces, automatic saving, and durable
layout history. Workspaces include tool settings and the arrangement of tools and
panels. Reset All Brushes restores brush defaults within the current workspace;
there is no separate saved-layout library. Other hosts follow the
[approved workspace-manager design and handoff](workspace-manager-host-handoff.md).
Layout undo is separate from document undo. A drag is treated as one layout
change, and cancelling it restores the original arrangement.

## Zen mode and the camera

Zen mode controls which parts of the interface remain visible while drawing.
Shared logic manages visibility, reveal behavior and the state that keeps controls
available during a menu or interaction. Zen hides the header and docked chrome
and keeps floating panels; preferences choose whether Capy stays visible and
whether panels reveal near occupied screen edges.

Hiding controls does not resize the document viewport or move the camera. The
usable workspace area can inform an explicit Fit Canvas command, but ordinary
visibility changes should not move artwork under the pen.

## Adding UI behavior

Put a behavior in shared Rust when it determines command meaning, validation,
layout topology or document state. Keep widget construction, native focus,
accessibility and OS service calls in the frontend. `layer-host` shares transport
and session integration for Android, Apple and Windows; it does not replace their
native UI implementation.

The [shared UI reference](shared-ui.md) describes the detailed boundary. See
[settings](settings.md), [numeric controls](numeric-controls.md) and
[theme colors](theme-colors.md) for the existing shared policies before adding a
platform-specific version. [Panel transparency](panel-transparency.md) explains
how the shared presenter blurs the canvas behind translucent panels. Host coverage is listed in the
[platform guide](../platforms/README.md).
