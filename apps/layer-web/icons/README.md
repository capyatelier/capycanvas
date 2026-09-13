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
`layer-ui` supplies command/icon identities to the hosts. GTK 4.22 uses `GtkSvg`
paintables with symbolic foreground paints, preserving SVG transforms, group
opacity and fixed black/white fills. Its color toolbar explicitly binds the two
swatches to the live foreground/background palette.
Android also reads this bank directly and paints the vectors at the requested
device size, preserving fixed swatch fills. Windows stages theme-specific copies
for WinUI's SVG image source, replacing only `currentColor` and preserving the
original geometry, explicit paints and opacity attributes.
Apple builds compile vector assets from this bank, retaining foreground and
fixed paints in SVG drawing order. The complete action mapping, reference
research and design decisions are in the
[cross-platform icon audit](../../../docs/ui/icon-audit.md).
Web-only browser-window controls load the original two-arrow fullscreen icons
from this bank directly; no fullscreen button is added to GTK.
Collapsed-sidebar expand buttons use `chevron-double-right` on the left and
`chevron-double-left` on the right, centered and pointing toward the canvas.

Keep 16×16 icon geometry (the capybara retains its own viewBox).
Keep ordinary SVG fill/stroke attributes authoritative. Existing symbolic classes
remain for compatibility, but GTK's production renderer reads the vectors through
[GtkSvg](https://docs.gtk.org/gtk4/class.Svg.html), without traditional symbolic
loading that discards explicit paints. The color swatch uses the symbolic success
palette for its selected-color fill.
Do not vendor GNOME icons/fonts or other incompatible assets for visual parity.
