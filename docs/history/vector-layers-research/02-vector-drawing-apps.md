# Vector drawing / painting apps: research for Capy Canvas vector layers

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Scope: freehand vector brushes for inking and illustration, not bezier editors. Research date 2026-09-25.

Method note: Adobe's helpx.adobe.com (Fresco and Illustrator manuals) and docs.blender.org returned HTTP 403 to automated fetches. For Adobe I used community threads and search snippets of the help pages. The Illustrator tool behaviour below is established product behaviour and is marked **[K]**; verify it against https://helpx.adobe.com/illustrator/user-guide.html. The web-search quota ran out partway through. Linearity's current business status and Procreate's 2026 roadmap were not checked.

---

## 1. Clip Studio Paint (CSP): the reference for comic inking

### Data model
- A vector line is a **centerline path plus control points**. CSP records "the start point, end point, and curvature of each line", and the editable properties are **color, brush tip shape, thickness, opacity, and control point positions/types (curve vs corner)** ([CSP manual: Vector layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm)). Pen pressure data is stored with the line ([official tips #600](https://tips.clip-studio.com/en-us/articles/600)).
- The reverse-engineered `.clip` format (an SQLite DB with `VectorObjectList` blobs) confirms this. Each stroke holds **color, stroke opacity, and brush radius**. Each point holds **coordinates, a segment kind (straight/quadratic/cubic/spline), bezier controls, and per-point width and opacity *factors*** ([clipfile-rs](https://github.com/Aodaruma/clipfile-rs)). So each point's width is a multiplier on the stroke's base brush size, which is how the Object tool can rescale every line at once.
- **The brush is stored per stroke and re-rendered.** The Object tool exposes Brush size, Ink, Color jitter, Anti-aliasing, and **Brush shape (Brush tip, Spraying effect, Stroke, Texture)** plus **Starting and ending** (taper) for the selected lines ([Object tool guide](https://www.clip-studio.com/site/gd_en/csp/toolguide/csp_toolguide/200_category/200_category_object_vector.htm)). "Apply brush shape" swaps the brush of existing lines to any preset ([CSP ASK 16119](https://ask.clip-studio.com/en-us/detail?id=16119)). CSP therefore stamps its normal raster brush-tip engine along the vector path at render time, and **brush tips and textures replay**.
- **Curve type depends on correction settings.** Higher *Post correction* yields fewer control points. Post correction on gives quadratic bezier curves, and off gives spline curves ([Correction settings](https://help.clip-studio.com/en-us/manual_en/810_subtools/C.htm)).

### Drawing-time aids
- **Stabilization** (optionally adjusted by speed) and **Post correction**, which simplifies control points after the stroke ([C.htm](https://help.clip-studio.com/en-us/manual_en/810_subtools/C.htm)). **Sharp angles** keeps corners pointed instead of rounded.
- **Vector magnet**: while drawing, the new line snaps to and **merges with nearby existing line endpoints** into one line. It has 5 strength levels. Lines with a different color, brush, or anti-aliasing will not join ([Cheishiru guide](https://tips.clip-studio.com/en-us/articles/7586), [C.htm](https://help.clip-studio.com/en-us/manual_en/810_subtools/C.htm)).
- **Snap to control points** (View > Snap) aligns anchors and handles to neighbouring points and tangents ([Vector layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm)). 4.0 (Mar 2025) added **object snap** for vector layers, rulers, and text to grid, guides, and canvas edges/center ([4.0 release](https://www.clipstudio.net/en/news/202503/12_01/)).
- 5.0 (Mar 2026) added **Smart Shapes**, which draw lines, curves, and shapes without switching tools, and **velocity dynamics** for size and opacity ([5.0 release](https://www.clipstudio.net/en/news/202603/11_01/)).

### Vector eraser (the most-cited feature)
- **Erase touched area**: cuts the geometry where the eraser passes, splitting a line into two.
- **Erase up to intersection**: one tap or drag removes the segment between the nearest crossings with other lines.
- **Erase whole line**: deletes the entire stroke on touch ([Vector layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm)).
- **Refer all layers**: intersections can come from other vector layers ([E.htm](https://help.clip-studio.com/en-us/manual_en/810_subtools/E.htm)).
- With *Vector eraser* unchecked, or when painting with the transparent colour, CSP instead records **transparent vector lines**, which are non-destructive "erase strokes" ([Vector layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm)).

### Object / Operation tool
- Selects lines and shows their control points. It can move, scale, rotate, and change color, width (0.1–2000 px), opacity, anti-aliasing, and brush shape after drawing.
- A **"adjust line thickness" on transform** toggle decides whether scaling also scales stroke width ([Cheishiru](https://tips.clip-studio.com/en-us/articles/7586)).

### Correct line tools ([Vector layers § Correct line](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm))
- **Control point**, with 7 modes: move, add, delete, **switch corner**, **adjust line width** (drag left/right on a point), **adjust opacity** (same gesture), and **cut/split line** at a point.
- **Pinch vector line**: grab and drag a section of a line. Settings are Pinch level, Effect range, **Fix ends** (both / the further end / free), and Add control point ([pinch guide](http://www.clip-studio.com/site/gd_en/csp/toolguide/csp_toolguide/100_reference/Pinchline.htm)).
- **Simplify vector line**: 5 levels, plus *Smooth corner* and *Delete short lines* ([Cheishiru](https://tips.clip-studio.com/en-us/articles/7586)).
- **Connect vector line**: joins nearby endpoints after the fact, with 5 strengths.
- **Adjust line width**, which works like a brush over the lines. Modes are **Thicken / Narrow** (additive), **Scale up / Scale down** (multiplicative), and **Fix width**. Options are *Process whole line*, *Add control point and correct*, *Smoothening range*, and *At least 1 pixel* ([A.htm](https://help.clip-studio.com/en-us/manual_en/810_subtools/A.htm)).
- **Redraw vector line**: trace over a part of a line to replace it. Options are Fix end, All layers, Connect lines, and Simplify ([R.htm](https://help.clip-studio.com/en-us/manual_en/810_subtools/R.htm)).
- **Redraw vector line width**: trace along a line with pen pressure to re-record its width profile.

### Fill
- **Fill and Gradient do not work on vector layers.** Artists colour on a raster layer below and use *Refer other layers / Refer multiple* ([Vector layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm)).
- **Fill up to vector path / "Stop at vector center line"** fills to the centerline, so the fill tucks under the ink with no white halo. **Close gap** and **Area scaling** handle leaks ([Fill tool](https://help.clip-studio.com/en-us/manual_en/420_fill/Fill_Tool.htm), [CSP ASK 9816](https://ask.clip-studio.com/en-us/detail?id=9816)).

### Limits
- **Color mixing, Continuous spray, Watercolor boundary, and "Do not extend beyond reference layer line"** are disabled on vector layers ([CSP ASK 24872](https://ask.clip-studio.com/en-us/detail?id=24872)), as are watercolor edges ([tips 16882](https://tips.clip-studio.com/en-us/articles/16882)).
- **Patterned brush patterns shift after cuts**, and free transform shrinks the pattern rather than distorting the shape ([Cheishiru](https://tips.clip-studio.com/en-us/articles/7586)).
- Lines on multiple vector layers can't be adjusted at once.

### Export
- SVG export or "Copy vectors as SVG" writes **line shape only**. Pressure/taper, brush tip, anti-aliasing, and (per the manual) color and thickness are lost ([Vector layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm), [tips 3714](https://tips.clip-studio.com/en-us/articles/3714)).
- Users call the exported lines "very thick and buggy" in Illustrator ([ASK 94464](https://ask.clip-studio.com/en-us/detail?id=94464), [ASK 14719](https://ask.clip-studio.com/en-us/detail?id=14719)).
- SVG import is supported, and fills import as outlines.

### What artists say
- **Praise:** lossless resizing, changing the brush after drawing, the vector eraser, connect lines, and magnet ([doncorgi](https://doncorgi.com/blog/vector-layers-in-clip-studio-paint/), [tips 600](https://tips.clip-studio.com/en-us/articles/600)).
- **Complaints:** no fill on vector layers, weak export, texture limits, and lines that won't join because of mismatched properties.

---

## 2. Adobe Fresco
- **Three engines.** Pixel brushes, **Live brushes** (simulated oil and watercolor, raster), and **Vector brushes** ([Wikipedia](https://en.wikipedia.org/wiki/Adobe_Fresco)).
- **Status.** Fresco has been **free since Oct 2024** (v6.0) ([Digital Production](https://digitalproduction.com/2024/10/24/adobe-fresco-goes-free/)). Development is now iPad-first: iOS 7.x shipped in 2025–26, while Windows stalled at about 6.0 ([Wikipedia](https://en.wikipedia.org/wiki/Adobe_Fresco)).
- **Vector brushes.** The basic set is round, taper, flat, chisel, and terminal. Settings are size, roundness, angle, **taper**, **pressure dynamics**, **velocity dynamics**, and **smoothing**, plus later **Jitter** vector brushes for texture ([helpx vector-brushes, via search](https://helpx.adobe.com/fresco/using/vector-brushes.html)). Strokes are clean, tapered, and solid-coloured, with no texture replay.
- **Data model: filled outlines (Blob-Brush style).** An Adobe community expert: "Vector in Fresco has always created shapes rather than lines" ([thread](https://community.adobe.com/t5/fresco-discussions/vector-lines-turn-to-filled-compound-paths-from-fresco-to-illustrator/td-p/12534525)). In Illustrator, strokes arrive as **filled compound paths**, "a filled area between two paths". There is no centerline, so you cannot re-stroke or re-width them ([workflow thread](https://community.adobe.com/questions-646/is-a-vector-fresco-illustrator-workflow-possible-308280)).
  - Files carry **thousands of anchors**, and Illustrator's Simplify struggles or hangs on them ([thread](https://community.adobe.com/questions-646/problems-with-fresco-vector-drawings-and-simplify-tool-in-adobe-illustrator-306584)).
  - A 2021 release added "simplified vector strokes with fewer anchor points" ([2021 summary](https://helpx.adobe.com/si/fresco/using/whats-new/2021.html)).
- **Editing after drawing is minimal.** Users complain they cannot change thickness, color, or brush, or "adjust the curves after drawing" ([thread](https://community.adobe.com/t5/fresco-discussions/edit-vector-shape-in-fresco/td-p/14259077), [thread](https://community.adobe.com/t5/fresco/what-is-the-point-of-vector-brushes-in-fresco/m-p/11230497)). Edits are limited to selection transforms, "Load as selection" on vector layers, and editable shapes (2024) ([Fresco what's new](https://helpx.adobe.com/fresco/using/whats-new/2025-1.html)).
- **Vector eraser and vector trim.**
  - The eraser subtracts geometry from the shapes.
  - The **Vector trimmer** (the secondary state of the eraser) removes a segment between intersections with a single swipe, and **three scribbles delete the whole stroke** ([LinkedIn Learning](https://www.linkedin.com/learning/design-tools-weekly/vector-trimming-in-adobe-fresco)).
  - An early bug left hidden leftover paths that showed up in Illustrator; it was fixed in 2.7 ([thread](https://community.adobe.com/t5/fresco-discussions/fresco-issues-with-illustrator/td-p/11808189)).
- **Fill on vector layers** creates vector shapes. Fills leave **anti-aliased hairline gaps** against line art, and *Color margin* is disabled for vector ([thread](https://community.adobe.com/t5/fresco-discussions/fill-a-vector-shape/td-p/12650171), [bugs](https://community.adobe.com/questions-646/vector-layers-fill-bugs-305670)).
- **Interop.** Illustrator "Open a copy" keeps the shapes as vector; PSD export rasterizes. Users repeatedly ask for **true stroke paths** ([feature request](https://community.adobe.com/t5/fresco-discussions/feature-request-stroke-path-in-fresco/m-p/13465508)).
- **Verdict.** Beautiful, fast, and scalable. The trim gesture is excellent for touch. But it is "vector" only in the scalability sense, not in editability.

---

## 3. Concepts (TopHatch)
- **Infinite canvas, "every stroke is an editable vector."** You "can always edit your lines instead of undoing them" ([workspace](https://concepts.app/en/manual/workspace)). The current build is 2026.03.x.
- **Brushes** ([brushes & tools](https://concepts.app/en/manual/brushes-and-tools)):
  - **Pen** (velocity → width), **Fountain pen** (inverse), **Dynamic pen** (pressure), **Fixed width**, and **Wire** (constant screen width, for CAD and wireframes).
  - Soft and hard **pencils** (tilt, pressure, velocity), a **Marker** whose chisel follows stylus rotation, **Airbrush**, **Watercolor** (blends with consecutive strokes on the layer), **Fill**, and **Dotted**.
  - Textured brushes render **"raster on vector"**; Dotted is "a single vector stroke with raster dots rendered on top". So the model is a **centerline with input samples, rendered by a textured brush**.
- **Change after the fact**: select strokes, then pick a different **tool type, color, size, opacity, *or smoothing*** from the wheel. It edits the selection only ([tutorial](https://concepts.app/en/tutorials/select-edit-notes-drawings-designs/)). Re-smoothing and re-brushing existing strokes implies Concepts keeps raw input samples (pressure, tilt, velocity) and treats smoothing and brush as render parameters (inference).
- **Nudge**: pull a stroke "like a piece of string", or push it from outside with a circular nib whose size sets the falloff. It keeps brush properties. Filters cover locked items, all layers vs the active layer, and ignoring eraser strokes ([Nudge](https://concepts.app/en/tutorials/nudge-tool/)).
- **Slice**: a vector "eraser" that **cuts strokes into independent pieces**, or destroys the parts it sweeps over. Its puck size is adjustable, so the usual flow is slice, then select and delete the remainder ([brushes](https://concepts.app/en/manual/brushes-and-tools)).
  - **Hard and soft Masks** are non-destructive eraser *strokes*.
- **Selection transforms**: *Scale* (scales stroke width too) vs **Stretch** (keeps tool size), plus distort, skew, warp, group, lock, and duplicate ([selection](https://concepts.app/en/ios/manual/selection)).
- **Smoothing** runs from 0% (raw) to 100% (a straight line).
- **Precision** ([precision tools](https://concepts.app/en/manual/precision-tools)):
  - Grids, including perspective grids.
  - **Snap** to stroke endpoints and key points.
  - **Shape Guides** (line, arc, angle, ellipse, rectangle), as physical-style stencils.
  - **Shape recognition** (hold at the end of the last stroke).
  - Measure with real-world scale.
- **Export** is PNG, JPG, PSD, SVG, DXF, and PDF ([export](https://concepts.app/en/manual/export)).
  - SVG uses "a single line weight per stroke and very rough texture support", so users should draw with **Fixed Width/Wire** for vector output.
  - Mask strokes export as **white lines**.
  - Vector PDF loses textures.
- **Praise:** textured strokes that don't look vector, the edit-instead-of-undo loop, and the infinite canvas.
- **Complaints:** **no fill bucket**, no booleans, anti-aliasing quality, and a feature gap between iPad and Windows/Android ([Parka Blogs](https://www.parkablogs.com/picture/concepts-app-review-sketching-vector-and-infinite-canvas)).

---

## 4. Affinity (Designer → unified "Affinity by Canva", Oct 30 2025, free)
- One app with Vector, Pixel, and Layout studios. It is free, with optional Canva AI. The current release is 3.3 (Sep 2026) ([Wikipedia](https://en.wikipedia.org/wiki/Affinity_(software))).
- **New in 3.x: Vector Blob Brush** ("build new shapes from brush outlines… add to existing shape's geometry", with pressure) and **Vector Blob Erase**. It also has Shape Builder, Vector Flood Fill, Knife, and raster-to-curves conversion ([TechSpot](https://www.techspot.com/downloads/7804-canva-affinity.html)).
- **Vector Brush tool**: a **curve with a brush applied**. Brush types are solid, **textured intensity** (a greyscale PNG bitmap), and textured image ([2dgameartguru](https://2dgameartguru.com/vector-brush-creation-in-affinity-designer/)).
  - The raster texture is **stretched or repeated along the curve**, with head, body, and tail segments and corner handling.
  - Settings are width, *size variance*, *opacity variance*, controller (pressure/velocity), and a **ramp profile** editable by nodes ([modify strokes](https://s3-eu-west-1.amazonaws.com/affinity-docs/help/designer/English.lproj/pages/Painting/modifyStrokes.html)).
- **Pressure lives on the curve.** The Stroke panel's **Pressure graph** can be edited after drawing: drag or add nodes, save it as a profile, and apply it to any curve ([pressure](https://s3-eu-west-1.amazonaws.com/affinity-docs/help/designer/English.lproj/pages/Painting/pressure.html)). The nodes of the stroke's path stay fully editable ([Tuts+](https://design.tutsplus.com/tutorials/how-to-use-the-vector-brush-in-affinity-designer--cms-108818)).
- **Pencil tool** [K]: freehand curves with an optional fill, and it can extend or modify existing curves ("sculpt"). Pressure is captured as the same stroke pressure profile.
- **Pixel persona interplay** [K]: pixel painting goes onto pixel layers, which can be clipped inside vector shapes. Vector brush strokes stay vector (bitmap texture along a path) until you rasterize.
- **Weakness** [K]: one pressure profile per stroke, expressed as a graph over normalized length, is clumsy for localized width edits. Artists long asked for an Illustrator-style Width tool.

---

## 5. Adobe Illustrator: drawing tools [K]
Introduction dates: Blob Brush CS4 (2008), Bristle brush and Width tool CS5 (2010), Shaper CC 2015.2 ([Wikipedia](https://en.wikipedia.org/wiki/Adobe_Illustrator)).

- **Paintbrush**: draws a *path* with a brush applied (calligraphic, scatter, art, pattern, or bristle). Options are Fidelity (accurate ↔ smooth), Fill new strokes, Keep selected, and **Edit selected paths within N px** (redraw near a selected path to reshape it).
- **Pencil**: the same Fidelity option, *Close paths when ends are within N px*, **Edit selected paths** (redraw over part of a path to replace that part, like CSP's Redraw vector line), and *Option toggles Smooth tool*.
- **Blob Brush**: paints **filled outline shapes (no stroke)** and **auto-merges** with overlapping paths of the same fill and stacking. Options are *Keep selected*, *Merge only with selection*, and Fidelity. Size, angle, and roundness vary by **pressure, tilt, bearing, rotation, stylus wheel, or random**. Pairs with the **Eraser tool**, which cuts and closes filled shapes. This is the model Fresco copies.
- **Width tool (Shift+W)**: drag **width points** anywhere on a stroke, with asymmetric sides via Option-drag. The result can be saved as **Variable Width Profiles** and applied from the Stroke panel. Art and pattern brush widths can follow **tablet pressure**. It is the canonical after-the-fact "line weight" edit.
- **Brushes**:
  - **Calligraphic**: an elliptical nib whose diameter, angle, and roundness are driven by pressure, tilt, and bearing.
  - **Art**: vector artwork stretched along the path, with options to stretch, stretch between guides, or scale.
  - **Scatter**: copies of artwork placed along the path.
  - **Pattern**: tiles for side, inner and outer corners, start, and end.
  - **Bristle**: simulated bristles, output as many translucent paths, which makes it heavy to print and export.
  - All of these are *live*: change the brush on a path at any time, or use **Expand Appearance / Outline Stroke** to bake to fills.
- **Smooth tool**: drag along a path to reduce anchors and jitter.
- **Path Eraser**: drag along a path to remove that portion.
- **Eraser**: erases through filled shapes and strokes, splitting them.
- **Scissors and Knife** cut paths.
- **Join tool**: scribble over overlapping or open ends to join and trim them.
- **Shaper**: draw a rough rectangle, ellipse, polygon, or line and it becomes a live shape. Scribbling over overlapping shapes **deletes, merges, or punches** them, producing an editable Shaper Group.
- **Live Paint Bucket + Gap options**: colours regions formed by *intersecting paths* and auto-closes small, medium, or large gaps. It is the vector-native answer to "fill my line art".
- **Praise:** precision, the Width tool, live brushes, and the ecosystem.
- **Complaints:** freehand inking feels laggy and "CAD-like". Pressure only works with specific brushes. Blob brush files balloon. Users ask for CSP-style erase-to-intersection ([Animate thread](https://community.adobe.com/questions-540/erase-to-intersection-137768): brush strokes "are rendered as vector outlines rather than distinct line segments", so intersection deletion fails).

---

## 6. Linearity Curve (formerly Vectornator)
- **Brush tool**: "freeform paths with variable widths". It is pressure-sensitive (a Pressure toggle in the Brush Editor), and custom brushes have roundness, angle, and **contour/profile**. "Vector paths are endlessly editable at any time" ([brush tool](https://www.linearity.io/features/brush-tool/)).
- **Pencil tool**: freeform paths with a **smoothness slider that can be re-applied to a selected path**, which removes nodes. It can fill ([drawing tools guide](https://www.linearity.io/academy/curve/ipad/user-guide/vector-editing/drawing-tools)).
- **Model**: a centerline path plus a width/profile (Illustrator-like). It is design-focused, with no inking-specific cleanup tools. Auto Trace handles raster → vector.

---

## 7. Others and open-source precedent
- **Raster only:** Procreate, Procreate Dreams, Tayasui Sketches, Infinite Painter, Artstudio Pro, and Sketchbook (spun out to Sketchbook Inc. in 2021). They are useful only as UX references for QuickShape-style shape snapping and stabilizers [K].
- **Amadine** is a Mac/iPad bezier illustrator with pressure pencil and brush tools; **Vectorworks** is CAD. Neither is relevant.
- **PaintTool SAI "Linework layer"** [K]: a classic anime inking precedent, with centerline strokes and dedicated **Pressure** (edit per-point pressure) and **Weight** (whole-stroke width) tools, plus Edit, Curve, and Line tools.
- **OpenToonz (open source)**: "a *single vector stroke with a variable thickness*" per line. Tools are **Pinch**, **Control Point Editor**, and **Pump** (thickness). Brush options are Accuracy, Smooth, **Break** (split at sharp angles), and Pressure. **Tape tool closes gaps**, and **Fill paints regions formed by strokes** ([docs](https://opentoonz.readthedocs.io/en/latest/drawing_animation_levels.html)).
- **Inkscape PowerStroke LPE**: a centerline plus width knots, with the outline generated as a fill ([wiki](https://wiki.inkscape.org/wiki/PowerStroke)).
- **Krita vector layers** are SVG and can't use Freehand, Dynamic, or Multibrush tools. Painting brushes don't work on them, which shows what happens when vector layers don't reuse the brush engine ([docs](https://docs.krita.org/en/user_manual/vector_graphics.html)).
- **perfect-freehand** (MIT, with a Rust port): points plus pressure → **outline polygon** (size, thinning, smoothing, streamline, simulated pressure, tapers). It is the de facto web model, used by tldraw and Excalidraw ([GitHub](https://github.com/steveruizok/perfect-freehand)).
- **VectorStyler** added "erase up to intersection" as a Shift-eraser mode after a user request citing CSP ([forum](https://www.vectorstyler.com/forum/topic/182/tool-option-erase-up-to-intersection)). This shows demand beyond comics.

---

## Synthesis

### Two data models in the wild (plus a hybrid)

**(A) Stroke objects: centerline + per-point attributes + a brush reference, re-rendered.**
- Used by CSP, Concepts, OpenToonz, SAI, and Blender Grease Pencil.

**(B) Filled outline shapes: the outline polygon or bezier *is* the stroke.**
- Used by Fresco, Illustrator Blob Brush, Affinity Vector Blob Brush, Animate brush, and perfect-freehand.

**(Hybrid) Bezier path + width profile + applied brush (art/texture).**
- Used by Illustrator Paintbrush/Width, Affinity Vector Brush, Linearity, and Inkscape PowerStroke. It is A-like but designer-centric: one profile per path, and few inking cleanup tools.

| Concern | (A) Centerline stroke | (B) Filled outline |
|---|---|---|
| Change brush, size, colour, taper later | Trivial: re-render (CSP Object tool, Concepts wheel) | Impossible; Fresco users' #1 complaint |
| Width edits | Per-point width factors; Adjust line width, Redraw width | Only by boolean-painting more shape |
| Reshape (pinch, nudge, redraw, control points) | Natural; few points | Many anchors (thousands per Fresco file); reshaping distorts the width |
| Textured or raster brushes | **Replays the stamp engine along the path** (CSP, Concepts) | Solid colour only; texture needs a clip or mask |
| Erasing | Split the centerline at an arclength parameter; intersections are curve–curve tests, so **erase-to-intersection is easy** | Boolean subtraction; "intersection" is ill-defined because outlines overlap |
| Merging same-colour strokes | Needs explicit Connect or magnet | Automatic union (Blob brush): great for flat shapes and lettering |
| Fill bucket | Needs a reference fill, stopped at the centerline, plus gap closing | Shapes are already fills; fills against outlines leave AA seams (Fresco) |
| SVG/PDF export | Poor unless you *also* compute outlines; CSP exports centerline-only, which is why it looks "thick and buggy" | Exact visual match, but huge and uneditable in Illustrator |
| Determinism | Needs seeded jitter so edits don't re-randomize; CSP patterns shift after cuts | n/a |

**Recommendation for Capy Canvas: make (A) canonical and derive (B) on demand.**
- **Store per stroke:**
  - A spline centerline (Catmull-Rom or cubic, with a corner flag per point).
  - Per-point pressure-derived **width factor**, **opacity/flow factor**, and optionally tilt, azimuth, and timestamp, keeping the **raw samples** so smoothing can be re-run.
  - An immutable **brush preset snapshot**, colour, base size, start/end taper, and a **random seed**.
- **Render** with the existing GPU stamp engine: stamps are placed by arclength, and jitter and texture offsets are keyed to `(seed, arclength)`, so cutting or reshaping doesn't reshuffle texture.
- **Compute the outline polygon** (perfect-freehand style) for hit-testing, fill boundaries, SVG/PDF export, and an explicit **"Expand to shape"** command. That gives both CSP-grade editability and Illustrator-grade export.
- **Restrict "vector-safe" brush features**, as CSP does: no smudge, color mixing, wet edges, or continuous spray, because those depend on the canvas beneath.

### Ranked "killer" vector-drawing features cited by artists
1. **Erase up to intersection / vector trim.** CSP; Fresco trim; requested by Animate users; VectorStyler added it on request. The single most-cited reason inkers stay on CSP.
2. **Lossless resize/transform, with a "scale line width" toggle.** CSP, Concepts (Scale vs Stretch).
3. **Line-width adjustment after drawing.** CSP Adjust line width and control point width, Illustrator Width tool, Affinity pressure graph, SAI Weight/Pressure.
4. **Change brush, colour, or size of existing strokes.** CSP Apply brush shape, Concepts tool wheel. Its absence is Fresco's main criticism.
5. **Stabilization plus post-correction that yields few, clean control points.** CSP; Concepts re-smoothing after the fact.
6. **Direct reshape**: pinch/nudge and redraw-over. CSP Pinch/Redraw, Concepts Nudge, Illustrator Pencil "edit selected paths".
7. **Whole-stroke erase and cut-in-two (slice)**, via tap or scribble. CSP, Concepts, Fresco three-scribble.
8. **Vector magnet and connect lines.** CSP.
9. **Fill that respects vector line art**: stop at the centerline, close gaps, refer to other layers. CSP; Illustrator Live Paint; OpenToonz Tape.
10. **Control-point tool**: add, delete, move, corner, split, per-point width and opacity. CSP.
11. **Editable start/end taper.** CSP "Starting and ending", Fresco taper.
12. **Shape recognition and guides.** CSP 5.0 Smart Shapes, Concepts shape guides.
13. **Faithful export.** Weak everywhere, so an opportunity: export outlined fills that match the render, optionally with centerlines in a separate group.

### Recommended MVP vector drawing toolset
**Layer and data**
- A "Vector layer" type storing model (A).
- Rendering with the shared brush engine.
- Per-layer anti-aliasing.

**Tools (MVP)**
1. **Vector pen/brush**: any vector-safe preset. Options are stabilization (with adjust-by-speed), post-correction strength (fewer points), sharp corners, **vector magnet** (0–5), and start/end taper.
2. **Vector eraser**, three modes: **touched area** (split), **up to intersection** (with a *refer all vector layers* option), and **whole stroke**. On touch, scribble-to-delete could be a later gesture.
3. **Object/select tool**:
   - Tap and lasso selection.
   - Move, scale, rotate, flip, with a *scale line width* toggle.
   - Change colour, size, opacity, brush preset, and taper for the selection.
   - Delete, duplicate, and move to layer.
4. **Control point tool**: move, add, delete, toggle corner, and split, plus drag-to-adjust width and opacity at a point.
5. **Adjust line width** (brush-style): thicken, thin, scale, or fix width, with a *whole stroke* toggle.
6. **Pinch/Nudge**: a drag-to-reshape falloff brush with a fix-ends option.
7. **Connect lines** and **Simplify** (whole selection or layer, with a level).
8. **Fill integration**: the existing raster fill can reference vector layers, with **"stop at vector centerline"** and **close gap**, rather than vector-native fills at first.
9. **Export**: SVG and PDF as **outlined filled paths** matching the render. Textured brushes fall back to embedded raster, or are flagged. There is also an optional centerline-only SVG, plus PSD and PNG.

**Phase 2**
- Redraw vector line and Redraw width with pressure.
- Smart shapes and shape guides.
- Snap to control points and endpoints.
- A vector-native fill region layer (Live Paint / OpenToonz style) that fills regions formed by strokes.
- A Blob-brush-style "shape brush" for flat fills.
- "Expand stroke to shape" and booleans.
- SVG import (paths → strokes with default width).
- Raster → vector line conversion.

**Implementation cautions drawn from competitors**
- **Seed randomness by arclength.** It avoids CSP's pattern shift after cuts.
- **Keep raw input samples.** Concepts-style re-smoothing and brush swapping depend on them.
- **Don't let vector layers lose the brush engine.** Krita's vector layers are underused because of this.
- **Keep the point count low at draw time** with post-correction, because every editing tool and export scales with it. Fresco's thousands of anchors are the anti-pattern.
- **Define join rules explicitly.** Magnet and connect must decide how to reconcile different brushes and colours; CSP simply refuses to join lines that differ in colour, brush, or anti-aliasing.
