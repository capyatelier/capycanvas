# Shared application icons

These SVGs are project assets, not imported GNOME artwork. Generic interface
icons are MIT OR Apache-2.0. **Exception:** the four `layer-zen-*-symbolic.svg`
capybara marks listed in BRANDING.md are covered only by the separate
[branding license](../../../BRANDING.md), not either software license.
These vector traces use the owner's four supplied screenshots. Looking up is
the default, followed by Facing forward, Bathing and Sleeping. The capybara
SVGs use square viewBoxes, centered artwork and a consistent maximum
extent; `currentColor` supplies light/dark tint without duplicate drawings.
The GTK, Android and PWA build scripts derive app/launcher icons from Looking up.
Web loads these files directly. GTK embeds the same files in its resource bank;
there are no generated copies or toolkit-specific icon drawings to maintain.
`layer-ui` supplies command/icon identities to the hosts. GTK 4.22 uses `GtkSvg`
paintables with symbolic foreground paints, preserving SVG transforms, group
opacity and fixed black/white fills. Its color toolbar explicitly binds the two
swatches to the live foreground/background palette.
Live toolbar and window-bar paint pairs retain this geometry and draw the shared
front swatch last. Their fills use the shared mapped paint previews and opaque
checker composites; selection and rendition changes refresh existing icons.
Category symbols in tabs and customization lists keep the fixed SVG paints.
Android also reads this bank directly and paints the vectors at the requested
device size, preserving fixed swatch fills. Windows stages theme-specific copies
for WinUI's SVG image source, replacing only `currentColor` and preserving the
original geometry, explicit paints and opacity attributes.
Apple builds compile vector assets from this bank, retaining foreground and
fixed paints in SVG drawing order. The icon design rules are in the
[UI guide](../../../docs/ui/README.md#icons).
Web-only browser-window controls load the original two-arrow fullscreen icons
from this bank directly; no fullscreen button is added to GTK.
The Color panel uses the original `color-square`, `color-triangle` and
`color-swap` symbols: rounded geometry and consistent 1.5 px strokes, shared
directly by GTK and Web.

Draw new icons on a 16×16 viewBox. A few older icons use 24×24, and the capybara
marks keep their own square viewBoxes.
The shared `more-small` and `grip` icons use 2px dots centered at y=3, 8 and 13,
for 12px-tall artwork within their 16px boxes. Their integer coordinates align
the dots' bounds to pixels: one column at x=8 for `more-small`, two at x=5 and 11
for `grip`. The workspace switcher and Layers footer share `more-small`;
full-height controls use `more`, with 4px dots and 16px-tall artwork. Render all
grab handles in a 16px box so their dots retain the same size in every context.
Keep ordinary SVG fill/stroke attributes authoritative. Existing symbolic classes
remain for compatibility, but GTK's production renderer reads the vectors through
[GtkSvg](https://docs.gtk.org/gtk4/class.Svg.html), without traditional symbolic
loading that discards explicit paints. The color swatch uses the symbolic success
palette for its selected-color fill.
Do not vendor GNOME icons/fonts or other incompatible assets for visual parity.
