# Shared application icons

These SVGs are project assets, not imported GNOME artwork. Generic interface
icons are MIT OR Apache-2.0. **Exception:** the four `layer-zen-*-symbolic.svg`
capybara marks listed in BRANDING.md are covered only by the separate
[branding license](../../../BRANDING.md), not either software license.
These vector traces use the owner's four supplied screenshots. Looking up is
the default, followed by Facing forward, Bathing and Sleeping. The canonical
24px SVGs use square viewBoxes, centered artwork and a consistent maximum
extent; `currentColor` supplies light/dark tint without duplicate drawings.
The GTK, Android and PWA build scripts derive app/launcher icons from Looking up.
Web loads these files directly. GTK embeds the same files in its resource bank;
there are no generated copies or toolkit-specific icon drawings to maintain.
`layer-ui` supplies command/icon identities to both hosts.
Web-only browser-window controls load the original two-arrow fullscreen icons
from this bank directly; no fullscreen button is added to GTK.
Collapsed-sidebar expand buttons use `chevron-double-right` on the left and
`chevron-double-left` on the right, centered and pointing toward the canvas.

Keep 16×16 icon geometry (the capybara retains its own viewBox).
Stroked paths declare `transparent-fill foreground-stroke` plus per-path stroke
properties so GTK's symbolic parser and standard SVG consumers agree. This is
documented in [GTK's symbolic format](https://gnome.pages.gitlab.gnome.org/gtk/gtk4/icon-format.html).
The color swatch uses the symbolic success palette for its selected-color fill.
Do not vendor GNOME icons/fonts or other incompatible assets for visual parity.
