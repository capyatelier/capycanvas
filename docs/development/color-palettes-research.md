# Color palettes: research and proposal

Research date: 2026-09-24. This is a design proposal, not implemented behavior.
The scope is a Palettes panel beside Color in Paint and Photo, and a palette
section below the existing color controls in Sketch's color drawer.

The Artist toolkit scope was selected. The revised
[panel design and interactive mockups](color-palettes-panel.md) supersede the
presentation suggestions below: history comes first, palette addition uses a
trailing + tile, and the selector and editable color name live in the footer.
The palette chooser covers the swatches in place. Generator UI is now deferred,
likely to a separate panel; the research below is background, not the latest
panel specification.

## Starter palette curation

The implemented ten-palette set is described in the
[panel specification](color-palettes-panel.md#starter-palettes). Research on
2026-09-24 focused on recognizable subgenres and actual palette usage, rather
than ten variations on muted painting ramps. Lospec reports substantial use of
[Sweetie 16 by GrafxKid](https://lospec.com/palette-list/sweetie-16),
[ENDESGA 32](https://lospec.com/palette-list/endesga-32), and
[Resurrect 64 by Kerrie Lake](https://lospec.com/palette-list/resurrect-64): their
pages showed more than 60,000, 200,000 and 350,000 downloads respectively.
Downloads establish use on that site, not market-wide preference or a ranking.

| Reference | Curation decision |
| --- | --- |
| Sweetie 16, ENDESGA 32 and [PICO-8](https://lospec.com/palette-list/pico-8) | Pixel Arcade has distinct saturated hue groups and shared dark/light anchors; fewer redundant tints. |
| [Lospec's dark palettes](https://lospec.com/palette-list/tag/dark), including Gothic Bit, BloodMoon21 and Bastille-8 | Dark Fantasy concentrates on low values and restrained stone, blood-red and moss accents. |
| [Fairydust 8 by Yousurname](https://lospec.com/palette-list/fairydust-8) | Candy Pastels covers several hues while keeping most colors light; a small plum group provides outlines. |
| [Lichtenstein learning resource, National Galleries of Scotland](https://www.nationalgalleries.org/art-and-artists/features/roy-lichtenstein-learning-resource) | Pop Art limits the dominant hues to strong red, yellow and blue, plus ink/paper; suited to flat comic shapes and halftones. |
| [RISOTTO fluorescent pink](https://risottostudio.com/products/fluro-pink-806u) and [RISO ink range](https://www.riso.co.jp/english/product/digital_dup/consumables/index.html) | Riso Print concentrates on pink and blue ink families, violet tones and warm paper. These are screen colors, not ink recipes or overprint predictions. |
| [Adobe Color's style categories](https://color.adobe.com/es/explore), including synthwave and vaporwave | Synthwave uses violet shadows, cyan lights and saturated sunset accents, clearly separate from the high-value pastels. |
| [Adobe's psychedelic design discussion](https://blog.adobe.com/en/publish/2021/08/03/psych-out-design-is-funky-bold-trending-now) and [retro background examples](https://www.adobe.com/products/firefly/features/background-generator/groovy-background.html) | Seventies Print uses amber/orange, avocado and brown, with dusty rose. A warm, earthy graphic choice rather than another neon palette. |
| [British Museum on Hokusai's blues and print variations](https://www.britishmuseum.org/blog/great-wave-spot-difference) | Woodblock emphasizes indigo/Prussian-blue relationships and warm paper, with small vermilion accents. It is an interpretation, not a sampled historical print. |
| [Ink by AprilSundae](https://lospec.com/palette-list/ink) | Retain a deliberately limited value-study option. Ink takes the separate choice of neutral grays from black to white. |

These are references for our own curation, not claims that a genre has one
canonical palette. Ocean Study remains the subdued natural-painting option.
The range comes from different hue restrictions, contrast and proportions of
light/dark colors, as well as different swatch counts (11–17). Within each set,
colors remain adjacent by hue family or value. Do not force every set into the
same three dark-to-light ramps: that erased the differences between genres.

## Recommendation

Build one shared palette model with those two presentations. Start with fast
swatch selection, reliable saving and organization, then add image extraction
and generation from selected colors. Favor controls an artist can direct:
preserve these colors, add shadow/highlight steps, find a complementary accent,
or extract the colors of this reference.

The product comparisons below establish precedents, not measured user demand.
The proposed defaults and priorities are design judgments to validate with
artists using mouse, touch, and pen.

## What current tools establish

| Tool | Documented behavior | Implication for Capy Canvas |
| --- | --- | --- |
| [Procreate palettes](https://help.procreate.com/procreate/handbook/colors/colors-palettes) | Active palette below the color controls; compact and larger named cards; image/camera capture; palette sharing. | Keep frequently used colors close to the wheel, with a larger view available for names and touch targets. |
| [Krita Palette Docker](https://docs.krita.org/en/reference_manual/dockers/palette_docker.html) | Named swatches, groups, spatial organization, name/ID search, and several interchange formats. | Artists need intentional organization and import, beyond a flat collection of color chips. |
| [Clip Studio Color Set](https://help.clip-studio.com/en-us/manual_en/300_color/Color_Set_palette.htm) | Swatch grids and named lists, configurable tile density, duplication and import/export. Its [Intermediate](https://help.clip-studio.com/en-us/manual_en/300_color/Intermediate_Color_palette.htm) and [Approximate](https://help.clip-studio.com/en-us/manual_en/300_color/Approximate_Color_palette.htm) palettes provide blends and nearby variants. | Adjustable density and controlled color families are useful drawing features. |
| [Affinity Photo 2 Swatches](https://affinity.help/photo2/en-US.lproj/pages/Panels/swatchesPanel.html) | Separate recent colors, document/application palettes, and extraction from images with a color-count control. | Distinguish working history, reusable collections, and colors that travel with a document. |
| [Coolors generator](https://coolors-help.zendesk.com/hc/en-us/articles/360010581980-Generate-a-palette) | Lock chosen colors and regenerate the others; insert intermediate colors; adjust a palette together; simulate color-vision differences. | Generation should preserve decisions and support iteration. |
| [Adobe Fresco colors](https://helpx.adobe.com/fresco/desktop/draw-paint-animate-and-share/colors.html) | Automatic palettes from imported artwork and multicolor swatches usable by supported brushes. | Extraction is an established workflow; storing a brush's multicolor load is a separate, more ambitious capability. |

Some documentation is version-specific. In particular, Krita's current palette
manual marks document storage as disabled since 5.0; document palettes here are
grounded in Affinity's documented behavior, not a claim of current Krita support.

## Proposed presentation and everyday behavior

Paint and Photo get a dockable **Palettes** panel beside the Color panel, with
both visible in the default arrangement. Allow normal docking, floating and
tabbing afterward. Sketch gets the same swatch view beneath its wheel, initially
showing two or three rows. Let that section expand or scroll within the available
height while retaining access to the wheel. Opening the palette library or
generator is explicit; choosing a swatch leaves the drawer open for further use.

The palette section has a small, consistent structure:

```text
[Palette name / Library selector]       [+] [More]
Saved swatches, in the artist's chosen order
Recent colors, visually separated
[Generate…]
```

The plus action saves the current color immediately, with an automatic editable
name. Empty space is not an invisible destructive or editing action. The library
selector provides palette search, new/duplicate/rename, and import/export;
larger collections can expose swatch search and a named list view. Provide a few
small curated starter palettes, including a value scale, without crowding out
the user's own collections.

- A click/tap selects a color using the existing color-target rules, updates
  the wheel, and preserves the active drawing tool. Color alpha is part of the
  swatch; brush opacity remains a separate setting. Mask editing must follow
  the app's mask-color routing and preserve artwork colors.
- Saved positions remain stable. Sorting by hue/value or regrouping is an
  explicit, undoable action. A responsive grid may wrap but preserves sequence;
  named groups can later preserve ramp rows. Arbitrary empty grid cells are an
  optional advanced layout rather than a first-release requirement.
- Provide explicit Edit, Replace with Current Color, Rename, Duplicate, Remove,
  and Move actions. Swatch selection must never silently overwrite a swatch or
  recolor already-painted pixels.
- Recent colors record accepted color changes, with duplicates collapsed; wheel
  drag intermediates and picker hover previews do not fill the history. Pinning
  a recent color copies it to the saved palette. Clear recent colors separately.
- Offer comfortable touch targets and a compact density option. Keyboard focus,
  arrow navigation, activation, accessible color names, and a non-color-only
  selection marker belong in the initial implementation.
- Reorderable swatch tiles use **hold, then drag for mouse, touch and pen**.
  Before the hold, motion preserves scrolling. Touch/pen hold may open a menu;
  dragging closes it and release without dragging retains it. Mouse holds only
  arm reordering; secondary click opens the menu. Explicit handles and panel
  title/tab bars drag after movement slop without a hold. A completed move is
  one undo/redo action; cancellation restores the original order. These follow
  the [application convention](../ui/drag-and-reorder.md), regardless of other
  products' gestures. Provide keyboard/menu alternatives to dragging.

## Generation that helps drawing

Use one **Generate…** workflow with previews and explicit Save as New Palette
or Add Selected Colors. Generation never rewrites a saved palette in the
background, and its preview does not change the current paint.

| Mode | Proposed behavior | Priority |
| --- | --- | --- |
| From image or canvas | Import a reference, or choose the document/selection; set color count; choose dominant colors or a more varied result; manually pin important samples. | First generation milestone. |
| Shades and ramps | Select an anchor and create a chosen number of steps toward light/dark or another color; optionally shift shadows cooler and highlights warmer. | First generation milestone; particularly useful for painting. |
| Harmony | Analogous, complementary, split-complementary and triadic suggestions; lock chosen swatches and regenerate the rest. | First generation milestone. [Procreate Harmony](https://help.procreate.com/procreate/handbook/colors/colors-harmony) provides a familiar precedent. |
| Related colors | A small neighborhood around a fixed anchor, with controllable lightness/chroma variation. Selecting a candidate does not move the anchor until requested. | Follow-up; avoids a moving target while choosing colors. |
| From current painting | Extract a snapshot of colors from the latest document; offer explicit refresh and saving. | Follow-up; useful for recovering a working palette. |

For extraction, a large background should not consume every available swatch.
Offer a dominant/varied choice, remove near-duplicates, ignore fully transparent
pixels and allow an artist to preserve a small but important accent. Crop or
selection control is more predictable than promising automatic semantic
understanding of skin, foliage or materials. Show the source preview and count
before saving. Imported-image colors must be interpreted using their profile;
canvas extraction should use document color data before display/proof mapping.

For SDR ramps and color similarity, evaluate Oklab/OKLCH using the existing color
infrastructure. Its [author's explanation](https://bottosson.github.io/posts/oklab/)
describes smoother perceptual transitions and independent lightness/chroma
control. This is a proposed implementation choice: it does not guarantee artistic
quality, physically correct pigment mixing, or HDR perceptual uniformity. Keep
HDR intensity and gamut handling explicit, preserve tagged source definitions,
and qualify HDR generation separately. A display preview is never the stored
color. Where an export format cannot represent the palette, explain the
conversion and export a copy.

## More ambitious options

| Feature | Precedent and opportunity | Main tradeoff |
| --- | --- | --- |
| Palette roles and proportions | Let artists label main, supporting, accent, shadow and highlight colors; generate around those roles and show a weighted preview. This is our proposed extension of constrained generation. | Roles and intended proportions cannot reliably be inferred from a swatch list alone. Start with manual labels. |
| Limited-palette guidance | [Krita gamut masks](https://docs.krita.org/en/user_manual/gamut_masks.html) restrict artistic color choices. We could visualize the palette's allowed region on the wheel and suggest nearby colors. | Keep guidance optional; an artistic gamut mask is different from a display's reproducible gamut. |
| Mixing surface | [Clip Studio Color Mixing](https://help.clip-studio.com/en-us/manual_en/300_color/Color_Mixing_palette.htm) and [Rebelle's Mixing Palette](https://escapemotions.com/products/rebelle/manual/8/interface/panel-mixing-palette/) offer a surface to paint, mix and sample. | Requires a retained mixing surface, tools and history; pigment behavior also needs an appropriate mixing model. |
| Multicolor brush swatches | Fresco demonstrates sampling several colors into one brush load. Useful for foliage, hair and painterly strokes. | Changes brush data and rendering, beyond storing a single color. |
| Linked colors and palette variations | [Affinity global colors](https://affinity.help/photo2/en-US.lproj/pages/Clr/globalClr.html) update objects that reference a swatch. Could support character costumes, fills and alternate colorways. | Existing raster paint is not a swatch reference. Raster recoloring needs a separate mask/adjustment/remapping design and preview. |
| Personalized or prompt-based generation | [Khroma](https://www.khroma.co/) learns preferred colors; [Illustrator Generative Recolor](https://helpx.adobe.com/ca/illustrator/desktop/use-generative-ai/recolor-artwork-with-generative-recolor.html) explores recoloring from prompts. | Optional later mode. Evaluate consistency and artist control before adding model/service dependencies. Core extraction, ramps and harmony can run locally without AI. |
| Value and color-vision checks | Grayscale and simulated color-vision previews can help check separation; contrast checks are useful for text and graphic work. | A palette alone cannot establish readability or accessibility of a finished composition. Evaluate colors in their actual relationships. |

The most promising differentiator is a palette that helps construct coherent
color families: a handful of chosen anchors, useful value steps, and controlled
alternatives. Prompt generation is one possible entry point, not a prerequisite.

## Existing foundation and implementation boundaries

[The shared library](../../crates/layer-ui/src/color/library.rs) already has
stable IDs, palette/swatch names, tagged `RgbColor` definitions, validation,
create/rename/remove, save and use operations. Its current limits are 64 palettes
and 4096 swatches across the library. The [GTK library view](../../apps/layer-linux/src/color_library.rs)
is a management dialog; it closes when a saved color is chosen. This proposal
adds a persistent working view and new operations rather than another library.

The significant storage decision is that `ColorLibrary` currently lives inside
[ColorState](../../crates/layer-ui/src/color.rs), which is included in
[saved workspace working state](../../crates/layer-ui/src/workspace_session.rs).
Recommend **My Palettes** shared across workspaces, plus **Document Palettes**
embedded in the project. Workspace settings should remember the active palette,
view density and panel arrangement. Moving existing libraries must preserve
distinct same-named palettes, remap colliding IDs, and keep existing saves
readable. Application storage needs coordination across open windows; document
storage needs project persistence and document dirty/undo semantics.

Add a Palettes panel registration and update the
[default layouts](../../crates/layer-ui/src/layout_presets.rs) conservatively,
preserving customized workspaces. Reuse one shared presentation model in the
docked panel and Sketch drawer. Rust should own library mutations, generation,
drop validation and transaction history. Hosts own native timing/slop, capture,
file pickers and rendering. Extraction should run asynchronously with stale
results rejected when its source changes.

The implementation now includes recent colors, saved palettes, shared reorder
preview/history, and interchange with the priority applications: ACO and CLS for
Clip Studio Paint, Procreate `.swatches`, ACO for Photoshop, ASE and `.afpalette`
for Affinity, and GPL/KPL for Krita. See the
[implementation specification](color-palettes-panel.md) for its exact scope and
conversion rules. Further work includes replacement/duplication, bulk operations
and generation. Native Affinity and Clip Studio exports remain undone: both
applications document ASE or ACO import, and their native writers could not be
checked on those applications.

## Delivery options

| Scope | Includes | Assessment |
| --- | --- | --- |
| Essential | Panel/drawer, saved swatches, recents, management, reorder/undo, migration and basic import/export. | Smallest useful release and the foundation for every other option. |
| Artist toolkit | Essential plus image extraction, ramps, harmonies and lock/regenerate. | Recommended target, delivered as foundation then generation. Strong drawing value with local algorithms. |
| Extended color workflow | Roles, mixing, linked recoloring, multicolor brush loads, optional learned generation. | Several independent projects; prototype selectively after the core workflow is used. |

Validate the implementation with real drawing tasks: assembling a palette from
a reference, repeatedly choosing colors while painting, preserving a palette
across workspace/document switches, and reordering with each pointer device.
Cover cancellation and one-step undo, palette persistence, exact color-definition
round trips, mask routing, small-screen drawer fit and extraction latency.
