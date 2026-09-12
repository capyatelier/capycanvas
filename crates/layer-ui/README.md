# layer-ui

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-ui` implements editor behavior shared by the native and web clients. It
coordinates tools, commands, camera movement, workspace layout, preferences and
file operations around the drawing engine. Frontends supply the actual widgets,
focus handling, accessibility and operating-system services.

## Actions and views

`UiSession` owns the engine and editor state. A frontend sends a typed `UiAction`,
such as selecting a layer or moving a panel. The session applies the action and
identifies which sections of UI state changed.

Views describe controls and their current values; they are not rendered images.
A host can refresh the layer list or tool settings without replacing unrelated
widgets. Buttons, menus and shortcuts share command identities and availability
rules, so their behavior stays consistent across different workspace arrangements.

The workspace model describes both panel placement and configurable contents.
Layout changes have their own history, separate from artwork undo. Preferences,
workspace state and project data also have separate persistence models.

## Sliding workspace tabs

At pointer down, capture every tab's original `TabHit` (including its full natural
width) and the visible tab-strip bounds, in logical workspace coordinates. Once
the drag is recognized, send `DragWorkspace` with `ContactPhase::Down` and the
original press point, then call `UiSession::begin_tab_drag` with that geometry.

After each drag move, `tab_drag_preview(position)` returns the dragged tab's
clamped bounds and each neighbor's horizontal offset. Render these positions
without moving the original hit targets; animate neighbor offsets in the host.
Remove the visual overlay when the preview becomes `None`, or on release,
cancellation, capture loss or blur. Continue sending the raw pointer position to
`DragWorkspace` so clamping does not interfere with undocking.

The core precomputes switch points from the dragged tab's leading edge to each
neighbor's original midpoint. The same fixed points apply in both directions,
with no hysteresis. It also uses these points for the drop target and committed
order, so hosts must not implement their own tab-reordering thresholds.
On detachment it preserves the grab offset within the original tab, rather than
within the source group, keeping the grabbed point under the pointer.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) and [session.rs](src/session.rs) | Public state/action types and session coordination. |
| [tools.rs](src/tools.rs) and [tool_settings.rs](src/tool_settings.rs) | Tool groups and controls for the active tool. |
| [layout.rs](src/layout.rs), [customization.rs](src/customization.rs) and [workspace.rs](src/workspace.rs) | Docking, configurable panel contents and workspace history. |
| [interaction.rs](src/interaction.rs) and [camera.rs](src/camera.rs) | Routing pointer gestures and navigating the canvas. |
| [settings.rs](src/settings.rs), [shortcuts.rs](src/shortcuts.rs) and [numeric.rs](src/numeric.rs) | Preference definitions, key bindings and numeric editing rules. |
| [document_files.rs](src/document_files.rs) and [project_files.rs](src/project_files.rs) | File requests, save checkpoints and document adoption. |

Keep command meaning and validation here when adding a control. Native widget
construction and storage stay in the host. Start with the
[workspace guide](../../docs/ui/README.md) or
[settings guide](../../docs/ui/settings.md); the
[shared UI reference](../../docs/ui/shared-ui.md) describes the detailed boundary.
