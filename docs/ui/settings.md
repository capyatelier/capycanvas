# Settings and persistence

[Technical documentation](../README.md) · [Workspace and UI](README.md)

Preferences describe how the app behaves; workspace state describes the panel
arrangement; a project describes the drawing. They have separate storage models
so changing one does not silently modify another.

## Shared preference definitions

[`settings.rs`](../../crates/layer-ui/src/settings.rs) defines the versioned
`Settings` value, defaults, preference pages, field types, validation and
platform-specific availability. The frontend renders these definitions rather
than maintaining its own copy of allowed values or defaults.

The pages cover appearance, canvas navigation and cursors, layers, pen input,
keyboard shortcuts and application information. A preference row can describe a choice,
number, toggle or text field, together with its current value, enabled state and
reset action. Theme colors use the shared
[palette transformation](theme-colors.md); numeric controls use the same
[parsing and stepping policy](numeric-controls.md) as tool panels.

An accepted preference edit applies immediately and requests persistence.
**Language** updates the running session, including open dialogs, search results,
validation errors and shortcut recording. It preserves the drawing and unfinished
edits. System follows the operating system's preferred languages. The picker
offers only complete, reviewed catalogs, using each language's native name.
Restoring a language unavailable in this build keeps the System default.

Preference pages, rows, choices, resets and search results use the session's
shared localization context. Search normalizes Unicode compatibility forms and
matches translated labels as well as canonical English labels. Closing
a settings dialog dismisses the view; it is not a second Apply operation.
Dependent fields are enabled or disabled by shared rules, so different clients
cannot disagree about whether a setting is valid.

About ends with a read-only two-column row, Dedicated to / Nagu, after Source
code. Application license shows its value without a description.

**Cursor shape** is one shared setting for canvas tools, with None first,
followed by Cross, Triangle, Dot, Single-pixel dot, Sight, Tool, Tool and brush
size, and brush-size
outlines with no center marker, a cross, a dot, or a single-pixel dot. The
brush-size options retain the resolved brush-tip shape and dynamics. Existing
saved cursor modes remain valid, and Reset still selects the brush-size outline.
Single-pixel markers occupy one physical display pixel at any host scale.
Dot is a tiny cross. Cross, Dot, and Sight use dark strokes with a light surround;
Sight also has a center dot. Their silhouettes match the shared dropdown icons.

Tool shows the active tool's shared icon with its working point at the pointer:
the pen nib, brush bristles or eraser edge. Geometric shapes use their center.
It follows the selected medium, selection tool and figure or gradient shape, and shows the
Eraser when erasing. Tool and brush size adds the resolved brush-tip outline for
brush tools; other tools keep their icon. Tool icons stay the same screen size
while zooming and have dark silhouettes with a light surround on either theme.

With None selected, mouse and trackpad hover still show Sight. Confirmed
screenless tablet pens do too; display pens and pens whose device type is unknown
stay hidden. GTK reads the tablet's native pointer property, and Windows reads
the native pen device type. Hosts without that information leave pens unknown.
This only changes cursor presentation: screenless pens retain pen input behavior.

**Hide cursor when painting** hides the painting cursor during contact, except
that brush-size outlines and their center markers stay visible for the Eraser,
transparent color, and a pen's physical eraser. None stays invisible during contact.
The shared Rust cursor model and GPU presenter apply this behavior on every host.

**Use Pass Through for new groups**, in the Canvas page's Layers group, is off by
default, so empty new groups use Normal. On, new groups use
[Pass Through](../internals/documents.md#groups-and-pass-through). Grouping existing
layers also uses Pass Through when their blend modes or standalone adjustments
need the existing backdrop. The preference affects only groups made afterwards.

## Keyboard shortcuts

[`shortcuts.rs`](../../crates/layer-ui/src/shortcuts.rs) associates key chords with
typed editor actions. Shared code handles defaults, user overrides and conflicts.
A shortcut invokes the same command as a toolbar or menu item.
Settings store user chords and modifier maps. Generated shortcut definitions are
runtime values; hosts receive the shortcut page and row views.

The host translates native keyboard events and preserves normal widget behavior.
Typing in a text field must not accidentally trigger a canvas shortcut. Platform
modifiers and reserved OS interactions also need native testing.

Each binding has a scope: Application, Canvas, or specific tool categories.
Canvas and tool scopes apply only while no chrome control such as a divider owns
keyboard focus. Recording a chord that another action already uses anywhere its
scope overlaps is a conflict; Reassign moves the chord. At runtime a chord runs
the most specific binding that is enabled, so a disabled one lets the next run:
Delete and Backspace delete the selected guide with the Ruler and Move tools,
and otherwise clear the selected pixels. Defaults and presets share a chord
across scopes only in such deliberate layers. An explicitly saved
binding suppresses a newer default on the same chord in an overlapping scope, so
upgrades never shadow existing choices.

Tool, brush and mode keys work on tap and while held: a tap switches, and holding
the key while drawing returns to the previous tool or mode on release. Modes are
Paint with transparency and the view and ruler toggles such as Snap to rulers.
Relative step bindings use the setting's own numeric step and bounds and repeat
while held.

The shortcuts page lists categories, with one search line that also accepts a
pressed chord, a tool filter and an All actions / With shortcuts / Customized
filter. A single ASCII character matches keys; other characters search action names. The editor is one
sheet: the action's keys, Add Shortcut with inline recording, and reset. Actions
that share a name, such as the Eraser tool and the Eraser brush, describe which
is which.

## Modifier keys

Modifier keys are the first shortcut category. Holding one uses an action until
release, chosen per tool category, so Alt samples color with drawing, blending,
fill and gradient tools, sets the source with the Clone Stamp and Healing Brush,
and does nothing with selection tools, which keep their own Alt behavior, or
with the Spot Healing Brush, which has no source to set. With a tool filter, the
shortcuts page shows each modifier key's action for that kind of tool. Space
pans. Any key or button can be a modifier key, alone or
with Shift, Ctrl or Alt, including letters, F13–F24, gamepad and tablet pad
buttons. Escape stays free to cancel recording. A key is either a shortcut or a
modifier key, never both; recording one against the other offers Reassign, and
recording an existing modifier key opens it.
Hosts choose modifier actions through `OpenModifierPicker` and `ChooseAction`.

The table is derived from the keymap until the artist edits it, then stored
whole in `hold_keys`. While drawing, every fully held entry is in effect; a
combination such as Ctrl+Space replaces the keys it contains, and among equals
the newest press wins. Releasing returns to the earlier hold, or to the tool
captured before the first hold. A hold pressed or released during a stroke
applies when the stroke finishes. Blur ends every hold. Explicitly choosing a
tool ends tool holds until their keys are released. Modifiers owned by a hold do
not change later chords, so Ctrl+Z still undoes while Ctrl samples.

## Keymap presets, import and export

[`keymaps.rs`](../../crates/layer-ui/src/keymaps.rs) holds versioned keymap
presets as data. Each preset records:

- ID, revision and title
- The source application, version or checked date, and keyboard layout
- Source links
- Binding and gesture overlays
- Explicit differences from the source application

Bindings resolve in layers: CapyCanvas defaults, then the chosen preset, then
the user's overrides. Choosing or changing a preset never touches overrides.
Resetting one shortcut returns it to the preset's binding.

The presets are Photoshop Style, Krita Style, GIMP Style, Affinity Style, Clip
Studio Paint Style and Procreate Style. Each preset records its source links and
reproduces only bindings verified against them, mapped to actions with the same
meaning in CapyCanvas. Everything else is listed
as a difference, for example:

- Photoshop's R Rotate View tool
- Krita's E erase-mode toggle
- Procreate's QuickMenu

Unverified rows are never shipped as bindings. A preset change that alters
bindings needs a new revision. The shortcuts page marks a saved keymap with an
older revision as outdated.

Keymap files use the `.capykeys` extension, format `capycanvas-keymap`, version
1. A file contains the preset reference, shortcut overrides, gesture overrides,
per-tool pen buttons and a customized modifier key table. Export is
deterministic. Import is previewed before anything changes:

- The preview lists added, changed and removed bindings, and unavailable
  actions or presets.
- Imported bindings merge over the current overrides and take their chords from
  other actions.
- Unknown action IDs are reported and not applied.
- Files from a newer format version are refused.

Each host provides the file chooser. GTK uses `GtkFileDialog`, Web uses a
download link and a file input, Android uses the Storage Access Framework,
Windows uses the Windows App SDK file pickers and reads or writes the file off
the UI thread, and macOS and iPadOS use the system file exporter and importer.

The binding editor shows the action's description, its keys and default, and
any chord that a more or less specific scope resolves differently.

## Remotes and gamepads

Keyboard-emulating remotes, foot pedals and page turners work through ordinary
key events and can be bound like any key. That includes F13–F24 and volume or
media keys, which hosts may report as, for example, `AudioVolumeUp` or
`AudioRaiseVolume`. Rust stores one canonical name for each such key, such as
`volumeup`. On Android and Windows an unbound volume or media key keeps its
system meaning. The app claims it only while a shortcut or modifier key uses it,
or while a shortcut is being recorded.

Standard-layout gamepad buttons record and resolve as keys named `gamepad_a`,
`gamepad_r1` and so on. Web polls the Gamepad API, Android forwards gamepad
key events, Windows polls `Windows.Gaming.Input` on a worker thread while its
window is active, and macOS and iPadOS read the GameController framework's
extended gamepad and send its input to the key editor window. Windows reads the
analog triggers as `gamepad_l2` and `gamepad_r2` with hysteresis; the system
keeps the guide button. Gamepad buttons do not repeat natively, so the Web,
Windows and Apple adapters repeat a held button after 500 ms and then every
50 ms, and only repeatable bindings act on those repeats.

Sticks send their current deflection as `axes` input:

- The left stick pans.
- Pushing the right stick up zooms in.

Rust applies a 0.15 radial dead zone and a squared response, scaled by the
scroll pan and zoom speeds. It integrates motion on each frame, pauses it while a
stroke or other canvas contact owns the view, and clears it on blur. A
disconnect releases held buttons and centers the sticks on Web, Android,
Windows, macOS and iPadOS; on Windows deactivating the window does the same.

GTK has no gamepad adapter, because GTK itself exposes no gamepad API. A
Bluetooth device works only when it presents one of these standard input
classes. Bluetooth support alone does not make a device compatible.

## Touch gestures and pen buttons

Input settings map finger taps and pen side buttons to the same shortcut
definitions. By default a two-finger tap undoes and a three-finger tap redoes.
Four-finger taps and the lower and upper side buttons, plus a third on Linux, do
nothing until chosen. An unbound side button stays with the tablet driver and the host's
previous behavior. Tap overrides are stored under `gestures`, keyed by trigger ID.
An empty value turns off a default.

Each pen button opens its own page with an action per tool category, stored
under `pen_buttons`. Tools, brushes and modes last while the button is held;
other actions run once on press. The eraser end is a separate setting: it can
keep the current tool or switch to a drawing tool group while that end is near
the tablet, and it can paint with transparency. By default it erases with the
current brush.

Hosts supply native contact timestamps, the platform long-press time and touch
slop; the timing carries over to every drawing opened in the window. iPadOS
sends UIKit's long-press duration and allowable movement. The shared recognizer
turns a tap into an action only when all of these hold:

- Two to four fingers land without lifting in between.
- None of them moves farther than the slop.
- Every finger lifts within the long-press time.
- No pen, picker, placement or stroke owns the canvas.

A recognized tap restores any view jitter from its brief contacts. Hosts that
cannot supply timestamps send `time_ns: 0`, which disables taps.

Side-button presses and releases arrive as `pen_button` input, never as pen
samples. They use the same hold lifecycle as held keys. A button pressed during
a stroke changes the tool only after the stroke ends, and blur releases it. GTK
reports Wayland stylus buttons 2, 3 and 8, and tablet pad buttons as
`pad_button_N` keys when the compositor leaves them to the app. Android reports
tablet buttons `KEYCODE_BUTTON_1`–`16` as the same `pad_button_N` keys. Web
reports pointer `buttons` bits 2 and 4. Android reports the stylus primary and secondary buttons. macOS reports a
tablet pen's right and other mouse buttons as the lower and upper buttons, and
its eraser end. Windows reports the Windows Ink barrel button as the lower
button and the inverted pen as the eraser end, in hover and in contact. Windows
Ink has no upper button, so Windows lists only the lower one. iPadOS does not
show the pen button or eraser end rows until its host delivers the same input.
iPadOS and Windows deliver finger taps with native touch timestamps and list the
tap rows; macOS receives no finger contacts and lists no taps. Windows keeps
three- and four-finger contacts for its own touch gestures while those are on
in Windows Settings, so only two-finger taps reach the app by default.

An Apple Pencil double-tap or squeeze follows the iPad's Apple Pencil setting.
iPadOS sends it as `stylus_action` input when that setting switches between the
current tool and the eraser, or the last used tool; the other choices do
nothing. The last used tool is the last one the artist chose, never one that a
held key or pen button selected. A switch during a stroke applies after the
stroke ends, and none applies while Settings, a popup or command search is open.

## Saving and loading

The host performs storage through its own APIs. Native clients write application
settings in platform storage; the web client uses browser storage. A failed
write must be reported without blocking pen input.

Stored data may come from any build, older or newer, and is never migrated.
Reading it never fails and never shows a message:

- Preferences: hosts pass the saved text to `UiAction::RestoreSavedSettings`, or
  to `Settings::restore` when they need settings before a session exists. Each
  field this build reads and validates is kept, and every other field keeps its
  default. Loading does not rewrite the saved copy; the next change replaces it.
- Export presets load through `ExportPresets::restore`, and the hidden-profile
  list keeps the IDs this build reads. A copy this build cannot read counts as
  empty, and the next change replaces it. The web color-preference database
  replaces its stores when its version changes.
- Workspaces: startup replaces a store it cannot read; see
  [workspace startup](../internals/workspace-ownership.md#startup-always-adopts-a-workspace).
- Caches discard data they cannot use.
- Artwork is never replaced automatically. A recovery copy this build cannot
  open is still offered, with Discard.

`node apps/layer-web/test.mjs --headless --stale-storage` writes stale data into
every web store, reloads, and requires a clean start and a working Export dialog.

Only shortcut overrides are stored, so an unmodified default can evolve with the
app. New settings define their default and validation in Rust before a host adds
a control.

[`WorkspaceState`](../../crates/layer-ui/src/workspace.rs) has its own version
and validation for layout. Every host stores named workspaces through
`layer-workspace`, which saves them automatically; see the
[workspace manager](default-workspaces.md#workspace-manager). Preferences and
workspace state do not belong in a `.capy` project and do not count as unsaved
artwork.
