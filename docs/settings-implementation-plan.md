# Capy Canvas preferences

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

Implemented after the Wayland canvas/input milestone. The rendering
path remains unchanged: preferences do not introduce any work on the live GPU
canvas path.

## Ownership and layout

`layer-ui/settings.rs` owns the versioned settings value, five page definitions,
field metadata, platform filtering, dependencies, validation, search and the
transactional editor. `shortcuts.rs` supplies the single keymap and shortcut
editor data; bindings execute the existing typed `UiAction`, not a parallel
command system. `UiSession` coordinates the engine and requests host services.

GTK's `preferences.rs` maps the view to native rows. Web's `preferences.js` maps
the same serialized view to DOM controls. Widgets only transport edits; neither
host decides defaults, valid values, shortcut collisions or setting availability.
This is a small settings-specific row model, not a general UI schema/framework.
It remains synchronous and Wasm-portable; OS objects and I/O live in the hosts.

| Page | Working controls |
| --- | --- |
| Appearance | System/Light/Dark; Zen reveal and keep-visible distances |
| Canvas | Five cursor modes; scroll pan/zoom speeds |
| Pen & Input | Pressure response; feedback enable, prediction horizon and tip lock; platform predictions where supplied |
| Keyboard Shortcuts | Commands, brushes, size presets and momentary pan; record, clear, reset, conflict replacement |
| About | Capy Canvas version, application license and platform renderer |

Feedback-dependent fields are disabled in the core when feedback is off.
GTK does not advertise predicted platform samples it does not provide.
Defaults preserve the previous drawing behavior: System theme, 80px edge reveal,
40px keep-visible margin, linear pressure, normal scroll speeds, 8ms prediction.

## Native presentation and research

GTK requires libadwaita 1.9. Use `AdwDialog` containing
`AdwNavigationSplitView`, with an `AdwViewSwitcherSidebar` controlling
`AdwViewStack`; pages use `AdwPreferencesPage`, groups and native rows. Below
620 logical dialog units, navigation collapses to a page/back flow. These
components supply native adaptive navigation and window controls.
[ViewSwitcherSidebar](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.ViewSwitcherSidebar.html),
[NavigationSplitView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.NavigationSplitView.html).

The stock `AdwPreferencesDialog` is useful for conventional preferences but
does not provide this explicit persistent sidebar layout; the general
`AdwSidebar` adds a separate item model that is unnecessary for a fixed ViewStack.
We use native controls throughout, not custom canvas-composited settings.
[PreferencesDialog](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.PreferencesDialog.html),
[Sidebar](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.9/class.Sidebar.html).

GTK's top-right primary menu has New Window, Preferences, Keyboard Shortcuts and
About Capy Canvas. Shortcuts/About deep-link to the same editor; calling
`OpenSettings { page }` from another element preserves the current draft.
This follows GNOME's placement of application-level entries in the main menu.
[GNOME menu guidance](https://developer.gnome.org/hig/patterns/controls/menus.html).

Web uses a gear opening Preferences directly, with the same sidebar/page flow,
native input behavior, original shared SVGs, copyable information values and
pen-oriented focus appearance. Desktop dialogs target 800 × 620 logical pixels.
Mobile native frontends can present the same view as a navigation page instead
of a desktop modal; there are no toolkit types in the shared model.

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
  shortcut; Reset All restores defaults in the draft. Clear disables a binding.
- Up to four alternatives can be supplied programmatically; the initial
  recording UI replaces the selected action's alternatives with one chord.
  Menu hints update from applied core state, not an uncommitted draft.
- The browser cannot own browser/OS-reserved combinations. The core rejects
  common reserved browser chords rather than claiming they will work.
- Pan stores the actually pressed key. Releasing it clears the momentary mode
  even after modifiers/focus change; an in-progress contact remains a pan.

GNOME Settings was consulted for the interaction pattern: recording, an explicit
collision decision, and preserving unaffected alternatives. Its GPL source was
not copied, vendored or translated into this MIT OR Apache-2.0 codebase.
[GNOME keyboard shortcut editor](https://gitlab.gnome.org/GNOME/gnome-control-center/-/blob/main/panels/keyboard/cc-keyboard-shortcut-editor.c).

## Transactions and persistence

`PreferenceAction::Edit` validates before modifying the draft. Invalid edits
leave it unchanged and produce view errors. Apply validates the complete value,
updates engine/input settings and emits a durable `SaveSettings` request.
Cancel/dismiss drops the draft. Navigation, search, recording and canceled edits
never write storage. The bulk `EditSettings` action is available to programmatic
callers but does not form a second host UI path.

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
failures appear in the status message. Explicit subsequent Apply can replace it.

Applied settings are relayed to other native windows (GTK's theme manager is
display-wide) and browser tabs through the validated restore action, without a
save echo. Native atomic writes are serialized on the I/O pool; older queued
snapshots are skipped so they cannot overwrite a newer save. Open drafts are
preserved. Workspace disk persistence, document file flows and AI settings
remain separate work, not nonfunctional controls in this dialog.

## Validation

- Core tests cover platform definitions, dependencies, atomic validation, search,
  deep links preserving drafts, JSON compatibility, requests/acknowledgements,
  custom actions, shortcut recording/conflicts/reset, editing guards, momentary
  pan and applied menu hints.
- `native_preferences_and_shortcuts` exercises real GTK controls/key-controller
  signals, all pages/themes, recording and replacement, adaptive presentation,
  multiple GPU windows, and isolated atomic save/restore into a new window.
- `node apps/layer-web/test.mjs --preferences` verifies DOM controls, all
  pages/themes, core filtering, search, dependencies, shortcut conflicts,
  compact recording prompt, Apply/Cancel, persisted reload and executable
  restored shortcuts.
- The existing GTK workspace, hardware browser and parity suites remain
  regression tests. Native-input pacing is rerun separately, without concurrent
  browser/GPU benchmarks.
- Review PNGs are in `artifacts/ui/preferences/`.
  They are actual GTK/browser captures, not mockups. The web-only platform
  prediction row and native-only window controls are intentional differences.

No simulation of physical tablet delivery or display scanout is implied by
these UI tests. Native event/presentation measurements remain documented in
`artifacts/benchmarks/gtk-wayland.md`.
