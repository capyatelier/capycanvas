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

The pages cover appearance, canvas navigation and cursors, pen input, keyboard
shortcuts and application information. A preference row can describe a choice,
number, toggle or text field, together with its current value, enabled state and
reset action. Theme colors use the shared
[palette transformation](theme-colors.md); numeric controls use the same
[parsing and stepping policy](numeric-controls.md) as tool panels.

An accepted preference edit applies immediately and requests persistence. Closing
a settings dialog dismisses the view; it is not a second Apply operation.
Dependent fields are enabled or disabled by shared rules, so different clients
cannot disagree about whether a setting is valid.

**Cursor shape** is one shared setting for painting tools, with None first,
followed by Cross, Triangle, Dot, Single-pixel dot, Sight, and brush-size
outlines with no center marker, a cross, a dot, or a single-pixel dot. The
brush-size options retain the resolved brush-tip shape and dynamics. Existing
saved cursor modes remain valid, and Reset still selects the brush-size outline.
Single-pixel markers occupy one physical display pixel at any host scale.
Dot is a tiny cross. Cross, Dot, and Sight use dark strokes with a light surround;
Sight also has a center dot. Their silhouettes match the shared dropdown icons.

With None selected, mouse and trackpad hover still show Sight. Confirmed
screenless tablet pens do too; display pens and pens whose device type is unknown
stay hidden. GTK reads the tablet's native pointer property, and Windows reads
the native pen device type. Hosts without that information leave pens unknown.
This only changes cursor presentation: screenless pens retain pen input behavior.

**Hide cursor when painting** hides the painting cursor during contact, except
that brush-size outlines and their center markers stay visible for the Eraser,
transparent color, and a pen's physical eraser. None stays invisible during contact.
The shared Rust cursor model and GPU presenter apply this behavior on every host.

## Keyboard shortcuts

[`shortcuts.rs`](../../crates/layer-ui/src/shortcuts.rs) associates key chords with
typed editor actions. Shared code handles defaults, user overrides and conflicts.
A shortcut invokes the same command as a toolbar or menu item.

The host translates native keyboard events and preserves normal widget behavior.
Typing in a text field must not accidentally trigger a canvas shortcut. Platform
modifiers and reserved OS interactions also need native testing.

Each binding has a scope: Application, Canvas, or specific tool categories.
Canvas and tool scopes apply only while no chrome control such as a divider owns
keyboard focus. The most specific applicable scope wins, so `[` can step brush
size on the canvas and still reach an Application binding elsewhere. Two
bindings conflict only when their chord, specificity and scopes overlap. Tool
scopes use the tool underneath an active hold, so Alt followed by a held eraser
key works from a sampling override. An explicitly saved binding still suppresses
a newer default on the same chord, so upgrades never shadow existing choices.

Held bindings temporarily replace the tool: Alt samples color with drawing,
blending, fill and gradient tools, and Space still pans. Held rows may record a
lone Shift, Ctrl or Alt. The newest held key wins. Releasing it returns to the
earlier hold, or to the tool captured before the first hold. A hold pressed or
released during a stroke or other canvas interaction applies when that
interaction finishes. Blur ends every hold. Explicitly choosing a tool also ends
them and keeps the chosen tool. Modifiers owned by a hold do not change later
chords, so Ctrl+Z still undoes while Alt samples. Relative step bindings use the
setting's own numeric step and bounds and repeat while held.

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

The initial presets are Photoshop-, Krita-, Clip Studio Paint- and
Procreate-inspired. They reproduce only rows marked as sourced in the
[shortcut audit](../history/command-input-shortcut-audit-2026-09-25.md) and map
them to actions with the same meaning in CapyCanvas. Everything else is listed
as a difference, for example:

- Photoshop's R Rotate View tool
- Krita's E erase-mode toggle
- Procreate's QuickMenu

Unverified rows are never shipped as bindings. A preset change that alters
bindings needs a new revision. The shortcuts page marks a saved keymap with an
older revision as outdated.

Keymap files use format `capycanvas-keymap`, version 1. A file contains the
preset reference, shortcut overrides and gesture overrides. Export is
deterministic. Import is previewed before anything changes:

- The preview lists added, changed and removed bindings, and unavailable
  actions or presets.
- Imported bindings merge over the current overrides and take their chords from
  other actions.
- Unknown action IDs are reported and not applied.
- Files from a newer format version are refused.

Each host provides the file chooser. GTK uses `GtkFileDialog`, Web uses a
download link and a file input, and Android uses the Storage Access Framework.

The binding editor shows each action's group, scope (everywhere, on the canvas,
or specific tools) and where its binding came from: CapyCanvas default, a preset
or custom. It also lists any chord that a more or less specific scope resolves
differently.

## Remotes and gamepads

Keyboard-emulating remotes, foot pedals and page turners work through ordinary
key events and can be bound like any key. That includes F13–F24 and volume or
media keys, which hosts may report as, for example, `AudioVolumeUp` or
`AudioRaiseVolume`. Rust stores one canonical name for each such key, such as
`volumeup`. On Android an unbound volume or media key keeps its system meaning.
The app claims it only while a shortcut uses it.

Standard-layout gamepad buttons record and resolve as keys named `gamepad_a`,
`gamepad_r1` and so on. Web polls the Gamepad API and Android forwards gamepad
key events. Gamepad buttons do not repeat natively, so the Web adapter repeats a
held button after 500 ms and then every 50 ms, and only repeatable bindings act
on those repeats.

Sticks send their current deflection as `axes` input:

- The left stick pans.
- Pushing the right stick up zooms in.

Rust applies a 0.15 radial dead zone and a squared response, scaled by the
scroll pan and zoom speeds. It integrates motion on each frame, pauses it while a
stroke or other canvas contact owns the view, and clears it on blur. A
disconnect releases held buttons and centers the sticks on both hosts.

GTK has no gamepad adapter, because GTK itself exposes no gamepad API. A
Bluetooth device works only when it presents one of these standard input
classes. Bluetooth support alone does not make a device compatible.

## Touch gestures and pen buttons

Input settings map finger taps and pen side buttons to the same shortcut
definitions. By default a two-finger tap undoes and a three-finger tap redoes.
Four-finger taps and both side buttons do nothing until chosen. An unbound side
button stays with the tablet driver and the host's previous behavior. Only
overrides are stored, under `gestures`, keyed by trigger ID. An empty value turns
off a default.

Hosts supply native contact timestamps, the platform long-press time and touch
slop. The shared recognizer turns a tap into an action only when all of these
hold:

- Two to four fingers land without lifting in between.
- None of them moves farther than the slop.
- Every finger lifts within the long-press time.
- No pen, picker, placement or stroke owns the canvas.

A recognized tap restores any view jitter from its brief contacts. Hosts that
cannot supply timestamps send `time_ns: 0`, which disables taps.

Side-button presses and releases arrive as `pen_button` input, never as pen
samples. They use the same hold lifecycle as held keys. A button pressed during
a stroke changes the tool only after the stroke ends, and blur releases it. GTK
reports Wayland stylus buttons 2 and 3. Web reports pointer `buttons` bits 2 and
4. Android reports the stylus primary and secondary buttons. Windows, macOS and
iPadOS do not show these rows until their hosts deliver the same input.

## Saving and loading

The host performs storage through its own APIs. Native clients write application
settings in platform storage; the web client uses browser storage. Shared code
validates loaded values and handles supported migrations before replacing live
state. A failed write must be reported without blocking pen input.

Only shortcut overrides are stored, so an unmodified default can evolve with the
app. Retired fields and actions are handled by explicit migration rules. New
settings should define their default, validation and migration behavior in Rust
before a host adds a control.

[`WorkspaceState`](../../crates/layer-ui/src/workspace.rs) has its own version and
validation for layout. Android, Apple and web currently persist workspace state; GTK does not yet
restore saved layouts automatically. Preferences and workspace state do not belong in a `.capy`
project and do not count as unsaved artwork.

The [platform setup pages](../development/README.md) link to implementation and
validation records for each host. The earlier
[settings implementation record](../history/settings-implementation-plan.md)
contains migration history and past UI checks; it is not a separate source of
current defaults.
