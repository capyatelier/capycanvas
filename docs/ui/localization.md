# Localization

[Workspace and UI](README.md)

Shared localization lives in `layer-ui::localization`. Embedded Fluent
catalogs in `assets/locales/<tag>/` supply shared UI text. Catalogs exist for English, Japanese, Simplified Chinese, Traditional Chinese
and Korean. Catalog presence does not establish complete translated host
interfaces.
`SHIPPED_LANGUAGES` identifies languages enabled for release. Draft catalogs may
omit English messages while shared text is migrated; present entries still need
valid identities, arguments, references and formatting. Shipped catalogs must be
complete.

## Shared text

English defines message identities and permitted named arguments. Use semantic
kebab-case keys for meanings, independently of their wording. The build script
generates `MessageId` from English; do not maintain a second key inventory.

When adding copy, add its complete English message to the existing feature
catalog, then resolve it in shared Rust with the owner's active `Localizer`.
Reuse an existing identity only when its meaning and arguments match. Expose
resolved labels or typed feature copy to hosts; remove the superseded English
presentation path. Canonical English helpers belong only in explicit search
aliases, fixtures or documentation, not active UI projection.

Use `text(MessageId)` for cached static copy and a typed feature helper with
`FluentArgs` for dynamic copy. Retain complete dynamic captions in the existing
view or draft when their semantic inputs change. Pointer motion and ordinary
state publication must reuse that copy while reading live command state.
Add a window-free regression for active text, unchanged action identity and
literal arguments, and check affected host consumers.

`launch_localization` extracts only the saved language field and matches host language tags against `SHIPPED_LANGUAGES` before warming one launch context. Other malformed saved fields cannot erase a valid language preference. `NativeHost::launch` then restores settings through the existing fieldwise restore before publishing a view or attaching a GPU. `BootstrapView` supplies resolved progress, failure and accessibility copy before GPU creation; unexpected platform details belong in diagnostics.

`UiLanguage` identifies a language. `resolve_language` matches a preference and
ordered host-supplied language tags without reading the environment. A `Localizer`
owns an immutable active language, concurrent Fluent bundles and cached simple
labels. The build script derives parameterless identities from all canonical
English branches and references. `text(MessageId)` returns their warmed shared
display text; `format(MessageId, &FluentArgs)` resolves a complete dynamic message.
Only identities in generated `MessageId::STATIC` are eligible for `text` and
static resource labels. Eligibility follows every canonical English selector
branch and referenced message or term; a translation that omits an argument
cannot make an argument-bearing English identity static. Formatting failure records a
diagnostic and retries English. Invalid English must fail development checks.

The session stores its sole launch context privately in `UiState`. State clones
retain the same `Arc`; serialization omits it. Editing or restoring the saved
language preference does not replace this context. Language takes effect at the
next launch.

Native text editors own Escape while editing, including composition. Shared
shortcuts leave surrounding popups and drawers open until focus leaves the editor.

Create the localization context before editing begins. Share it with the owners
that need its text; prepare catalogs and static labels outside painting, pointer
and animation callbacks. Hosts present resolved strings from shared views and
retain stable action identities. Hosts must not duplicate translation or fallback
policy. Canvas Size and Image Size retain resolved labels and complete status messages in their existing dialog drafts; only opening or editing the draft formats status copy. Layer, mask and saved-selection menus resolve complete captions when opened, preserving typed actions and literal document names. Commands, shortcut sections, tool families and brush presets retain
semantic identities when their display labels change. Search normalizes text with
NFKC and Unicode lowercase, checks active and English labels, and scores character
distance. Shortcut key queries accept one ASCII graphic character.

## Catalogs and arguments

Keep punctuation and grammar in complete messages. Pass counts as numbers and
user names as literal arguments. Translations may omit unused English arguments
and choose their own plural branches, but must not require new inputs. Fluent
functions are unsupported; selectors remain available. Preserve
custom names, filenames and imported metadata. Display strings and Fluent bidi
isolation are not saved-name generators. Use typed action, category and resource
identities for routing, grouping and availability; never compare translated
labels or recognize English text to recover an identity.

When translating, add matching identities to the same domain under the locale
folder. Preserve named arguments, reference structure, units and literal brand
names; use selectors for counts and the painting terminology used elsewhere in
the app. Adding a locale requires registering its tag and embedded domains in
`UiLanguage` and the catalog build, without adding it to `SHIPPED_LANGUAGES`
until its release gates pass.

Catalog structure and formatting checks run in the ordinary `layer-ui` test
suite; see [testing](../development/testing.md#localization). Structural checks
cannot establish translation quality. A language needs complete catalog and
surface coverage, independent linguistic review with painting terminology, and
native layout, accessibility and IME journeys before it is advertised. Seed
drafts alone do not satisfy those gates.

Known document, toolbar and workspace refusals carry typed reasons through storage and host boundaries. Item summaries retain workspace error payloads, including numeric labels and bounds; cold storage never needs a localizer. UI action boundaries project the reason with the active context. Unexpected storage diagnostics remain available for logs, while the UI uses a complete refusal selected by the error kind. Keymap import headings and counted preview sections are formatted when the import is prepared and retained with the preview.
