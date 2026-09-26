# Vector editors: path toolsets, conventions and data models (as of 2026-09-25)

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Scope: Illustrator, Inkscape, Affinity, Graphite, Linearity Curve, Figma, plus short notes on CorelDRAW, Boxy SVG, Vectorpea and Amadine. The report ends with a feature matrix, a tiered roadmap and the most-used operations.

Items with a link come from that source. Unlinked shortcut and behavior details are long-standing documented behavior that I know from background knowledge. Check them against the app before relying on the exact keys. Graphite claims come from its current source code on GitHub (`master`, fetched 2026-09-25).

---

## 0. What changed in 2024–2026

- **Affinity:** Canva bought Serif in March 2024. On 30 Oct 2025 it replaced Designer, Photo and Publisher with one app, "Affinity", which has Vector, Pixel and Layout "studios" and is free with no watermark or feature limits. The AI features (Generative Fill, Expand, Remove BG) need Canva Pro. It runs on Mac and Windows, with iPad still to come. The latest stable release is 3.3.0 (Sept 2026). ([Canva newsroom](https://www.canva.com/newsroom/news/all-new-affinity/), [Wikipedia](https://en.wikipedia.org/wiki/Affinity_(software)), [MacRumors](https://www.macrumors.com/2025/10/31/canva-relaunches-affinity-free-app/))
- **Illustrator:**
  - 29.0 (Oct 2024) added Objects on Path, made Mockup generally available, and added gradient support to Image Trace ([Adobe community](https://community.adobe.com/t5/illustrator-discussions/illustrator-max-2025-v29-0-align-object-to-path-image-trace-text-to-vector-graphic-and-more/td-p/14914853)).
  - The Dimension tool arrived in 28.1 and gained sticky dimensions and custom scales in 2025 ([Adobe](https://helpx.adobe.com/illustrator/desktop/measure-and-align/plot-and-measure/about-dimension-objects.html)).
  - The 2026 releases added Generative Shape Fill and multi-model Text to Vector (Firefly Vector 4, GPT Image, Gemini). Launch is up to 3x faster and incremental saves up to 6x faster ([Vectortwist](https://vectortwist.com/adobe-illustrator-2026-new-features/)).
  - Turntable, which generates rotated views of 2D art, is now generally available ([Fast Company](https://www.fastcompany.com/91518262/adobe-illustrator-turnable-launching)).
- **Inkscape:**
  - 1.3 (Jul 2023) added the Shape Builder, Fracture/Flatten and a new LPE dialog ([1.3 notes](https://wiki.inkscape.org/wiki/Release_notes/1.3)).
  - 1.4 (Oct 2024) let the Shape Builder cut rasters, added modular and axonometric grids and .afdesign import, and changed node deletion ([1.4 notes](https://wiki.inkscape.org/wiki/Release_notes/1.4)).
  - 1.5 is still in development as of Aug 2026. It brings a GTK4 port, a new renderer, OkLab/OkLch color management, a CapyPDF export engine, lasso object selection and a node "confine-to-path" (Alt) mode ([1.5 notes](https://wiki.inkscape.org/wiki/Release_notes/1.5)).
- **Graphite:**
  - 2024 brought boolean ops, then non-destructive path editing and a new render engine ([blog](https://graphite.art/blog/)).
  - The desktop apps (Win/Mac/Linux) reached release candidate, RC2 on 6 Jan 2026 ([graphite.art](https://graphite.art/)).
  - A 2026 update added a vector blend tool, a gradient tool with midpoint and spread controls, and panel docking.
  - Keyframe animation is planned for late 2026 ([features](https://graphite.art/features/)).
- **Figma:**
  - Figma Draw (7 May 2025) added a shape builder, lasso, multi-point editing, better booleans and outline stroke, brushes, text on path, pattern fill and noise/texture ([Figma blog](https://www.figma.com/blog/introducing-figma-draw/)).
  - July 2025 added Simplify and Offset path, plus typed and multi-selected variable-width points ([forum](https://forum.figma.com/product-updates-3/icymi-check-out-all-of-the-recent-updates-from-the-july-25-release-notes-livestream-43455)).
  - AI "Vectorize" (image to vector) arrived on 4 Feb 2026 ([92learns](https://blog.92learns.com/figma-vs-illustrator-vectorize/)).
- **Linearity Curve** (formerly Vectornator, renamed Jul 2023):
  - Freemium plans since Feb 2024 and Auto Trace presets (Sketch, Photo, Illustration, Basic Shapes) since Jul 2024.
  - "Cleaner path editing" in 5.34 (Jun 2025) and local/iCloud file saving in Dec 2025.
  - "Path bending" in 6.10 (May 2026).
  - It is Apple-only ([What's new](https://www.linearity.io/whats-new/)).
- **CorelDRAW:**
  - 2025 added CorelDRAW Web and a better Painterly Brush ([Corel](https://www.coreldraw.com/en/blog/product/coreldraw-graphics-suite-2025/)).
  - 2026 (6 Mar 2026) was mostly AI features and 3x faster launch ([Softprom](https://softprom.com/coreldraw-graphics-suite-2026-new-features)).
  - On 22 Sep 2026 Corel added text-to-vector AI and a PowerTRACE that is 4x faster ([TEXINTEL](https://www.texintel.com/press-room/corel-09-26-p-new-ai-vector-graphics-tools-power-creative-workflows-in-coreldraw-graphics-suite-2026)).

**Trend:** new work in the incumbents is mostly AI generation and performance. Figma and Graphite are where the path-editing UX is actually changing: vector networks, segment "bend/mold", non-destructive graphs and variable width in a UI tool.

---

## 1. Adobe Illustrator (the reference implementation)

**Pen (P)** ([Adobe](https://helpx.adobe.com/illustrator/using/tool-techniques/pen-tool.html), [Tuts+](https://design.tutsplus.com/tutorials/illustrators-pen-tool-the-comprehensive-guide--vector-141), [LucasFonts](https://www.lucasfonts.com/learn/mastering-pen-tool)):
- Click makes a corner point. Click-drag makes a smooth point with mirrored handles.
- Shift constrains to 45°.
- Alt/Opt while dragging breaks the handle into a cusp. Alt over an anchor gives the Anchor Point tool.
- Holding Space with the mouse still down repositions the anchor being placed.
- Ctrl/Cmd gives a temporary Direct Selection tool.
- The cursor changes to show close ("o") or continue from an endpoint ("/").
- The Pen adds or deletes anchors automatically when hovering a path.
- Rubber Band shows a preview of the next segment.
- Click the last anchor to retract its out-handle.

**Other drawing tools:**
- **Curvature (Shift+~):** click makes smooth points and double-click or Alt-click makes corners; curves are fitted automatically ([Adobe](https://helpx.adobe.com/illustrator/using/tool-techniques/curvature-tool.html)).
- **Pencil (N):** a Fidelity slider (Accurate↔Smooth), "edit selected paths", and Alt for straight segments.
- **Smooth tool** and **Path Eraser** ([Adobe](https://helpx.adobe.com/illustrator/using/tool-techniques/smooth-tool.html)).
- **Paintbrush (B)** applies Calligraphic, Scatter, Art, Pattern and Bristle brushes.
- **Blob Brush (Shift+B)** paints filled shapes that merge with same-colored neighbors.
- **Shaper (Shift+N)** recognizes gestures as shapes and uses scribble to combine or cut.

**Node editing:**
- Direct Selection (A), Group Selection and Lasso (Q).
- Anchor Point tool (Shift+C) toggles corner/smooth and breaks handles.
- Add anchor (+) and Delete anchor (−).
- Node types are only **corner** and **smooth** (colinear handles whose lengths can differ). There is no explicit symmetric or auto type.
- **Live Corners** widgets on any corner give round, inverted-round or chamfer corners with a radius.
- The **Contextual Task Bar** for a path offers Simplify, Smooth, remove anchors, connect endpoints, cut at anchors and convert to corner/smooth ([Adobe](https://helpx.adobe.com/illustrator/using/contextual-task-bar.html)).

**Cut, join and clean up:**
- Scissors (C), Knife, Eraser (Shift+E) and Reshape.
- Join (Ctrl+J), Average (Ctrl+Alt+J), and a **Join tool** that trims and joins overlapping ends with a scrub gesture.
- **Simplify** uses an automatic slider ([Adobe](https://helpx.adobe.com/illustrator/desktop/draw-shapes-and-paths/modify-paths/auto-simplify-paths.html)).
- **Offset Path** exists as a destructive object command and as a live effect.
- **Outline Stroke**, Expand and Expand Appearance.

**Booleans and region tools:**
- The Pathfinder panel has Shape Modes (Unite, Minus Front, Intersect, Exclude). Alt-click makes a live compound shape.
- Its Pathfinders (Divide, Trim, Merge, Crop, Outline, Minus Back) are destructive.
- **Shape Builder (Shift+M):** drag across regions to merge them, Alt to delete ([Adobe](https://helpx.adobe.com/illustrator/using/tool-techniques/shape-builder-tool.html)).
- **Live Paint Bucket (K):** fills faces and edges of overlapping paths with gap detection and never restructures the paths.

**Width:**
- **Width tool (Shift+W)** adds width points (position along the path plus left/right widths) with on-canvas handles.
- Width shapes can be saved as **Variable Width Profiles** and reused on any stroke, and they also drive Art and Pattern brush width ([Adobe](https://helpx.adobe.com/illustrator/using/tool-techniques/width-tool.html)).

**Shapes:**
- Rectangle (M), Rounded Rectangle, Ellipse (L), Polygon, Star, Line (\), Arc, Spiral, Rectangular Grid, Polar Grid and Flare.
- **Live Shapes** keep their parameters: corner radius per corner, polygon side count, pie angles.

**Transform, align and repeat:**
- Bounding box, Rotate (R), Reflect (O), Scale (S), Shear, Free Transform (E), Puppet Warp, Envelope Distort and Transform Each.
- **Repeat** (radial, grid, mirror) is live.
- The Align panel supports a key object and distribute spacing.
- Objects on Path (2024) and the Dimension tool.

**Paint:**
- Gradients: linear, radial and **freeform** (points and lines), and gradients on a stroke (along or across).
- **Gradient Mesh (U)** and Blend (W).
- Recolor/Generative Recolor.

**Structure:** clipping masks (Ctrl+7), compound paths (Ctrl+8), opacity masks and Draw Inside.

**Stroke:**
- Butt, round and projecting caps; miter, round and bevel joins with a miter limit.
- Align stroke inside, center or outside (closed paths only).
- Dashes with "align to corners/ends".
- Arrowheads with scale and tip/end placement.
- Width profiles and brushes along the path.

**Text:** Type on a Path, Area Type, Retype and Create Outlines.

**Trace:** Image Trace (presets, now with gradient, shape and transparency modes) and Text to Vector Graphic.

**Snapping:** Smart Guides (Ctrl+U), snap to point, pixel, grid and glyph, and Outline mode (Ctrl+Y).

**Typical workflows:** logos and brand marks, icons, editorial illustration, print and packaging (Dimension tool, Mockup), infographics, textile and patterns, lettering, and cut/plot files.

**What Illustrator does distinctively:**
- **Shape Builder** and **Live Paint** give region-based building on top of overlapping paths.
- **Width tool plus profiles**.
- The **Appearance panel**.
- The widest Pathfinder set.
- A big third-party plugin ecosystem (Astute Graphics and others).

**Data model:**
- A path is an ordered list of anchors, each with in/out handles and a smooth flag, and can be open or closed. A compound path is several subpaths with a fill rule.
- Each object, group or layer has an **appearance stack**: any number of fills and strokes, each with its own opacity and blend mode, plus live effects applied per attribute (Offset Path, Roughen, Zig Zag, Warp, Transform). Effects stay non-destructive until "Expand Appearance".
- Graphic Styles save whole stacks ([Smart Notes](https://adobeillustratorsmartnotes.com/illustrator/appearances/appearances.html), [Noble Desktop](https://www.nobledesktop.com/learn/illustrator/exploring-the-appearance-panel-in-adobe-illustrator-advanced-styling-with-multiple-strokes)).
- Width data is a list of width points attached to a stroke attribute, not to the geometry.

---

## 2. Inkscape 1.3 / 1.4 (1.5 in development)

**Pen/Bézier (B):**
- Five modes: **Regular Bézier, Spiro, BSpline, straight segments and Paraxial** ([manual](https://inkscape-manuals.readthedocs.io/en/latest/pen-tool.html)).
- Click makes a node and drag makes handles. Enter or right-click finishes the path, Backspace removes the last node, Ctrl snaps angles (15° steps), and Shift appends to the selected path.
- The **Shape** option adds an effect to the drawn path: triangle in or out (Power Stroke or Taper), ellipse, from clipboard (Pattern Along Path) or bend from clipboard.
- 1.4 added a "draw guides" option ([1.4 notes](https://wiki.inkscape.org/wiki/Release_notes/1.4)).

**Pencil (P):** a smoothing slider, the same Spiro and BSpline modes, and a **Pressure** option that makes a Power Stroke with min and max widths.

**Calligraphy (C):** maps pressure and tilt to filled outlines.

**Node tool (N):**
- Four node types: **Cusp (Shift+C), Smooth (Shift+S), Symmetric (Shift+Y) and Auto-smooth (Shift+A)**.
- Insert nodes with Insert or a double-click. Join (Shift+J), Break (Shift+B), join with a segment, delete a segment.
- Segment to line (Shift+L) or to curve (Shift+U). Drag a segment to bend it.
- Alt-drag lasso and a Corners-LPE button on the toolbar (1.3).
- 1.3 added curve-fitting node deletion (from FontForge), and 1.4 changed Alt+double-click to straighten a segment.
- 1.5 adds **Alt to slide nodes along the path** and a node-distance field ([1.5 notes](https://wiki.inkscape.org/wiki/Release_notes/1.5)).

**Path menu:**
- Union (Ctrl++), Difference (Ctrl+−), Intersection (Ctrl+*), Exclusion (Ctrl+^), Division (Ctrl+/) and Cut Path (Ctrl+Alt+/).
- Combine (Ctrl+K) and Break Apart (Ctrl+Shift+K), plus Split Path, **Fracture** and **Flatten** (1.3).
- Object to Path (Ctrl+Shift+C), Stroke to Path (Ctrl+Alt+C), Simplify (Ctrl+L) and Reverse.
- Inset and Outset (Ctrl+( and Ctrl+)), and **Dynamic** (Ctrl+J) and **Linked** (Ctrl+Alt+J) offsets.
- **Shape Builder (X)**, 1.3: click adds a region and Shift-click removes one ([1.3 notes](https://wiki.inkscape.org/wiki/Release_notes/1.3)).
- **Paint Bucket (U)** rasterizes the view, flood-fills a region and traces the result back into a path.

**Live Path Effects (LPEs, non-destructive and stackable)** ([manual](https://inkscape-manuals.readthedocs.io/en/latest/live-path-effects.html)):
- Stroke and width: **Power Stroke** (variable width with on-canvas knots) and **Taper Stroke** (1.4 added clamped tips).
- Curve construction: **BSpline** and **Spiro**.
- Corners and offsets: **Corners (Fillet/Chamfer)** and Offset.
- Placement and deformation: Pattern Along Path, Bend, Envelope/Perspective and Lattice Deformation.
- Organic and sketchy looks: Roughen, Sketch and Hatches.
- Copies and symmetry: Knot, Interpolate Sub-Paths, Mirror Symmetry, Rotate Copies and Tiling.
- Also a **Boolean Operation LPE** (a live boolean), Slice, Simplify, Dashed Stroke, Measure Segments, Fill Between Many, Clone Original and Ruler.
- The 1.3 dialog sorts effects into Favorites, Edit/Tools, Distort, Generate, Convert and Experimental.

**Shapes:** Rectangle (R) with rx/ry, Ellipse and arc (E), Star/Polygon (*) with rounding and randomization, Spiral (I) and 3D Box. All stay parametric.

**Other tools:** Tweak (W), Spray (A), Eraser, Measure (M), Connector (O), Gradient (G) with linear and radial gradients, Mesh gradient (SVG2 draft) and Pattern editor (1.3).

**Structure and styling:** clip and mask, clones (Alt+D), tiled clones, symbols, markers (arrowheads, editable on canvas), dashes, `paint-order`, Text > Put on Path, and Flow into Frame.

**Trace Bitmap:** Potrace (brightness, edges, color multi-scan), Autotrace centerline and **Pixel Art** (Kopf-Lischinski).

**Snapping and layout:** a simple/advanced snap popover, rectangular, axonometric and modular grids (1.4), guides, Align & Distribute, the Transform dialog and multipage documents.

**Typical workflows:** web SVG, logos and icons, **laser, plotter and Cricut cut files** (HPGL/DXF, AxiDraw extensions), diagrams and maps, and scientific figures.

**What Inkscape does distinctively:**
- The **LPE stack**, including Power Stroke, BSpline/Spiro drawing and Fillet/Chamfer.
- An SVG-native XML editor.
- Python extensions.
- Pixel-art tracing.
- Clones and tiled clones.

**Data model:**
- Everything is SVG. Each element has **one fill and one stroke**, with `paint-order` and markers. Multiple strokes need duplicates or clones.
- An LPE keeps `inkscape:original-d` and writes its output to `d`, with parameters in `<inkscape:path-effect>`, so the file still renders in other SVG apps.
- **Power Stroke turns a stroke into a filled outline.** Its width knots are stored as (t, offset) pairs with interpolators (CubicBezierSmooth, CentripetalCatmullRom, Spiro…).
- Primitives keep parameters in `sodipodi:` attributes.

---

## 3. Affinity: Designer 2 and the 2025 unified "Affinity" (Vector studio)

**Tools** ([Dezign Ark tool list](https://dezignark.com/blog/the-ultimate-guide-to-all-68-tools-in-affinity-3/)):
- Selection and structure: Move, Artboard, Node, **Point Transform**, **Corner** and **Contour**.
- Drawing: Pen, Pencil, **Path Brush** (vector brush), **Knife** and **Stroke Width**.
- Paint: Fill, Transparency and **Vector Flood Fill** ([help](https://www.affinity.studio/help/tools-tools-vector-flood-fill/)).
- Shapes and text: Shape, **Shape Builder**, Artistic and Frame Text, Place and Vector Crop.
- Measuring and sampling: **Measure**, **Area**, Colour and Style pickers.

**Pen (P):**
- Four modes: **Pen, Smart (automatic smooth curves), Polygon and Line** ([Tuts+](https://design.tutsplus.com/tutorials/how-to-use-pen-and-node-tools-in-affinity-designer--cms-108796)).
- Click makes a corner and drag makes a curve. Alt/Opt toggles between sharp and smooth or breaks handles. Space repositions a node mid-drag, and Shift constrains.

**Pencil (N):** a stabilizer (window or rope), pressure, and a sculpt mode that edits a selected curve.

**Path/Vector Brush:** textured raster images stretched or repeated along a vector path, with pressure and stabilizer.

**Node tool (A):**
- Three node types: **Sharp, Smooth and Smart** (auto).
- Actions: Break, Close, Join, Reverse, Smooth curve and convert to curves.
- Drag a segment to bend it.
- Transform mode for the selected nodes.

**Corner tool:** non-destructive per-node corners (rounded, concave or straight/chamfer) with a radius, on any curve.

**Contour tool:** interactive inward or outward offset with join types, plus Expand Stroke.

**Knife (with Scissors):** cuts along a drawn stroke or at a node.

**Booleans:** Add, Subtract, Intersect, Xor, Divide and Combine. Alt-click makes a **live compound**, and the **Shape Builder** (2.0) adds and subtracts regions interactively.

**Other 2.x features:**
- **Vector Warp**: a non-destructive mesh or perspective warp on vectors and text.
- X-ray view, DXF/DWG import, and Measure and Area tools ([Tuts+ v2](https://design.tutsplus.com/articles/top-6-new-features-from-the-affinity-version-2-announcement--cms-93501)).
- The **Stroke Width tool** (2.5, May 2024) edits the pressure profile on canvas ([AlternativeTo](https://alternativeto.net/news/2024/5/affinity-2-5-brings-variable-fonts-qrcode-tool-stroke-width-tool-and-native-arm64-support)).

**Shapes:** a large parametric library (rectangle with per-corner type and radius, rounded rectangle, ellipse, triangle, diamond, trapezoid, polygon, star, double/square star, donut, pie, segment, arrow, cog, crescent, heart, callout, tear, cloud and more). Each keeps smart handles until it is converted to curves.

**Paint:** linear, elliptical, radial, conical and bitmap fills, and a basic mesh fill.

**Structure:** clipping by nesting a layer as a child, and masks.

**Stroke panel:** caps, joins, dashes, arrowheads, alignment, a **pressure graph**, "scale with object" and "stroke behind fill".

**Appearance panel:** multiple fills and strokes per object.

**Text and snapping:** text on path. A snapping manager with candidate snapping and pixel alignment. Symbols and constraints.

**Image trace:** Affinity has historically had no native image trace. It is a long-standing request, and I could not confirm it exists in v3.

**Typical workflows:**
- Illustration, especially mixed with raster: pixel layers and raster brushes in the same document.
- Logos, icons and UI mockups.
- Print, using Layout in the same app.

**What Affinity does distinctively:**
- The **Vector, Pixel and Layout studios in one document**, where you can raster-paint inside a vector file. This is the closest precedent for Capy Canvas.
- The Corner tool.
- The rich parametric shapes.
- Raster-textured vector brushes.
- Stabilizers.
- A free price since Oct 2025.

**Data model:**
- A curve is a set of subpaths whose nodes carry handles and a node type.
- Corner rounding is stored non-destructively per node.
- **Width is a single pressure profile**, a 1D curve over normalized path length, attached to the stroke rather than to the nodes. The Width tool edits that curve.
- A brush stroke is a path plus a reference to a raster texture.
- Vector Warp and compound booleans are live.

---

## 4. Graphite (graphite.rs, Rust, Apache-2.0)

This is the most relevant architecture for a Rust core.

**Status:** alpha. The web app runs at editor.graphite.art and the desktop RC runs on Win/Mac/Linux. It is vector-first, and raster brushing is experimental, laggy and disabled by default on desktop ([It's FOSS](https://itsfoss.com/graphite-graphics-editor/), [XDA, Sep 2026](https://www.xda-developers.com/stopped-exporting-from-photoshop-illustrator-one-open-source-app-now-handles-both-jobs/)).

**Tools and shortcuts** (from `editor/src/messages/input_mapper/input_mappings.rs`): the key mapping deliberately follows Illustrator.
- Select (V), Path (A), Pen (P), Freehand (N) and Spline.
- Shape (Y), with Rectangle (M), Ellipse (E) and Line (L) on their own keys, plus Polygon, Star, Arc, Spiral, Grid, Arrow and Circle (`ShapeType` in `shape_tool.rs`).
- Text (T), Fill (F), Gradient (H), Eyedropper (I), Brush (B), Artboard and Navigate (Z).

**Pen modifiers** ([tracking issue #1870](https://github.com/GraphiteEditor/Graphite/issues/1870); `HandleMode` in `pen_tool.rs`):
- Handle modes are Free, ColinearLocked and ColinearEquidistant.
- C toggles colinear handles, Alt makes them equidistant, Ctrl locks the angle and Shift snaps to 15°.
- Space moves the anchor while dragging, Tab swaps to the other handle, and G/R/S grab, rotate or scale the outgoing handle.
- Backspace deletes a handle.
- A Polyline mode and a Spline mode have been proposed ([PR #2368](https://github.com/GraphiteEditor/Graphite/pull/2368)).

**Path tool:**
- Click a segment to insert a point, Alt-click to delete the segment, and drag a segment to **mold** it (`molding_segment` in `path_tool.rs`).
- Double-click toggles smooth or sharp.
- R/S rotate or scale a handle about its anchor.

**Booleans:** Union, Subtract Front, Subtract Back, Intersect and Difference, as a **non-destructive node** (`node-graph/nodes/path-bool`).

**Procedural vector nodes** (`vector_nodes.rs`):
- Path editing and cleanup: Offset Path, Round Corners, Bevel, Simplify, **Solidify Stroke** (outline stroke), Spline, Dash Pattern, Cut Path, Cut Segments, Close Path, Separate Subpaths, Merge by Distance, Decimate, Relax Points and Auto Tangents.
- Blending and deformation: Morph (blend), Box Warp and Extrude.
- Instancing and point generation: Copy to Points, Scatter, Jitter, Poisson-disk, Voronoi and Triangulate.
- Measurement and sampling: Position/Tangent on Path, Path Length, Area, Centroid and Sample Polyline.
- Also repeat/instancing, 20+ string/regex nodes and a QR code node.

**Roadmap:** a non-destructive shape builder, vector mesh, image trace, text on path and per-glyph styles ([features](https://graphite.art/features/)).

**Typical workflows:** procedural and generative graphics (radial repeats, scatter), parametric design, motion graphics, and tinkering. It is not ready for production print work: no CMYK, no .ai import and no mesh gradients.

**What Graphite does distinctively:**
- **Every tool action writes into a node graph.** The layer panel and node graph are two views of one document, and you can toggle between them.
- Edits stay non-destructive: a hand-drawn path is a node holding a list of edits.

**Data model** (from `vector_types.rs`, `vector_attributes.rs`, `vector_modification.rs` and `style.rs`):
- `Vector` is **a graph, not a chain**. `PointDomain` holds ids and positions. `SegmentDomain` holds id, start and end point indices and `BezierHandles::{Linear, Quadratic, Cubic}`. `colinear_manipulators` lists the handle pairs locked at 180°.
- Fills are computed from the **faces of the segment graph**, the same idea as Figma's vector networks. The domains are meant to take custom attributes later.
- User edits are stored as `VectorModification` deltas (InsertPoint, InsertSegment, SetHandles, ApplyPointDelta…) inside a Path node.
- Styling uses `FillChoice` (none, solid or gradient) and **one `Stroke`**: weight, dash lengths and offset, cap, join, miter limit, **align** (inside, center or outside), a transform and `PaintOrder`. More strokes come from stacking nodes.
- The workspace `Cargo.toml` pulls in `kurbo` 0.13 for geometry and `vello` 0.10 on `wgpu` 29 for rendering.

---

## 5. Linearity Curve (Mac/iPad/iPhone)

**Tools and keys** ([shortcuts](https://www.linearity.io/academy/curve/mac/user-guide/shortcuts-and-gestures/tools-shortcuts/)): Selection (V), Node (A), Scissors (C), Pen (P), Pencil (N), Brush (B), Text (T), Rectangle, Line and Oval (R/L/O), Eraser (E), Eyedropper (I) and Shape Builder (M).

**Pen:**
- Click or tap makes a node, and drag creates handles.
- ⌥ makes a **disconnected** node, ⇧ makes an **asymmetric** node at 45°, and ⇧⌥ snaps a single handle.
- Double-click the last node to finish, or click the first node to close ([drawing tools](https://www.linearity.io/academy/curve/mac/user-guide/vector-editing/drawing-tools/)).

**Node types:** Single (no handles), **Mirror**, **Asymmetric** and **Disconnected**, with a corner radius per node.

**Path operations:** Open/Close, Join, Combine (compound), **Outline Path**, **Offset Path**, Reverse, boolean operations, Mask and the Shape Builder ([editing tools](https://www.linearity.io/academy/curve/mac/user-guide/vector-editing/editing-tools/)).

**Freehand:**
- The **Brush** draws variable-width freeform paths with pressure, roundness, angle, minimum width, and saved custom brushes.
- The Pencil has a smoothness slider.

**Shapes:** rectangle with radius, oval, polygon, line, star and spiral (with decay).

**Auto Trace:** presets for Sketch, Photo, Illustration and Basic Shapes (2024).

**Other:** background removal, Path bending (2026), and the separate Linearity Move animation app.

**Typical workflows:** iPad illustration with Apple Pencil, social and marketing graphics from templates, quick logos, and bringing Procreate sketches in through Auto Trace.

**What Linearity does distinctively:** **Auto Trace from raster sketches** (the Procreate-to-vector pipeline) and a touch-first UI.

**Data model:** not publicly documented. The four node types map to handle constraints (mirrored, same angle, independent).

---

## 6. Figma (vector networks, plus Figma Draw since 2025)

**Vector networks** ([Figma blog](https://www.figma.com/blog/introducing-vector-networks/), [engineering deep-dive](https://alexharri.com/blog/vector-networks)):
- A segment can join **any two points**, so three or more segments can meet at one vertex and paths can branch. There is no drawing direction.
- Caps and joins render correctly at 3-way junctions.
- Fillable **regions** are found automatically by minimal-cycle detection after splitting segments at intersections. Each region is filled on its own with the **Paint bucket (Shift+B)**, which replaces winding-number reasoning.
- The Pen connects to any existing point or segment, and Esc leaves the path open ([help](https://help.figma.com/hc/en-us/articles/360040450213-Vector-networks)).

**Vector edit mode (Enter)** ([help](https://help.figma.com/hc/en-us/articles/360039957634-Edit-vector-layers)):
- Tools: Move (V), Pen (P), **Bend** (hold Cmd/Ctrl: drag a curve directly and the handles are solved for you), Paint (Shift+B), **Lasso (Q)**, **Cut (X)**, **Eraser (Shift+E)**, **Shape builder** and **Variable width**.
- Handle mirroring has three options: none, angle, or angle and length.
- Corner radius per point, and multi-point bounding-box transforms.

**Caps:** round and square caps plus six arrow or marker caps, set **per endpoint**.

**Booleans:** Union, Subtract, Intersect and Exclude are **live boolean groups** until you Flatten them (Cmd+E).

**Path cleanup:** Outline stroke, plus **Simplify** and **Offset** (Jul 2025) ([Simplify](https://help.figma.com/hc/en-us/articles/33792593975575-Simplify-a-vector-path), [Offset](https://help.figma.com/hc/en-us/articles/33792861450263-Offset-a-vector-path)).

**Figma Draw:**
- Pencil and Brush, with Stretch and Scatter brush types.
- Variable width with Tab to step between width points, typed values and multi-select.
- Text on a path, pattern fill, noise and texture, progressive blur and repeat transforms ([Figma Draw](https://www.figma.com/blog/introducing-figma-draw/)).

**Shapes:** rectangle with per-corner radius and **corner smoothing** (squircles), ellipse with arc sweep and inner ratio, polygon, star with ratio, line and arrow.

**Style:** multiple fills and strokes per layer, stroke alignment (inside, center or outside), per-side stroke weights, and linear, radial, angular and diamond gradients. There are no mesh gradients.

**Tracing:** AI Vectorize (Feb 2026).

**Typical workflows:** UI icons, design-system assets, product illustration, and light illustration through Figma Draw.

**What Figma does distinctively:** vector networks, the Bend interaction, region paint bucket, per-endpoint arrowheads, live booleans and squircle corners.

**Data model:**
- A layer holds a vertex list, a segment list (with handles), regions (loops with their own fills and winding rule), and per-vertex corner radius and cap.
- Paints are an array of fills and an array of strokes.
- SVG export has to split the network into several `<path>` elements, and export can change its look.

---

## 7. Brief notes on the others

**CorelDRAW:**
- Drawing tools: Freehand (F5), 2-Point line, **Bézier**, **Pen**, **B-Spline**, Polyline, 3-Point Curve, **Smart Drawing** (shape recognition) and **LiveSketch** (fits curves to strokes).
- **Shape tool (F10)** node types: **cusp, smooth and symmetrical**, with join/break, reduce nodes and elastic mode.
- Cutting and sculpting: Knife, **Virtual Segment Delete**, Eraser, and Smudge, Roughen, Twirl, Attract and Repel.
- Shaping: Weld, Trim, Intersect, Simplify, Front minus back, Back minus front and Boundary.
- **Smart Fill** fills enclosed regions, like Live Paint.
- Corner and effect tools: Fillet, Scallop and Chamfer; live Contour, Blend, Envelope, Distort and Extrude; **PowerClip** for clipping.
- **Artistic Media** gives variable-width, pressure and brush strokes.
- **PowerTRACE** traces bitmaps; the pixel-based Painterly brush and AI text-to-vector are new ([Corel](https://www.coreldraw.com/en/product/coreldraw/)).
- Typical workflows: sign-making and vinyl cutting, engraving, apparel and screen printing, and print shops.

**Boxy SVG:**
- An SVG-native web and desktop editor (Win/Mac/Linux/ChromeOS; v4.70, May 2025).
- Pen, **quadratic and cubic spline tools** and an **Arc tool**, booleans, text along a path, a code editor, and a history slider ([Wikipedia](https://en.wikipedia.org/wiki/Boxy_SVG), [site](https://boxy-svg.com/)).
- Used mainly for web SVG and icons.

**Vectorpea:**
- A browser-based vector editor by the makers of Photopea, laid out like Illustrator. It opens and saves **AI, PDF and SVG**, is free with ads, and needs no account ([Vectorpea](https://www.vectorpea.com/), [Korben](https://korben.info/en/vectorpea-vector-graphics-editor-photopea.html)).
- It shows that familiar Illustrator conventions alone make a vector editor usable.

**Amadine (BeLight; Mac, iPad, iPhone):**
- Pen, Pencil, a **Width tool with saved profiles**, pressure strokes (Wacom and Apple Pencil) and Image Trace (paid).
- Isometric, dimetric and trimetric grids, and glows, shadows and blur ([App Store](https://apps.apple.com/us/app/amadine-vector-design-art/id1339198386)).
- Paid for with a one-time lifetime license.

---

## 8. Feature matrix

Legend:
- ● a first-class tool or command
- ◐ available, but secondary, menu-only or through an effect or LPE
- ○ absent or very limited
- p planned

| Feature | AI | Inkscape | Affinity | Graphite | Linearity | Figma | Corel |
|---|---|---|---|---|---|---|---|
| Bézier pen (click / drag) | ● | ● | ● | ● | ● | ● | ● |
| Auto-curve pen (Curvature / Spiro / BSpline / Smart / Spline) | ● Curvature | ● Spiro, BSpline | ● Smart | ● Spline | ○ | ○ (Bend) | ● B-spline, LiveSketch |
| Node tool with node-type switching | ● 2 types | ● 4 types | ● 3 types | ● | ● 4 types | ● mirror modes | ● 3 types |
| Drag a segment to bend it | ◐ | ● | ● | ● (mold) | ● (2026) | ● Bend | ● |
| Scissors / knife | ● | ◐ Break / Cut Path | ● | ◐ node | ● | ● Cut | ● |
| Join, close, reverse | ● | ● | ● | ● | ● | ● | ● |
| Simplify / smooth | ● | ● | ◐ | ● node | ◐ | ● (2025) | ● |
| Offset path | ● (+ live) | ● (+ dynamic / LPE) | ● Contour | ● node | ● | ● (2025) | ● Contour |
| Outline stroke / expand | ● | ● | ● | ● | ● | ● | ● |
| Booleans (4 core + divide) | ● | ● | ● | ● (no divide) | ● | ◐ (4) | ● |
| Live / non-destructive booleans | ◐ compound shape | ◐ LPE | ● | ● | ○ | ● | ○ |
| Shape Builder | ● | ● (1.3) | ● | p | ● | ● (2025) | ○ |
| Region fill (Live Paint style) | ● | ◐ (raster bucket) | ● | ● (face fill) | ○ | ● | ● Smart Fill |
| Width tool / profiles | ● | ◐ Power Stroke | ● (2.5) | ○ | ◐ brush | ● (2025) | ◐ |
| Pressure → vector width | ● | ● | ● | ○ | ● | ● | ● |
| Brushes along path (art / scatter / pattern) | ● | ◐ LPE | ● raster-textured | ○ | ◐ | ● (2025) | ● |
| Pencil with smoothing | ● | ● | ● stabilizer | ● | ● | ● | ● |
| Parametric rect / ellipse / polygon / star / spiral | ● | ● | ● (many) | ● | ● | ● (no spiral) | ● |
| Per-corner radius on any path | ● Live Corners | ◐ Corners LPE | ● Corner tool | ● node | ● | ● | ● Fillet |
| Align / distribute | ● | ● | ● | ● | ● | ● | ● |
| Linear / radial gradient | ● | ● | ● | ● | ● | ● | ● |
| Mesh / freeform gradient | ● | ● mesh | ◐ | ○ p | ○ | ○ | ● |
| Clipping mask | ● | ● | ● | ● | ● | ● | ● PowerClip |
| Compound paths | ● | ● | ● | ● | ● | ● (network) | ● |
| Multiple fills / strokes per object | ● | ○ | ● | ◐ nodes | ○ | ● | ○ |
| Dashes / caps / joins / arrowheads | ● | ● | ● | ◐ no arrows | ◐ | ● per endpoint | ● |
| Text on path | ● | ● | ● | p | ◐ | ● (2025) | ● |
| Image trace | ● | ● | ○ | p | ● | ● AI (2026) | ● |
| Smart guides / snapping | ● | ● | ● | ● | ● | ● | ● |
| Blend / morph | ● | ◐ LPE | ○ | ● (2026) | ○ | ○ | ● |
| Procedural / effect stack | ◐ effects | ● LPE | ◐ | ● graph | ○ | ◐ | ◐ |

Boxy SVG and Vectorpea cover the ● rows for pen, node, booleans, shapes, text on path and gradients. They lack width, shape-builder and region-fill tools.

---

## 9. Tiers

### MVP: table stakes that every editor has

1. **Selection (V)** with the bounding-box move, scale and rotate gestures, plus group/ungroup, z-order and layers.
2. **Pen (P)** with the de facto conventions:
   - Click makes a corner and drag makes a smooth point with mirrored handles.
   - **Alt/Opt breaks a handle**, **Shift snaps the angle** (45° in Adobe, 15° in Graphite and Inkscape), and **Space repositions** the anchor being placed.
   - Ctrl/Cmd gives temporary direct select, and Backspace removes the last point.
   - Click the start point to close, and use Enter or Esc to finish an open path.
   - Hovering an endpoint continues the path.
   - Show a rubber-band preview.
3. **Direct selection / Node tool (A)**:
   - Marquee and shift-select nodes, drag anchors and handles, and nudge with the arrow keys.
   - Convert corner↔smooth by double-click, Alt-drag or Shift+C.
   - Add a point by clicking a segment, delete points, and **delete-and-heal**.
4. Break at a node (scissors), join (Ctrl+J), close and reverse.
5. Primitives: rectangle with corner radius, ellipse, polygon, star and line.
6. **One fill and one stroke**: solid color and linear/radial gradients, width, caps, joins, miter limit and dashes, plus an eyedropper.
7. **Booleans**: union, subtract, intersect, exclude. Compound paths with an even-odd or nonzero fill rule.
8. Clipping mask.
9. Align, distribute and numeric transform.
10. Snapping to points, grid and guides.
11. Pencil or freehand with smoothing and curve fitting.
12. Outline stroke (expand).
13. SVG import and export.

### v2: what intermediate users expect

- **Shape Builder**, click or drag over regions, which has become standard since Inkscape (2023) and Figma (2025) added it.
- **Offset path** and **Simplify**.
- **Variable width**:
  - Pressure from the tablet stored as editable width points.
  - A **Width tool**: drag out from the path to set width, Alt for one side only, and saved profiles.
  - Taper presets.
- A **curvature-style pen** (Illustrator Curvature, Affinity Smart, Inkscape BSpline/Spiro).
- Segment bend or mold by dragging the curve itself.
- Per-node **live corner radius** (round, inverted or chamfer).
- **Smart guides** (alignment and spacing hints), and lasso selection of nodes.
- Knife (cut along a drawn stroke) and a vector eraser.
- Arrowheads and markers, text on path, and **image trace**.
- **Live / non-destructive booleans**.
- **Region paint bucket** (Live Paint / vector flood fill).
- Brushes along a path (art, scatter, pattern).
- Multiple fills and strokes per object (appearance stack).
- Repeat or instancing (radial, grid, mirror).

### Advanced or niche

- Mesh and freeform gradients.
- Blend or morph.
- Envelope, lattice and perspective warp, puppet warp, and a perspective grid.
- A full effect stack (Inkscape LPE / Illustrator Appearance).
- A **procedural node graph** (Graphite).
- **Vector networks** (Figma).
- Fracture/Flatten for screen print.
- Centerline and pixel-art tracing.
- Dimension and measure tools.
- Intertwine.
- CMYK and spot colors, and HPGL/DXF output for cutters.
- Generative AI: text-to-vector, recolor, shape fill, Turntable.

---

## 10. The ~15 most-used operations

Nobody publishes per-tool telemetry. Adobe's iPad redesign was run on the principle of "80% of the functionality with 20% of the tools" ([Adobe blog](https://blog.adobe.com/en/publish/2020/10/20/redesigning-illustrator-for-the-ipad)), but the post gives no numbers.

The ranking below combines five proxies:
- (a) single-letter shortcuts that every app now shares: V, A, P, N, T, I, and M/R/L/O for shapes.
- (b) Illustrator's Contextual Task Bar actions for paths ([Adobe](https://helpx.adobe.com/illustrator/using/contextual-task-bar.html)).
- (c) the iPad taskbar's "commonly used features": Shape Builder, alignment, pathfinders, create outlines and repeats.
- (d) the vector-editing fixes users asked for that Figma shipped: vertex selection, align and spacing, path closing, booleans and outline stroke, multi-node editing, shape builder and lasso ([Figma Draw](https://www.figma.com/blog/introducing-figma-draw/)).
- (e) the top Illustrator UserVoice requests, such as enclosed-only marquee selection and cleaner trace joins ([UserVoice](https://illustrator.uservoice.com/forums/333657-illustrator-desktop-feature-requests)).

The ranking:
1. Select, move, scale and rotate with the bounding box (V), plus duplicate (Alt-drag).
2. **Direct-select / node edit (A)**: drag anchors and handles and marquee-select nodes.
3. **Pen drawing (P)**: click for corners and drag for curves.
4. **Convert a node between corner and smooth**, or break a handle (Alt, Shift+C, double-click).
5. Draw rectangles and ellipses, including rounded corners.
6. Set fill and stroke color and stroke weight, and use the eyedropper.
7. **Unite and subtract**, through Pathfinder buttons or the **Shape Builder**.
8. **Align and distribute**.
9. Group, ungroup and arrange.
10. Add and delete anchors, and delete-and-heal.
11. Join, close and cut paths (Ctrl+J, scissors).
12. **Outline stroke** and create outlines from text.
13. **Clipping mask**.
14. **Offset path**.
15. Pencil or brush freehand with smoothing, followed by **Simplify/Smooth**.
16. Image trace or vectorize.
17. Variable width (Width tool or pressure), mostly for illustrators.

---

## 11. Implications for Capy Canvas

- **Keep the industry shortcuts:** P pen, A node, V select, N pencil, Shift+C convert, Shift+M shape builder, Shift+W width, Ctrl+J join, Ctrl+7/8 clip and compound, plus the Pen's Alt, Shift and Space modifiers. Graphite shows that a new editor can copy them.
- **Data model choice:**
  - SVG-style subpath chains are the simplest and export losslessly.
  - A graph model (Figma, Graphite) enables branching and region fill, but export has to split the graph, and the fill logic (splitting at intersections, finding minimal cycles) is harder.
  - A middle path: store chains, and add region fill as a separate "Live Paint"-style layer computed from the arrangement of the curves.
- **Variable width is the painter's feature.** Store width points (t, left, right) on the stroke, as Illustrator, Figma and Inkscape Power Stroke do, rather than one global pressure curve as Affinity does. That keeps per-point editing and taper presets possible.
- **Brushes along a vector path using the existing GPU raster brush engine** would be the differentiator. Affinity (raster-textured vector brushes) and CorelDRAW (Painterly) do this, and the incumbents mostly do not.
- **Non-destructive modifiers:** Inkscape's LPE stack, Illustrator's Appearance panel and Graphite's node graph all store the base geometry plus an ordered list of parametric modifiers (offset, corners, boolean, width, dash, repeat). Planning for that from v1 avoids destructive-only commands later.
- **Useful Rust building blocks:** `kurbo` (Graphite), Graphite's `path-bool` boolean crate and `offset_bezpath` algorithm, and the `simplify` and `spline` nodes are all Apache-licensed prior art.
