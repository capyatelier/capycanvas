# Configurable base colors

[Technical documentation](../README.md)

Appearance offers **Dark theme base color** (`#333333`) and **Light theme base color**
(`#b8b8b8`). These are opaque sRGB hex strings: exactly `#RRGGBB`, case-insensitive,
stored in lowercase. Editing commits on Enter/Done or leaving the field. Rust
rejects invalid input without changing the accepted setting or saving it.
System/Light/Dark selection remains independent of both colors.

## Inventory and existing relationships

Before this change, most surfaces were independently chosen constants, not
computed from the base. The equations below reconstruct those choices; they
are the new calibration, not a claim about the old implementation.

Let `B` be the chosen base in **encoded sRGB** (channels 0–255), `W` white,
`K` black, and `mix(a,b,t) = (1-t)a + tb`. Dark's reference base `D=51`,
light's `L=184`. We round only the final channel value. An opaque RGB triplet
in the table is transformed per channel using the same rule as a grey.

| Color role and consumers | Dark default / relationship | Light default / relationship | Treatment |
| --- | --- | --- | --- |
| Canvas surround, header button/title backgrounds, status numbers, browser theme color, GPU-unavailable background | `#333333` = B | `#b8b8b8` = B | Chosen base |
| Panel bodies, selected tabs and concave joins, tool ribbons, expanded drawers, popovers/menus | `#414141` = mix(B,W,14/204) | `#ededed` = mix(B,W,53/71) | Regenerate |
| Inactive tab bar | `#2e2e2e` = mix(B,K,5/51) | `#dedede` = mix(B,W,38/71) | Regenerate |
| Panel input backgrounds, inactive compact slider track | `#333333` = B | `#fafafa` = mix(B,W,66/71) | Regenerate |
| Native GTK view background | `#2b2b2b` = mix(B,K,8/51) | `#e4e4e4` = mix(B,W,44/71) | Regenerate |
| Preferences and web dialog background | `#333333` = B | `#fafafb`: white mix 66/71 for R,G, 67/71 for B | Regenerate |
| Android settings background | Same as dark preferences | `#fafafa` = mix(B,W,66/71) | Regenerate; retain platform default |
| GTK/web settings sidebar | `#2e2e32`: black mix 5/51 for R,G, 1/51 for B | `#ebebed`: white mix 51/71 for R,G, 53/71 for B | Regenerate |
| GTK unfocused sidebar | `#28282c`: black mix 11/51 for R,G, 7/51 for B | `#f2f2f4`: white mix 58/71 for R,G, 60/71 for B | Regenerate |
| Android settings sidebar | Tab bar | Panel body | Reuse palette roles |
| Native GTK alert dialog | `#36363a`: white mix 3/204 for R,G, 7/204 for B | `#fafafb`, as preferences | Regenerate |
| GTK/web settings cards | White at 8% in GTK; text at 8% in web, over the settings surface | White | Existing compositing already follows the new surface; white stays white |
| Android settings cards | `#414141`, same as panel | White | Reuse panel / keep white |
| Slider thumb | `#d3d3d3` = mix(B,W,160/204) in web/Android | `#fafafa` = mix(B,W,66/71) | Regenerate; GTK numeric slider uses this role too |
| Compact grey slider fill | mix(panel,text,50%) | mix(panel,text,50%) | Recomputed from panel; CSS sRGB, native Compose interpolation retained |
| Main text and monochrome icons | `#fafafb` | `#2e2e32` | Fixed per mode |
| Secondary text, headings, shortcut hints | Main text at 55–65% opacity | Same | Keep opacity; effective pixel color changes with the surface |
| Android settings secondary text | `#bcbcbc` | `#666666` | Fixed per mode |
| Plain button fill | White at 13/255 alpha | Black at 13/255 alpha | Keep overlay |
| Hover / pressed controls | Text at 8% / 16% over local surface | Same | Keep overlay, recomposite |
| Search field / selected preferences category / dialog close background | Text at 10% | Same | Keep overlay |
| Checkbox/radio outlines, separators, scroll thumbs, disabled controls | Foreground overlays, usually 10–35%; disabled opacity 36–50% | Same pattern | Keep opacity, recomposite |
| Web preference row divider | `#80808026` | Same | Keep translucent neutral |
| Settings slider inactive track / inactive switch | Text at 12% / 20% | Same | Keep overlay |
| Accent, selection, focus, links, checked controls, drop indicators | Web/Android `#3584e4`; GTK native accent (drop hint currently separate blue) | Same | Do not tint; selection remains accent at 22% |
| Error / warning text, invalid numeric border | Web `#ff7b63` / `#e5a50a`, border `#ee5555`; native semantic roles | Web `#c01c28` / `#9c5700`, same border; native semantic roles | Fixed semantic colors per mode |
| Shadows, inset shades, modal dimming | Black with existing opacities; panel 16%, expanded drawer 40%, web modal 8/15 | Same shadows; web modal 2/15 | Keep; already blends over new surfaces |
| Cursor outline, marker and dash contrast | Black + white | Black + white | Unchanged: must contrast against artwork, not UI |
| Brush/color swatches, layer thumbnails, document white, painted pixels, imported/AI images | Artwork colors | Artwork colors | Never transform |
| Brush preview PNGs | Existing light-stroke variant, transparent background | Existing dark-stroke variant, transparent background | Keep; no baked panel background to regenerate |
| Branding, favicon, install icons, manifest/splash launch colors | Fixed branded/launch assets | Fixed branded/launch assets | Unchanged; install assets cannot follow a per-user setting |

Native toolkit focus/selection/error colors remain native rather than replacing
their complete semantic palettes. New surface roles map to libadwaita's
[documented CSS variables](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/css-variables.html),
including inactive header/sidebar variants. GTK's native card and shade overlays
continue to composite normally. Transparent areas remain transparent.

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

Text and semantic colors are not derived from the base. As requested, there is
no automatic mode switching or luminance restriction: choose a dark base for
dark mode, and a mid/light base for light mode. Highly saturated or opposite-mode
colors can reduce contrast; this does not promise contrast compliance for every
hex value.

## Ownership and cost

- `layer-ui::Settings` owns the two validated colors and the text-field schema.
  `theme.rs` is the only source of surface transformation/calibration.
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
