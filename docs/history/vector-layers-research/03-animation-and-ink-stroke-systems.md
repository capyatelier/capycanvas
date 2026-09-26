# Vector stroke systems in animation tools, ink SDKs and open-source drawing apps

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Research for Capy Canvas vector layers, dated 2026-09-25.

**How this was researched.** I shallow-cloned google/ink (`1b220ee`), google/ink-stroke-modeler, flxzt/rnote (`29ea24a`), opentoonz (`16661c8`), xournalpp (`b8b3a59`), perfect-freehand (`176e00f`) and Wacom's universal-ink-library, and read their data structures directly. Apple, Microsoft, Blender, Toon Boom, Synfig, OpenToonz and Krita docs were read with fetch tools. The session's web-search quota ran out partway through. Items marked **(unverified)** come from background knowledge and were not re-checked against a source.

---

## TL;DR

- **There are two main ways to store strokes:**
  - **(A) Record the input and replay it through a brush.** Used by google/ink (Jetpack Ink), Excalidraw, tldraw and Wacom UIM's sensor data. You can swap the brush after the fact, but reshaping the geometry is awkward.
  - **(B) Store fitted, editable geometry with per-point attributes baked in.** Used by PencilKit, Toonz `.pli`, Harmony pencil lines, Blender Grease Pencil, Rnote, Xournal++, Inkscape, Moho and Synfig. Geometry is directly editable, but pressure-to-width is usually baked in, so a brush swap can't re-derive the look.
  - **Hybrids** keep both: Wacom UIM (sensor data plus a spline, with an index mapping) and google/ink (inputs plus an optional cached mesh).
- **google/ink is the closest match to what Capy wants.**
  - Its stroke = `StrokeInputBatch` (raw, pre-smoothing inputs stored as delta-coded columns) + `Brush` (full brush definition, textures included) + a derived `PartitionedMesh`.
  - It has explicit brush-format versioning (`min_version`, `newer_brush_families` fallbacks).
  - It stores a noise seed per stroke.
  - It is Apache-2.0 and ships WGSL, Metal and Skia shaders.
- **Determinism is the weak point of replay.**
  - Excalidraw's look depends on empirically tuned constants in the renderer.
  - Blender 4.5 removed a radius clamp "with no versioning", so old files may render differently.
  - PencilKit and google/ink both gate content by version.
  - Everyone who does particles or texture stores a **random seed** per stroke, and newer systems also store **texture/grain phase** so a split stroke keeps its look (PencilKit `renderState.grainOffset`, WILL `ts/tf`).
- **Erasing ranges from simple to hard:** delete the whole stroke; split at parameter ranges (Rnote, Xournal++, Blender 4.3+, PencilKit `substroke(range:)`, WILL `ts/tf`); per-stroke clip masks (PencilKit `mask`); mesh booleans (Jetpack Ink `Stroke.subtract/split`, which can't be serialized yet; Blender 5.3 Carver).
- **Fills in animation tools come from the stroke graph, not from pixels:**
  - Toonz computes regions from stroke intersections with an autoclose tolerance.
  - Harmony uses invisible strokes and zones, with a Close Gap setting of 0–10 px.
  - Blender 5.2 added a Delaunay solver with automatic gap detection and `fill_id` groups with even-odd holes.
- **What Capy (MIT/Apache, cargo-deny blocks GPL) can reuse:**
  - google/ink, ink-stroke-modeler (C++ and the Rust port), perfect-freehand and its Rust ports, OpenToonz (BSD-3), Excalidraw (MIT), and the Wacom UIM format and library (Apache-2.0).
  - **Not reusable as code:** Rnote (GPL-3), Xournal++ (GPL-2+), Blender, Inkscape, Krita, Synfig (all GPL), and tldraw (proprietary "tldraw license"). Their file formats can still be implemented for interop.

---

## 1. Animation tools

### Toon Boom Harmony (proprietary)

**Data model: three kinds of vector stroke.**
- **Pencil lines** are *central vectors*: a centerline plus a thickness profile. Control points sit "along the length of the central spine" and thickness is edited with the **Pencil Editor**. Sources: [About Pencil Tool](https://docs.toonboom.com/help/harmony-24/premium/drawing/about-pencil-tool.html), [Pencil Editor](https://docs.toonboom.com/help/harmony-22/premium/drawing/about-pencil-editor-tool.html).
  - **Thickness stencils** save a thickness profile along the line. They replace pressure and can be applied after the fact. Pencil lines can also carry textures.
  - *Auto Adjust Thickness* and *Line Pushing* re-shape existing lines with over-strokes ([Pencil properties](https://docs.toonboom.com/help/harmony-22/premium/reference/tool-properties/pencil-tool-properties.html)).
- **Brush strokes** are *contour vectors*: filled outline shapes whose control points lie on the contour. They're edited with the Contour Editor.
  - Smoothing has two stages, "Centerline Smoothing" and "Contour Smoothing" ([Brush properties](https://docs.toonboom.com/help/harmony-24/premium/reference/tool-properties/brush-tool-properties.html)).
- **Textured vector brushes** are "composed of a greyscale bitmap mask applied to their colour". The contour stays vector and the texture is a raster alpha mask, so recolouring from the palette keeps the texture. When a stroke is scaled or deformed, Harmony warps the mask and "will automatically generate new pixels" ([About Brush Tool](https://docs.toonboom.com/help/harmony-24/premium/drawing/about-brush-tool.html)).
  - **This is the key hybrid precedent for a stamp engine.** The stamp look is baked into a raster mask and does not replay.

**Layers and editing.**
- Each drawing has embedded Overlay, Line Art, Colour Art and Underlay layers.
- "Automatically Create Colour Art" copies line strokes into the Colour Art layer as you draw, so paint sits under the lines (the embedded-layer names are **(unverified)** beyond that option).
- Strokes are separate objects unless **Auto-Flatten** merges them into existing art, which cuts overlapping strokes. This is the same idea as Adobe Animate's merge-drawing model.
- Brush strokes can be converted to pencil lines.

**Fill.**
- Colour zones are bounded by visible *and invisible* strokes.
- The Paint tool's **Close Gap** setting runs from 0 to 10 px (presets 2, 4 and 8) and automatically adds an invisible closing stroke ([Paint properties](https://docs.toonboom.com/help/harmony-24/premium/reference/tool-properties/paint-tool-properties.html)).
- The Close Gap tool draws an invisible stroke between the two nearest endpoints near where you scribble ([Closing gaps](https://docs.toonboom.com/help/harmony-24/premium/colour/close-gaps.html)).
- For pencil lines, "the centreline is used to determine whether or not the contour is closed". The *Auto Close Gap* drawing option exists because visible tips can overlap while the centerlines don't.

**Format and rendering.** Drawings are saved as proprietary `.tvg`. Morphing works only pencil-to-pencil, and textures are dropped when morphing.

### OpenToonz / Tahoma2D (BSD-3)

**Data model** (`toonz/sources/include/tstroke.h`, `tcurves.h`, `tgeometry.h`):
- `TStroke` is a chain of `TThickQuadratic` curves: quadratic Béziers whose control points are `TThickPoint {x, y, thick}`.
- Other per-stroke data: a style id (palette index), a self-loop flag, `OutlineOptions` (cap, join, miter) and a group id.
- Freehand input (points with pressure-derived thickness) is fitted by `TStroke::interpolate(points, error)` (cubic fit, then quadratic). The "Accuracy" setting controls the fit error ([OT drawing docs](https://opentoonz.readthedocs.io/en/latest/drawing_animation_levels.html)).
- Thickness 0 gives a wireframe stroke that doesn't render but still bounds areas. Raw input is **not** kept.

**Regions and fills.**
- `TVectorImage::Imp` keeps `m_strokes` (`VIStroke`: a stroke, its edge list and group id), `m_regions`, `m_autocloseTolerance` and `IntersectionData`.
- `tcomputeregions.cpp` intersects all strokes to build a planar graph and adds `TAutocloseEdge` segments for gaps within tolerance (`tl2lautocloser.cpp`). Regions are then extracted and keep their fill style across recomputes where possible.
- Fill modes are Areas, Lines, or both, with rect, freehand or lasso multi-fill. The **Tape tool** joins endpoint-to-endpoint, endpoint-to-line or line-to-line, manually or automatically in a rectangle ([painting docs](https://opentoonz.readthedocs.io/en/latest/painting_animation_levels.html)).

**Rendering.**
- `tstrokeoutline.h` computes outline points carrying texture coordinates (u = width parameter, v = arc length) for textured and "vector brush" styles.
- Regions are tessellated with the GLU tessellator (`common/tvrender/ttessellator.cpp`).
- `tinbetween.cpp` interpolates between vector drawings.

**`.pli` format** (`image/pli/pli_io.h`):
- A binary tag stream: `THICK_QUADRATIC_CHAIN_GOBJ`, `THICK_QUADRATIC_LOOP_GOBJ`, `GROUP_GOBJ`, `INTERSECTION_DATA_GOBJ` (persisted region and fill topology), `OUTLINE_OPTIONS_GOBJ`, `PRECISION_SCALE_GOBJ`, `AUTOCLOSE_TOLERANCE_GOBJ` and a palette.
- Coordinates are quantized with a precision scale. Thickness is quantized to `maxThickness/255`, although newer files use a different thickness write method.

**Editing tools:** Pump (thickness), Control Point Editor, Pinch, Cutter, and a vector eraser that removes segments.

### Moho (proprietary)

- **Points and curves** ([scripting API](https://mohoscripting.com/classes/M_Point)):
  - `M_Point` has `fPos`, animated `fAnimPos` and animated per-point `fWidth`.
  - Curves ([M_Curve](https://mohoscripting.com/classes/M_Curve)) store animated per-point curvature, offsets and weights (Bézier handles).
- **Shapes** ([M_Shape](https://mohoscripting.com/classes/M_Shape)) reference curve edges with `fHasFill`, `fHasOutline`, `fComboMode` and a style. Fill and outline are properties of shapes built on shared points.
- Everything is keyframeable per point. That is the animation-first design: the stroke *is* the rig.
- Line "brushes" are bitmap stamps along the vector outline **(unverified)**. The `.moho` file is zipped JSON **(unverified)**.

### Adobe Animate (proprietary; maintenance mode since February 2026)

- **Status:** Adobe announced the product's end on 2026-02-02 and reversed the next day to an indefinite "maintenance mode" with no new features ([Wikipedia](https://en.wikipedia.org/wiki/Adobe_Animate)).
- **Shape model** (inherited from Flash/SWF):
  - A `DOMShape` is a set of **edges**, each a quadratic-Bézier path string.
  - Each edge has `fillStyle0` (left), `fillStyle1` (right) and an optional `strokeStyle` ([xfl2svg edge.py](https://github.com/PluieElectrique/xfl2svg/blob/master/xfl2svg/shape/edge.py)).
  - Shapes are faces of a planar map. In **Merge Drawing** mode, overlapping shapes cut and merge; in **Object Drawing** mode they stay separate groups.
- **Strokes:**
  - Fixed-width strokes, plus the Width tool's **variable width profiles** (width points along the stroke, saveable as profiles).
  - Art and pattern brushes bend vector art along the path.
  - The Paint Brush and Fluid Brush use pressure and velocity **(details unverified)**.
  - The Brush tool historically produced *fills* (outlines), not strokes.
- **Format:** XFL/FLA is zipped XML. Ruffle's source is the best open reference for the SWF-side rendering.

### Blender Grease Pencil v3 (GPL; 4.3 and later)

- **Data model:** each drawing is a `CurvesGeometry` with named attributes ([architecture doc](https://projects.blender.org/blender/blender-developer-docs/raw/branch/main/docs/features/grease_pencil/architecture.md)).
  - **POINT attributes:** `position`, `radius` (metres; the old px-thickness is gone), `opacity`, `vertex_color`, `rotation` (texture), and `delta_time`, which is "written while drawing… used by the build modifier to replay".
  - **CURVE attributes:** `cyclic`, `material_index`, `start_cap`/`end_cap`, `softness`, `curve_type` (POLY/BEZIER/CATMULL_ROM/NURBS), `aspect_ratio`, `fill_color`, `fill_opacity`, `init_time`, and since 5.1 `fill_id` and `hide_stroke`.
  - **Pressure is not stored.** It is baked into radius and opacity at capture time.
- **Rendering:**
  - Each segment is a quad. The fragment shader evaluates an "uneven capsule" (two circles with radii r1 and r2) with hardness falloff: an SDF-style approach with no CPU tessellation of strokes ([gpencil_frag.glsl](https://github.com/blender/blender/blob/main/source/blender/draw/engines/gpencil/shaders/gpencil_frag.glsl)).
  - Dot and square materials are generated at render time (5.2).
  - Anti-aliasing is SMAA, plus optional SSAA (4.5).
- **Editing and erasing:**
  - 4.3 rewrote the eraser to **cut** strokes at the eraser edge and create new points. The "soft" eraser writes graded opacity ([4.3 notes](https://projects.blender.org/blender/blender-developer-docs/raw/branch/main/docs/release_notes/4.3/grease_pencil.md)).
  - 5.3 added a **Carver** tool that does a true boolean subtraction of strokes and fills along a lasso, creating holes as needed ([5.3 notes](https://projects.blender.org/blender/blender-developer-docs/raw/branch/main/docs/release_notes/5.3/grease_pencil.md)).
- **Fills:**
  - Since 5.1, curves sharing a `fill_id` are triangulated together by constrained Delaunay triangulation with **even-odd holes** ([5.1 notes](https://projects.blender.org/blender/blender-developer-docs/raw/branch/main/docs/release_notes/5.1/grease_pencil.md)).
  - The **5.2 Fill tool** added a "Delaunay" solver that "creates exact geometry from the boundary strokes… automatic gap detection… zoom independent", replacing the old pixel flood-fill-then-trace as the default ([5.2 notes](https://projects.blender.org/blender/blender-developer-docs/raw/branch/main/docs/release_notes/5.2/grease_pencil.md)).
  - "Fill Guides" are helper strokes for closing gaps. 5.3 added multi-region fill.
- **Determinism warning:** 4.5 removed the 0.001 minimum-radius clamp, and "no versioning is applied… may render differently" ([4.5 notes](https://projects.blender.org/blender/blender-developer-docs/raw/branch/main/docs/release_notes/4.5/grease_pencil.md)).
- **Interop:** SVG and PDF export, including fills with holes and animated SVG.

### Krita vector layers (GPL-3)

- **Storage:** SVG 1.1 shapes embedded in `.kra` ([manual](https://docs.krita.org/en/user_manual/vector_graphics.html)).
- **The brush engine doesn't work on vector layers.** Freehand, Dynamic and Multibrush tools are excluded. Paths and polylines "cannot have line-variation". Only the Calligraphy tool produces pressure-varying filled outlines, with no centerline kept.
- **Why artists call them weak:**
  - No pressure or textured brushes.
  - No stroke-graph fill or gap closing.
  - Shapes are generic SVG objects rather than ink strokes.
  - Per Krita developers in the [freehand-on-vector request](https://krita-artists.org/t/drawing-freehand-on-vector-layer/1179): a "wishbug for almost ten years", with performance fears (smudge, filters and clone would force re-rendering every underlying stroke) and the calligraphy tool needing a rewrite first.

### Synfig (GPL-3)

- **Outline layer:** a spline where width is tied to each vertex.
- **Advanced Outline layer:** "width points" move freely along the spline at a parameter position, with tips (rounded, squared, peak, flat), smoothness and dashes ([docs](https://synfig.readthedocs.io/en/latest/layers/advanced_outline.html)).
- **Draw tool:** maps pressure to width, "Width Max Error" fitting, Auto Loop and Auto Link to existing vertices, and a separate **Region** layer for fills ("Fill Last Stroke") ([Draw tool](https://synfig.readthedocs.io/en/latest/tools/draw.html)).
- **Format:** `.sif`/`.sifz` (gzipped XML).

*Not requested, but relevant:* **Clip Studio Paint** vector layers **(unverified)**:
- Any raster brush, including textured ones, works on vector layers. Lines are stored as control points with per-point width and opacity and re-rendered through the brush engine. That is exactly Capy's "GPU stamp engine on vectors" goal.
- The vector eraser can erase the touched area, erase "up to intersection", or erase the whole line.

---

## 2. Ink and note SDKs and apps

### Apple PencilKit (plus Notes and Freeform, which use it)

Sources: [PKStrokePoint](https://developer.apple.com/documentation/pencilkit/pkstrokepoint-swift.struct), [PKStroke](https://developer.apple.com/documentation/pencilkit/pkstroke-swift.struct), [PKStrokePath](https://developer.apple.com/documentation/pencilkit/pkstrokepath-swift.struct), [PKContentVersion](https://developer.apple.com/documentation/pencilkit/pkcontentversion).

- **`PKStrokePoint` fields:**
  - `location`, `timeOffset` (seconds since stroke start), `size` (CGSize), `opacity`, `force`, `azimuth`, `altitude`.
  - `secondaryScale` (iOS 17), `threshold` ("alpha threshold for clipping", iOS 26) and `lateralJitter` ("particle jitter at the stroke edge", iOS 27).
- **`PKStrokePath`** is a **cubic uniform B-spline** whose control points are `PKStrokePoint`s, plus `creationDate` and `id`.
  - It offers `interpolatedPoints(in:by:)`.
  - iOS 27 adds `init(bezierPath:…)`, which converts Béziers into B-splines.
  - The stored points are *modeled* control points, not raw touches.
- **`PKStroke`** holds:
  - `ink` (an `inkType` such as pen, pencil, marker, monoline, fountainPen, watercolor, crayon or reed, plus a `color`), `path` and `transform`.
  - `mask`, a pre-transform clip path used by the pixel eraser, with `maskedPathRanges`.
  - `randomSeed`, `renderGroupID` (iOS 27; marker runs composite "as if… still wet") and `renderState` (iOS 27; e.g. `grainOffset` so crayon grain stays put on `substroke(range:)`).
  - `requiredContentVersion`.
- **Eraser types:** `.vector` (whole stroke), `.bitmap` (partial) and `.fixedWidthBitmap`. `PKDrawing.erasePath(_:mask:transform:)` arrived in the 2026 SDKs.
- **Versioning:** `PKContentVersion` runs from 1 to 5: v2 added the iPadOS 17 inks, v3 barrel roll, v4 the reed pen, and v5 stroke render state. Drawings declare `requiredContentVersion`, and a canvas can cap the maximum version so it doesn't emit strokes older OS versions can't render.
- **Serialization and rendering:** `PKDrawing.dataRepresentation()` is an opaque binary blob. Rendering uses Metal; `image(from:scale:)` rasterizes.

### Windows Ink, ISF and OneNote

- **Model:**
  - `InkStroke` holds `InkPoint {Position, Pressure, TiltX, TiltY, Timestamp}` ([InkPoint](https://learn.microsoft.com/en-us/uwp/api/windows.ui.input.inking.inkpoint)).
  - `InkDrawingAttributes` has `Color`, `Size`, `PenTip` (circle or rectangle), `PenTipTransform`, `FitToCurve` (Bézier vs polyline), `IgnorePressure`, `IgnoreTilt`, `DrawAsHighlighter`, `Kind` (Default or Pencil), `PencilProperties.Opacity` and `ModelerAttributes` ([InkDrawingAttributes](https://learn.microsoft.com/en-us/uwp/api/windows.ui.input.inking.inkdrawingattributes)).
- **Rendering:** Direct2D `ID2D1Ink`, "a single continuous stroke of variable-width ink, as defined by a series of Bezier segments and widths" ([ID2D1Ink](https://learn.microsoft.com/en-us/windows/win32/api/d2d1_3/nn-d2d1_3-id2d1ink)).
- **Format:** **ISF** (Ink Serialized Format), saved as "a GIF image with additional metadata" so non-ink apps see a picture. ISF is described as the most compact persistent representation of ink ([save/load ink](https://learn.microsoft.com/en-us/windows/apps/develop/input/save-and-load-ink)). InkML (W3C) is the XML alternative.
- **Erasing:** UWP's eraser removes whole strokes. WPF's `InkCanvas` EraseByPoint splits strokes **(unverified)**. OneNote stores ink inside its `.one` format **(unverified)**.

### google/ink C++ core and Jetpack Ink (`androidx.ink`), Apache-2.0

**`Stroke`** = `Brush` + `StrokeInputBatch` + `PartitionedMesh` ([stroke.h](https://github.com/google/ink/blob/main/ink/strokes/stroke.h)).
- `SetBrushColor` never regenerates the mesh. Changing size, epsilon or tip does.
- `Subtract(mask_shape)` and `Split(tolerance)` edit the mesh but **keep the original inputs**.

**`CodedStrokeInputBatch`** ([proto](https://github.com/google/ink/blob/main/ink/storage/proto/stroke_input_batch.proto)) is struct-of-arrays and lossy delta-coded:
- `CodedNumericRun {repeated sint32 deltas; float scale; float offset}` per channel.
- Channels: `x/y_stroke_space` ("original (pre-stroke-modeler)" positions), `elapsed_time_seconds`, `pressure` 0–1, `tilt` 0–π/2, `orientation` 0–2π, `barrel_twist`, per-stroke `tool_type` (mouse/touch/stylus), `stroke_unit_length_in_centimeters` (physical scale) and `noise_seed`.
- Optional channels are omitted for devices that don't report them.
- The proto comment: "Once a StrokeInputBatch is recorded, it is generally never changed; however, a client app can change the brush specification after the fact."

**`Brush`** = `color`, `size_stroke_space`, `epsilon_stroke_space` (mesh fidelity) and `BrushFamily` ([brush.proto](https://github.com/google/ink/blob/main/ink/storage/proto/brush.proto), [brush_family.proto](https://github.com/google/ink/blob/main/ink/storage/proto/brush_family.proto)):
- **`BrushFamily`** has `coats[]`, `input_model` (passthrough or sliding window, 20 ms by default), `texture_id_to_bitmap` (embedded PNGs), `client_brush_family_id`, `min_version` and **`newer_brush_families`** (fallback chain for devices on older library versions).
- **`BrushTip`** has scale x/y, corner rounding, slant, pinch, rotation, particle gap (distance and duration) and `behaviors[]`.
- **`BrushBehavior`** is a node graph:
  - Sources: pressure, tilt, speed, acceleration, distance, time, direction, barrel twist, and more.
  - Operators: damping, response curves, noise with seed, integrals, binary ops, tool-type filters.
  - Targets: width, height, slant, rotation, position offset, hue, chroma, lightness, opacity, texture animation.
- **`BrushPaint`** has texture layers (tiling or **stamping**, origin at stroke origin, first input or last input, blend modes), color functions and `self_overlap` (ANY, ACCUMULATE or DISCARD). The self-overlap choice matters for translucent strokes.

**Rendering:**
- `BrushTipExtruder` models a tip shape at each modeled input and joins consecutive shapes with a triangle mesh "as though each tip shape smoothly morphs into the next". Tip sizes below epsilon produce breaks for dashes and dots.
- Particle brushes emit separate textured quads. This is stamping, emulated inside the mesh model.
- Vertices carry side and forward derivatives plus labels, so the shader can outset about 0.5 px for anti-aliasing without shrinking the stroke ([stroke_vertex.h](https://github.com/google/ink/blob/main/ink/strokes/internal/stroke_vertex.h)).
- Backends: Android `Mesh`, Skia, Metal, and a 2026 **WebGPU/WGSL** shader ([StrokeShader.wgsl](https://github.com/google/ink/blob/main/ink/rendering/webgpu/StrokeShader.wgsl)).
- `PartitionedMesh` has a lazily built static R-tree for hit-testing.

**Status** ([release notes](https://developer.android.com/jetpack/androidx/releases/ink)):
- 1.0.0 stable shipped 2025-12-17. 1.1.0-alpha09 shipped 2026-09-23 and moved color shift to Oklab.
- It is going Kotlin Multiplatform, with iOS rendering via `MetalRenderer`.
- The experimental "partial stroke / mesh editing / pixel eraser" splits strokes, but there are still "no serialization APIs to save the erased PartitionedMesh results".
- Serializing input batches gives "a tiny fraction of the size of traditionally stored strokes".
- It builds only with Bazel 7, abseil and protobuf, and "no hard guarantees about interface stability".

### Ink Stroke Modeler (Apache-2.0) and its Rust port

- **Input:** `Input {event_type, position, time, pressure?, tilt?, orientation?}`.
- **Stages:** wobble smoothing (a time-windowed moving average blended by speed), a spring-mass position model with drag, then prediction (Kalman or stroke-end), then interpolation of the stylus state ([repo](https://github.com/google/ink-stroke-modeler)).
- **Rust port:** `ink-stroke-modeler-rs` (MIT OR Apache-2.0, partial) is what Rnote uses ([repo](https://github.com/flxzt/ink-stroke-modeler-rs)).

### Wacom WILL 3 / Universal Ink Model (library Apache-2.0)

Source: [universal-ink-library](https://github.com/Wacom-Developer/universal-ink-library), `uim/model/inkdata/strokes.py`.

- **`Stroke`** = a Catmull-Rom `Spline` with a **layout mask** of per-point channels:
  - X, Y, Z, SIZE, ROTATION, RED, GREEN, BLUE, ALPHA, SCALE_X/Y/Z, OFFSET_X/Y/Z and TANGENT_X/Y.
  - Plus **`ts/tf` start and end parameters** (partial strokes after erase), `style` (brush URI, render mode URI), `random_seed`, and **`sensor_data_id` / `sensor_data_offset` / `sensor_data_mapping`**, which point back to the raw input.
- **`SensorData` channels** are URIs such as `will://input/3.0/channel/{X,Y,Z,Timestamp,Pressure,RadiusX,RadiusY,Azimuth,Altitude,Rotation}`, with input-device and environment context.
- **Brushes** are vector (polygon) or raster (shape and fill textures, spacing, scattering, rotation mode, blend). Brush definitions are embedded in the file.
- **Format:** a RIFF container with protobuf chunks (UIM 3.0 and 3.1), plus a semantic "knowledge graph" layer. InkML and WILL 2 parsers are included.
- **The most complete "keep raw and processed, linked" design.**

### Xournal++ (GPL-2+)

- **`.xopp`:** gzipped XML. Each `<stroke tool="pen|highlighter|eraser" color width fill capStyle style ts fn>` holds text `x y x y …`.
- **Pressure is baked:** `width="base w1 w2 …"` stores **absolute per-segment widths** (pressure × base width), so resizing a stroke has to rescale every z value (`Stroke.cpp`) ([SaveHandler.cpp](https://github.com/xournalpp/xournalpp/blob/master/src/core/control/xojfile/SaveHandler.cpp)). Timestamps are per stroke only, used for audio sync.
- **Rendering:** Cairo. Pressure strokes are drawn one segment at a time, each with its own `cairo_set_line_width` ([StrokeViewHelper.h](https://github.com/xournalpp/xournalpp/blob/master/src/core/view/StrokeViewHelper.h)).
- **Erasers:** the standard eraser splits strokes at exact intersection parameters (`ErasableStroke`). There are also whiteout and delete-stroke erasers.

### Rnote (Rust, GPL-3)

- **Data model:**
  - `BrushStroke {path: PenPath, style: Style}`, where `PenPath {start: Element, segments: Vec<Segment>}` and `Element {pos: Vector2, pressure: f64}`.
  - Positions and pressure are serialized to 3 decimals, with no timestamp, tilt or seed on points.
  - `Segment` is `LineTo`, `QuadBezTo` or `CubBezTo` ([element.rs](https://github.com/flxzt/rnote/blob/main/crates/rnote-compose/src/penpath/element.rs), [segment.rs](https://github.com/flxzt/rnote/blob/main/crates/rnote-compose/src/penpath/segment.rs)).
  - Three builders: simple, curved (fitted) and modeled (ink-stroke-modeler-rs).
- **Styles:** `Smooth {width, color, fill, pressure_curve, line_style, cap}`, `Rough` (roughr, with seed) and `Textured` (dot distribution, with seed).
- **Rendering:**
  - The smooth style subdivides each Bézier into lines and builds a **variable-width outline polygon** (start cap, positive offset, end cap, reversed negative offset), then fills it via piet/cairo ([smooth/mod.rs](https://github.com/flxzt/rnote/blob/main/crates/rnote-compose/src/style/smooth/mod.rs)).
  - Per-stroke render images are cached. An `rstar` R-tree indexes strokes.
- **Erasing:** the splitting eraser cuts at **segment** granularity (`split_colliding_strokes`), so it's coarse ([trash_comp.rs](https://github.com/flxzt/rnote/blob/main/crates/rnote-engine/src/store/trash_comp.rs)).
- **Format:** `.rnote` is gzip + serde JSON with a semver header and chained per-version migrations (`maj0min5patch8.rs` … `maj0min15.rs`). It also imports and exports `.xopp`, SVG and PDF.

### perfect-freehand, Excalidraw and tldraw

- **perfect-freehand** (MIT) ([repo](https://github.com/steveruizok/perfect-freehand)):
  - Input: `[x, y, pressure?]`. Output: an outline polygon for a single fill.
  - Streamline lerps each point toward the input. Radius = `size * easing(0.5 - thinning*(0.5 - pressure))`. `simulatePressure` derives pressure from speed.
  - Corners get round caps. Start and end tapers are applied, and cap noise is trimmed.
  - Self-intersecting outlines rely on nonzero fill. Semi-transparent strokes look fine, but the outline can't be textured.
- **Excalidraw** freedraw ([types.ts](https://github.com/excalidraw/excalidraw/blob/master/packages/element/src/types.ts), [shape.ts](https://github.com/excalidraw/excalidraw/blob/master/packages/element/src/shape.ts)):
  - Fields: `points[]` (local), `pressures[]`, `simulatePressure`, and a newer `strokeOptions {variability: "variable"|"constant", streamline}` alongside `strokeWidth` and `seed`.
  - Rendering uses perfect-freehand with **"magic numbers backed by visual verification"** (`SIZE_FACTOR 4.25`, `THINNING 0.6`, easeOutSine). "Constant" mode uses a different generator (`@excalidraw/laser-pointer`).
  - **Versioning is done through optional-field defaults**: "Unknown/absent variability falls back to the original variable rendering".
  - Outlines are cached per element in a WeakMap.
- **tldraw** draw shape ([TLDrawShape.ts](https://github.com/tldraw/tldraw/blob/main/packages/tlschema/src/shapes/TLDrawShape.ts)):
  - Fields: `segments[{type: free|straight, path, dim?: 2|3}]`, `isPen`, `isComplete`, `isClosed`, `size`, `scale`, `scaleX/Y`.
  - `path` is "delta-encoded base64… first point Float32 (12 bytes)… subsequent points Float16 deltas (6 bytes each)". Migration v5 omits z when there's no pressure.
  - Rendering uses its own perfect-freehand-style outline. The license is proprietary.

**GoodNotes and Notability** are closed formats with vector ink, exported to PDF as paths **(unverified)**.

---

## 3. Inkscape (GPL-2+)

- **Pencil tool with pressure:**
  - It records pressure samples, fits the centerline with `bezier_fit_cubic_r`, and turns the pressure samples into Power Stroke knots `(t, width)` ([pencil-tool.cpp](https://gitlab.com/inkscape/inkscape/-/raw/master/src/ui/tools/pencil-tool.cpp)).
  - The SVG output is `<path d="…computed outline…" inkscape:original-d="…centerline…" inkscape:path-effect="#pe"/>` plus `<inkscape:path-effect effect="powerstroke" offset_points="t,w | t,w …" interpolator_type="CentripetalCatmullRom" start_linecap_type end_linecap_type linejoin_type scale_width sort_points …/>` ([lpe-powerstroke.cpp](https://gitlab.com/inkscape/inkscape/-/raw/master/src/live_effects/lpe-powerstroke.cpp)).
  - Other SVG viewers see only the baked `d`. Inkscape re-runs the LPE on edit.
- **Other modes:** BSpline and Spiro modes apply `bspline` and `spiro` LPEs to `original-d`. There's an optional Simplify LPE, and shape modes use Pattern Along Path ([freehand-base.cpp](https://gitlab.com/inkscape/inkscape/-/raw/master/src/ui/tools/freehand-base.cpp)).
- **Calligraphy tool:** a physical nib simulation (mass, drag, thinning, tremor, wiggle, angle from tilt, cap rounding) that outputs a **plain filled path**. No centerline or input is kept ([calligraphic-tool.cpp](https://gitlab.com/inkscape/inkscape/-/raw/master/src/ui/tools/calligraphic-tool.cpp)).
- **The pattern:** keep the source centerline and parameters, and bake the result for interop. That's a good template for Capy's SVG export.

---

## 4. Comparison of stroke data models

| System | Stored geometry | Per-point attributes | Per-stroke | Raw input kept? | Render approach | Partial erase | License |
|---|---|---|---|---|---|---|---|
| google/ink / Jetpack Ink | raw input columns, delta-coded (plus optional mesh) | x, y, t, pressure, tilt, orientation, barrel twist | Brush (family, color, size, epsilon), tool type, cm/unit, noise seed | **yes** (canonical) | tip-shape extrusion → triangle mesh, shader AA; particles = textured quads | mesh subtract/split (not serializable yet) | Apache-2.0 |
| Wacom UIM | Catmull-Rom spline plus separate sensor data | layout-masked: x, y, z, size, rotation, rgba, scale, offset, tangent | style (brush URI), seed, ts/tf, sensor mapping | **yes** (linked) | vector polygon brush or raster stamping | ts/tf ranges | Apache-2.0 (lib) |
| PencilKit | cubic uniform B-spline control points | location, timeOffset, size, opacity, force, azimuth, altitude, secondaryScale, threshold, lateralJitter | ink, transform, mask, seed, renderGroupID, renderState | no | Metal, per ink type | mask or substroke | proprietary |
| Windows Ink | points (Bézier fit at render) | position, pressure, tiltX/Y, timestamp | drawing attributes (tip, size, transform, pencil) | mostly | D2D ink (Bézier plus widths) | whole stroke | proprietary |
| Toonz .pli | thick quadratic chain | (x, y, thick) | style id, loop, outline options, group | no | outline strips plus GLU-tessellated regions | vector eraser cuts | BSD-3 |
| Harmony | pencil: centerline plus thickness; brush: contour | thickness profile | palette colour, texture mask | no | vector plus greyscale bitmap mask | cut, flatten | proprietary |
| Blender GP v3 | CurvesGeometry (poly, Bézier, Catmull-Rom, NURBS) | position, radius, opacity, vertex_color, rotation, delta_time | material, caps, softness, fill_id, fill_color, init_time | no (pressure baked) | per-segment quad plus capsule SDF; CDT fills | cut eraser, Carver boolean | GPL |
| Rnote | line, quad or cubic segments | pos, pressure | Smooth, Rough or Textured style (+ seed) | no | variable-width outline polygon, cached per stroke | segment-level split | GPL-3 |
| Xournal++ | polyline | x, y, absolute width | tool, color, fill, cap, ts | no | Cairo, per-segment line width | exact split | GPL-2+ |
| perfect-freehand, Excalidraw | raw points | x, y, pressure | width, streamline, variability | **yes** | outline polygon fill | — (whole element) | MIT |
| tldraw | raw points (f16 deltas) | x, y, z | size, isPen, scale | **yes** | outline polygon | — | proprietary |
| Inkscape | original-d plus LPE knots → d | width knots (t, w) | LPE params | no | SVG fill | path booleans | GPL-2+ |
| Moho | animated points and curves | pos, width (animated), curvature | shape fill/outline, style, brush | no | vector plus bitmap brush | — | proprietary |
| Synfig | spline plus width points | vertex width or free width points | tips, smoothness, dashes | no | software rasterizer | — | GPL-3 |

---

## 5. Lessons for a "record input, re-render on the fly" design

**What to record for each stroke:**
- **Columns** (struct-of-arrays, each optional):
  - x and y in document or layer space (f32).
  - t as ms since stroke start.
  - Normalized pressure 0–1 (**never baked width**; Xournal++ shows the cost).
  - Altitude and azimuth. Store the canonical forms; also keep tiltX/Y only if a host reports them natively. PencilKit uses azimuth and altitude; Windows uses tiltX/Y; ink uses tilt and orientation.
  - Barrel twist or roll (Apple Pencil Pro; ink `barrel_twist`; PencilKit v3).
- **Per-stroke header:**
  - Tool type (mouse, touch, pen, or eraser tip, since brush behavior differs).
  - Device-to-physical scale (ink `stroke_unit_length_in_centimeters`).
  - Absolute creation time (Blender `init_time`, PencilKit `creationDate`) for timelapse and replay.
  - **RNG seed**.
  - Brush reference plus version.
  - Colour, possibly as a palette reference (Toonz and Harmony make recolouring free).
  - Layer transform.
- **Don't store:** predicted points. Keep them only in a volatile tail, as ink's `InProgressStroke` does with fixed vs volatile inputs.

**Raw vs smoothed:**
- Store **raw, pre-modeler inputs** as the truth, the way google/ink and UIM do. Smoothing, stabilizer and fit parameters then belong to the brush or input model and can be re-tuned later.
- Geometric edits (control-point reshape, Harmony-style Pencil Editor thickness edits, Synfig width points) need an editable representation. Use UIM's pattern: an optional **derived spline override** that records its source-sample mapping, with an explicit rule that once the user has edited geometry, the override wins and the raw input becomes provenance only.
- Keep inputs even after mesh edits. Jetpack Ink's `subtract` keeps them, which is exactly why its erased results can't be regenerated from inputs yet. Record the erase as data, not only as a geometry side effect.

**Determinism and renderer drift:**
- Version everything:
  - A brush-format version: ink `min_version`, with `newer_brush_families` as the fallback chain.
  - A **stamp-generator version per stroke**.
  - A document content version, like `PKContentVersion`.
  - Migrations for the file format (Rnote's per-semver converters).
- Make the geometry deterministic in Rust:
  - Compute stamp positions, sizes, angles and jitter on the CPU from arc length.
  - Use counter-based RNG (hash of seed and stamp index) so a subrange re-renders identically.
  - Let the GPU only rasterize the stamp list. GPU float differences then only affect sub-pixel coverage.
- When an old engine version is retired, either keep the old code path or ship a **cached raster of the layer** in the file as a fidelity fallback. Harmony's textured strokes store their bitmap mask for exactly this reason.
- Avoid Excalidraw-style magic constants that aren't versioned, and Blender-4.5-style unversioned clamps.

**File size:**
- Delta-code and quantize each channel (ink uses `CodedNumericRun` with sint32 zigzag deltas and a float scale; tldraw uses Float16 deltas at about 6 B/point), then compress with zstd or gzip.
- Omit channels the device doesn't have.
- Rough estimate: at 240 Hz with x, y, t, p, tilt, azimuth and twist at 1–2 B each, that's about 7–14 B/point, or about 2–3 KB per second of drawing. 5,000 strokes × 150 points × 10 B is about 7.5 MB before compression.
- By comparison, JSON with 3 decimals (Rnote) is roughly 40–60 B/point.
- Deduplicate sub-epsilon moves at capture.

**Performance with thousands of strokes:**
- Never re-stamp everything per frame:
  - Cache per stroke (Rnote images, ink `PartitionedMesh`, Excalidraw WeakMap).
  - Better for a stamp engine: cache per **layer tile** at a few mip levels and invalidate by stroke bounding box through an R-tree (`rstar` in Rnote, a static R-tree in ink).
- Re-render dirty tiles in paint order, clipped to the tile, which only needs strokes intersecting it.
- Keep the live stroke on the same code path as replay so it looks identical after commit.

**Erasing:**
1. **Whole-stroke delete:** trivial.
2. **Parameter-range split:** recommended as the default. Store fragments as `(source stroke id, t0..t1)` over the *same* input, like WILL `ts/tf` and PencilKit `substroke(range:)` with `renderState`. Stamps keep their arc-length phase and seeded jitter, so the remainder is pixel-identical and undo is cheap.
3. **Soft or pixel erase of textured strokes:** a per-stroke or per-layer **alpha mask** (PencilKit `mask`; raster mask layers). Mesh booleans are hard to reconcile with stamps.
- Split at exact intersection parameters (Xournal++) rather than at segment granularity (Rnote).

**Brush-definition changes:**
- **Embed a snapshot** of each used brush, textures included, in the document (ink `texture_id_to_bitmap`, UIM brush definitions) and key it by `(brush uuid, content hash)`.
- When a user edits a library brush, existing strokes keep their snapshot unless they choose "update strokes to new brush". Offer a bulk relink.
- Split brush parameters by cost, as ink does:
  - Colour and paint-only changes: re-shade only.
  - Tip, size or epsilon changes: regenerate geometry.

**Fills (for animation-style line art):**
- Compute regions from stroke centerlines plus half-widths as a planar arrangement (Toonz; Blender's 5.2 Delaunay solver) with a gap-closing tolerance.
- Close gaps with invisible strokes (Harmony's 0–10 px, Toonz autoclose, Blender fill guides).
- Store fills as separate objects (Blender `fill_id` with even-odd holes) on a Colour-Art-style sublayer **under** the textured lines, so stamp edges don't leave halos.
- Keep a seed point or region fingerprint so fills survive re-computation, and persist the topology as Toonz's `INTERSECTION_DATA` does.

---

## 6. What Capy can reuse (repo is MIT OR Apache-2.0; `deny.toml` allows Apache, BSD, MIT, ISC and Zlib only)

**Compatible:**
- **google/ink** (Apache-2.0, C++20, Bazel, abseil, protobuf).
  - Best to adopt its **proto schemas** (`stroke_input_batch.proto`, `brush_family.proto`) as a reference or directly, for interop with Jetpack Ink files. `prost` can generate Rust types from them.
  - Its behavior-graph brush model and WGSL shaders are worth studying.
  - FFI through `cxx` is possible but heavy, and the maintainers give no API stability guarantee.
- **ink-stroke-modeler** (Apache-2.0) and **ink-stroke-modeler-rs** (MIT/Apache, partial port): drop-in input smoothing and prediction in Rust.
- **perfect-freehand** (MIT): the algorithm is tiny. Rust ports: [`freedraw`](https://github.com/ducflair/freedraw) (MIT, 1.0.4), [`perfect_freehand`](https://github.com/sibaiper/perfect_freehand) (MIT). Useful for an outline-polygon "ink" brush, cheap previews, hit-testing or SVG export.
- **Excalidraw** (MIT): code is usable, but it's mostly glue around perfect-freehand.
- **OpenToonz / Tahoma2D** (BSD-3): region computation and autoclose (`tcomputeregions.cpp`, `tl2lautocloser.cpp`), thick-quadratic fitting and `.pli` I/O are reusable as reference or port. The code is old C++ and Qt-coupled.
- **Wacom universal-ink-library** (Apache-2.0, Python): the format spec and protobuf definitions, useful for UIM and InkML interop.
- **Rust building blocks:** `kurbo` (curve fitting, offsets), `lyon` (tessellation), `i_overlay` / `clipper2` (booleans for vector erase and fills), `i_triangle` (CDT for fills), `rstar` (spatial index), `vello` / `tiny-skia` (outline rendering). All are MIT, Apache, or BSD.

**Study only (GPL or proprietary):**
- Rnote (GPL-3; the closest Rust architecture), Xournal++, Blender GP (its attribute schema is a good checklist), Inkscape (the LPE storage pattern), Krita, Synfig, and tldraw (proprietary).
- Implementing their **file formats** for import and export (`.xopp`, `.rnote`, SVG with `inkscape:original-d`) doesn't require their code.
