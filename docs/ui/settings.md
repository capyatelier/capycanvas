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

## Keyboard shortcuts

[`shortcuts.rs`](../../crates/layer-ui/src/shortcuts.rs) associates key chords with
typed editor actions. Shared code handles defaults, user overrides and conflicts.
A shortcut invokes the same command as a toolbar or menu item.

The host translates native keyboard events and preserves normal widget behavior.
Typing in a text field must not accidentally trigger a canvas shortcut. Platform
modifiers and reserved OS interactions also need native testing.

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
