# Vector layers: tool subset, stroke storage and new-artist journey

[Technical documentation](../README.md) · [Design and validation history](README.md)

Research date: **2026-09-25**. Source baseline: **`796f7f5b`**.

Research and proposed direction for future vector support. Recommendations and
phases below describe potential work, not implemented capabilities.

This report combines eight research streams. Their full write-ups, with every source URL, are in [`vector-layers-research/`](vector-layers-research/):

| # | Stream | File |
|---|---|---|
| 1 | Vector path editors (Illustrator, Inkscape, Affinity, Graphite, Figma, Linearity, CorelDRAW…) | [01-vector-editors.md](vector-layers-research/01-vector-editors.md) |
| 2 | Vector drawing apps (Clip Studio Paint, Fresco, Concepts, Affinity brushes, Illustrator drawing tools) | [02-vector-drawing-apps.md](vector-layers-research/02-vector-drawing-apps.md) |
| 3 | Animation, ink SDK and open-source stroke systems (Harmony, OpenToonz, Blender GP, PencilKit, Jetpack Ink, Rnote…) | [03-animation-and-ink-stroke-systems.md](vector-layers-research/03-animation-and-ink-stroke-systems.md) |
| 4 | File formats and the "replayable stroke" question (SVG, PDF, InkML, ISF, UIM, Jetpack Ink…) | [04-file-formats-and-replay.md](vector-layers-research/04-file-formats-and-replay.md) |
| 5 | Artist experience with vector drawing (tutorials, Reddit, CSP TIPS, pain points) | [05-artist-experience-drawing.md](vector-layers-research/05-artist-experience-drawing.md) |
| 6 | Artist experience with path editors (curricula, beginner struggles, makers/Cricut) | [06-artist-experience-path-editors.md](vector-layers-research/06-artist-experience-path-editors.md) |
| 7 | Rust ecosystem and algorithms (licenses, measured wasm sizes, complexity) | [07-rust-feasibility.md](vector-layers-research/07-rust-feasibility.md) |
| 8 | Capy Canvas codebase assessment (file:line references at `796f7f5b`) | [08-codebase-assessment.md](vector-layers-research/08-codebase-assessment.md) |

**Caveats on the evidence**
- **Web-search cap:** every stream ran out of web searches (200 per session) partway through, and later facts came from direct page fetches, crate sources and cloned repos.
- **Blocked Adobe and Inkscape manuals:** some behavior of Illustrator's and Affinity's tools, and some shortcuts, come from general product knowledge; the source files mark these **[K]** or "unverified".
- **Reddit coverage:** Reddit threads came from an archive API, so we saw post text and some top comments, not full discussions.
- **Counts and rankings:** the popularity figures and the wishlist are tallies across roughly 140 sources, not a survey.

**Relationship to the other vector research record.** [Vector drawing and editing research](vector-drawing-research.md) is an independent study of the same question, from the same day and the same source baseline. The two agree on:
- an editable centerline plus width profile as the stroke model, with immutable brush definitions embedded in the document;
- SVG with filled outlines as the exchange format;
- no usable cross-application standard for replaying brush strokes;
- trim, width correction, fill and easy selection before Bézier handles;
- the stale-documentation finding.

They differ on four decisions that should be settled together:

| Decision | This record | [vector-drawing-research.md](vector-drawing-research.md) |
|---|---|---|
| Brushes on vector layers | Every dry and contact brush, including textured ones, replayed by the existing stamp engine from the start; only canvas-reading brushes excluded | Start with three geometric brushes (monoline, pressure ink, chisel); keep textured and wet painting on raster layers |
| Authoritative stroke data | Recorded samples, until the user edits nodes; width edits stored as adjustments over the original pressure, so a brush swap re-derives width | Resolved width profile and fitted path; samples kept as optional provenance |
| Fallback for readers without the brush engine | The saved layer pixels; outlines computed on demand (optional solid-stroke outlines later, open question 8) | A saved portable appearance per object (filled paths, or images where required) |
| Text | Deferred to Phase 4 | Basic editable text before claiming logo and sticker coverage |

---

## 1. Executive summary

**Recommendation in one paragraph.** Build **one vector layer type** whose basic object is an **editable centerline stroke**:
- recorded pen samples, or a Bézier curve;
- a width profile;
- a paint, which is either one of our existing brushes (rendered by the stamp engine) or a solid color.

Add **closed shapes** as the second object type, for fills, merged shapes, imported SVG and cut files. Store the vectors as the source of truth and keep the layer's existing raster tiles as the render cache.

`.capy` files store **the original strokes, not filled outlines**:
- the recorded samples;
- a copy of each brush embedded in the file;
- width edits as a separate adjustment that leaves raw pressure untouched;
- an engine version per stroke.

Outlines are computed only for export, merge/subtract and hit-testing (section 7.2).

Ship the **drawing side first**, with the Clip Studio Paint (CSP) inking experience as the bar to meet. Then add path editing (curvature pen, node editing, merge/Shape Builder, outline stroke, offset), and join the two with conversion commands. Export SVG and PDF as filled outlines that any app can read. Embed our own stroke data under a private namespace so Capy Canvas can reopen and edit its own SVGs losslessly.

**Main findings**

1. **Vector means two different things to users.** Illustrators and comic artists mean *editable ink*. Designers, makers and print customers mean a *scalable file* (SVG/PDF for Cricut, laser cutters, logos). Most frustration comes from products that promise both and deliver one:
   - Fresco has scalable-looking lines that can't be edited and no SVG export.
   - CSP has great editable ink, but its SVG export drops line width.
   - Krita has SVG shapes that don't use its brush engine.

   No product we studied does both well. That is the opening for us.
2. **The editable-centerline model (CSP, Concepts, OpenToonz, SAI, Blender Grease Pencil) is the one that can serve both worlds.** Change the brush, width or color after drawing, erase up to an intersection, and reshape strokes. From that model you can still *compute* outlines for export and for merge/subtract. The filled-outline model used by Fresco and Illustrator's Blob Brush can't go back the other way. Fresco users' most common complaint is that they can't edit strokes after drawing them.
3. **The one feature artists name most is "erase up to intersection" / "vector trim".** It appeared in about 30 of roughly 140 sources and is the biggest "aha" in every popular inking tutorial. Next come editing strokes with the same brushes as raster (about 20), filling flats against the ink with no gaps (about 18), adjusting line width after drawing (about 16), and stabilization with hold-to-snap (about 14).
4. **Path editors all converge on the same core set**:
   - Select (V), node edit (A), pen (P) with click-for-corner and drag-for-curve, shape primitives, fill and stroke.
   - The four merge operations (union, subtract, intersect, exclude), clipping, align and snapping, a smoothed pencil, "outline stroke", and SVG.

   Courses teach **shapes and merging before the Bézier pen**: Adobe's own book has the Curvature tool in lesson 6 and the Pen tool in lesson 7. Shape Builder is the single most-loved feature. Bézier handles are the biggest beginner complaint.
5. **No widely used standard exists for replayable brush strokes.**
   - **W3C InkML (2011)** is the only vendor-neutral format and Microsoft Office uses it for ink, but its brush model is just a tip shape and no painting app reads it.
   - **Wacom's Universal Ink Model** and **Android's Jetpack Ink protobufs** can describe real brushes, but each is one vendor's format tied to one engine.
   - **Apple's PencilKit** format is opaque.

   Every painting app uses a private format, and Procreate stores no strokes at all. **SVG has no variable-width stroke, and nothing is on track to add one**, so everyone exports pressure strokes as filled outlines. The practical answer is our own format inside `.capy`, embedded in SVG for round-trip. InkML import/export is an optional extra for ink tools.
6. **Feasibility is good for drawing and moderate for path editing.**
   - **Drawing:** our engine can already rebuild any stroke from its raw samples, a frozen `BrushSnapshot` and a deterministic seed. It uses this only for the most recent stroke, then discards the data (`crates/layer-engine/src/brush.rs:270-279`, and the determinism tests at `canvas.rs:3093-3180`). Vector drawing is mostly persistence, re-rendering only the changed region, eraser geometry and UI.
   - **Path editing:** this needs new dependencies:
     - **kurbo** for geometry;
     - **i_overlay** and **linesweeper** for merge and subtract;
     - **vello_cpu** for filling shapes;
     - **usvg** for SVG import and **krilla** for PDF export.

     All are MIT/Apache. Together they add about 0.55 MB compressed to the web build, measured against 6.8 MB today; the PDF crates would load only on demand.
   - **Risks:**
     - merge/subtract on curves is still early-beta in Rust;
     - vello's GPU renderers need wgpu 29, but we vendor wgpu 30.0.1;
     - Android GPU cost if we re-render too much.

---

## 2. Two approaches to vector, and the data model that bridges them

### 2.1 Data models found in the wild

| Model | Who | Stored | Strengths | Weaknesses |
|---|---|---|---|---|
| **A. Centerline stroke** | CSP, Concepts, OpenToonz, SAI linework, Blender GP v3, Harmony pencil lines, PencilKit, Jetpack Ink, Wacom UIM | Centerline (samples or spline) + per-point width/opacity + brush reference + seed | Swap brush, width or color later; intersection-aware erasing; reshape; textures replay through the brush engine | Export needs computed outlines; merge/subtract needs outlining first; fills need gap logic |
| **B. Filled outline** | Fresco vector, Illustrator Blob Brush, Affinity 3 Vector Blob Brush, Animate brush, perfect-freehand (Excalidraw, tldraw) | The outline polygon *is* the stroke | Exact SVG/PDF export; merge/subtract and same-color welding are natural | No re-brushing or width edits; thousands of anchors; "intersection" is ill-defined; solid color only |
| **Hybrid: path + width profile + applied brush** | Illustrator Paintbrush/Width tool, Affinity Vector Brush, Linearity, Inkscape Power Stroke, Synfig | Bézier path + width knots (+ art brush) | Designer precision; editable width | One profile per path; few inking cleanup tools |

Two further variants:
- **"Record input, re-render"** (Jetpack Ink, Excalidraw, tldraw, Wacom UIM sensor data): stores raw pen input and re-renders it through a brush. This is model A with raw samples as the truth.
- **Fitted geometry with baked pressure** (PencilKit, OpenToonz, Blender, Rnote, Xournal++): edits geometry more easily, but bakes pressure into width, so swapping the brush can't re-derive the look.

**The best designs keep both**:
- **Wacom UIM** stores sensor data plus a spline, with a mapping between them.
- **Jetpack Ink** stores its inputs plus a cached mesh.
- **Inkscape** stores `original-d` plus Live Path Effect (LPE) parameters, and writes the computed `d`.

### 2.2 What that means for us

Model A is the right basic object, and it naturally covers the editor side too:
- **A pen-tool path is a centerline stroke** whose source is a Bézier curve instead of recorded samples. Its paint can be a solid stroke (the designer's case) or one of our brushes (CSP's "stroke path with brush").
- **A drawn stroke becomes an editable Bézier** by curve fitting (kurbo). Once the user edits its nodes, the fitted curve becomes the truth.
- **"Convert line to shape"** (Illustrator's Outline Stroke / Expand) turns any stroke into a closed **shape** using a variable-width outline. That feeds merge/subtract, Shape Builder, cut files and SVG.
- **Shapes** (closed regions with a fill and an optional outline) also come from shape tools, merge results, fills and SVG/PDF import.

This follows the Inkscape and Wacom pattern: editable source plus computed geometry. It is also the precedent that makes "best of both worlds" work, not two separate subsystems.

---

## 3. Landscape: what each kind of app offers

### 3.1 Path editors: the shared core

Source: stream 1, section 8, has the full 30-row matrix.

| Tier | Features |
|---|---|
| **Table stakes (MVP)** | Selection with bounding-box transforms; **pen** (click = corner, drag = smooth, Alt breaks the handle, Shift constrains, Space moves the point being placed, click the start point to close); **node tool** (move anchors and handles, marquee, add/delete, corner↔smooth); cut/join/close/reverse; rect (corner radius), ellipse, polygon, star; one fill + one stroke (caps, joins, dashes); linear/radial gradients; **union/subtract/intersect/exclude**; compound paths; clipping; align/distribute; snapping; smoothed pencil; **outline stroke**; SVG in/out |
| **Expected by intermediate users** | **Shape Builder** (added by Inkscape in 2023 and Figma in 2025, so now standard); offset path; simplify; **variable width** (pressure → editable width points, Width tool, profiles); **curvature-style pen** (Illustrator Curvature, Affinity Smart, Inkscape BSpline/Spiro); **drag a segment to bend it** (Figma Bend, Graphite mold); live corners; smart guides; knife and vector eraser; text on path; **image trace**; live (non-destructive) merge; region paint bucket (Illustrator Live Paint, Figma); brushes along a path; multiple fills/strokes; repeat |
| **Advanced / niche** | Mesh gradients, blend/morph, envelope/warp, full effect stacks (Inkscape LPEs, Illustrator Appearance), procedural node graph (Graphite), vector networks (Figma), CMYK/spot, generative AI |

**The 15 most-used operations**, inferred from shared shortcuts, Adobe's contextual task bar, the fixes Figma shipped and UserVoice requests; no vendor publishes telemetry:
1. Select / move / scale / rotate / duplicate
2. Node editing
3. Pen drawing
4. Corner ↔ smooth conversion
5. Rectangles and ellipses
6. Fill/stroke color and weight, eyedropper
7. **Union/subtract, or Shape Builder**
8. Align and distribute
9. Group and arrange
10. Add or delete points
11. Join, close and cut
12. Outline stroke / text to outlines
13. Clipping mask
14. Offset path
15. Pencil or brush, then simplify

**Shortcut conventions to copy** (Graphite and Vectorpea show that a new editor can adopt them):
- V select, A node, P pen, N pencil, Shift+C convert, Shift+M Shape Builder, Shift+W width, Ctrl+J join, Ctrl+7/8 clip and compound.
- Inkscape's Ctrl+( and Ctrl+) for inset/outset, and Ctrl+L to simplify.

**The most relevant precedents for a painting app**:
- **Affinity** (free since Oct 2025): Vector, Pixel and Layout studios work in one document, and its vector brushes stretch raster textures along curves.
- **Graphite** (Rust; kurbo + vello + linesweeper; MIT/Apache): components we can borrow with attribution, including its vector domain model, pen and path tools, offset and intersection code, and brush sample channels.

### 3.2 Vector drawing apps: the tools that matter

| App | Data model | What artists praise | What artists complain about |
|---|---|---|---|
| **Clip Studio Paint** | Centerline + control points with per-point width/opacity factors; brush stored per stroke and **re-rendered with the raster brush engine** (textures replay) | Vector eraser (touched area / **up to intersection** / whole line); Object tool swaps brush, size or color of selected lines; Correct Line Width (thicken, thin, scale, fix); Simplify, Pinch, Connect, Redraw line and width; control points (move, add, delete, corner, width, opacity, split); vector magnet; fill "up to vector centerline" + close gap on a raster layer referencing the ink | Normal eraser leaves **invisible transparent strokes**; fill and gradient disabled on vector layers; "pixelated / fake vector" (renders at canvas resolution); SVG export drops width, color and taper; no mixing or watercolor brushes on vector; texture shifts after cuts; lag with heavy hatching |
| **Adobe Fresco** | **Filled outlines** (Blob-Brush style) | Clean, tapered pressure lines drawn "without wrestling with the pen tool"; **Vector Trimming** swipe gesture | Can't change brush, width or color after drawing; thousands of anchors in Illustrator; no SVG export (PDF → Affinity workaround); hairline gaps between fill and line |
| **Concepts** | Centerline + raw input; raster-on-vector textured brushes | Change brush, color, size **or smoothing** of existing strokes; **Nudge** (pull a line like string); **Slice**; infinite canvas; shape guides; SVG/DXF/PDF export | No fill bucket; exports are one line weight per stroke with textures stripped; Android/Windows lag behind iPad |
| **Affinity** | Path + raster texture + one pressure graph per curve; new Vector Blob Brush (3.x) | Textured vector brushes; editable pressure profile; nodes stay editable | One pressure profile per stroke is clumsy for local edits |
| **Illustrator (drawing tools)** | Path + appearance (width points, brushes) | Width tool and profiles; Pencil "edit selected paths" (redraw over a path to replace it); Blob Brush auto-merge; Live Paint with gap options | Freehand feels laggy and "CAD-like"; no erase-to-intersection |
| **Harmony / OpenToonz / Blender GP** | Centerline + thickness (Harmony pencil lines, Toonz thick quadratics, GP radius); Harmony textured strokes are vector outlines with a raster mask | **Fills computed from the stroke graph** with auto-close gap tolerance (Harmony 0–10 px, Toonz autoclose, Blender 5.2 Delaunay fill); invisible gap-closing strokes; fill placed *under* the line art | Animation-centric UIs |
| **Krita vector layers** | SVG shapes; the brush engine is **not** used | Nothing notable | "Useless for most line art"; no pressure; a wish-bug for about 10 years. The anti-pattern to avoid. |

**Killer features ranked by how often artists cite them** (streams 2 and 5):
1. Erase up to intersection / trim
2. Draw with the same brushes as raster, and keep strokes editable
3. Fill flats against ink with no gaps
4. Adjust line width after drawing
5. Stabilization plus hold-to-snap
6. Resize without quality loss (with a "scale line width" toggle)
7. Swap brush, size or color of existing strokes, one at a time or all at once
8. Grab to reshape (pinch/nudge/bend)
9. Real SVG/PDF export that keeps width and fills
10. Keep a hand-drawn, textured look
11. An eraser that behaves intuitively
12. Curve tool
13. Simplify
14. Performance with thousands of hatching strokes
15. Comic tooling (panels, balloons, speed lines)
16. Connect/magnet, off by default

---

## 4. How artists actually work (the journeys we need to support)

### 4.1 Canonical workflows

**Illustration or comic inking** (CSP; the most common vector workflow among artists):
1. Rough sketch on a raster layer at low opacity or tinted blue.
2. **Ink on a vector layer** with a G-pen and pressure on. Stabilization is typically 20–30, or up to 100 for shaky hands.
3. **Vector eraser, "up to intersection"**, to clean overlaps. Artists draw past crossings on purpose, then zap the overshoots.
4. Refine: correct line width, simplify, pinch, control points.
5. Optionally **swap the brush for the whole layer**. Tutorial comments call this out: *"GLOBALLY CHANGE BRUSH ON VECTOR LAYER OMG"*.
6. **Flats on a raster layer below**: mark the ink as a reference layer, then fill with close-gap and "fill up to the vector centerline".
7. Shade on raster (Multiply clipped to flats; highlights on an Add Glow layer). Color the lines with a clipped layer above the ink.
8. Comics add vector **panel borders**, vector **balloons**, and **speed/focus lines** from rulers. Blacks are spotted on raster.

**Scalable line art for merch, stickers and Cricut** (Fresco; the most common route for hobby and freelance work):
1. Sketch with a pixel brush.
2. Ink with a vector brush, pressure off for monoline.
3. **Trim** the overshoots.
4. Vector bucket fill.
5. Export PDF, then use Affinity or Illustrator to get SVG, because Fresco has no SVG export.

**Painter to vector, the multi-app workaround** (Procreate and ibisPaint users):
1. PNG.
2. Image Trace in Illustrator, Vectorizer.AI or Cricut's converter.
3. Clean up nodes and gaps.

Tutorials for this reach 180K–370K views. The complaints: *"522 nodes where a clean version… is about 59"*, *"jagged and awkward"*, double outlines around single lines, and gaps that block fills.

**Maker / cut files** (the largest hobby population; Cricut reported about 5.9M active users at the end of 2025):
- Closed, **welded** shapes with few nodes.
- Text converted to outlines.
- An **offset border** for stickers.
- No overlapping or double lines.
- Correct stroke-vs-fill semantics per machine: Cricut cuts a stroke-only path twice; LightBurn maps stroke color to cut layers.
- SVG/DXF/PDF output in physical units.

**Logo from a sketch** (freelancers):
1. Place the sketch.
2. Rebuild it from shapes plus Shape Builder, or trace with the pen/curvature tool.
3. Convert text to outlines.
4. SVG/PDF.

**Design sketching** (Concepts: industrial design, architecture):
1. Infinite canvas with grids.
2. Pressure or "wire" pens.
3. Nudge and Slice.
4. DXF/PDF.

### 4.2 The learning curriculum people actually follow

**Vector drawing** (from the 20+ most-viewed tutorials, including CSP's official 1.32M-view video, Loading Artist at 585K, and Piascik/Fresco at 274K):
1. Vector vs raster ("won't lose quality when resized").
2. Make a vector layer and ink over a sketch.
3. **Erase/trim to intersection.**
4. Thicken, thin and simplify.
5. Reshape by grabbing.
6. Swap brush or color of existing lines.
7. Fill on a layer that references the ink.
8. Export (taught late).

**Path editing** (Adobe *Classroom in a Book*, Domestika, Logos By Nick, Affinity Revolution):
1. Select and transform.
2. **Shapes, with a shapes-only mini project** (logo, mountain, badge).
3. Fill vs stroke.
4. **Merge / Shape Builder.**
5. Align and snapping.
6. **Curvature/pencil, then pen.**
7. Node editing and corners.
8. Width and outline stroke.
9. Groups, clipping, holes.
10. Text to outlines.
11. Image trace.
12. Export.

**The lesson from both curricula:** artists get a satisfying result *before* they touch a Bézier handle.

### 4.3 What breaks adoption (with representative quotes)

- **The feature is hard to find.** On CSP's official video, *"I've been using Clip Studio Paint like an idiot"* has 1.2K likes and *"…nearly a year without having this information?"* has 1.2K more. Vector hides in a layer-type menu under a jargon name.
- **The first erase or fill goes wrong.**
  - *"PSA: Do NOT use the normal eraser with your vector drawings"*: in CSP it draws invisible transparent vector lines.
  - Fill is disabled on CSP vector layers.
  - Fresco vector fills leave hairline gaps: *"needs an Expand by 1-5 pixels and call it 'Overfill'"*.
  - Many beginners give up on vector right there.
- **"Why are my vector lines pixelated?"** CSP renders at canvas resolution, which leads to *"fake vector?"* threads.
- **Lines look too clean.** *"it feels a bit sterile"*, *"lose the uniqueness of a brush tip"*.
- **Performance** with hatching, and file bloat from excess control points.
- **Control points are fiddly.** *"easier to redraw a line than vector it into place"*.
- **Path editors** have their own traps:
  - Bézier handles (*"literally doing my head in, HEEEEELP!"*).
  - Merge that fails silently on groups, text or line-only objects (*"One object is not a path"*).
  - Stroke vs fill vs "expand".
  - Strokes that don't scale with the object.
  - Open paths filling haphazardly.
  - Holes in letters like B and O.
  - Traces that produce hundreds of thousands of nodes.
  - Hidden "inside a group" states.

**What makes adoption stick** (the "aha" moments):
- An overlap disappears in one swipe: *"where has this been my whole life???"* (396 likes), *"so fun to zap those lines"*.
- Re-skinning finished ink.
- Thickening a line without redrawing it.
- Editable ink as an **accessibility aid** for shaky hands and carpal tunnel: correct instead of redraw.
- Drawing "like Procreate" and getting vector out: *"I can't tell you how many Google searches I've done looking for a program that allows me to draw like I can on Procreate, but in vector form"*.

### 4.4 Positioning

- **Linux:** CSP has no Linux build, and Krita's vector layers are weak for ink. "Pressure-sensitive, editable ink on Linux" fills a known gap. Krita-Artists threads show demand from artists who left CSP.
- **Everywhere else:** no app offers *editable pressure ink* together with *clean SVG/cut-file output* in one free, cross-platform tool.

---

## 5. File formats and the "replay" question

### 5.1 Is there a standard for replayable brush strokes?

**No.** The options:

| Format | Status | Can it describe a painting brush? | Who reads it |
|---|---|---|---|
| **W3C InkML** | Recommendation (2011) | No: ellipse/rectangle tip, color, width, transparency | MS Office ink, OneNote API, handwriting datasets |
| **Microsoft ISF** | Published spec (Open Specification Promise) | No (tip, size, fit-to-curve) | Windows Ink (as GIF with embedded ISF) |
| **Wacom Universal Ink Model 3.1** | Vendor spec (RIFF + protobuf); library Apache-2.0; no ISO/W3C status found | **Yes**: raster brushes with shape/fill textures, spacing, seed | Wacom ecosystem |
| **Jetpack Ink (`androidx.ink`) protos** | Open source (google/ink, Apache-2.0); BrushFamily serialization stable since 1.1.0-alpha02 | **Yes**: behavior graph, textures, `min_version` + fallback families | Android apps using Jetpack Ink |
| **PencilKit `PKDrawing`** | Opaque; readable only through Apple's API | Yes (Apple's inks) | Apple platforms |
| App formats (`.clip`, `.xopp`, `.rnote`, Excalidraw, tldraw) | Private | Engine-specific | Only their own app |

- **Replay needs the brush engine too.** Replaying samples reproduces a look only with identical engine behavior: spacing, dynamics curves, jitter RNG, blending.
- **Every "real" brush format is tied to one engine**: ABR, Krita `.kpp`, Procreate `.brush`, CSP `.sut`. MyPaint's `.myb` with libmypaint (ISC license) is the only portable engine-plus-format pair.
- **What "replay elsewhere" can realistically mean:**
  1. static geometry (SVG/PDF outlines);
  2. media (time-lapse video, animated SVG draw-on);
  3. ink data for ink tools (InkML);
  4. exact re-editing only in Capy Canvas, from our own data embedded in SVG/PDF.

### 5.2 What's safe in SVG

- **Interoperable SVG:**
  - Use SVG 1.1 syntax with presentation attributes.
  - Safe features: paths, groups with transforms, fill/stroke/opacity, caps/joins/dashes, linear and radial gradients, embedded PNG, and `viewBox` with physical units.
  - Avoid `<style>` blocks: Cricut and Silhouette ignore them.
  - Mesh gradients and hatches were removed from SVG 2.
  - Blend modes, filters and masks are patchy outside browsers.
- **No variable-width stroke** exists or is proposed (W3C ISSUE-2271, open since 2009).
- **Inkscape's pattern:** Live Path Effects keep `inkscape:original-d` plus `<inkscape:path-effect>` parameters, and write the computed outline into `d`. Unknown namespaces are preserved; Ink/Stitch depends on this.
- **The Inkscape pattern to avoid:** regenerating `d` on re-edit can overwrite changes other apps made.
- **Makers need explicit semantics:**
  - centerline strokes for plotting, scoring and running stitch;
  - clean filled outlines for vinyl and engraving;
  - no clip, mask or style;
  - physical units.

---

## 6. Feasibility in Capy Canvas

### 6.1 What we already have (verified against the code)

- **Deterministic stroke replay.** `DabGenerator::generate(&Stroke, …)` (`crates/layer-engine/src/brush.rs:270-279`) rebuilds dabs from raw samples plus a `BrushSnapshot`.
  - Samples store position, mapped pressure, tilt, twist and time.
  - Randomness is `mix_seed(brush.seed, stroke_id)`: integer hashes, exact on every platform.
  - Tests assert live dabs equal replayed dabs on real Wacom captures (`canvas.rs:3093-3180`).
  - It is used only to re-render the latest stroke, then discarded. `Stroke` is documented as *"Never part of document persistence or undo history"* (`crates/layer-core/src/lib.rs:1202-1222`).
- **A pipeline for generated content.** Figures and fills are queued as `pending_operations`, rendered by the GPU and read back into immutable tile revisions (`canvas.rs:768-830`). A vector-layer rebuild can take the same route and get caching, undo, saving and GPU recovery for free.
- **A file format with room for it.** `.capy` stores JSON metadata plus deduplicated LZ4 blobs, and `SelectionIndex` already pulls binary data out of the JSON. A `VectorIndex` would copy that pattern (`crates/layer-core/src/project_storage.rs`).
- **A fill tool that already does most of what CSP-style flats need.** The Region fill supports **Reference layers**, **Close gaps** and **expansion** (`crates/layer-ui/src/region_tools.rs:46-100`, `tools.rs:438-441`).
- **Other reusable pieces:**
  - Figure tools (line/rect/ellipse, `crates/layer-core/src/figures.rs`), which could become live vector shapes.
  - Selection contours with a GPU even/odd filler.
  - Screen-resolution overlay contracts for handles and path previews.
  - `Arc`-shared history.
  - Real pen-capture fixtures in `artifacts/strokes` and `artifacts/pen-traces`.

### 6.2 What's missing

- **Geometry:** no curve geometry anywhere in `Cargo.lock` (no kurbo, lyon, usvg, vello). No path renderer with good anti-aliasing and nonzero winding; the selection filler is even/odd only, with 4 samples.
- **Model and tools:** no vector object model or layer kind, and no path tools.
- **Formats:** no SVG or PDF import/export, and no text stack.
- **Versioning:** no engine or renderer version tag on strokes.
- **Display resolution:** display is capped at document resolution, because composition runs in document space. Crisp vector display at any zoom would need view-space composition, which is a deep change (see open question 2).
- **Stale docs:** the README (`README.md:149-151`), `docs/internals/documents.md` and `docs/reference/gpu-brush-engine.md:262` say strokes are persisted and replayed. The code says otherwise. Fix these before designing on top of them.

### 6.3 Recommended technology stack (all allowed by `deny.toml`)

| Role | Crate | License | Maturity | wasm size (gzip)¹ |
|---|---|---|---|---|
| Geometry: fitting, stroking, offset, simplify, nearest, arc length | **kurbo 0.13** | MIT/Apache | Stable; Graphite, vello and usvg use it | 36 KB |
| Spatial index | **rstar 0.13** | MIT/Apache | Stable | 14 KB |
| Robust polygon ops, clip/slice, **variable-width outline** | **i_overlay 9.0** | MIT/Apache | Stable core, integer-deterministic; variable stroke is **6 days old** | 61 KB |
| Merge/subtract on curves | **linesweeper 0.4** | MIT/Apache | **Early beta**; GitHub archived (moved to Radicle); Graphite uses it but reported crashes | 74 KB |
| Fill rendering of shapes and paths | **vello_cpu 0.2** (then vello_hybrid) | MIT/Apache | Most mature vello renderer; no wgpu coupling | 126 KB with simd128 |
| SVG import | **usvg 0.48** (no text at first) | MIT/Apache | Mature | 158 KB |
| PDF export | **krilla 0.8** (lazy-loaded on web) | MIT/Apache | Typst's backend; PDF/A | 416 KB |
| PDF/AI import (later) | hayro 0.7 | MIT/Apache | Early; returns kurbo paths | 1.38 MB (lazy) |
| Image tracing (later) | visioncortex 0.9 (vtracer core) | MIT/Apache | Outline tracing only | — |
| Gap closing and region code to port | OpenToonz `tl2lautocloser`, `tcomputeregions` | BSD-3 | Reference implementation | — |

¹ Measured in isolated wasm size-test builds (opt-level z, LTO, stripped). The core set (kurbo + i_overlay + linesweeper + vello_cpu + usvg + rstar) is about **554 KB gzip**. The web build doesn't enable `simd128` today; enabling it more than halves vello_cpu.

**Don't link or copy:**
- potrace, autotrace, Rnote, Krita, Inkscape and Blender (GPL);
- lib2geom (LGPL/MPL);
- mupdf (AGPL);
- clipper2-rust (BSL-1.0, not on the allowlist).

Rnote (a Rust ink app) is the closest architecture to study, but GPL rules out reusing its code.

### 6.4 Rendering strategy

- **Textured and brush strokes:** replay through the existing contact-placement and stamp engine, clipped to the damaged region. Split pieces keep the parent seed and an **arc-length phase**, so texture and jitter don't shift after an erase (CSP gets this wrong). PencilKit (`renderState.grainOffset`) and Wacom UIM (`ts/tf`) do this.
- **Solid pressure strokes** (G-pen, monoline): optionally one analytic **tapered-capsule SDF quad per segment**, MAX-blended into the per-stroke coverage buffer the engine already keeps. That uses far fewer primitives than dabs.
- **Shapes and filled paths:** vello_cpu into dirty tiles first. vello_hybrid comes later, once the wgpu 29 vs 30 mismatch is resolved.
- **Cache:** the layer's raster pages. Invalidate by `old ∪ new bounds` through the R-tree and re-render only intersecting objects in z-order. During drags, freeze the "below" and "above" layers and render only the moving selection.
- **Persist the raster cache in `.capy`.** Files then open fast and **look identical even after brush-engine changes**. Only regions the user edits are re-rendered with the current engine. This is our answer to appearance drift (Blender 4.5 shipped an unversioned change that alters old files). Also tag each stroke with an `engine_version`. Section 7.2 covers the full storage policy.
- **Exclude brushes that read the canvas** (smudge, wet mix, watercolor, liquify). They depend on stroke order and existing pixels. CSP excludes them too.

### 6.5 Main risks

| Risk | Mitigation |
|---|---|
| Merge/subtract robustness on real artwork | Put booleans behind a trait with two backends: linesweeper, and flatten → i_overlay (integer) → refit with kurbo as the default and fallback. Fuzz both, apply time and size limits, never run on the input path, and store results in history instead of recomputing them |
| wgpu 29 (vello) vs vendored wgpu 30.0.1 | Start with vello_cpu. Port or wait for vello_hybrid |
| Android GPU cost (wide brushes already measured at about 49 ms GPU per update) | Strictly incremental re-rendering; below/above caches during drags; SDF fast path for solid pens |
| Memory and file size of raw samples | Quantize and delta-encode the channels (Jetpack Ink's `CodedNumericRun` and tldraw's Float16 deltas are the precedents), store them in the binary blob store, never in JSON arrays. Estimated around 10 B per point vs about 50 B in JSON |
| Cross-platform floating-point drift in replay | Persist the raster cache and derived geometry; use i_overlay's integer mode where results must be bit-exact |
| Editing speed- or time-dependent strokes (airbrush) | Resample with interpolated time, or restrict those strokes to transform, recolor and delete |
| About 169 `LayerKind` equality checks that a new kind silently falls through | Audit each one as part of adding the kind |
| Web bundle size | Lazy-load a separate import/export wasm (PDF, text); enable `simd128` |

---

## 7. Recommendation

### 7.1 Product principles

1. **Drawing first; the editor side builds on it.** Our users are painters and comic artists. The first release should beat CSP's inking loop. Path tools come next, using the same objects.
2. **Vector should never be worse than raster at the five things beginners reach for first:** a smooth pen, the eraser, fill, resize, and undo. Each must work on a vector layer, and never fail with an error or create invisible artifacts.
3. **Same brushes, still editable.** Vector strokes render with our brush engine, textures included. Not SVG-only shapes (Krita's mistake), and not outline blobs (Fresco's mistake).
4. **Make results satisfying before handles appear.** Shapes, merging, trimming and bending come before Bézier handles, following both curricula.
5. **Name operations in plain language and show state:**
   - "Convert line to shape", not "Expand Appearance".
   - "Merge", "Cut out", "Border".
   - Show open/closed, point count and "line vs shape" on the selection.
6. **Be honest about export.** Warn when textured strokes will be rasterized or flattened.

### 7.2 What `.capy` stores: strokes, not outlines

**Decision.** For strokes, the source of truth is the **original stroke**:
- the recorded samples;
- the brush it was drawn with, embedded in the file;
- a seed and texture phase;
- width adjustments;
- an engine version.

Filled outlines are **derived**: computed for SVG/PDF export, merge/subtract, "Convert line to shape" and hit-testing, and never stored in `.capy` in v1. The layer's rendered pixels are still saved, as the display cache and as the fallback for anything that can't run the brush engine. Shapes (merge results, converted lines, fills, imported SVG) have no centerline, so their outline geometry *is* their source of truth.

**Why outlines can't be the source of truth once we support width editing**
- **Width belongs to the centerline:** `width(s) = brush_size × dynamics(pressure(s), tilt, speed) × adjustments(s)`.
  - With only an outline, thickening means pushing both sides outward and resolving collisions at overlaps, loops and tight curves.
  - Recovering the centerline from an outline means computing a medial axis, which is lossy and fragile.
  - This is the Fresco trap: its outline-based vectors can't be re-thickened, re-brushed or recolored, and that is its users' most common complaint.
- **Every other editing feature needs the centerline too:** brush swap, erase-to-intersection, nudge/pinch, control points and re-texturing after a cut.
- **Strokes are also smaller.** Quantized, delta-coded samples cost about 10 B per point. A pressure outline has anchors along both sides, and Fresco files show thousands of anchors per drawing.

**How width edits are stored**
- **Raw pressure is never rewritten.** Width edits are a separate adjustment on the stroke:
  - a whole-stroke `width_scale`;
  - a `fixed_width` flag that ignores the brush's size dynamics;
  - a sparse `width_profile` of knots `(s, factor)` over the source stroke's arc-length parameter;
  - optional start/end taper overrides.
- **Mapping to tools:**
  - Thicken/Thin/Scale tools and the local width brush add or edit knots.
  - "Fixed width" sets the flag.
  - Redraw-width with pen pressure replaces knots over a range.
  - Clip Studio Paint works the same way (per-point width factors on top of the brush size), and so does Illustrator (width points kept apart from the path).
- **Width edits survive later edits:**
  - Swapping the brush later still evaluates the *original* pressure through the new brush's dynamics, and the user's adjustments still apply on top.
  - Knots use the source stroke's parameter, so erase-splitting a stroke into `(source, t0..t1)` pieces needs no remapping.
- **Asymmetric width** (Illustrator's Alt-drag one side) can't be drawn by centered stamp dabs. Support it only on Curve strokes with Solid paint (`w_left`/`w_right` knots). For brush strokes, offer "Convert line to shape" instead.

**The "an editor without the brush" concern, case by case**

| Case | Answer |
|---|---|
| The user edits or deletes a brush preset, or opens the file on a device without their custom brush | **Embed brushes in the file**, not references to the library. `VectorContent` carries a deduplicated table of full `BrushSnapshot`s (the engine already freezes one per stroke) plus their tip and grain assets by content digest (already embedded in projects today). Library edits never change existing strokes. Offer an explicit "Update strokes to the current brush" or relink command. Precedents: CSP stores the brush per stroke, Jetpack Ink embeds brush families with textures, Wacom UIM embeds brush definitions |
| A future Capy Canvas changes or retires the engine code that drew a stroke | The file opens pixel-identical, because the saved raster cache is what's displayed. Each stroke carries `engine_version`. Only strokes the user edits are re-rendered, and so is anything else in the cleared region, because the rebuild re-renders every stroke in that area. Re-render with the recorded engine version while its code still ships. After a version is retired, show once, "This stroke will be redrawn with the current brush engine", on first edit. Never silently change the look on open. Map retired brushes forward the way Jetpack Ink does (`min_version` plus fallback brush families) |
| A different application | Other apps don't read native painting formats (`.clip`, `.procreate`, `.kra` paint layers). They exchange SVG, PDF, PSD and PNG. **Outlines belong in those exports** (section 7.6), which compute them from the strokes and can embed our private stroke data for round-trip. Any tool that reads the `.capy` tile format already gets full-quality pixels |
| A future lightweight or third-party `.capy` reader that wants vectors but has no brush engine | **Not in v1.** If needed later, persist outlines for **Solid-paint strokes only**. They are exact, deterministic and cheap to store. For textured brush strokes an outline is lossy anyway, and the pixels are the better fallback (open question 8) |

**Summary of what is stored**

| Data | Stored in `.capy`? | Role |
|---|---|---|
| Samples (x, y, t, pressure, tilt/azimuth, twist; quantized and delta-coded), seed, phase, `engine_version` | Yes (binary `VectorIndex` blobs) | Source of truth for drawn strokes |
| Embedded brush table (full `BrushSnapshot`s plus tip/grain assets by digest) | Yes (deduplicated) | Makes the file self-contained |
| Width adjustments (scale, fixed-width flag, knots, taper overrides) | Yes | Source of truth for width edits |
| Curve source (Bézier + width knots) for pen paths and node-edited strokes | Yes | Source of truth once the user edits nodes |
| Shapes (outline path, fill rule, fill/outline paint, origin) | Yes | Source of truth for closed shapes |
| Layer raster pages | Yes (as today) | Display cache; fallback for any reader |
| Per-stroke filled outlines | No (computed on demand) | Export, merge/subtract, Convert line to shape, hit-testing |
| Fitted curves for control-point display | No, until the user edits nodes | Editing aid |

### 7.3 Data model (shared Rust, `layer-core`)

```text
LayerKind::Vector
Layer { raster: RasterRevision  /* persisted render cache */,
        vector: Arc<VectorContent> /* source of truth */ }

VectorContent { objects: Vec<Arc<VectorObject>> /* z-order */,
                brushes: dedup table of BrushSnapshot + tip/grain asset digests /* embedded, self-contained */ }
  index: rstar R-tree (rebuilt on load, never saved)

VectorObject::Stroke {
    id, transform: Affine, bounds,
    source: Samples { channels: x,y,t,pressure,tilt/azimuth,twist (quantized, delta-coded blob),
                      tool_type, seed, phase, engine_version }
          | Curve   { path: BezPath, width_knots: [(t, w_left, w_right)], opacity_knots },
    range: t0..t1,                    // piece of the source after erase-splitting
    width: WidthAdjust { scale, fixed_width, profile: [(s, factor)], taper_start, taper_end },
    fitted: Option<Curve>,            // derived for control-point editing; becomes truth after a node edit
    paint: Brush { brush: index into brushes, color }   // stamp engine
         | Solid { color, cap, join, dash }             // SDF / vello_cpu
    // no stored outline: computed on demand (7.2)
}
VectorObject::Shape {
    id, transform, bounds, path: BezPath, fill_rule,
    fill: Option<Paint>, outline: Option<StrokeStyle>,
    origin: Drawn | ShapeTool{params} | Merge | Fill{seed_point, gap} | Imported | ConvertedFromStroke
}
```

- **Undo:** `ReplaceLayer` with `Arc`-shared object lists, plus the pending raster revision. Charge history by `Arc` identity (as selections already are), not by re-serializing the layer.
- **Erase:** splitting keeps the same sample source. A piece is `(source, t0..t1)` with the same seed and phase, so what remains renders identically and undo is cheap.
- **File format:** add a `VectorIndex` (the `SelectionIndex` pattern) and bump the format version. Brush snapshots are deduplicated and textures are already content-addressed.
- **Hosts** only capture input and render view models. Hit-testing, erasing, fitting, merging, fills, serialization and history all live in Rust. This is the split AGENTS.md requires for shared validation and history.

### 7.4 Toolset: the smallest set that covers most needs

**Phase 1 — "Ink": vector drawing that matches CSP's inking loop**

| Tool / behavior | Details and defaults |
|---|---|
| **Vector layer** | Shown in Add Layer **next to** Paint at the top level, with an icon and subtitle (see 7.5). All dry and contact brushes work; brushes that read the canvas are hidden or disabled with an explanation |
| **Draw** | Existing pens and stabilizer; add **hold-to-straighten/snap** (Procreate QuickShape, CSP 5.0 Smart Shapes) |
| **Vector eraser** | Three visible modes: **Trim to intersection (default)**, Cut touched part, Whole line. *The eraser on a vector layer always edits geometry and never creates transparent strokes.* An option to use intersections from all vector layers. Hold Shift (or the pen's eraser end) for vector erase |
| **Select lines** | Tap or lasso to select strokes. A selection bar shows **Pen / Size / Color / Opacity** (one-tap brush swap for one line or all lines). Move/scale/rotate/flip with a **"scale line width"** toggle (on by default) |
| **Line width** | Whole-stroke width scale and "fixed width" (Phase 1). A local thicken/thin brush (Phase 2) |
| **Fill flats** | Fill on a vector layer is never an error. It offers "Fill on a new color layer below (uses this ink)". Reuse the Region fill with **Reference layers + Close gaps + expansion** (already implemented). Add "fill to the centerline", so the fill tucks under the line with no halos |
| **Export** | PNG as today, plus **SVG (outlines)** and **PDF** (see 7.6) |

**Phase 2 — "Reshape": editing without handles**

- **Grab to reshape** (Concepts' Nudge / CSP's Pinch): displace samples with a falloff, so no fitting is needed. Option to fix the ends.
- **Local width brush** (thicken, thin, scale), and editable **start/end taper**.
- **Simplify** (whole stroke or a local brush) with a visible point count.
- **Control points** from fitting (kurbo `fit_to_bezpath`, with corner detection): move/add/delete, corner↔smooth, split, per-point width and opacity.
- **Redraw over a line** to replace a segment (Illustrator Pencil's "edit selected paths", CSP Redraw).
- **Connect lines**, and a **magnet that is off by default** (it causes "squiggly overlaps").
- **Curve tool** (click points, previewed live), the SAI/CSP curve that artists ask for.
- **Comic helpers:** make panel borders and balloons vector shapes; speed and focus lines via the existing rulers.

**Phase 3 — "Shapes & paths": the editor side**

- **Shapes:** rect (per-corner radius), ellipse, polygon, star, line. Extend the Figure tools so they create *live* shapes with editable parameters.
- **Pen:** a **curvature pen by default** (click points; the next segment is previewed; auto-smooth nodes; double-tap for a corner), and a classic Bézier pen with Illustrator's modifiers for experienced users.
- **Node tool:** move anchors and handles, marquee/lasso nodes, add or delete-and-heal, corner/smooth/symmetric, **drag a segment to bend it** (Figma Bend), snap handles to horizontal/vertical, snap points to extrema. On touch, on-screen modifier chips replace Alt and Shift.
- **Merge:** union, subtract, intersect, exclude, divide, plus **Shape Builder** (drag across regions to merge, Alt/Option to delete). Make them *just work*:
  - auto-ungroup;
  - auto-outline strokes, keeping round caps;
  - holes come out right, with fill rules never shown to beginners;
  - on failure, a plain-language reason with a **Fix** button.
- **Convert line to shape** (variable-width outline via i_overlay, then refit), **Offset/Border** (welded, with rounded corners, for stickers), **Join/Cut/Close**.
- **Vector region fill** (Illustrator Live Paint / Toonz style): faces from stroke **centerlines** plus auto-close gap segments. The fill is stored as a static Shape under the ink. A live-updating version is XL; defer it.
- **SVG import** (usvg, no text at first).
- **Cut-ready check and export presets** (7.6).

**Phase 4 — extensions (in rough priority order)**

- Image trace (visioncortex), with a detail slider and live point count.
- Width profiles (save and apply).
- Live corners.
- Text and text-to-outlines (parley + skrifa), then text on path.
- PDF/AI import (hayro, lazy-loaded).
- DXF export.
- Brushes along a path as a first-class option.
- vello_hybrid on the GPU.
- Time-lapse replay export (our strokes make this nearly free).
- InkML import/export.
- **Zoom-crisp display** for vector layers (open question 2).
- Centerline tracing of line art (XL).

Deliberately **not** in scope: mesh gradients, a node graph, Figma-style vector networks, CMYK/spot color, and live effect stacks. They are niche for our audience and expensive.

### 7.5 The new-artist journey

**Naming and placement**
- **Keep the name "Vector layer".** Every tutorial and search term uses it: CSP, Fresco, Krita, Affinity and Illustrator all do. Add a subtitle, **"Editable lines & shapes — trim, reshape, resize"**.
  - The drawing-experience study proposed "Ink layer". That name reads well, but it disconnects people from the tutorials they will follow (open question 1).
- Put **Paint** and **Vector** side by side at the top of Add Layer, with icons. Show a small "editable" badge on inking pens in the brush picker.
- **Never auto-create a layer per stroke** (ibisPaint drew backlash for this). The one exception: when the user picks an inking pen while on a sketch layer, offer once, "Ink on a new vector layer?", with "Always" and "Never" options.

**The first hour (guided and skippable; each step is a real result)**

| Minutes | Moment | What we show |
|---|---|---|
| 0–3 | First stroke on a vector layer | A toast: *"These lines stay editable"*, with three 3-second loops: trim an overlap, thicken a line, swap the pen |
| 3–10 | Clean-up | Trim-to-intersection is the default eraser mode. A one-time coach mark when the eraser first crosses an intersection. The satisfying "zap" |
| 10–15 | Re-skin | Tap a line and use the selection bar's Pen / Size / Color. Then "Select all → swap pen" on the whole layer |
| 15–25 | Flats | The first fill attempt creates a color layer below the ink, with close-gap on and the fill tucked under the lines. No error dialog |
| 25–35 | Shapes and merge (makers and logos) | Circle, rounded rect and star, then Shape Builder, then Border → a sticker or badge |
| 35–45 | Bend, don't handle | Drag a curve directly; the curvature pen for a logo. Handles stay hidden until requested |
| 45–60 | Make it real | Export presets: **Print PDF**, **Sticker (PNG + cut-line SVG)**, **Cricut/Silhouette SVG**, **Laser SVG/DXF**. A cut-ready check lists issues, each with a Fix button |

Offer an optional **touch-friendly pen-tool practice** later. bezier.method.ac, the popular pen-tool game, needs a keyboard.

**Starter templates**
- **Comic page:** Sketch (blue, 30%), Ink (vector), Flats (refers to Ink), Shade (Multiply, clipped), Frames (vector), Text.
- **Sticker / cut file:** a monoline vector ink preset plus the SVG export preset.
- **Webtoon:** a long canvas plus panels.

**Defaults that prevent the classic mistakes**
- Pressure is on with a G-pen-like default. A one-tap **Monoline** preset (pressure off, round caps) serves stickers, logos and Cricut.
- Stabilizer is moderate (around CSP's 20–30), and hold-to-snap is on.
- Scaling a selection scales line width by default, with the toggle visible.
- An open path gets only a stroke, and closing it adds a fill. Open endpoints are highlighted, with an "auto-close within N px" option.
- Merging with strokes, groups or text automatically converts them first. Holes are created correctly, and fill rules and winding are never shown to beginners.
- Every compound action (trace + expand + merge, fill + new layer) is **one undo step**.
- Performance: simplify on commit, and cache per tile. Hatching must not lag, because lag feeds the "vector is slow" folklore.
- Don't claim "sharp at any zoom" until we render at view resolution. Claim "resize without quality loss", which *is* true because we re-render from the vectors.

**Drag and touch rules.** Dragging vector objects, nodes and handles on the canvas is direct manipulation. `docs/internals/input.md` exempts it from the hold-to-drag convention, but touch and pen affordances still need designing: larger hit targets, modifier chips, and handles that appear only when requested. Any list or panel UI we add (for example, an object list) must follow `docs/ui/drag-and-reorder.md`.

### 7.6 Interop plan

**Export**
1. **SVG "Capy editable"** (the default):
   - SVG 1.1 syntax, presentation attributes, no `<style>`, `width`/`height` in mm, and a `viewBox`.
   - One filled `<path>` per stroke (the variable-width outline, simplified to 2–3 decimals).
   - Layers as `<g inkscape:groupmode="layer">`.
   - A private `xmlns:capy` namespace carries stroke IDs, brush references and samples (compressed, in `<metadata>`), plus a geometry fingerprint. On re-import, we restore full strokes only when the visible geometry still matches the fingerprint. Otherwise another app edited the art, so we import what's visible and offer "restore original strokes". This avoids Inkscape's overwrite trap.
2. **SVG "Plain"**: the same file without private data.
3. **SVG "Cut / Plot"** (and later DXF):
   - Choose **centerlines** (`fill=none`, one color per operation) or **welded outlines**.
   - No clip, mask, gradient, image or text.
   - Erasers and clips applied as real geometry.
4. **PDF** via krilla: outlines, blend modes, soft masks and ICC color. It's the right target for wide-gamut documents, since SVG effectively means sRGB. Optionally embed the private stroke data for round-trip.

**Textured strokes can't be expressed in SVG.** Offer three options:
- a solid outline (the default; lossy but universal, and what Concepts does);
- the outline clipping an embedded raster of the stroke;
- flagging them in the export dialog.

**Import**
- **SVG** via usvg. Paths become Shapes; stroked paths become solid Strokes with a Curve source. Filters and masks are rasterized or refused with a notice. Read our `capy:` data in a second pass with roxmltree.
- **PDF and PDF-compatible AI** via hayro, later.
- **Ink formats** later, all cheap to add: InkML (Office/OneNote), PencilKit (through the Apple host API), ISF (through the Windows host), Excalidraw, tldraw and Xournal++ JSON/XML.
- **Don't build around a stroke-interchange standard.** Consider borrowing Jetpack Ink's input encoding (struct-of-arrays, delta-coded, optional channels, `min_version`) for our own `VectorIndex`.

### 7.7 Phasing and sizes

| Phase | Scope | Size (S/M/L/XL) | Unlocks |
|---|---|---|---|
| **0. Groundwork** | Fix stale docs; add kurbo + rstar after `cargo deny` and a wasm size check; `VectorContent` model with the embedded brush table and width adjustments (7.2); `VectorIndex` in the file format; identity-based history accounting; `engine_version` on strokes | M | Everything |
| **1. Ink** | `LayerKind::Vector` (audit about 169 kind checks); persist strokes at pen-up (`canvas.rs:1427-1490`); regional rebuild; 3-mode vector eraser; select, recolor, re-brush and transform; whole-stroke width; fill into a new layer using Reference + Close gaps; SVG and PDF outline export; UI on all six hosts | L | The CSP inking loop on Linux; scalable line art |
| **2. Reshape** | Nudge/pinch, local width brush, tapers, simplify, fitting plus control points, redraw, connect, curve tool, hold-to-snap | L | "Correct instead of redraw" (also an accessibility win) |
| **3. Shapes & paths** | Live shapes, curvature and Bézier pens, node tool with bend, merge + Shape Builder (dual backend), convert line to shape, offset/border, vector region fill, SVG import, cut-ready check and presets, GPU/vello_cpu path filling with nonzero winding | L–XL | Logos, stickers, cut files, painter → vector without other apps |
| **4. Extensions** | Trace, text, PDF import, DXF, width profiles, live corners, time-lapse, InkML, vello_hybrid, zoom-crisp display | L–XL each | Depth |

The per-feature sizes in [07-rust-feasibility.md](vector-layers-research/07-rust-feasibility.md) §13 back these up. Most tools are S–M. The large items (L) are brush-stroke capture and replay, fitting plus control points, the pen tool, hardening merge on real art, PDF import and text on path. Live-updating fills and centerline tracing are XL.

---

## 8. Open questions for the team

1. **Name:** "Vector layer" with a descriptive subtitle (recommended, because it matches the tutorials people search for) or "Ink layer" (friendlier, but disconnected from existing tutorials)?
2. **Display resolution:** is document-resolution rendering acceptable for v1? CSP does the same, and it causes "why pixelated?" threads. The alternative is composing vector layers at view resolution when zoomed in. A limited version is feasible: only for layers with Normal blend and no effects above them.
3. **Fill model:** is raster flats referencing the ink (reuses the existing tool; the CSP norm) enough for v1, with vector region fills in Phase 3? Or do makers need vector fills in Phase 1?
4. **Raw samples after node edits:** keep them forever as a record of the original input, which costs file size? Or drop them once the edited curve becomes the truth?
5. **Maker scope:** how far do we go (DXF, cut-ready check, Cricut presets)? The audience is large, and the evidence suggests yes.
6. **Path renderer:** port vello_hybrid to wgpu 30, wait for upstream, or extend our own GPU crossing-count filler with nonzero winding and more samples?
7. **Merge/subtract risk:** do we accept linesweeper (beta) behind a polygon fallback? Or ship polygon-only merge first and add curve-exact results later?
8. **Vector fallback for other readers:** is the saved pixel cache a sufficient fallback for readers without the brush engine? Or should `.capy` also store exact outlines for Solid-paint strokes, so lightweight or third-party readers get vectors? (Recommendation: pixels only in v1; see 7.2.)
