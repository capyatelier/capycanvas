# Command catalog and search

The command bar projects `UiSession::command_catalog()` and executes through
the existing `UiAction` dispatcher. It does not implement a second undo stack
or tool engine. The implementation follows the
[command and input investigation](../history/command-input-shortcut-audit-2026-09-25.md).

## Shared boundary

`command_catalog.rs` adapts live application menus, supported `CommandId`s,
brush resources, tool families, tool choices, current tool parameter schemas,
and basic color operations. The catalog also describes held pan; held entries
are binding targets and are excluded from executable search results.

Commands use existing snake-case wire identities. Nested action identities
include their typed arguments. Active-layer adapters omit transient layer IDs
and the next value of boolean toggles; invocation resolves a fresh menu model
against the current editing target. Resource and saved-selection choices retain
their resource IDs. Labels and Rust `Debug` formatting are not identities.
The explicit legacy shortcut ID mapping preserves existing v1 preferences.

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
effective shortcut and optional checked state. The footer shows the selected
result's unavailability explanation or contextual guidance. GTK shows the shared
catalog's concise behavior/scope description, falling back to the menu location
when one exists. Tool settings show their name, current value and hard input
range, formatted by the shared numeric schema. The single footer line prioritizes
errors and unavailable reasons; it does not repeat basic keyboard navigation.
The GTK popup opens one-fifth down the workspace (48–192px),
anchored independently of result count. Escape returns from parameter entry
to the query, then dismisses. Reopening starts with an empty query.

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
popover behavior and motion preferences. Web uses a search-only Wasm publication
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

| Surface | Route |
| --- | --- |
| Supported command IDs | Live command flags and ordinary dispatch; retired tonal IDs excluded |
| File/Edit/Layer/Select/Filter/View/Window/Help actions | Existing live menu providers, including active-layer and resource adapters |
| Brushes and tool-family cycling | Resource choices and ordinary tool dispatch |
| Current tool numeric settings | Shared schema and a value-entry step |
| Held pan | Cataloged as held; existing input lifecycle remains authoritative |
| Focused palette Undo/Redo | Shared palette history, with origin focus retained through search |
| Native text editing and drawing-tab focus actions | Existing native focus owners; general focus-context bindings require the resolver stage |
| Complex forms, file pickers and confirmations | Existing native dialogs and host requests |
| Measurements, restore/completion messages, raw pen samples | Private transport; never independently cataloged as commands |

The general context resolver, tokenized held/continuous overrides, device
adapters and compatibility presets remain the later C–F stages in the
investigation. The command bar does not create Bluetooth support or reproduce
other editors' held-modifier behavior by itself.

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
