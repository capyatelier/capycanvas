# Localization

[Workspace and UI](README.md)

The localization foundation lives in `layer-ui::localization`. Embedded Fluent
catalogs in `assets/locales/<tag>/` supply shared UI text. The foundation includes
English, Japanese, Simplified Chinese, Traditional Chinese and Korean seed
catalogs; it does not establish complete translated host interfaces.
`SHIPPED_LANGUAGES` identifies languages enabled for release. Draft catalogs may
omit English messages while shared text is migrated; present entries still need
valid identities, arguments, references and formatting. Shipped catalogs must be
complete.

## Shared text

English defines message identities and permitted named arguments. Use semantic
kebab-case keys for meanings, independently of their wording. The build script
generates `MessageId` from English; do not maintain a second key inventory.

`UiLanguage` identifies a language. `resolve_language` matches a preference and
ordered host-supplied language tags without reading the environment. A `Localizer`
owns an immutable active language, concurrent Fluent bundles and cached simple
labels. The build script derives parameterless identities from all canonical
English branches and references. `text(MessageId)` returns their warmed shared
display text; `format(MessageId, &FluentArgs)` resolves a complete dynamic message.
Translations may omit arguments without changing a resource reference's eligibility. Formatting failure records a
diagnostic and retries English. Invalid English must fail development checks.

The session stores its sole launch context privately in `UiState`. State clones
retain the same `Arc`; serialization omits it. Editing or restoring the saved
language preference does not replace this context. Language takes effect at the
next launch.

Create the localization context before editing begins. Share it with the owners
that need its text; prepare catalogs and static labels outside painting, pointer
and animation callbacks. Hosts present resolved strings from shared views and
retain stable action identities. Hosts must not duplicate translation or fallback
policy. Commands, shortcut sections, tool families and brush presets retain
semantic identities when their display labels change. Search normalizes text with
NFKC and Unicode lowercase, checks active and English labels, and scores character
distance. Shortcut key queries accept one ASCII graphic character.

## Catalogs and arguments

Keep punctuation and grammar in complete messages. Pass counts as numbers and
user names as literal arguments. Translations may omit unused English arguments
and choose their own plural branches, but must not require new inputs. Fluent
functions are unsupported; selectors remain available. Preserve
custom names, filenames and imported metadata. Display strings and Fluent bidi
isolation are not saved-name generators.

Catalog structure and formatting checks run in the ordinary `layer-ui` test
suite; see [testing](../development/testing.md#localization). Structural checks
cannot establish translation quality. A language needs complete catalog and
surface coverage, independent linguistic review with painting terminology, and
native layout, accessibility and IME journeys before it is advertised. Seed
drafts alone do not satisfy those gates.
