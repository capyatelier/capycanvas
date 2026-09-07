# Shared application icons

These SVGs are project assets, not imported GNOME artwork. Generic interface
icons are MIT OR Apache-2.0. **Exception:** `layer-zen-symbolic.svg` is the
owner-contributed capybara mark, covered only by the separate
[branding license](../../../BRANDING.md), not either software license.
The owner confirmed authorship of the artwork used for that vector trace.
Web loads these files directly. GTK embeds the same files in its resource bank;
there are no generated copies or toolkit-specific icon drawings to maintain.
`layer-ui` supplies command/icon identities to both hosts.

Keep 16×16 icon geometry (the capybara retains its own viewBox).
Stroked paths declare `transparent-fill foreground-stroke` plus per-path stroke
properties so GTK's symbolic parser and standard SVG consumers agree. This is
documented in [GTK's symbolic format](https://gnome.pages.gitlab.gnome.org/gtk/gtk4/icon-format.html).
The color swatch uses the symbolic success palette for its selected-color fill.
Do not vendor GNOME icons/fonts or other incompatible assets for visual parity.
