# Command catalog and search

[Workspace and UI](README.md) · [Settings](settings.md)

The command bar projects the shared `CommandSearchView` and executes through
the existing `UiAction` dispatcher. It does not implement a second undo stack
or tool engine.

## Shared boundary

`command_catalog.rs` adapts live application menus, supported `CommandId`s,
tool families, brush resources and brush sets, and every ruler, shape, auto
select, fill and gradient variant. It also adapts the current tool's choices
(such as picker source and sample size) and numeric settings, the active
layer's properties, every managed workspace, the paint color slots and quick
colors. Layer properties become numeric, choice and toggle entries, such as
Layer opacity and Layer blend mode. The catalog also describes held pan; held
entries are binding targets and are excluded from executable search results.

Commands use existing snake-case wire identities. Nested action identities
include their typed arguments. Active-layer adapters omit transient layer IDs
and the next value of boolean toggles; invocation resolves a fresh menu model
against the current editing target. Resource and saved-selection choices retain
their resource IDs. Labels and Rust `Debug` formatting are not identities.
Shortcut IDs keep the v1 `command.<Variant>` spelling of existing preferences.
Menu items that repeat an active-layer command, such as the Layer menu's Clear,
Delete, Rasterize Source and Repair Source Profile, share that command's ID.
The managed Restore Starting Layout item likewise shares the Reset Layout
command's ID, so each operation appears once. Layer property and
selection-coverage IDs omit the active layer, as other active-layer actions do.
The commands that the View menu leaves to the Proof panel are omitted in the
same way.

Every unavailable entry carries a specific reason. Commands use the same gates
as dispatch: document, snapshot, workspace and canvas idleness, selection and
mask targets, locks, transform state, proofing, HDR and zoom limits. The same
text is published on each retained command as `disabled_reason` (see
[shared UI](shared-ui.md#actions-and-observation)). Menu actions report layer
grouping and deletion errors, missing selections or masks, filters while
editing a mask, locked layers, and Apply Mask on a group or effect layer, whose
masks stay live. The generic "Unavailable in the current tool or edit target"
text remains only a fallback.

Search execution accepts only an ID found in a newly evaluated catalog. It
cannot execute arbitrary serialized internal events. Search invocation also
checks the originating document epoch. Live validation and normal history
remain in dispatch. Numeric entry uses the existing `NumericControl` schema,
including expression parsing, units, bounds and tool applicability.

## Presentation

The opener is a configurable command in Edit and the toolbar/title-bar tool
catalog. Its defaults are Primary+Shift+P and Primary+K; the existing browser
reservation rules filter out Primary+Shift+P on Web.

The shared search model owns ranking, eight-result limits, five recent choices,
selection, disabled reasons and numeric entry. Hosts own text/IME, focus,
accessibility, popup capture and animation. A result contains its name, one
effective shortcut and optional checked state. `CommandSearchView::detail` is
the complete footer line; GTK, Web and Android show it verbatim. It holds the error or
unavailable reason when there is one. Otherwise it holds the selected command's
concise behavior or scope description, falling back to its menu location.
Numeric entries show their name, current value and hard input range, formatted
by the shared numeric schema. The footer never repeats basic keyboard
navigation.

`CommandSearchStyle` also carries the 12px corner radius and the placement
rule: the bar opens one-fifth down the visible workspace, clamped to 48–192px
(`CommandSearchStyle::top`), anchored independently of the result count. Web
and Android apply it to the height left visible by the on-screen keyboard;
their compact layouts (below 600px/dp wide) keep a 16px edge margin. Escape
returns from parameter entry to the query, then dismisses. Reopening starts
with an empty query.

On every host with command search the bar is panel glass, as described in
[panel transparency](panel-transparency.md): its body uses the panel glass fill
and publishes its bounds, so the presenter blurs the artwork behind it. The
search field stays opaque. Menus, popovers and tooltips remain opaque.

Hosts retain the meaningful editor focus before opening. Palette focus routes
Undo/Redo to color reorder history; the search entry taking focus cannot switch
that history domain. `Commit` carries current native text so a serial host can
submit immediately after typing without executing a stale result snapshot.
Text-field history remains with the native editor: search explains that context
instead of silently executing artwork Undo. History metadata can explicitly
delegate to dispatch for forms and operations whose outcome determines history.

Search builds an index when opened and performs no I/O or thumbnail work while
typing. `COMMAND_SEARCH` changes leave workspace model/content revisions alone.
GTK updates only the popup for these changes; opening and closing use native
popover behavior and motion preferences. The popup is its own Wayland surface,
so the workspace publishes its body as an extra glass region from the popup's
position whenever the popup's layout changes, and removes it when the popup
unmaps. Web uses a search-only Wasm publication
and retains workspace DOM controls. Its native modal dialog supports IME,
combobox/listbox accessibility, reduced motion and visual-viewport sizing.
Outside-dismissal contacts must not reach the canvas. Closing restores the
origin focus, falling back to the canvas if the original element cannot take focus.

Android uses a Compose dialog with native text/IME, 48dp targets and the same
palette, typography, spacing and immersive-window policy as the editor. The
native window stays fixed while Compose sizes the visible card, avoiding a
WindowManager resize when the result count changes. IME insets constrain the
scrolling results while the entry and selected-result explanation stay visible.
The entrance uses the native animation duration scale.

Windows uses a light-dismiss WinUI popup placed by `CommandSearchStyle`, with a
native TextBox for text and IME, a search glyph and a close button. Its footer
shows the shared `detail`, and its body joins the workspace glass regions while
open. Result rows expose
UI Automation names, help text and selection. While search is closed, focus
changes under the window root report canvas, palette or text scope. Search
packets use their own latest-value slot beside workspace motion and camera,
so opening, typing and closing never rebuild the workspace.

macOS and iPadOS present a SwiftUI card above the editor with the shared
width, inset, gap, radius and row height; iPadOS rows are at least 48pt. It
opens at `CommandSearchStyle::top` of the height the software keyboard leaves
visible, shows the shared `detail` footer, uses the panel glass fill with an
opaque search field, and enters over 120ms unless Reduce Motion is on. A
transparent layer beneath it takes outside contacts, so dismissal never reaches
the canvas. Escape, arrows and Return are handled on the focused native field
(on iPadOS a UIKit field with priority key commands, because the system
consumes Escape first); entering the parameter step selects its value. Edit → Search Commands… and Command-key canvas chords
report the origin focus (text, palette or canvas) before opening, and closing
returns keyboard focus to the canvas unless search began from a text field.

Native transports distinguish search revisions from workspace revisions and
publish a small `command_search` packet (including explicit null on close).
Android and Apple observe it separately from the retained workspace. Keyboard actions
publish immediately instead of waiting for the continuous-input throttle.
`Back` resolves the current search/parameter phase in Rust; delayed query
events cannot discard the parameter step. Query, move and selection events
that arrive after the search closes (for example a native field reporting its
final text as it resigns) are ignored rather than reported as errors.

## Coverage

| Source | Route |
| --- | --- |
| Supported command IDs | Live command flags and ordinary dispatch; retired tonal IDs and Proof-panel-owned proofing commands excluded |
| File/Edit/Layer/Select/Filter/View/Window/Help actions | Existing live menu providers, including active-layer, saved-selection and effect resources |
| Tools, brushes, brush sets and tool variants | Tool families, brush resources, brush sets, rulers, shapes, auto select/fill sources and gradients; current tool choices |
| Current tool numeric settings | Shared schema and a value-entry step |
| Active layer properties | Numeric properties as value entries; choices and toggles as entries; one history step each |
| Workspaces | Every managed workspace, beyond the Window menu's first five |
| Colors | Paint slots, swap and quick colors |
| Held pan and held tools | Cataloged as held and excluded from results; the shared hold lifecycle owns press, release and blur |
| Relative tool-setting steps | Cataloged with their bindings; disabled with a reason when the tool lacks the setting |
| Focused palette Undo/Redo | Shared palette history, with origin focus retained through search |
| Native text editing and drawing-tab focus actions | Existing native focus owners; general focus-context bindings require the resolver stage |
| Complex forms, file pickers and confirmations | Existing native dialogs and host requests |
| Rows, tiles and palettes other than the active target | Their context menus; they need an explicit target, not the active one |
| Panel-local controls | Slider bookmarks, per-setting reset, color panel presentation, color/curve/gradient editors; each owns its gesture |
| Toolbar and title-bar editing | Customization surfaces and their context menus |
| Proof panel controls | Host-routed proof actions, as the View menu defers to them |
| Next/previous drawing, drawing tab order, close window | Host-owned multi-document and window lifecycle; Drawings and Close Drawing are cataloged commands |
| Brush size presets | Covered by the Brush size value entry; `size.N` bindings remain shortcuts |
| Custom shortcut actions | Shortcut-only; search shows their binding on the matching catalog entry, never arbitrary serialized actions |
| Measurements, drag/drop phases, restore/completion messages, raw pen samples | Private transport; never independently cataloged as commands |

Scoped bindings and held tool overrides are described in
[settings](settings.md#keyboard-shortcuts), and finger taps and pen side buttons in
[settings](settings.md#touch-gestures-and-pen-buttons). Remotes and gamepads are in
[settings](settings.md#remotes-and-gamepads), and keymap presets and files in
[settings](settings.md#keymap-presets-import-and-export).

## Checks

```sh
cargo test --locked -p layer-ui -p layer-host
bash tools/performance/workspace-motion.sh gtk --native-test=native_command_bar_input --native-storage
LAYER_NATIVE_CAPTURE_DIR=/tmp/command-bar-glass \
  bash tools/performance/workspace-motion.sh gtk --native-test=native_command_bar_glass --native-storage
LAYER_TEST_ARTIFACTS=/tmp/command-search bash tools/performance/workspace-motion.sh web --command-bar
pwsh -NoProfile -File apps/layer-windows/scripts/exercise-command-search.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe
```

`native_command_bar_glass` paints sharp stripes behind the bar at every
transparency level in both themes: inside the bar they must be blurred and
tinted, Off must be opaque, and where a shrinking or closed bar used to be they
must be sharp again. Android runs `art.capycanvas.AndroidCommandSearchTest`
(see [Android development](../development/android.md)); macOS and iPad run
`EditorLaunchTests/testCommandSearch`.
