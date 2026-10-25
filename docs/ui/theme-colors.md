# Theme colors

[Workspace and UI](README.md) · [Panel transparency](panel-transparency.md)

Appearance offers **Dark theme base color** (`#333333`) and **Light theme base color**
(`#b8b8b8`). These are opaque sRGB hex strings: exactly `#RRGGBB`, case-insensitive,
stored in lowercase. Editing commits on Enter/Done or leaving the field. Rust
rejects invalid input without changing the accepted setting or saving it.
System/Light/Dark selection remains independent of both colors.

GTK, Web, Android, Windows, macOS and iPadOS show each base color inline on its title
row as four grey circles and a Custom circle that reveals a hex entry beside them: dark
`#1f1f1f`, `#292929`, `#333333`, `#3d3d3d`; light `#a4a4a4`, `#b8b8b8`,
`#cccccc`, `#dedede`.

## Surface roles

[`theme.rs`](../../crates/layer-ui/src/theme.rs) derives every surface from the
chosen base. The equations reproduce the reference surfaces at the default bases.

Let `B` be the chosen base in **encoded sRGB** (channels 0–255), `W` white,
`K` black, and `mix(a,b,t) = (1-t)a + tb`. Dark's reference base `D=51`,
light's `L=184`. We round only the final channel value. An opaque RGB triplet
in the table is transformed per channel using the same rule as a grey.

| Color role and consumers | Dark default / relationship | Light default / relationship | Treatment |
| --- | --- | --- | --- |
| Canvas surround, browser theme color, GPU-unavailable background | `#333333` = B | `#b8b8b8` = B | Chosen base |
| Panel bodies, selected tabs and concave joins, tool ribbons, expanded drawers, popovers/menus | `#414141` = mix(B,W,14/204) | `#ededed` = mix(B,W,53/71) | Regenerate |
| Inactive tab bar | `#2e2e2e` = mix(B,K,5/51) | `#d2d2d2` = mix(B,W,26/71) | Regenerate |
| Workspace switcher well | mix(tab bar, B, 25%) | Same | Glass chip; see [panel transparency](panel-transparency.md) |
| Title-bar controls, bars, menu labels, drawing-tab strip, readouts, GTK close button, Zen Capy, footer zoom/HDR/proof status | B | B | Glass chip: opaque at transparency Off, translucent otherwise |
| Selected workspace and title-bar tool | Accent tint of reference grey 82 (`#40546e` from `#3584e4`) | Accent tint of reference grey 196 (`#afc6e5`) | See [Accent color](#accent-color); stays visible on the light title-bar well |
| Selected drawing tab | Panel body, opaque | Same | Matches selected panel tabs |
| Panel input backgrounds, inactive compact slider track | `#333333` = B | `#fafafa` = mix(B,W,66/71) | Regenerate |
| Native GTK view background | `#2b2b2b` = mix(B,K,8/51) | `#e4e4e4` = mix(B,W,44/71) | Regenerate |
| Every dialog, sheet and alert on every client (`settings`) | `#333333` = B | `#fafafb`: white mix 66/71 for R,G, 67/71 for B | Regenerate |
| Android settings background | Same as dark preferences | `#fafafa` = mix(B,W,66/71) | Regenerate; retain platform default |
| GTK/web settings sidebar | `#2e2e32`: black mix 5/51 for R,G, 1/51 for B | `#ebebed`: white mix 51/71 for R,G, 53/71 for B | Regenerate |
| GTK unfocused sidebar | `#28282c`: black mix 11/51 for R,G, 7/51 for B | `#f2f2f4`: white mix 58/71 for R,G, 60/71 for B | Regenerate |
| Android settings sidebar | Tab bar | Panel body | Reuse palette roles |
| GTK/web settings cards | White at 8% in GTK; text at 8% in web, over the settings surface | White | Existing compositing already follows the new surface; white stays white |
| Android settings cards | `#414141`, same as panel | White | Reuse panel / keep white |
| Slider thumb | `#d3d3d3` = mix(B,W,160/204) in web/Android | `#fafafa` = mix(B,W,66/71) | Regenerate; GTK numeric slider uses this role too |
| Transparency checkerboard (`checker_light`, `checker_dark`) | `#ffffff` / `#cccccc` in 5px cells | Same | Fixed in both modes and every transparency backdrop; GPU and managed previews decode sRGB before linear-light compositing |
| Compact grey slider fill | mix(panel,text,50%) | mix(panel,text,50%) | Recomputed from panel; CSS sRGB, native Compose interpolation retained |
| Main text and monochrome icons | `#fafafb` | `#2e2e32` | Fixed per mode |
| Secondary text, headings, shortcut hints | Main text at 55–65% opacity | Same | Keep opacity; effective pixel color changes with the surface |
| Android settings secondary text | `#bcbcbc` | `#666666` | Fixed per mode |
| Plain button fill | White at 13/255 alpha | Black at 13/255 alpha | Keep overlay |
| Hover / pressed controls | Text at 8% / 16% over local surface | Same | Keep overlay, recomposite |
| Search field, text fields on a dialog surface, selected preferences category, dialog close background | Text at 10% | Same | Keep overlay; fields inside cards and on panels use the input background |
| Checkbox/radio outlines, editor separators, scroll thumbs, disabled controls | Foreground overlays, usually 10–35%; disabled opacity 36–50% | Same pattern | Keep opacity, recomposite |
| Dialog and popover lines: row dividers, settings sidebar edge, card outline and shadow, scroll shades, separators and split handles, popover outline, dialog edge | libadwaita's contrast against the surfaces each line touches | Same | `palette.lines`; see [Lines](#lines) for what each client binds |
| Settings slider inactive track / inactive switch | Text at 12% / 20% | Same | Keep overlay |
| Accent, focus, links, checked controls, drop indicators | Resolved accent: saved, else system (GTK, Android 12+), else `#3584e4` (drop hint currently separate blue) | Same | Not derived from the base |
| Layer clipping rail (`relationship`) | Accent with OKLCH lightness reduced by 0.06, clamped at zero | Same | Preserve hue; reduce chroma only to stay in gamut. FX connectors use ordinary neutral link ink. |
| Panel and toolbar selection | Accent tint of reference grey 82, same as the title bar | Accent tint of reference grey 213 (`#c0d7f6`) | See [Accent color](#accent-color) |
| Error / warning text, invalid numeric border | Web `#ff7b63` / `#e5a50a`, border `#ee5555`; native semantic roles | Web `#c01c28` / `#9c5700`, same border; native semantic roles | Fixed semantic colors per mode |
| Other shadows, inset shades, modal dimming | Black with existing opacities; panel 16%, expanded drawer 40%, web modal 8/15 | Same shadows; web modal 2/15 | Keep; already blends over new surfaces |
| Cursor outline, marker and dash contrast | Black + white | Black + white | Unchanged: must contrast against artwork, not UI |
| Brush/color swatches, layer thumbnails, document white, painted pixels, imported/AI images | Artwork colors | Artwork colors | Never transform |
| Brush preview PNGs | Existing light-stroke variant, transparent background | Existing dark-stroke variant, transparent background | Keep; no baked panel background to regenerate |
| Branding, favicon, install icons, manifest/splash launch colors | Fixed branded/launch assets | Fixed branded/launch assets | Unchanged; install assets cannot follow a per-user setting |

Scope status and axis labels, layer metadata and summaries use main text at
55% opacity. Apply that alpha once: a derived text color in Web, or opacity on
an opaque text foreground in native controls.

Native toolkit error colors remain native rather than replacing their complete
semantic palettes; GTK's accent follows the resolved accent. New surface roles map to libadwaita's
[documented CSS variables](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/css-variables.html),
including inactive header/sidebar variants. GTK's native card and shade overlays
continue to composite normally. Transparent areas remain transparent.

Title-bar controls are filled with the chosen base, so they vanish over the
canvas surround. Their opacity follows the [panel transparency](panel-transparency.md)
level: opaque at Off, frosted glass blurred in the canvas renderer otherwise.
Joined bars and Menu Labels share one surface behind their members; standalone
controls carry their own. Hover, press and selection feedback composite over
this surface. Header text has no outline; no host adds a toolkit backdrop blur.

## Lines

libadwaita draws dividers and outlines at fixed opacities tuned for its own
surfaces. Ours are lighter in dark mode and follow the chosen base, and a fixed
black opacity removes more lightness from a lighter surface, so it draws a
stronger line. [`theme.rs`](../../crates/layer-ui/src/theme.rs) publishes
`palette.lines` instead: every line keeps libadwaita's contrast against the
surfaces it touches.

For each line, take libadwaita 1.9's ink, opacity and the surface it is painted
on, and measure the smallest OKLab lightness step from the blended line to any
surface it touches. On our surfaces, solve the opacity that gives the same
smallest step on the same side. Blending is encoded sRGB, as GTK and browsers
composite.

In dark mode, row dividers are instead the background their card sits on, 5%
darker: each sRGB channel × 0.95, so `#303030` on `#333333`. In light mode they
follow the rule above. The Preferences sidebar edge touches the
sidebar and the content. In dark mode our sidebar is darker than its content,
the reverse of libadwaita, so the edge keeps libadwaita's step against the
sidebar and differs a little more from the content. As in libadwaita, the edge
keeps its color when the window is unfocused.

Each set has the same roles: `card_shade` (row dividers, opaque in dark mode), `card_ring`,
`card_shadow` and `card_far_shadow` (the card's 1px outline and two shadows),
`shade` (scroll edges, drawn at 75%), `border` (separators and split handles in
the text color; GTK binds its opacity to `--border-opacity`) and `highlight`
(the dialog sheet's inner outline). `sidebar_border`, `sidebar_shade` and
`popover_border` (over the canvas surround) complete the set.

| Background | Used by |
| --- | --- |
| `settings` | Every dialog, sheet and alert |
| `surround` | Cards over the canvas, such as the image drop label, and the popover outline |
| `panel` | Popover menus and scroll shades, the panel configuration list; libadwaita's popover is its reference |

GTK binds every role. Web binds row dividers, the card shadow, the Preferences
sidebar edge and menu separators. Android binds row dividers, the card ring and
shadows, dialog separators and menu separators. Apple binds row dividers, dialog
and menu separators and the popover outline. Windows binds row dividers, the
Preferences sidebar edge, dialog footer separators and menu separators. Native
sidebars, alerts and menus on Android, Apple and Windows keep their platform
lines.

GTK sets the roles on each dialog's `sheet`, so modal dimming, which libadwaita
derives from `--shade-color`, keeps its strength. Values other than dark row
dividers stay translucent because libadwaita derives other colors from them. Lines the app draws on editor
chrome (toolbar and column dividers, drawer splits, thumbnail borders) keep their
own colors.

At the default light base, dialogs already use libadwaita's surfaces, so their
lines equal libadwaita's. At the default dark base, the
sidebar edge drops from 36% to 10%.

## Accent color

Below the base colors, GTK, Web, Android, Windows, macOS and iPadOS Appearance offer **Accent color**: System, libadwaita's nine accent colors
(Blue `#3584e4`, Teal, Green, Yellow, Orange, Red, Pink, Purple, Slate) and a
Custom circle that reveals a `#RRGGBB` entry prefilled with the current accent.
The entry commits on Enter or when it loses focus after an edit; invalid hex is
rejected in Rust like the base colors. Selecting a circle commits immediately.
Settings store only a chosen color; an absent value follows the system accent
that the host reports with `SystemThemeChanged`, and hosts without one use Blue.
GTK reports libadwaita's accent; Android 12 and later report Material You's
`system_accent1_500`; Windows reports the personalization accent (`UISettings`
Accent) and macOS reports `NSColor.controlAccentColor`, and both follow its changes.
The web and iPadOS have no System circle, and there choosing Blue stores nothing,
so Reset stays disabled.

The resolved accent is published as `palette.accent`. GTK assigns it to
`--accent-bg-color`, so libadwaita switches, checks, suggested buttons and
focus rings follow it, and derives `--accent-color` from it as usual.
`palette.accent_foreground` is the text and icon color on accent fills: white,
as libadwaita uses for its nine accents, or the dark text color when a custom
accent is lighter than OKLab 0.72. GTK assigns it to `--accent-fg-color`; Web
and Android use it for suggested buttons, checkboxes and Material `onPrimary`.
Selected rows and tiles (workspace rows, Zen icon tiles, toolbar-manager
buttons, vertical drawing tabs) use `selection`; drop-target overlays keep a
translucent accent so the content beneath stays visible. Web binds
the palette roles to `--accent`, `--selection`, `--header-selection` and
`--header-selection-hover`; Android reads the same roles into its palette.

Selection tints keep only the accent's OKLCH hue. Each tint takes the OKLab
lightness of a base-relative reference grey (computed with the same transform as
every surface below), chroma `min(0.05, accent chroma)` so grey accents stay
grey, and reduces chroma further only if the color would leave sRGB:

| Role | Dark reference grey | Light reference grey |
| --- | --- | --- |
| `selection` (panels, toolbars, lists) | 82 | 213 |
| `header_selection` (workspace switcher, title-bar tools) | 82 | 196 |
| `header_selection_hover` | `header_selection` lightness + 0.03 | `header_selection` lightness − 0.03 |

With the default bases every accent gives the same lightness, so warm accents
look muted and Slate's tints read as blue-grey.
A custom light base moves the light tints with it.

## Transformation

For each channel of an existing surface `C` and its mode's reference base `A`:

```
if C <= A: result = B * C / A
else:      result = B + (255 - B) * (C - A) / (255 - A)
```

This is a calibrated black/white mix. It exactly reproduces every reference
surface at the default base, preserves each channel's shade ordering, works
for colored bases, and stays in gamut without clipping. Whites remain white;
surfaces near white carry only a small tint. Unlike adding a fixed RGB offset,
it does not crush multiple bright surfaces against 255.

Use encoded sRGB deliberately, matching the existing CSS color mixing and
the colors used to tune this UI. CSS distinguishes
[sRGB and linear-light interpolation](https://www.w3.org/TR/css-color-4/#interpolation-space).
An Oklab lightness/chroma transformation could offer more perceptual uniformity,
but requires more conversions and gamut handling and is unnecessary for this
small, default-preserving theme control. This is a UI palette, not paint mixing.

Text and semantic colors are not derived from the base. There is
no automatic mode switching or luminance restriction: choose a dark base for
dark mode, and a mid/light base for light mode. Highly saturated or opposite-mode
colors can reduce contrast; this does not promise contrast compliance for every
hex value.

## Ownership and cost

- `layer-ui::Settings` owns the two validated base colors, the optional accent
  and their preference schemas. `theme.rs` is the only source of surface, line and
  accent-tint calibration.
- `UiState.palette` contains resolved colors. Hosts bind them to GTK variables,
  DOM variables, or Compose colors, with no copies of the transformation.
- The GPU surround uses the same base converted with the complete sRGB transfer
  function, including its near-black linear segment. Conversion is prepared
  with the palette, not repeated per drawing frame.
- Palette computation is settings/theme work, not input/dab/render work. GTK
  reloads its window-scoped CSS provider only when the palette actually changes.
  It removes the provider when the window closes.
- Existing saved settings get the original defaults. Invalid saved hex values
  are rejected before any settings are applied. Accepted edits use the existing
  persistence path. No document version or GPU resources change.
- Before the Rust session starts, web and Android retain fixed launch colors.
  Once initialized, custom colors also work without an available web GPU.
