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
| Pen & Input | Pressure response; feedback enable, prediction horizon up to 64 ms and tip lock; platform predictions where supplied |
| Keyboard Shortcuts | Search commands, brushes, size presets and momentary pan; open details to add/remove/reset alternatives and resolve conflicts |
| About | Capy Canvas version, application license, platform renderer, Website and Source Code links |

Feedback-dependent fields are disabled in the core when feedback is off.
GTK does not advertise predicted platform samples it does not provide.
Defaults preserve the previous drawing behavior: System theme, 80px edge reveal,
40px keep-visible margin, linear pressure, normal scroll speeds, 8ms prediction.
All UI text uses the shared Rust `UI_TEXT_PT` constant (11 pt), including panels,
tabs, menus, preference descriptions and zoom/rotation status text. There is no
font-size setting. Text controls and inline step symbols use font-relative sizes;
tool icons, brush previews, sliders and checkboxes retain their dimensions.
The core ignores the retired `panel_text_pt` field when loading saved settings,
without dropping other preferences or relaxing validation of unknown fields.

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

The sidebar extends to the bottom of the dialog; Apply/Cancel belong to the
content pane. A stock `edit-find-symbolic` search toggle at the sidebar's
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
`OpenSettings { page }` from another element preserves the current draft.
This follows GNOME's placement of application-level entries in the main menu.
[GNOME menu guidance](https://developer.gnome.org/hig/patterns/controls/menus.html).
All native header menus use `GMenu`/`GtkPopoverMenu`, with enabled/check states
and accelerator hints from the shared command model. View includes Zen Mode.
Header text buttons retain libadwaita's 17px horizontal padding.

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
  shortcut; Reset All restores defaults in the draft. Removing the last
  alternative disables the action's keyboard binding.
- Each row opens an editor showing current alternatives and original defaults.
  Add records another chord; Remove and Reset operate in this dialog, not the
  table. Up to four alternatives are supported in both hosts. Duplicates and
  limits are validated in Rust. Menu hints update from applied core state,
  not an uncommitted draft; GTK's native hint shows the first alternative.
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
  Use `--package --preferences` to run it against the production bundle.
- The existing GTK workspace, hardware browser and parity suites remain
  regression tests. Native-input pacing is rerun separately, without concurrent
  browser/GPU benchmarks.
- Review PNGs are in `artifacts/ui/preferences/`.
  They are actual GTK/browser captures, not mockups. The web-only platform
  prediction row and native-only window controls are intentional differences.

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

Browser validation on this machine also has an environment caveat: Chrome's
Wayland DMA-BUF import currently fails inside ANGLE. The optional `--headless`
test mode uses hardware Vulkan/WebGPU without that window-system path, following
[Chromium's GPU testing guidance](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/gpu/using-gpu-hardware-in-headless-chrome.md).
All preferences interaction, typography, persistence and cursor-preview assertions
pass there, but the suite's strict empty-log check reports a rendering warning
during GPU startup (`A valid external Instance reference no longer exists`).
It is not filtered out. Headless captures validate the DOM controls only: their
canvas is black, so they do not validate composed GPU ink or display delivery.
