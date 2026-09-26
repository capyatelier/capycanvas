# Command catalog and search

The command bar projects `UiSession::command_catalog()` and executes through
the existing `UiAction` dispatcher. It does not implement a second undo stack
or tool engine. The implementation follows the
[command and input investigation](../history/command-input-shortcut-audit-2026-09-25.md).

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
The explicit legacy shortcut ID mapping preserves existing v1 preferences.
Menu items that repeat an active-layer command, such as the Layer menu's Clear,
Delete, Rasterize Source and Repair Source Profile, share that command's ID.
The managed Restore Starting Layout item likewise shares the Reset Layout
command's ID, so each operation appears once. Layer property and
selection-coverage IDs omit the active layer, as other active-layer actions do.
The commands that the View menu leaves to the Proof panel are omitted in the
same way.

Every unavailable entry carries a specific reason. Commands use the same gates
as dispatch: document, snapshot, workspace and canvas idleness, selection and
mask targets, locks, transform state, proofing, HDR and zoom limits. Menu
actions report layer grouping and deletion errors, missing selections or masks,
filters while editing a mask, and locked layers. The generic "Unavailable in
the current tool or edit target" text remains only a fallback.

`ExecuteCommand` accepts only an ID found in a newly evaluated catalog. It
cannot execute arbitrary serialized internal events. Search invocation also
checks the originating document epoch. Live validation and normal history
remain in dispatch. Numeric entry uses the existing `NumericControl` schema,
including expression parsing, units, bounds and tool applicability.

The ten `ToolCategory` values describe behavior, independently of medium groups
and cycling families. `CommandToolContext` includes the current parameter IDs
and mask-editing target. Individual brush resources inherit their behavior
category; they do not create a separate shortcut scope.

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

On GTK, Web and Android the bar is panel glass, as described in
[panel transparency](panel-transparency.md): its body uses the panel glass fill
and publishes its bounds, so the presenter blurs the artwork behind it. The
search field stays opaque. Menus, popovers and tooltips remain opaque. The
Windows bar still uses the opaque panel color and composes its own footer from
the same fields; adopting the glass fill, `detail` and `CommandSearchStyle::top`
there is Windows follow-up work.

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

Windows uses a light-dismiss WinUI popup placed like GTK's, with a native
TextBox for text and IME, a search glyph and a close button. Result rows expose
UI Automation names, help text and selection. While search is closed, focus
changes under the window root report canvas, palette or text scope. Search
packets use their own latest-value slot beside workspace motion and camera,
so opening, typing and closing never rebuild the workspace.

Native transports distinguish search revisions from workspace revisions and
publish a small `command_search` packet (including explicit null on close).
Android observes it separately from the retained workspace. Keyboard actions
publish immediately instead of waiting for the continuous-input throttle.
`Back` resolves the current search/parameter phase in Rust; delayed query
events cannot discard the parameter step.

## Coverage and subsequent input work

| Source | Route |
| --- | --- |
| Supported command IDs | Live command flags and ordinary dispatch; retired tonal IDs and Proof-panel-owned proofing commands excluded |
| File/Edit/Layer/Select/Filter/View/Window/Help actions | Existing live menu providers, including active-layer, saved-selection and effect resources |
| Tools, brushes, brush sets and tool variants | Tool families, brush resources, brush sets, rulers, shapes, auto select/fill sources and gradients; current tool choices |
| Current tool numeric settings | Shared schema and a value-entry step |
| Active layer properties | Numeric properties as value entries; choices and toggles as entries; one history step each |
| Workspaces | Every managed workspace, beyond the Window menu's first five |
| Colors | Paint slots, swap and quick colors |
| Held pan | Cataloged as held; existing input lifecycle remains authoritative |
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

The general context resolver, tokenized held/continuous overrides, device
adapters and compatibility presets remain the later C–F stages in the
investigation. The command bar does not create Bluetooth support or reproduce
other editors' held-modifier behavior by itself. The bar is implemented on GTK,
Web, Android and Windows; the Apple presentation is separate follow-up work.

## Reproducible checks

```sh
cargo test --locked -p layer-ui -p layer-host
bash tools/performance/workspace-motion.sh gtk --native-test=native_command_bar_input --native-storage
bash apps/layer-web/build.sh
LAYER_TEST_ARTIFACTS=/tmp/command-search bash tools/performance/workspace-motion.sh web --command-bar
```

The GTK test uses an isolated compositor, real key delivery, light/dark popup
captures, numeric entry, repeated opening, disabled actions and outside-contact
dismissal. Native artifacts are written to the test runner's temporary directory.
`native_command_bar_glass` paints sharp stripes behind the bar and checks
compositor captures at Off, Low, Medium and High in both themes. Inside the bar
the stripes must be blurred and tinted by the glass fill, and Off must be
opaque. Where a shrinking bar or a closed bar used to be, they must be sharp:

```sh
LAYER_NATIVE_CAPTURE_DIR=/tmp/command-bar-glass \
  bash tools/performance/workspace-motion.sh gtk --native-test=native_command_bar_glass --native-storage
```
The Web test additionally checks native keyboard focus, ARIA selection, touch
activation at narrow width, retained workspace DOM and query-to-frame latency.

On Windows, `apps/layer-windows/scripts/exercise-command-search.ps1 -Executable
artifacts/windows/Release/CapyCanvas.exe` checks Primary+K, placement, parameter
entry and Back, keyboard selection, Escape, outside dismissal without painting,
touch activation, unavailable reasons and palette focus with real input.

For Android, build `:app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug`
with the SDK setup in [Android development](../development/android.md), install
both APKs onto the selected device, then run:

```sh
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w \
  -e class art.capycanvas.AndroidCommandSearchTest \
  art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

The instrumentation uses isolated workspace/recovery/color storage, real native
windows, injected keyboard/finger/stylus events, the device IME, menu activation,
parameter validation, retained panel identity and query-to-draw measurements.
Captures are saved under the app's external `validation/command-search` directory.
