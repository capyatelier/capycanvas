# Paper

[Developer guide](README.md) · [Texture programs](texture-programs.md) · [Brushes](../internals/brushes.md)

Paper gives a drawing a surface. A Paper layer holds the sheet's color and its
texture; every brush that reacts to tooth reads the height of the paper under
the layer it paints on. Textures come from built-in
[texture programs](texture-programs.md) or from imported images, and a small
paper library lets artists choose, import and reuse them. Brushes own no grain
textures.

This guide covers what artists see, the resolution model, the layer, brushes,
the library, lighting, rendering, saved data and what each host has.

## Scope

| First version | Later |
| --- | --- |
| Paper layer with Color, Texture and Scale | Lighting the thickness of paint with the paper |
| Texture visibility, Light and Shows through paint, lighting the canvas and exports; Texture visibility in New drawing and its presets | Light and Shows through paint in New drawing |
| One resolution per drawing, 300 ppi by default | Texture parameters in Properties and saved variations |
| 38 built-in papers from eighteen programs | Portable user texture programs, shared with a portable custom-filter contract |
| Every grain brush follows the paper | Per-brush textures |
| Importing a paper image and an app-wide paper library | Paper that changes as it is drawn on: burnishing, sizing, absorbency |
| A brief preview of the paper on the canvas when it changes | Non-repeating full sheets and deckle edges |

## What artists see

1. **New drawing.** The dialog chooses the sheet: size, resolution, color, a
   paper texture or none with its Texture visibility, and the paper color or
   transparency ([New drawing](../ui/new-drawing.md)). The drawing starts with a
   Paper layer under an empty layer, its texture at its real size.
2. **Choose a paper.** The Paper row in Layers shows its color and, beside it,
   its texture. Tapping the texture, or the Texture tile in Properties, opens
   Choose Texture; Use Texture applies the chosen paper.
3. **Scale the paper.** Dragging Scale shades the paper's relief over the
   canvas, following the slider at once, so its scale can be judged against
   the drawing. The same shading shows for a moment after a paper is chosen.
   It never appears in the artwork or export.
4. **Light the paper.** Texture visibility raises the texture on the canvas and in
   exports, lit from the upper left. Light turns where the light comes from,
   and Shows through paint sets how much texture shows through the paint: at
   100% the whole picture is lit like paint on rough paper, at 0% paint hides
   the texture it covers. Light and Shows through paint are dimmed while
   Texture visibility is 0.
5. **Draw.** Pencils, charcoal, pastels and textured paint brushes catch the
   tooth. Texture strength in Tool Settings still sets how much a brush reacts.
   Bristol and hot press break a pencil line only lightly; rough breaks it
   everywhere.
6. **Import a paper.** Import… in Choose Texture accepts an image file, adds it
   to the library and applies it.
7. **Resize, crop, rotate or flip the image.** The tooth stays attached to the
   marks already drawn, and Scale reports its new value.

## Controls

| Component | Controls |
| --- | --- |
| New drawing | Resolution (ppi) beside Width and Height, kept in presets. Paper chooses the texture or No paper and its Texture visibility; Paper color chooses its color or Transparent. Light and Shows through paint start at their defaults. |
| Image Size | Unchanged. It shows 300 ppi for a drawing without a resolution. |
| Layer › New | Paper, beside Solid Color and Gradient Fill. The filter drawer lists Paper with the other fills, from the catalog. |
| Layers panel | Paper stays at the bottom, outside groups, without a drag handle. Its color thumbnail opens Edit Color. Its texture thumbnail sits left of the name and opens Choose Texture unless the layer is locked. Add mask in the footer and the row menu's Add mask, mask-from-selection and Paste mask items are disabled while Paper is selected. When Paper is missing, every client shows a centered Add paper action immediately after the last layer, inside the scrolling list. It uses the standard layer row's height and padding, with regular text, square corners and native hover, pressed and focus feedback. Native keyboard activation works on the focused action; releasing a pointer outside cancels. Adding Paper again selects the existing sheet. |
| Properties | Titled Paper. Color with Use Color; Texture, a tile showing the current paper with logical-group spacing between its thumbnail and name, which opens Choose Texture; Scale, a logarithmic slider from 25% to 400%; Texture visibility, a slider from 0 to 300%; Light, a [dial](../ui/numeric-controls.md) with its angle; Shows through paint, a slider from 0 to 100%. Double-clicking a label returns it to its default: Scale 100%, Texture visibility 0%, Light 225° (upper left) and Shows through paint 100%. Light and Shows through paint are disabled while Texture visibility is 0. |
| Choose Texture | A dialog with Edit Color's width, title and footer, tall enough to show three full rows of papers. One grid of squircle tiles, each preview at its own size with the paper's name below: the built-in papers, then imported papers. The selected Paper's own image comes first when it is not in the library, with Add to Library. Imported papers have Rename and Delete in the tile's menu. Choosing a tile only marks it; Use Texture applies it and Cancel keeps the current paper. Import… sits at the left of the footer. |
| Canvas | The lit relief while Texture visibility is above 0. On a flat paper, relief shading while Scale changes and for a moment after a paper is chosen. |
| Tool Settings | Texture strength, unchanged. Brushes have no texture picker or texture scale. |

Choosing a paper is one undo step, and so is each Scale, Texture visibility, Light or
Shows through paint drag. The paper
feature adds no preference. A Properties title shows only the layer name when
it matches the layer type, so Paper is not titled "Paper (Paper)" and a Solid
Color named Solid Color is not repeated either.

## Resolution and scale

Resolution is set in two places and paper scale in one, and each number means
the same thing in every drawing.

- **Resolution belongs to the drawing.** New drawing sets it, 300 ppi by
  default and kept in saved new-drawing presets, and Image Size changes it.
  Nothing else shows or asks for one.
- **Paper resolution.** Paper uses the drawing's resolution as written; a
  drawing without one counts as 300 ppi, which Image Size shows. The
  resolution only maps the paper's real size onto pixels, so a 144 ppi screen
  drawing gets the same tooth per inch as a 600 ppi print.
- **Texture resolution.** Programs define their features in millimeters. An
  imported image uses the resolution in its file, except that a missing one or
  one below 150 ppi counts as 300 ppi: cameras, screenshots and screen software
  write 72 or 96 ppi by default. A 600 dpi scan keeps its real size, and an
  image saved at 72 ppi maps one image pixel to one drawing pixel in a 300 ppi
  drawing.
- **Scale compares the paper with the real sheet.** 100% means the texture
  appears at its real size when the drawing prints at its paper resolution. A
  new drawing or Paper layer starts at 100%; double-clicking the Scale label
  returns to 100%; choosing another texture keeps the Scale.
- **Image commands keep the paper on the marks.** Crop, Image Size, rotation
  and flips move and scale the paper with the pixels, and Scale reports the
  result. Changing only the resolution changes no pixels; Scale then reports
  how the texture compares with real paper at the new print size.
- **Scale is one number.** After a resize with different horizontal and
  vertical factors, Scale shows their geometric mean, and changing it makes
  the paper uniform again. Image commands can take Scale beyond the slider's
  25–400%; the field shows the actual value.
- **Brushes have no texture scale,** and brush sizes stay in pixels.

| Action | Paper on the canvas | Scale shows |
| --- | --- | --- |
| New drawing at 300 ppi | Real size | 100% |
| New drawing at 72 ppi | Real size at 300 ppi | 100% |
| Open a photo saved at 72 ppi, or with no resolution | Real size at 300 ppi | 100% |
| Image Size to 200%, resolution unchanged | Grows with the marks | 200% |
| Resolution from 300 to 600 ppi only | Unchanged | 50% |
| Crop, rotate or flip | Moves with the marks | Unchanged |
| Copy a Paper from a 300 ppi drawing into a 600 ppi drawing | Same pixels | 50% |
| Set Scale to 100% after any of these | Real size at the paper resolution | 100% |

## Paper layer

### Model

Paper is a built-in fill program, `paper`, in the Fill category beside Solid
Color and Gradient Fill. It keeps the decision that made the default paper an
ordinary fill: opacity, blending, history, thumbnails and color editing come
from the general layer rules. Paper stays at the bottom of the root stack;
other layers drop above it. It cannot be masked, grouped or duplicated. Adding a sheet
when none exists inserts it at the bottom, regardless of the selected group.

| Value | Kind | Rule |
| --- | --- | --- |
| `color` | Color | Opaque. Declared as the program's constant color, so Paper keeps the startup background, clear folding and color thumbnails of Solid Color. |
| `texture` | Texture (new) | A built-in program with its values, or an imported image. Defaults to the paper last chosen in Choose Texture, or Drawing. |
| `relief` | Number, 0–300% | Texture visibility: how far the texture stands out on the canvas and in exports. 100% lights the paper's own relief as the brushes feel it. 0 leaves the paper flat. |
| `light` | Angle, 0–360° | Where the light comes from, in degrees clockwise from the drawing's +x axis; 225° is the upper left. It turns with the artwork. |
| `through_paint` | Number, 0–100% | How much texture shows through paint above the paper. |

Paper's spatial reference maps paper millimeters to drawing pixels, and it is
the only stored scale. Scale is a Paper property read from it: the mapping's
pixels per millimeter divided by the paper resolution's, and editing Scale
rewrites the mapping's scale about the document origin. Inserting a Paper sets
the scale to the paper resolution, with the paper's origin at the document
origin. Crop, Image Size, rotation and flips compose the mapping, as they do
for Gradient Fill
([canvas geometry](../../crates/layer-core/src/canvas_geometry.rs)), so the
tooth follows the marks. Image Rotate offers only quarter turns and flips, so
the baked tile always stays aligned with the pixel grid.

`Document::new` creates a white Paper. One scene query, the nearest Paper at or
below a target, finds the paper for
[document creation](../../crates/layer-ui/src/document_creation.rs),
[photo projects](../../crates/layer-color/src/photo_project.rs), whose Paper is
hidden, and the [GTK opener](../../apps/layer-linux/src/files/open.rs).

### Which paper a stroke uses

- **The nearest Paper at or below the target** in its stack, then in each
  enclosing stack.
- **Appearance does not change tooth.** Visibility, opacity, blending and
  clipping affect only how Paper looks. A Transparent background hides the
  Paper but keeps its tooth.
- **No Paper below** uses the default paper at 100% of the paper resolution.
- **The stroke keeps its paper.** It is resolved at pen-down and held with the
  stroke's view and brush settings, so queued contacts are unaffected by later
  edits.

### Restrictions

| Restriction | Status | Reason |
| --- | --- | --- |
| Paper draws a flat color | Adopted | Fill shaders never bind a texture and Paper keeps every constant-color shortcut. Relief is light applied to the finished picture, not paper pixels. |
| Paper color is opaque | Adopted | Transparency comes from visibility or layer opacity; the startup background stays exact. |
| Tooth ignores appearance | Adopted | Brushes read one texture per stroke and never a mask. |
| No offset or angle on Paper | Adopted | Position and orientation come from the spatial reference that image commands already update. |
| A selected Paper paints the layer above | Adopted | Paper and unmasked fills use the nearest artwork layer above them. If that layer is inside a group, there is no drawable target. |
| No masks on Paper | Adopted for now | Shared edit validation refuses any edit that gives Paper a mask, and the mask controls are disabled on its row. A masked sheet adds row layouts, drawing targets and mask cases that painters rarely need. A file that already has one keeps it until it is deleted. |
| One Paper per drawing, pinned to the bottom | Adopted | The sheet stays below the artwork. Shared edit validation protects its position through reorder, group, duplicate and paste. |
| Normal blend only | Rejected | Blend modes cost nothing; forbidding them adds code. |

Flatten and Merge Visible keep the bottom root Paper as a separate layer, as
Procreate keeps its background color; everything above it merges. Merging into
any other Paper bakes it into paint, as for any fill.

## Brushes

- **Every grain brush follows the paper.** `BrushGrain` in
  [`layer-core`](../../crates/layer-core/src/lib.rs) holds only Texture
  strength; brushes have no grain asset, scale, rotation, offset jitter or
  moving grain, and [`builtin_masks.rs`](../../crates/layer-render-wgpu/src/builtin_masks.rs)
  has no CPU paper generators. Watercolor transport fields and mask tips are
  separate from paper. Transparent Glaze has no grain.
- **Blenders touch the paper.** Paper-aware blenders apply the dry-media
  contact to how much they blend, so a light pass keeps the tooth and pressure
  fills it ([paper-aware blenders](../history/brushes/paper-aware-blenders.md)).
- **Relief scales tooth.** Dry media use
  `S = contact paper × Texture strength × relief / 100 µm` in the
  [contact model](../history/brushes/dry-media-contact.md); the multiplicative
  grain of wet and stamp brushes scales by the same factor.
- **Brushes read the paper in document pixels.** `paper_uv` in
  [`contact_field.wgsl`](../../crates/layer-render-wgpu/src/contact_field.wgsl)
  adds the target layer's offset, so moving a layer does not shift its tooth.
- **Calibration.** Fine tooth paper gives every dry-media brush the tooth contrast
  of the single built-in paper that preceded Paper, so a 300 ppi drawing keeps
  its pencil, charcoal, chalk, pastel and crayon character. The other papers
  were tuned against it with every dry-media brush at three pressures. Every
  paper has tooth finer than a pencil tip, shaped like its own surface, so
  none fills a contact solid at medium pressure; smooth papers fill sooner,
  rough ones later. Program
  heights are tooth depths: a pencil reaches about 30–50 µm below the paper's
  middle, so deeper valleys stay white at any pressure.
- **Previews.** Toolbar strength and slider previews read the active drawing's
  paper; their cache key gains the paper's bake key. Catalog swatches are
  rendered on the default paper.

Per-brush textures stay out of the first version. They would need non-numeric
tool overrides (`ToolMemory` stores only numbers per setting), library
references from brush settings and a fallback when a texture is deleted. The
Paper layer already gives every brush the surface a painter chose; a
deliberately stylized brush texture is the remaining case.

## Paper library

### Items and storage

| Item | Source | Actions |
| --- | --- | --- |
| Built-in paper | A catalog preset of a texture program | Use |
| Imported paper | An image, stored as gray samples with a resolution | Use, Rename, Delete |
| The selected Paper's image, when not in the library | An image embedded in the drawing | Use, Add to Library |

Imported images are stored once by SHA-256 of their samples in a `papers`
folder of the data root, named in [`StorageRoots`](../../crates/layer-host/src/storage.rs)
and listed in the [storage guide](../internals/storage.md), with their previews
and an index of names and order beside them
([`papers.rs`](../../crates/layer-host/src/papers.rs)). The library allows 64
imported papers and 256 MiB; papers over a limit stay listed and removable. The
image's identity in a drawing comes from the same hash, so a reopened drawing
still finds its paper in the library. Hosts that have paper storage report the
library to the session ([`paper_library.rs`](../../crates/layer-ui/src/paper_library.rs));
a host without it shows only the built-in papers and no Import.

A new Paper layer uses the paper last chosen in Choose Texture. That is
remembered with the other new-drawing state, not offered as a setting; an
imported paper is read when the library loads. If it was deleted, the default
paper is used. New drawing chooses its own paper in the dialog.

### Choose Texture

One shared view model, presented by each host in a dialog that shares Edit
Color's shell: a single grid with the
built-in papers in catalog order (watercolor, drawing, pastel and Asian papers,
canvases and panels, then surfaces such as Concrete, Barn wood and Toilet),
then imported papers, with Import… at the left of the footer. When the selected Paper
uses an embedded image that is not in the library, that paper comes first with
Add to Library. Tiles show 16 mm of each paper under raking light with its
name. Built-in previews are made from their bakes by a test and ship with the
app ([texture programs](texture-programs.md#validation-and-tests)); an imported
paper's preview is made from its image when it is imported. Hosts request
previews by paper id and keep them per id, since a paper's preview never
changes. Choosing a tile marks it; Use Texture applies it, and an imported
paper that is not in memory is read on a worker first. The open dialog follows
library changes and keeps the marked paper by id. Imported papers take their names from their files; Rename and
Delete sit in their context menu, as for palettes, and Add to Library sits in
the drawing's own paper's menu. There is no separate library panel.

### Importing a paper

Import runs on a worker and never on a UI thread.

1. Decode the image through the existing open path and convert it to
   luminance; light is high.
2. Read the resolution from the file under the
   [paper resolution rule](#resolution-and-scale).
3. Limit each side to 4096 samples with the jittered resampler, adjusting the
   recorded resolution so the physical size is unchanged.
4. Make the edges meet. When opposite edges differ, the first tenth of each
   axis is cross-faded with the band past its end in a variance-preserving
   blend (Heitz and Neyret's histogram-preserving blending operator, HPG 2018),
   which keeps the height histogram and so the pencil tone, and that band is
   dropped.
5. Store the result and apply it to the selected Paper.

Requiring seamless images would be simpler, but most scans are not, and a seam
repeats as a grid that pencil shading reveals.

### Drawings and the library

Choosing an imported paper embeds its image in the drawing, so files open
anywhere. A paper embedded in someone else's drawing shows first in Choose Texture
with Add to Library; adding it reuses an equal stored image.

## Rendering and performance

- **Composition is unaffected.** Paper composites as a constant color, so
  startup, composition and its cost are those of Solid Color. Lighting is
  applied after composition ([lighting](#lighting)).
- **Bakes run off the frame path.** The [bake](texture-programs.md#the-bake)
  is GPU work in 256 × 256 pages, scheduled at brush priority by the compiler
  path, never on the CPU or in a browser task. One bake advances at a time,
  the most urgent and then the most recently requested first, and a new
  request drops unstarted bakes of the same paper at its priority, so a Scale
  drag never queues a bake for every step. A step is submitted only after the
  previous one has completed, so a slow GPU never queues bake work ahead of
  frames.
- **Bakes are cached in memory** by program, version, values and tile size,
  shared between drawings and released when unused. A rebake follows a change
  of texture, Size, Image Size or orientation, not a crop or a resolution-only
  change. A disk cache is not needed: a bake costs a few milliseconds on a
  desktop GPU and is not expected to exceed the shader preparation it already
  waits behind on a tablet.
- **Strokes wait for their paper** as they wait for shaders. The first frame
  does not.
- **Brush cost follows tooth contrast.** Brushes read the paper once per pixel,
  with its tile size and relief in the per-batch uniform and no extra shader
  variant. Dry media on Fine tooth paper run as fast as before Paper; rougher
  papers cost them a few percent more per update
  ([top tier](../performance/top-tier.md#paper)).
- **The paper preview** shades the displayed canvas with the paper's slope
  tile in the presenter's own pass, as the [lighting](#lighting) does, from the
  upper left at a fixed strength and also over a transparent canvas, while
  Scale changes and for 1.2 s after a paper is chosen, then fades out
  ([`paper.rs`](../../crates/layer-ui/src/effects/paper.rs)). A lit paper shows
  its own relief instead. While Scale moves, it stretches the slope tile of the
  finished bake of the same paper and orientation nearest in size, scaling its
  slopes to the new size, so every slider step shows at once, and holds bakes
  of that paper so the drag spends no GPU time on them; only a paper without
  any bake is baked during the drag. On release the paper's own bake and slope
  replace the stretched one. The visible interval starts after the viewport
  has encoded the paper's own slope; waiting for the bake or its slope does
  not consume that interval, and GTK's render thread reports that moment to
  the session. Replacing the renderer resends the relief and preserves its
  remaining visible interval.
- **Memory** is one byte per texel: 2.3 MB for a 128 mm watercolor tile at
  300 ppi, 4.3 MB for a 175 mm laid or washi tile, and at most 16 MB.

Every change to brush sampling measures the dry-media rows against a matched
control by the [measuring rules](../performance/measuring.md). Bake time counts
toward canvas readiness in [responsiveness](../performance/responsiveness.md);
it is measured on desktop only.

## Lighting

Paper lights the finished picture, as a painting on textured paper is lit; the
height texture is the tooth the brushes feel, so what a pencil catches and what
the light shows agree.

- **Shade.** Each pixel is multiplied in linear light by
  `S = 1 − visibility · (slope · light direction)`, from the paper's slope at that
  point. Slopes average to zero over a sheet, so the paper keeps its tone and
  only turns lighter towards the light and darker away from it. Brightening
  stops at the brightest channel's limit, so highlights never shift hue
  ([`paper_shade.wgsl`](../../crates/layer-render-wgpu/src/paper_shade.wgsl)).
  The blend is fixed: a multiply in linear light is how a raised sheet under
  one light changes the color reflected from it.
- **Slope.** The baked height tile becomes a slope tile with a Sobel kernel in
  physical units, `relief × 100 µm` of height over the texel's width, and a
  wrapping RG16F mip chain ([`paper_slope.wgsl`](../../crates/layer-render-wgpu/src/paper_slope.wgsl)).
  It is built once per bake on first use. The canvas samples it with the
  screen footprint, so the relief averages out as the view zooms out instead
  of shimmering.
- **Shows through paint.** At 100%, or when a layer above the paper reads its
  color (any blend mode but Normal or Pass Through, an adjustment, or clipping
  to the paper), the whole picture is shaded. Below 100%, with only Normal
  layers above, the paper leaves the composite. The canvas adds it back under
  the paint where the composite blends, then shades with weight
  `(1 − α) + α · Shows through paint`, where α is the paint's coverage.
  Exports capture the paint above the paper a second time for α, and match the
  canvas.
- **Edits are cheap.** Texture visibility, Light and Shows through paint change no
  composited pixel, so dragging them only presents the canvas again. The one
  edit that moves the paper into or out of the composite, Shows through paint
  crossing 100% or Texture visibility crossing 0, recomposites once.
- **The scale preview gives way.** A lit paper already shows its relief, so
  changing Scale or the texture does not flash the preview over it.

## Saved data

| Record | Change |
| --- | --- |
| `paper` built-in effect, version 1 | Values `color`, `texture`, `relief`, `light` and `through_paint`. The spatial reference maps paper millimeters to drawing pixels and holds the scale shown as Scale. |
| Effect value kind `texture` | `{"kind": "texture", "value": {"builtin": "watercolor", "version": 1, "values": {...}}}` or `{"kind": "texture", "value": {"image": {"ref": "..."}}}`. |
| `capy.image/1` | An imported paper is a `gray` image with a required resolution, read as data without profile conversion, as the [package contract](../reference/capy-package.md) plans for image inputs. |
| `capy.composition/2` | New drawings write their resolution. |

Fixed-file tests cover both texture forms, and the frozen built-in contracts
gain `paper` and each texture program. Brush presets are not stored in
drawings, so removing grain fields changes no file. Drawings made before Paper
keep their Solid Color.

## Status

| Area | State |
| --- | --- |
| Shared Rust | Paper layer, texture value, eighteen programs and 38 papers, bake, brushes on Paper, resolution, Scale, relief preview, lighting with Texture visibility, Light and Shows through paint on the canvas and in exports, library policy, import |
| GTK | Choose Texture from Properties and the Paper row, with Import, Rename, Delete and Add to Library, paper, Texture visibility and paper color in New drawing, with an Import… tile adding to the library, relief preview, Texture visibility, the Light dial, Shows through paint and the Add paper row. `native_paper_lighting`, `native_paper_picker_relief_and_scale` and `native_fill_drawing_targets_and_paper_order` cover the lighting controls, the dialog, the row thumbnails, drawing targets, fill masks, Paper's disabled mask controls and sheet creation in both themes. `native_paper_scale_drag` drags Scale with real input on a 12 MP drawing in both themes and records how soon each step reaches the canvas. |
| Windows | Choose Texture from Properties and the Paper row, with Import, Rename, Delete and Add to Library, paper and paper color in New drawing, with Import… adding to the library, shared relief preview and lighting, the Light dial, and the Add paper row. Image decoding, preview decoding and library files use native workers. The Layers Paper fixture covers the rows, Choose Texture, the Scale relief and the lighting controls in both themes. |
| Apple | Choose Texture from Properties and the Paper row, with Import, Rename, Delete and Add to Library, paper, Texture visibility and paper color in New drawing, with an Import… tile adding to the library, shared relief preview and lighting, Texture visibility, the Light dial, Shows through paint and the Add paper row. The `light` case of the property slider fixture covers the dial. Native workers run image decoding and library files through the shared `layer_host::papers::PaperTask`. Shared Add Paper and remembered-paper cold-start journeys cover layer creation and initial library selection on Mac and iPad. |
| Android | Builds and paints on Paper, offers the Add paper row, and paper, Texture visibility and paper color in New drawing, with an Import… tile. The paper library lists at startup and runs Import, Load, Add to Library, Rename and Delete through the shared `layer_host::papers::PaperTask` on IO. Choose Texture opens from Properties and the Paper row, with Import, Rename, Delete and Add to Library. Properties has Texture visibility, the Light dial and Shows through paint. `paperLightingFollowsReliefTheLightDialAndPaintCoverage`, `paperLibraryImportsRenamesAndDeletesWithoutChangingTheDrawing`, the texture dialog, missing-Paper and fill-row journeys and `newDocumentCreatesTheChosenResolution` cover it; `paperScaleShadesTheCanvasWhileDragging` checks the relief while Scale is held and after release. |
| Web | Builds and paints on Paper, offers the Add paper row, and paper, Texture visibility and paper color in New drawing, with an Import… tile. The paper library lives in the raster worker's IndexedDB store, which decodes, converts and encodes papers off the page thread; the page lists it after the canvas starts and runs every Paper request through the shared session capture and adoption. Choose Texture opens from Properties and the Paper row, with Import, Rename, Delete and Add to Library. The shared renderer lights the canvas, and Properties has Texture visibility, the Light dial and Shows through paint. `--paper-library`, `--paper-rows` and `--blending` cover the library, Choose Texture, the Scale relief, the lighting controls, layer controls and Resolution. |

## Measurements

The program structure rests on sourced process descriptions and scales; most
relief depths, the honeycomb geometry, grit coverage and fiber densities are
estimates.

| Measure | Method | Sets |
| --- | --- | --- |
| Relief, bearing curve and skew of each surface | Profilometry or photometric stereo of sheets the team owns | Relief and shaping in each program |
| Felt spacing on hot press, cold press and rough from two or three brands | Spectra and nearest-neighbor statistics of those height maps | `felt` and its presets |
| Honeycomb pitch, row spacing and rim depth | Raking-light scan of Mi-Teintes | `honeycomb` |
| Laid and chain spacing on modern Ingres papers | Transmitted-light scan | `laid` |
| Grit standard, coverage and protrusion | Microscopy | `sanded` |
| Canvas relief before and after each gesso coat | Profilometry of primed swatches | `canvas` |
| Graphite, charcoal and pastel coverage against pressure | Scanned swatches at controlled loads | Relief reference and Texture strength |
| Tile size at which repetition shows | Side-by-side renders | Program periods |
| Bake time per tier | Performance rows | Page size |

## Decisions

A capability stays only when it improves performance, has a meaningful
artistic effect, or makes the app simpler to use. The texture language's own
choices are in [texture program decisions](texture-programs.md#decisions).

| Question | Choice | Simpler option and why it loses, or wins |
| --- | --- | --- |
| Layer type | A fill program | A dedicated Paper occurrence. It was removed once already; a fill reuses every layer rule. |
| Visible relief on the Paper layer | No | It would make Paper a textured generator, losing the constant-color shortcuts, and paint covers it anyway. The simpler option wins. |
| Where relief shows | Lighting of the finished picture, with Shows through paint | Shading only the Paper's own pixels: paint, even thin washes, would hide the paper's texture entirely, unlike paint on real paper. |
| Light control | A dial beside its value | A slider hides that 0° and 360° meet. One circle setting Texture visibility by radius and Light by angle hides the amount and is hard to set precisely. |
| Shadow strength | Texture visibility alone | A separate strength: in the slope model the shading is the relief, so two controls would change the same thing. |
| Lighting blend | Multiply in linear light, fixed | A blend-mode choice. Overlay and soft light change the sheet's tone and hue; a lit surface reflects in proportion to its light. |
| Name | Shows through paint | Texture on paint or Paint coverage: neither says what 0% and 100% do. |
| Name of the lighting amount | Texture visibility | Relief: painters did not read it as how much texture shows, or 0% as none. Texture strength already names how brushes catch the tooth. |
| Which paper a stroke uses | Nearest Paper at or below the target | One Paper per drawing needs pinning and refusals across every structural command. |
| Texture parameters in Properties | Presets only | The simpler option wins for now. Presets with Scale cover the papers artists choose between; exposing parameters adds controls, text and frozen ranges for every program. |
| Paper preview on the canvas | Brief relief shading when the paper or its Scale changes | Thumbnails alone: a thumbnail cannot show scale relative to the drawing, which is the judgement Scale exists for. Shading while Choose Texture is open: it needs hover to preview a tile, which touch screens lack, and keeps another state. |
| Scale model | One stored scale, read as Scale against the paper resolution | A scale value stored relative to the resolution when the Paper was made: after resampling to 200% it would still read 100%, and resetting it would do nothing. A scale tied live to the current resolution: changing only the resolution would rescale the tooth under the marks already drawn. A scale in pixels: the same paper would look different at every resolution and 100% could not mean real size. |
| Low resolutions | Drawings use their resolution as written; imported images below 150 ppi count as 300 | Counting drawings below 150 ppi as 300 too. It gave screen drawings, such as the 144 ppi Digital presets, tooth at twice its real size, and made the resolution mean two things. Image files keep the floor because their 72 or 96 ppi is a default, not a scan resolution, and would make imported textures four times too large. |
| Control name | Scale | Size. Next to a paper, Size reads as the sheet's dimensions; Scale is the word texture controls use in Procreate and Photoshop. |
| Scale range | 25–400% | A separate, wider typed range. Real papers near their own size need nothing beyond it, and image commands can still take Scale outside it. |
| Choosing a paper | Choose Texture, a dialog like Edit Color, opened from Properties and from the texture thumbnail on the Paper row | A popover that applies on tap, from Properties only. It was hard to discover, and Use Texture and Cancel match Edit Color. |
| Texture tiles | Relief under raking light | A graphite stroke across each tile. It needs a brush render per tile, while raking light already shows the texture and its depth. |
| Texture layout | One grid, with the drawing's own paper first only when it is not in the library | Separate sections for this drawing, built-in and imported papers. The drawing's paper is usually already in the grid, so the section repeats it. |
| Built-in papers | 38: the art papers, boards and canvases painters buy, and surfaces from wood, pavement, earth, plants and household paper with a history in frottage and street painting | Adding Bristol vellum, raw linen, parchment and velour. Bristol vellum sits between Drawing and Bristol with little visible difference; painters draw on primed linen rather than raw; parchment is mostly color and translucency, and velour's fibers are finer than a texel, so both wait for paper behavior that a height cannot carry. |
| Paper in New drawing | Texture and paper color in the dialog, saved with presets | The paper last chosen. Art-store presets are papers as much as sizes: a watercolor block, toned pastel paper or a chalkboard. |
| Library management | Rename and Delete in Choose Texture | A library panel: a handful of imported papers does not need one. Delete alone: imported names come from file names such as scan_0042.png. |
| Resolution controls | New drawing and Image Size | A resolution field on Paper or on import. New drawing and Image Size are where painting and photo apps put resolution, and a second field would let the paper disagree with the drawing. |
| App-wide library | Yes, small | Imports stored only in drawings. Every new drawing would need the file imported again, and new drawings could not start on a chosen paper. |
| Seamless import | Variance-preserving edge blend | Requiring tileable images. Most scans are not tileable, and the seam grid shows in every shaded area. |
| Brush textures | Every grain brush follows Paper | Keeping bundled grains per brush. That keeps five grain fields, the CPU generators and textures that ignore the chosen paper. |
| Per-brush textures | Later | They need typed overrides and library references for a minority case. |
| Disk cache for bakes | None | The simpler option wins while bakes stay within the shader preparation they wait behind. |
| Texture previews | Shipped for built-in papers, made at import for imported ones | Rendering previews on the GPU at run time. It needs a request and readback path on every host for images that never change between releases; a test keeps the shipped previews equal to their bakes. |
| Library index | A small index file beside the papers | The workspace database. Papers are app-wide, not per workspace, and the index keeps the folder self-contained. |
| Tile actions | Rename, Delete and Add to Library in each tile's menu | Buttons in the dialog. They would show for every paper while applying to one. |

Choose Texture names its grid with the dialog title for accessibility, and the
marked tile reports its selected state; the Paper row's texture thumbnail is
named Choose Texture and describes the current paper.
Paper parameter uploads and bake admission return GPU mapping errors through
preparation, so hosts can retire a lost renderer through the usual recovery path.
