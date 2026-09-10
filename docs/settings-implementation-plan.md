# Capy Canvas preferences

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

Implemented after the Wayland canvas/input milestone. The rendering
path remains unchanged: preferences do not introduce any work on the live GPU
canvas path.

## Ownership and layout

`layer-ui/settings.rs` owns the versioned settings value, five page definitions,
field metadata, platform filtering, dependencies, validation, search and the
atomic per-edit updates. `shortcuts.rs` supplies the single keymap and shortcut
editor data; bindings execute the existing typed `UiAction`, not a parallel
command system. `UiSession` coordinates the engine and requests host services.

GTK's `preferences.rs` maps the view to native rows. Web's `preferences.js` maps
the same serialized view to DOM controls. Widgets only transport edits; neither
host decides defaults, valid values, shortcut collisions or setting availability.
This is a small settings-specific row model, not a general UI schema/framework.
It remains synchronous and Wasm-portable; OS objects and I/O live in the hosts.

## Copy guidelines

- Titles are short noun phrases, usually 2–4 words.
- Descriptions are one natural sentence about the visible behavior, usually
  8–12 words. Use fewer words when the meaning is already clear.
- Each title, description, option and authored help message is at most
  **54 characters**, including spaces and punctuation (Unicode characters,
  not bytes). Do not shorten copy by cutting it off at runtime.
- Use familiar terms: pointer, controls, window edge, pen, stroke and canvas.
  Prefer concrete verbs such as show, hide, move and draw. Avoid internal terms
  such as modeled geometry, predicted samples, chrome and occupied edges.
- Stay grammatical; avoid cryptic fragments and redundant explanations.
  Units belong beside values, not repeated in numeric titles.
- Omit a description when the title and value already explain the row.
  Preserve essential licensing qualifications. URLs, license identifiers,
  user-supplied names and external diagnostic details are data, not copy to trim.
- Define setting copy in Rust. Shared shortcut notices also come from the
  core; frontends only display them. Core tests cover every platform's catalog
  and reject titles/descriptions/options exceeding the limit.

Settings slider labels align with other settings rows, without the extra
6px inset used by panel sliders. GTK uses its existing page-level width clamp,
not a second centered clamp inside each numeric row.

Text fields carry their value, constraint, maximum length and placeholder in
the same row model. The two base-color fields use the hex-color constraint;
validation and palette generation are shared Rust behavior. See the
[theme color inventory and transformation](theme-colors.md).

| Page | Working controls |
| --- | --- |
| Appearance | System/Light/Dark; dark/light base hex colors; separate Zen mode section: Total zen, Button icon (four centered image tiles) |
| Canvas | Five cursor modes; scroll pan/zoom speeds |
| Pen & Input | Pressure response; live stroke preview, prediction time up to 64 ms and pen tip tracking; device pen prediction where supplied |
| Keyboard Shortcuts | Search commands, brushes, size presets and momentary pan; open details to add/remove/reset alternatives and resolve conflicts |
| About | Capy Canvas version, application license, platform renderer, Website and Source code links |

Feedback-dependent fields are disabled in the core when feedback is off.
GTK does not advertise predicted platform samples it does not provide.
Defaults: System theme, Partial Zen (Total zen off),
fixed 80px reveal/keep-visible distances, linear pressure, normal scroll speeds,
8ms prediction. Partial Zen keeps the Zen button visible and disables edge
reveal. GTK also projects split edge-toolbars; web/Android section views await
GTK review. Total Zen hides the button and enables edge reveal. Tab exits either.
Both Zen settings are persisted, validated and resettable through the Rust row model.
The Zen button exposes Total zen on right-click or touch-hold, with a
divider before the Change icon… settings link. Rust generates the menu from the rows, using
the same validated, persisted edit action as the settings page. Individual
preference edits/resets can execute without a settings dialog; navigation and
shortcut recording still require it to be open.
UI text uses the shared Rust `UI_TEXT_PT` base size (11 pt), including panels,
tabs, menus, preference titles and zoom/rotation status text. GTK settings row
descriptions follow [libadwaita's smaller subtitle style](https://github.com/GNOME/libadwaita/blob/main/src/stylesheet/widgets/_lists.scss):
measured at 5/6 of the title size (about 9.17 pt). Web uses the same hierarchy;
Android keeps its native settings typography described below. Self-explanatory
rows omit descriptions entirely, without reserving a blank line. There is no
font-size setting. Text controls and inline step symbols use font-relative sizes;
tool icons, brush previews, sliders and checkboxes retain their dimensions.
The core ignores the retired `panel_text_pt`, `zen_hide` and `zen_reveal` fields when loading saved settings,
without dropping other preferences or relaxing validation of unknown fields.
Retired `zen_behavior`, `zen_reveal_mode` and `zen_show_button` fields are ignored;
the replacement Total zen preference starts at its default. Retired Show panels
toolbar actions migrate to Zen, obsolete shortcut overrides are discarded, and
the old global `panels_visible` field is ignored. Individual panel placement is
preserved; there is no separate global hide state or command.

### Restoring defaults

Every editable preference exposes a context menu with **Reset to Default** on
the left and its formatted default in dim text on the right. It is disabled
when already at the default or when the setting itself is unavailable. There
is no permanent modified badge. Right-click, touch long press on the row and
keyboard context-menu activation open it. Active mobile text editors retain
their native selection menu; the setting's label provides the reset menu.

`Settings::default()` remains the sole source of defaults. Rust projects
`PreferenceRow.reset` (label, formatted value, enabled) and handles
`PreferenceAction::Reset`; frontends do not compare values or invent defaults.
Numbers carry an optional default in their shared `NumericControl`, so an empty
committed expression resets using the same resolver on every host. Hex fields
also reset on an empty commit. Clearing a draft does not immediately change the
setting; invalid nonempty text remains invalid. Ordinary panel number fields
have no implicit default and retain their existing empty-input validation.
Resets use the normal immediate-apply, validation and persistence path and do
not alter unrelated preferences, artwork or shortcut customizations.

## Native presentation and research

GTK requires libadwaita 1.9. Use `AdwDialog` containing
`AdwNavigationSplitView`, with an `AdwViewSwitcherSidebar` controlling
`AdwViewStack`; pages use `AdwPreferencesPage`, groups and native rows. Below
620 logical dialog units, navigation collapses to a page/back flow. These
components supply native adaptive navigation and window controls.
[ViewSwitcherSidebar](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.ViewSwitcherSidebar.html),
[NavigationSplitView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.NavigationSplitView.html).
Dialog presentation follows open/closed transitions from the model, not GTK
root membership: a closing sheet stays rooted during its animation and must
still be presentable if reopened immediately.

The sidebar extends to the bottom of the dialog; Done belongs to the
content pane and only dismisses the view. A stock `edit-find-symbolic` search toggle at the sidebar's
top-left reveals its search entry and result list. Search results and their
destination actions come from Rust. Keyboard Shortcuts has a separate search
entry. Touch suppresses stale pointer-hover styling without removing selected
or pressed feedback. This follows the structure of
[GNOME Settings](https://github.com/GNOME/gnome-control-center/blob/main/shell/cc-window.blp).

The stock `AdwPreferencesDialog` is useful for conventional preferences but
does not provide this explicit persistent sidebar layout; the general
`AdwSidebar` adds a separate item model that is unnecessary for a fixed ViewStack.
We use native controls throughout, not custom canvas-composited settings.

About follows libadwaita's convention of an application website and additional
project links, within our existing About settings page. Labels, destinations
and link-row metadata are defined once in `layer-ui/settings.rs`. GTK renders
native `GtkLinkButton` controls in activatable rows, using the system URI handler;
web renders ordinary links in a new tab with `noopener noreferrer`, preserving
the open drawing. These read-only rows are searchable, not editable settings.
[AdwAboutDialog links](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.AboutDialog.html#details),
[GtkLinkButton](https://docs.gtk.org/gtk4/class.LinkButton.html).
[PreferencesDialog](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.PreferencesDialog.html),
[Sidebar](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.Sidebar.html).

GTK's top-right primary menu has New Window, Preferences, Keyboard Shortcuts and
About Capy Canvas. Shortcuts/About deep-link to the same editor; calling
`OpenSettings { page }` from another element preserves accepted settings.
This follows GNOME's placement of application-level entries in the main menu.
[GNOME menu guidance](https://developer.gnome.org/hig/patterns/controls/menus.html).
All native header menus use `GMenu`/`GtkPopoverMenu`, with enabled/check states
and accelerator hints from the shared command model. Rust defines menu sections:
New Window is separate from Preferences/Keyboard Shortcuts/About; View separates
Fit Canvas, the appearance/visibility toggles (including Zen Mode), and Reset Layout.
Dark Mode is checked when the effective theme is dark, including in Auto mode;
toggling it sets an explicit light/dark override. Its label and state live in Rust.
Undo/Redo remain one group. GTK renders native section separators; web renders
horizontal rules at the same boundaries. GNOME Settings keeps its support entries
together, and Text Editor separates this final group from document/window actions:
[Settings menu](https://github.com/GNOME/gnome-control-center/blob/main/shell/cc-window.blp),
[Text Editor menu](https://github.com/GNOME/gnome-text-editor/blob/main/src/editor-window.ui).
Header text buttons retain libadwaita's 17px horizontal padding.

Web uses a gear opening Preferences directly, with the same sidebar/page flow,
native input behavior, original shared SVGs, copyable information values and
pen-oriented focus appearance. Desktop dialogs target 1000 × 744 logical pixels,
constrained to the available window. The web sidebar title is centered in the
whole sidebar, independently of the search button.
There are no toolkit types in the shared model.

On every platform, printable keyboard input outside an editable control opens
and focuses preferences search, preserving the first character and its case.
Rust owns this routing and query accumulation; a transient `search_focus`
revision asks native/DOM hosts to reveal the sidebar and place the caret at the
end, including in narrow layouts. Editable controls, IME composition, modifier
commands, shortcut editing/recording, and Space/Enter activation retain their
normal input behavior. Android applies focus-request text even when several keys
arrive before its asynchronous UI update; native field selection changes do not
emit settings edits.

### Android

Settings occupies the full available window, slides down from the top on entry
and slides up on dismissal. There is no full-width header above the panes. At
840 dp and wider a full-height 260 dp sidebar sits beside the main content.
The sidebar starts with a persistently visible search field, without a Settings
heading or a search-toggle button. An empty query shows categories; Rust supplies
the filtered results and navigation actions. The main pane has its own
centered page title and a filled Material Done button at the top right; Back
appears at its top left for details. Done closes the auto-saving overlay rather
than committing a form. Narrower windows use list/page navigation with Done in
the currently visible pane. Content is bounded to 632 dp for readable rows.

Android settings use native-scale 16 sp body/sidebar text, 14 sp descriptions,
18 sp group headings and 20 sp pane titles. Done has a 16 sp label in a 40 dp
visible button with a 48 dp touch target. The compact editor retains its shared
11 pt typography. Navigation glyphs are 20 dp in 48 dp rows, aligned with the
search glyph. Sidebar insets and button radii are 8 dp, row gaps 4 dp, and groups
use 24 dp spacing. Setting names appear above descriptions on the left; their
switch, number editor, choice value, information or link appears on the right.
Rows have a 72 dp minimum and grow for wrapped descriptions. The existing editor slider skin
is reused with a 48 dp settings touch height, a contrasting inactive track and
a release callback, without changing compact editor sliders. Controls retain
at least 48 dp touch targets.

Numeric rows use the [shared touch-first control contract](numeric-controls.md):
small integers have a trailing spin control; continuous/wide ranges have a plain
editable value above a slider with minus/plus buttons. The slider sits below
all labels and spans the row up to a 600 dp cap, not beside the description.
Rust owns IDs, text, groups, visibility, enabled state, defaults, choice options,
ranges, mapping curves, steps, resolution, expression parsing and persistence.
Native positions and text resolve through `NumericControl`, then the accepted
number submits through `Edit`. Adding a setting of an existing kind requires only core
changes, not Android row-specific wiring. A genuinely new control kind still
requires a renderer in each frontend.

Simple choices use native anchored dropdowns, including cursor previews and a
checkmark for the current value. Selection applies through Rust's `Edit` action
and dismisses the menu without navigating; Back or an outside tap dismisses it
without changing the value. Reset context menus also remain native menus.
Numeric editors stay in their rows. Editors that use a modal on GTK/web, such as
shortcuts (`shortcut_editor`), slide into the content pane from the right with a
Back arrow at its top left. There are no nested settings dialogs; recording,
conflicts and validation errors appear inline. Numeric text is
validated by Rust on IME Done or focus loss; sliders update live. A full-size
input barrier protects the still-mounted GPU canvas throughout entry and exit.

This follows Android's settings organization and adaptive list/detail patterns;
the full-screen Done presentation is a product choice for the editor, inspired by
the supplied Procreate reference, not an Android requirement.
[Android settings](https://developer.android.com/design/ui/mobile/guides/patterns/settings),
[canonical layouts](https://developer.android.com/develop/adaptive-apps/guides/canonical-layouts),
[native buttons](https://developer.android.com/develop/ui/compose/components/button),
[touch targets](https://developer.android.com/develop/ui/compose/accessibility/api-defaults).

## Shortcut contract

- Built-in commands, each brush, each size preset and momentary pan are
  individually assignable. Defaults include B/E/F/Z, Space-pan, Ctrl+Z,
  Ctrl+Shift+Z/Ctrl+Y, Ctrl+, and Ctrl+Shift+?.
- Register any parameterized semantic action with
  `PreferenceAction::RegisterAction { definition }`. Its stable `custom.*`
  ID, label, repeat policy and typed `UiAction` are serialized with settings,
  then appear in the same editor. No host switch statement is needed for
  another action. Raw pen samples and continuous contact records are not
  command bindings.
- The editor consumes normalized keys before native editing guards only while
  recording. Modifier-only events are ignored; Escape cancels recording; Tab
  stays reserved. Outside recording, native text editing and modal navigation
  take precedence. Application shortcuts do not inhibit desktop-global keys.
- Conflicts require explicit replacement. Replacing one alternative preserves
  the other action's remaining alternatives. Reset cannot silently steal a
  shortcut; Reset All restores defaults immediately. Removing the last
  alternative disables the action's keyboard binding.
- Each row opens an editor showing current alternatives and original defaults.
  Add records another chord; Remove and Reset operate in this editor, not the
  table. Up to four alternatives are supported in all hosts. Duplicates and
  limits are validated in Rust. Menu hints update from applied core state,
  immediately after each accepted edit; GTK's native hint shows the first alternative.
- Shortcut search includes current binding labels, in both the shortcut list
  and global preferences search: `z` also finds `Ctrl+Z` and `Ctrl+Shift+Z`.
  Rust marks rows whose binding set differs from its defaults (alternative order
  is irrelevant). GTK, web and Android bold the binding text, including Disabled,
  following [GNOME Settings' shortcut rows](https://github.com/GNOME/gnome-control-center/blob/main/panels/keyboard/cc-keyboard-shortcut-row.c).
  Reset remains in the editor, not a separate button in each list row.
- The browser cannot own browser/OS-reserved combinations. The core rejects
  common reserved browser chords rather than claiming they will work.
- Pan stores the actually pressed key. Releasing it clears the momentary mode
  even after modifiers/focus change; an in-progress contact remains a pan.

GNOME Settings was consulted for the interaction pattern: recording, an explicit
collision decision, and preserving unaffected alternatives. Its GPL source was
not copied, vendored or translated into this MIT OR Apache-2.0 codebase.
[GNOME keyboard shortcut editor](https://gitlab.gnome.org/GNOME/gnome-control-center/-/blob/main/panels/keyboard/cc-keyboard-shortcut-editor.c).

## Validation and persistence

`PreferenceAction::Edit` validates a candidate before replacing active settings.
Each accepted changed value updates engine/input settings and emits a durable
`SaveSettings` request. Invalid values leave settings unchanged and produce view
errors. `CloseSettings` only dismisses the view. Navigation, search, recording,
canceled recording and no-op edits never write storage. The bulk `EditSettings`
action uses the same validation/apply path for programmatic callers. There is no
separate whole-dialog draft or Apply/Cancel workflow on any platform.

GTK writes atomic replacements on GIO's background I/O pool to
`$XDG_CONFIG_HOME/layer/settings.json` (normally `~/.config/layer/settings.json`).
Web writes small applied snapshots to localStorage key `layer.preferences.v1`.
The internal `layer` names are retained; the user-facing name is Capy Canvas.
`LAYER_SETTINGS_FILE` selects an isolated native test file. Native tests otherwise
do not read or write the user's preferences.

A request remains queued until `CompleteRequest` returns success/error.
`RestoreSettings` validates the version/data without echoing another save.
Missing fields in older settings receive defaults; unsupported versions, invalid
values and unknown fields are rejected. Invalid saved data is not silently
overwritten at startup. Native read failures are reported to stderr; browser
failures appear in the status message. An explicit accepted settings edit can replace it.

Applied settings are relayed to other native windows (GTK's theme manager is
display-wide) and browser tabs through the validated restore action, without a
save echo. Native atomic writes are serialized on the I/O pool; older queued
snapshots are skipped so they cannot overwrite a newer save. Open settings views
reflect restored values without losing their navigation state. Workspace disk persistence, document file flows and AI settings
remain separate work, not nonfunctional controls in this dialog.

## Validation

- Core tests cover platform definitions, dependencies, atomic validation, search,
  deep links preserving settings, JSON compatibility, requests/acknowledgements,
  custom actions, shortcut recording/conflicts/reset, editing guards, momentary
  pan and applied menu hints.
- `native_preferences_and_shortcuts` exercises real GTK controls/key-controller
  signals, all pages/themes, recording and replacement, adaptive presentation,
  multiple GPU windows, and isolated atomic save/restore into a new window.
- `native_settings_typography` first measures an unstyled Adwaita row, then
  checks the application's title/subtitle fonts and captures settings geometry
  in both themes. Run `node apps/layer-web/test.mjs --package --settings-audit`
  afterward to compare every shared row's bounds and label positions/fonts,
  including shortcuts. References and review PNGs: `artifacts/ui/settings-audit/`.
  Web follows the native page's adaptive width clamp, 24px group spacing,
  row separators, and slider/image-row padding; omitted subtitles take no space.
- `node apps/layer-web/test.mjs --preferences` verifies DOM controls, all
  pages/themes, core filtering, search, dependencies, shortcut conflicts,
  compact recording prompt, per-edit persistence, dismissal, persisted reload and executable
  restored shortcuts.
  Use `--package --preferences` to run it against the production bundle.
- The existing GTK workspace, hardware browser and parity suites remain
  regression tests. Native-input pacing is rerun separately, without concurrent
  browser/GPU benchmarks.
- Android device tests cover full-screen geometry, real entry/exit and detail
  movement without nested dialogs, anchored choice menus in both themes,
  preview icons, current selection, Back/outside-tap dismissal, numeric rejection/acceptance,
  immediate persistence, multiple shortcuts, narrow-screen navigation and stylus
  isolation from the canvas, persistent search, and catalog-driven row/control
  geometry and numeric ranges. Review captures: `artifacts/android/settings-inline/final/`.
- Review PNGs are in `artifacts/ui/preferences/`.

The subtitle/copy audit passes 124 shared UI tests, Clippy, both native GTK
settings suites, and the packaged web settings geometry and interaction suites.
All five pages were compared in light and dark mode. GTK, web and both Android
ABIs build successfully; Android retains its typography and only omits the
newly empty image-selector description. The ten redundant descriptions and
base-color title changes are defined once in Rust.
  They are actual GTK/browser captures, not mockups. The web-only platform
  prediction row and native-only window controls are intentional differences.

The wider-dialog/type-to-search update passes 75 core tests, the native GTK
preferences suite in an isolated headless Wayland session, 18 Android device
tests (run `1788915228675`), Android lint, and 18 web packaging/launcher tests.
Packaged web preferences assertions pass, including first-character/caret
preservation, editable-field protection and collapsed-sidebar search. Its final
empty-console check still reports the existing GPU startup warning described
below. Android search review: `artifacts/android/settings-typing/38-type-to-search.png`.

Validation caveat (2026-09-07): an intermittent native extra-window teardown
crash was traced to a `GDK_PAD_GROUP_MODE` event with a NULL surface, before any
application event controller runs. GTK 4.22.4's Wayland tablet-pad mode handler
constructs this event from `seat->keyboard_focus` without checking for NULL;
`gdk_surface_handle_event` then dereferences it. The same handler remains on
upstream main. See [GTK's handler](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdkseat-wayland.c#L3562).
This is not a preferences file-I/O failure. An isolated control run with
`GDK_WAYLAND_DISABLE=zwp_tablet_manager_v2` passes the complete settings,
atomic save/restore and teardown suite with fatal GTK warnings enabled.
That switch is **test-only**: it disables stylus support too, so the application
does not set it. Normal tablet-enabled teardown remains affected and needs a
GTK dependency fix. The app separately releases its GPU worker before window
fields during final ownership teardown. Divider and repeated-window tests also
run with normal tablet support; they do not prove the intermittent GTK bug fixed.

No simulation of physical tablet delivery or display scanout is implied by
these UI tests. Native event/presentation measurements remain documented in
`artifacts/benchmarks/gtk-wayland.md`.

The cross-platform Zen rollout passes the packaged preferences and PWA suites
in Chrome on an isolated Mutter Wayland display, including real touch toggles,
context menus, all four icon choices, live icons, GPU ink and offline upgrades.
GTK's two native Zen regression tests and six Android Zen/settings emulator
tests pass. Both themes were visually reviewed; Android retains its full-screen
settings layout. Builds include the static PWA and ARM64/x86_64 Android APK.

The optional headless Chrome mode still reports
`A valid external Instance reference no longer exists` on this machine despite
passing DOM assertions. That warning is not filtered out, and its black-canvas
captures are not used as evidence of GPU rendering or display delivery.
