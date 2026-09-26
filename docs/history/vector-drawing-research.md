# Vector drawing and editing: research and recommendation for Capy Canvas

[Technical documentation](../README.md) · [Design and validation history](README.md)

Research date: **2026-09-25**. Source baseline: **`796f7f5b`**.

Research and proposed direction for future vector support. Recommendations and
milestones below describe potential work, not implemented capabilities.

## Recommendation in brief

Build an **artist-oriented vector layer containing editable strokes and filled paths**, integrated with the existing raster layers. Make freehand drawing, intersection trimming, line-weight correction, enclosed-region fill, and ordinary path editing work together. Use **SVG for exchanging finished geometry**, and extend **`.capy` for preserving editable stroke input and brush definitions**.

The opportunity is a complete journey: **sketch → ink → repair → color → adjust shapes → export**. The research supports bringing drawing and cleanup into one app. It does not support making a full Illustrator replacement, a procedural node editor, or a universal natural-media brush interchange engine the initial target.

Five decisions:

1. Preserve a stroke's editable centerline and width profile internally. Generate its visible outline for rendering and SVG export without destroying the original.
2. Start with three dependable geometric brushes: monoline, pressure ink, and a chisel/calligraphy brush. Keep textured and wet painting available through raster layers.
3. Treat trim-at-intersection, width correction, fill, and easy selection as core tools. These make vector drawing valuable before an artist learns Bézier handles.
4. Ship a compact real path editor: Pen, Nodes, primitive shapes, join/split/close, simplify, Boolean operations, clipping, alignment, and stroke-to-path conversion. Add basic editable text before claiming general logo/sticker coverage.
5. Test exporting and reopening an actual finished drawing as part of onboarding. A file ending in `.svg` is insufficient evidence that its content remains vector or editable.

**Formats answer:** standards for digital ink exist, particularly W3C InkML and Wacom's published Universal Ink Model. Neither establishes a documented shared artistic-brush replay workflow across Illustrator, Inkscape, CSP, Fresco, and Concepts. Retaining pen input is feasible; reproducing an arbitrary other application's brush also requires its rendering semantics. See the format comparison below. [InkML specification](https://www.w3.org/TR/InkML/), [Wacom ink model](https://developer-docs.wacom.com/docs/specifications/uim/model/).

**Capy-specific finding:** current code has reusable pen-input infrastructure, but historical brush strokes are **not** currently persistent document objects. Some overview documentation describes an older model. The current raster project format and source explicitly exclude historical contacts. This is a meaningful document-model extension, not just a new toolbar. [Current project format](../reference/project-format.md), [live `Stroke` definition](../../crates/layer-core/src/lib.rs).

## Scope and quality of evidence

The primary comparison covers eight applications: Illustrator, Inkscape, Affinity, CorelDRAW, Graphite, Clip Studio Paint, Fresco, and Concepts. Illustrator/CorelDRAW are production-editor references; CSP/Fresco/Concepts are drawing references; Affinity bridges disciplines; Inkscape is especially relevant to SVG and Linux; Graphite is an emerging architectural reference, not assumed to be an established industry standard.

Evidence combines official manuals/specifications, named practitioners' tutorials and interviews, and first-person community accounts. Tutorial pages and available video descriptions were reviewed; this is not a claim to have watched every linked video or performed hands-on compatibility testing. Vendor-hosted artist interviews are useful workflow evidence but have promotional selection bias. Forum reports are qualitative examples, not usage statistics or proof that an old bug still exists.

“Most people” is interpreted as **Capy's painters, comic artists, illustrators, and occasional graphic makers**. There is no representative survey here proving a coverage percentage. Priorities below are product judgments based on repeated tasks, cross-application overlap, and implementation dependencies. Platform editions differ; desktop documentation is not evidence of tablet parity.

## 1. What the applications expose

### Drawing-oriented applications

| Application | Main jobs and workflow | Key tools exposed | Lesson for Capy |
| --- | --- | --- | --- |
| **Clip Studio Paint** | Comic/character linework: sketch, ink on a vector layer, remove overshoots, correct shapes and line weight, color separately. | Object/control-point editing; change brush appearance after drawing; pinch, simplify, connect and redraw lines; local width correction; eraser modes for touched area, whole stroke, or intersection. Fill, Gradient and Blend are unavailable on vector layers. | Strong model for forgiving inking. Combine its repair tools with filled vector shapes so artists can finish more work in the same layer system. [CSP vector-layer manual](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm). |
| **Adobe Fresco** | Direct stylus illustration, lettering and mixed-media drawing; vector ink alongside pixel/live brushes, then export or continue in Illustrator. | Vector brushes with smoothing, pressure/taper and tip controls; vector trimmer; layered drawing. Pixel/live brush categories should not be confused with vector brushes. | Immediate drawing feel and quick cleanup matter. Provide a visible Trim tool instead of making a hidden shortcut the only way to discover it. [Vector brushes](https://helpx.adobe.com/fresco/desktop/draw-paint-animate-and-share/vector-brushes.html), [export and Illustrator handoff](https://helpx.adobe.com/in/fresco/using/publish-export-share.html). |
| **Concepts** | Ideation, product sketches, sketchnotes and design iteration: draw loosely, select strokes, move/recolor/restyle them, reorganize ideas, export a region. | Stroke lasso and item picker; grouping and transforms; post-drawing brush/width/opacity/smoothing changes; Nudge and Slice; grids, guides, measurement, artboards and an infinite canvas. | Make completed marks easy to select and revise without node editing. Separate a good editable sketch from a promise of identical external vector rendering. [Selection](https://concepts.app/en/manual/selection), [brushes/tools](https://concepts.app/en/manual/brushes-and-tools), [design tutorial](https://concepts.app/en/tutorials/how-design-concepts/). |

### Editing-oriented applications and hybrids

| Application | Main jobs and workflow | Key tools exposed | Lesson for Capy |
| --- | --- | --- | --- |
| **Adobe Illustrator** | Logos, icons, illustration, lettering and production graphics: construct or trace shapes, refine paths, style, organize and deliver assets. | Pen/Curvature/Pencil; direct anchor editing; smoothing/simplification; shapes and Shape Builder/Pathfinder; clipping/compound paths; Live Paint; Width; text, artboards, alignment and Image Trace. Paintbrush applies an appearance to a path; Blob Brush makes filled shapes. | Borrow the common construction and cleanup vocabulary; avoid forcing brush strokes to become expanded shapes while drawing. [Drawing/painting manual and tool index](https://helpx.adobe.com/illustrator/desktop/paint-and-fill/learn-painting-basics/about-fills-and-strokes.html), [brush families](https://helpx.adobe.com/in/illustrator/desktop/paint-and-fill/apply-and-edit-strokes/about-brushes.html), [Width](https://helpx.adobe.com/illustrator/using/tool-techniques/width-tool.html). |
| **Inkscape** | SVG artwork, logos, diagrams and cleanup of imported/traced paths: construct, edit nodes, combine shapes, refine, save/export. | Selector and Node tools; Bézier/Pencil/Calligraphy; primitives; Boolean and Shape Builder operations; offsets, simplify and stroke-to-path; gradients, text, clipping and tracing. Pressure drawing and live path effects provide some drawing/editor overlap. | Use SVG as an everyday editable exchange format, with understandable path operations. Editor-specific live effects still need evaluated geometry for interchange. [Basic tutorial](https://inkscape.org/de/en/doc/tutorials/basic/tutorial-basic.html/), [advanced tutorial](https://inkscape.org/es/doc/tutorials/advanced/tutorial-advanced.html?switchlang=es), [drawing approaches](https://inkscape-manuals.readthedocs.io/en/latest/ways-drawing.html), [pressure support](https://wiki.inkscape.org/wiki/Release_notes/1.0). |
| **Affinity: current Vector Studio; Designer 2 as a separately versioned reference** | Illustration, branding and mixed vector/raster work: sketch, construct curves, add color/texture and export artwork or artboards. | Pen/Node editing, shape construction, Boolean operations, Shape Builder, fills, text, snapping/alignment, stroke expansion and raster tools alongside vectors. Current Affinity also advertises Image Trace; older Designer tutorials should not be used to infer its absence today. | The closest broad interaction reference for a combined workspace. A brush following a vector path can still use a raster texture. [Current tools/formats](https://www.affinity.studio/graphic-design-software), [curve editing](https://www.affinity.studio/features/vector-drawing), [Designer 2 brush definitions](https://affinity.help/designer2/English.lproj/pages/Painting/create_custombrushes.html). |
| **CorelDRAW** | Production graphics and bitmap-to-vector preparation: import artwork, trace, clean geometry, fill regions, arrange text/shapes and prepare output. | Pick/Shape editing; Pen/Bézier/B-spline/freehand tools; Artistic Media vector brushes and calligraphy; LiveSketch; Smart Fill; contours and layout tools; PowerTRACE with outline and centerline approaches. Its Painterly Brush is explicitly pixel based. | Smart Fill's “make a new shape from this enclosed area” is particularly useful for artists. Distinguish silhouette tracing from recovering a line's centerline. [Toolbox](https://help.coreldraw.com/CorelDRAW/540111192/Documentation-Windows/CorelDRAW-en/CorelDRAW-Toolbox.html), [Smart Fill](https://help.coreldraw.com/CorelDRAW/540111192/Documentation-Windows/CorelDRAW-en/CorelDRAW-Apply-fills-to-areas.html), [trace types](https://help.coreldraw.com/CorelDRAW/540111192/Documentation-Windows/CorelDRAW-en/CorelDRAW-Choose-a-trace-type-and-profile.html). |
| **Graphite** | Vector illustration and procedural design: make shapes/paths, style them, combine/repeat them through editable operations, export results. | Pen/Path and primitives; colors/gradients; Boolean operations, alignment and layers backed by a node graph; procedural repetition and parameter editing; SVG import/export. | Preserve editable inputs behind operations, but keep the graph out of a beginner's critical path. Its documentation identifies it as alpha and describes the brush prototype as limited; do not use it as evidence of mature painting support. [Capabilities/limitations](https://graphite.art/learn/introduction/features-and-limitations/), [commands and file support](https://graphite.art/learn/interface/menu-bar/). |

These are representative tool families, not exhaustive menus. UI prototyping, technical CAD, full page layout and animation are outside this first scope.

## 2. What artists actually do, value, and struggle with

| Evidence | Observed workflow or preference | Product consequence |
| --- | --- | --- |
| **Liz Staley, CSP: “Using Vector Tools for Faster Inking.”** | Explicitly calls the vector eraser a favorite. Demonstrates inking over a sketch, quickly erasing crossing excess, and adjusting lines afterward. | The first demonstration should deliberately overshoot, trim, and repair a mark. This shows the benefit more clearly than zooming into an already perfect curve. [Artist tutorial](https://tips.clip-studio.com/en-us/articles/11333). |
| **Cheishiru, CSP vector guide.** | Uses vector layers heavily; teaches sketch → vector line art → erase excess → thicken selected lines. Explains how centerline intersections can leave visible remnants. | Prioritize local width correction and define trim behavior around visible brush ends, not only mathematical intersections. [Artist guide](https://tips.clip-studio.com/en-us/articles/7586). |
| **Chris Piascik, Fresco export tutorial, March 2026.** | The published video description teaches Fresco vector PDF → Affinity → path/color cleanup → SVG/EPS for merch, stickers and client work. It also calls out clipping-related rasterization. | Direct SVG export plus local path cleanup removes a concrete multi-app step. Export should identify any rasterized content. This evidence is the creator's description, not a reproduced hands-on test. [Tutorial](https://www.youtube.com/watch?v=_5wKyW_nx9k). |
| **Nick Saporito / Logos By Nick, Inkscape logo tutorial.** | Builds a logo from overlapping circles, spacing, stroke-to-path borders, centered text and simple accent shapes. | Beginners can make a useful graphic through shapes, alignment and a few path operations before mastering the Pen tool. [Written walkthrough](https://logosbynick.com/design-a-logo-with-inkscape/). |
| **Von Glitschka, Vector Basic Training.** | Teaches taking a drawn concept into precise vector form through analyzing shapes and systematic construction. | Provide a second learning route for mouse users: place a reference, construct a few clean shapes, then refine. [Creator's course/book overview](https://www.glitschkastudios.com/vbt). |
| **James Martin / Made By James, Affinity walkthrough.** | Vendor-hosted description covers custom workspace, Vector/Pixel/Layout transitions, artboards, Booleans, Pen/Text, and tracing a hand-drawn sketch. | A complete small project teaches how the tools connect. Workspace continuity matters, but tracing is just one entry point. [Tutorial overview](https://www.affinity.studio/blog/getting-started-with-affinity-tutorial). |
| **Christi du Toit, Affinity interview, June 2026.** | Develops rough ideas, refines a sketch, inks, fills flats and organizes deliverables. He works mainly in raster, uses vector guides, values the brush and custom shortcuts, and uses freehand selections for quick flat color. | “Best of both worlds” should retain raster painting's strengths. Do not force texture artists into all-vector production. Include selection/fill and familiar shortcuts in the beginner journey. [Artist interview](https://www.affinity.studio/blog/digital-illustration-workflow-christi-du-toit). |
| **Concepts' selection and sketchnoting tutorials.** | Select and rearrange what was already drawn; restyle marks; group clusters of notes; clean with Nudge/Slice. | Post-drawing selection and transformation are everyday creative tools, not advanced maintenance features. [Selection tutorial](https://concepts.app/en/tutorials/select-edit-notes-drawings-designs/), [sketchnoting workflow](https://concepts.app/en/tutorials/concepts-sketchnoting-toolbox/). |

Two useful first-person friction reports reinforce this pattern:

- In Fresco's community feedback thread, **Vector_Kat** describes sketching and rough vector drawing in Fresco, then moving to Illustrator for cleanup, and asks for Pen/Smooth tools. Other participants request approachable follow-along tutorials and simpler palette handling. These are historical requests, not a claim that all remain unaddressed. [First-person discussion](https://www.reddit.com/r/AdobeFresco/comments/1kaossv/hey_fresco_artists/).
- A raster-experienced illustrator in an Affinity discussion reports unexpected polygons when filling complex Pencil artwork. This is an example of the conceptual gap between filling a closed path and coloring a region bounded by several marks. [First-person account](https://www.reddit.com/r/Affinity/comments/1w5omam/help_vector_coloring_pipeline_recommendations/).

**Interpretation:** the recurring reward is being able to fix a drawing without redrawing it. The recurring barrier is hidden state: what is selected, whether a path is closed, whether a color applies to a fill or stroke, and what will survive export. There is enough evidence to prioritize these problems, but not to rank every tool by market-wide popularity.

## 3. Three meanings of “vector brush”

This distinction should guide the architecture and the product wording:

| Representation | What stays editable | What an SVG receiver can reasonably receive |
| --- | --- | --- |
| **Geometric stroke** | A centerline plus width/taper/tip parameters; or a filled outline. | A normal stroked path for constant width, or filled outlines for varying width and shaped tips. |
| **Brush attached to a path** | A path with stamp, texture, scatter or stretch behavior. | Expanded vector components when practical, or images/effects when the appearance is raster based. The original brush behavior usually needs native data. |
| **Recorded painting operation** | Input samples plus brush/material state and dependencies on earlier paint. | Usually a raster result or an approximation; recording its input does not turn the resulting wet paint into ordinary vector geometry. |

Illustrator documents multiple brush families, including art, scatter, pattern and bristle brushes. Affinity explicitly distinguishes a solid vector brush from textured brushes created from raster images. Thus “vector” in a tool name is not a guarantee of a pure-vector exported appearance. [Illustrator brush definitions](https://helpx.adobe.com/in/illustrator/desktop/paint-and-fill/apply-and-edit-strokes/about-brushes.html), [Affinity brush definitions](https://affinity.help/designer2/English.lproj/pages/Painting/create_custombrushes.html).

**Recommended user promise:** geometric ink remains editable and exports as scalable paths; textured painting keeps its appearance through raster content when necessary. Both can coexist in the same project. Export explains the result using “editable paths,” “outlined strokes,” and “embedded images,” rather than assuming artists know internal rendering terms.

## 4. File formats and the limits of brush replay

### Exchange comparison

| Format | Carries | Limits for our goal | Recommendation |
| --- | --- | --- | --- |
| **SVG** | Standard shapes, paths, fill/stroke styling and composition features. | Standard `stroke-width` does not encode an arbitrary per-point pressure/width curve or an artistic brush engine. Variable-width appearance can be represented as filled geometry. | Primary vector import/export. Keep native stroke editing separately. [SVG painting specification](https://www.w3.org/TR/SVG2/painting.html). |
| **PDF** | Useful artwork/document handoff; can contain vectors and images. | It does not promise recovery of the source application's editable brushes, layers or effects. | Secondary delivery format after SVG, with explicit vector/raster handling. Fresco's PDF handoff is a practical example, not proof of universal lossless interchange. [Fresco export](https://helpx.adobe.com/in/fresco/using/publish-export-share.html), [Affinity interchange limits](https://www.affinity.studio/graphic-design-software). |
| **InkML** | A W3C Recommendation for traces, time, pressure/orientation channels, grouping and brush properties. | Complex brush context can be application specific; the specification explicitly leaves such details to a higher-level application layer. It does not define Capy's wet-paint engine. | Potential later sensor-data interchange or archival adapter. Do not make it a prerequisite for drawing or SVG. [InkML, including section 4.3](https://www.w3.org/TR/InkML/). |
| **Wacom UIM (`.uim`, WILL ecosystem)** | Raw sensor data, spline geometry, brush/rendering configuration, styles/seeds and stroke organization; a published RIFF/Protocol Buffers serialization. | A richer ink model still requires compatible rendering behavior and a receiving application that supports it. Its published specification is not evidence of support throughout our target editor set. | Best ink-specific model to study for a later adapter. Validate against a named receiving app before productizing. [Model](https://developer-docs.wacom.com/docs/specifications/uim/model/), [official library and serialization overview](https://github.com/Wacom-Developer/universal-ink-library). |
| **Microsoft ISF** | Serialized Windows ink strokes and properties; Windows APIs can save/load it, including an image container with ink metadata. | A Windows ink ecosystem format, not a demonstrated interchange path for the surveyed art editors' brushes. | Only prioritize if a concrete Windows ink-import workflow emerges. [Microsoft ink storage documentation](https://learn.microsoft.com/en-us/windows/uwp/ui-input/save-and-load-ink). |
| **Application-native formats** | Each application's own editing model. | Not a shared cross-application brush contract. Native editability and cross-platform compatibility must be checked separately. | `.capy` should be Capy's complete editable source, with SVG as the exchange result. Concepts even documents separate native formats by platform. [Concepts export manual](https://concepts.app/en/manual/export). |

Brush preset exchange and stroke exchange are also different requirements: sharing a tip image or a preset does not, by itself, specify how another engine should interpret a completed stroke.

### What the drawing apps actually export

- **CSP:** the current manual says its SVG interchange records line shape and does not transfer brush tip, color or line thickness; clipboard vectors paste at uniform thickness. Treat this as a geometry interchange path, not faithful brush appearance. [CSP import/export section](https://help.clip-studio.com/en-us/manual_en/180_layers/Vector_layers.htm).
- **Concepts:** documents simplified vector exports, recommends Fixed Width/Wire brushes, and warns that texture and masking behavior differ. Its vector PDF loses brush texture; its SVG option can include texture images. These are limitations of its export mapping, not a claim that SVG cannot represent varying-width silhouettes. [Concepts export manual](https://concepts.app/en/manual/export).
- **Fresco:** the documented image-export list is PNG, JPG, PSD and PDF, with a separate Illustrator handoff; it does not list direct SVG export. The creator tutorial above shows a PDF-to-editor workaround. [Official export options](https://helpx.adobe.com/in/fresco/using/publish-export-share.html).
- **Inkscape:** distinguishes native Inkscape SVG, containing editor information, from Plain SVG, which omits it. This is a useful precedent for separating portable appearance from native authoring data. [Inkscape export guide](https://inkscape-manuals.readthedocs.io/en/latest/export-other-formats.html).

### Proposed Capy format contract

1. **Save `.capy`:** preserve vector objects, their editable paths and width profiles, applicable source samples, immutable brush snapshots/assets, transforms and ordering, plus a portable appearance fallback for brush strokes. Raster layers continue to preserve committed pixels.
2. **Export SVG, appearance-oriented default:** emit standard strokes where sufficient; expand pressure ink and chisel strokes into filled paths. Preserve groups, names where possible, and supported clips/gradients. Keep the native document unchanged.
3. **Offer centerline export as an advanced option:** useful for plotting or geometry transfer, with an explicit preview showing that pressure/brush appearance is simplified. Do not export both the centerline and its outline as visible objects.
4. **For mixed artwork:** show which parts will be embedded images, offer a vector-only export that identifies unsupported objects, and allow PNG when the artist wants a flat image. Do not silently omit unsupported content.
5. **Optional Capy metadata inside SVG:** useful for a Capy-to-Capy convenience path, but never required to display the file. External editors may drop it. If foreign edits change visible geometry, discard or invalidate stale native metadata instead of restoring an old stroke over the edited artwork.

**No universal round-trip promise:** an outlined pressure stroke can look right and remain node-editable in another editor while losing its original centerline and pressure controls. Recovering a centerline from an arbitrary outline is a separate, approximate reconstruction problem.

### Store editable strokes and a portable appearance

For thickening, thinning, taper and shape adjustments, **store both representations, with distinct roles**. The authoring source is the current editable stroke; its saved appearance lets a reader display the result without understanding that stroke's brush engine. This is a proposed Capy design, not a claim that the surveyed formats all implement this fallback contract.

| Saved data | Role |
| --- | --- |
| Editable centerline, width profile, tip/style parameters, transform | Authoritative model for stroke editing. Persist the resolved width profile, not just pressure values that require an unknown brush to interpret. |
| Immutable brush definition and referenced assets | The particular brush used by this object, independent of the currently installed preset. Deduplicate definitions/assets within the document. |
| Portable appearance: filled paths for geometric ink; images where required | Display/export fallback when the renderer is missing or incompatible. Preserve fill rules, opacity, grouping and composition, not merely a silhouette. |
| Original input samples, when retained | Optional provenance for refitting, pressure reinterpretation or later brush replacement. They are not the authoritative shape after path edits. |

For a simple round stroke, let `w(s)` be the saved full width along its centerline. Proportional thickening applies `w_new(s) = 1.2 × w(s)`; local correction changes only a chosen region. A zero-width tapered end stays zero under proportional scaling. An additive increase has different behavior and can make a pointed tip blunt. Expose these as intentional operations rather than conflating them. For chisel strokes, retain tip orientation/aspect information as well; a single width value does not fully describe the footprint.

An outline-only object can still be thickened with an offset or reshaped by moving nodes. However, inward offsets can collapse narrow regions, corners and holes need special handling, and the original centerline/width distribution is generally not uniquely recoverable. Thus outline offsets should coexist with stroke-width editing as separate tools.

**A changed preset should never silently change saved artwork.** Embed an immutable definition plus its textures/shapes, retain a rendering-model version and seed where relevant, and have strokes reference that exact resource. Editing an installed brush creates a new definition; applying it to existing strokes is an explicit undoable edit. The current live `BrushSnapshot` already follows a snapshot pattern, but durable vector objects and their resource references still need implementing. [Current snapshot definition](../../crates/layer-core/src/lib.rs).

Embedding the brush resolves a **missing asset or changed preset**. It does not resolve a **missing rendering algorithm**. A reader that understands `.capy` but lacks that algorithm uses the saved appearance; it must not silently substitute a similar brush. It can offer operations supported by the fallback, such as outline-node editing or transforms, while preserving the unsupported native payload. Bake complex raster-dependent appearances at an appropriate layer/group boundary when an independent stroke image would change composition.

Store an appearance alongside its object revision/resource digest, and publish matching source and fallback atomically. Regenerate the fallback after a supported edit. If an artist directly edits fallback outline nodes, convert that result into an ordinary filled-path object or keep the old editable stroke as a separate inactive source; do not pretend the old centerline still describes the new outline. Render either the source or fallback, never both.

Support basic round/chisel ink through a small published geometric model so future readers can implement useful width editing without reproducing every artistic brush. Wacom's specification likewise distinguishes spline data, optional raw input and rendering configuration; it supports the conceptual separation, but does not supply this proposed Capy fallback policy. [UIM stroke and rendering model](https://developer-docs.wacom.com/docs/specifications/uim/model/).

Other existing editors still need a `.capy` importer to open its container. **SVG remains the practical handoff:** filled outlines carry the geometric appearance without requiring the brush. A receiving editor can edit those outlines, but maintaining the original stroke controls requires explicit support for the native model or metadata. Standard SVG stroking does not encode our arbitrary width profile. [SVG painting model](https://www.w3.org/TR/SVG2/painting.html).

This costs extra storage and synchronization work. Deduplicated brush resources, compact curve geometry and compression should reduce that cost; measure actual file sizes. Preserve this appearance fallback for durability, while keeping disposable GPU tessellations and zoom caches out of the file.

### Why “just record the input and replay it” is only part of the solution

For Capy's own future editable ink, capture document-space positions, pressure, tilt/twist where supplied, time, input interpretation, brush parameters, asset identities, random seed and an engine/schema version. Save the final fitted path and width data as well, so reopening does not depend on rerunning a changed fitting algorithm.

For texture or wet paint, the result may also depend on sampling/spacing rules, pigment state, earlier canvas colors, masks, compositing and material-update boundaries. Replaying the same x/y/pressure sequence with another engine can produce a different image. Reordering a wet stroke may change subsequent strokes. GPU numerical differences can matter even when algorithms match.

Therefore, support editable geometric strokes first; retain existing raster results for painting. Evaluate independent textured stroke objects later. Whole-document editable wet-paint replay is a separate substantial project, with checkpoints and dependency tracking, rather than a condition for “vector drawing.”

## 5. Recommended subset and delivery order

Priority is based on completing user tasks. Effort is relative and assumes shared Rust implementation plus qualification on every host; these are not calendar estimates.

### M0: foundation and interoperability prototype

Establish persistent vector objects, geometric rendering, object selection/hit testing, undo, `.capy` save/reopen, and a small SVG import/export fixture set. Prove variable-width ink can become clean SVG outlines. Keep the first prototype small enough to expose geometry and compositing problems early.

### M1: complete freehand illustration loop

| Capability | Minimum useful behavior | Value / relative difficulty |
| --- | --- | --- |
| **Vector ink / freehand path** | Monoline and pressure ink first; then chisel. Pressure/taper controls, useful smoothing defaults, mouse-friendly width presets. Retain strokes independently. | Essential / medium-high. |
| **Select and transform** | Tap/click, marquee and stroke lasso; move/rotate/scale/duplicate; multi-select; clear object vs layer selection; explicit scale-stroke-width choice. | Essential / medium. |
| **Trim and erase** | Erase a whole stroke; trim to intersection; split/remove a touched stroke section. Preview affected geometry and undo each gesture in one step. | Highest drawing differentiation / high for reliable intersections. |
| **Line repair** | Change whole-stroke width/color; local width editing; smooth/simplify with preview; small-area nudge or redraw refinement when dependable. | High / medium-high. |
| **Pen, Nodes, basic shapes** | Lines and cubic curves; corner/smooth nodes; add/delete/move handles; split/join/close; rectangle, ellipse and line; fill/stroke controls. | Necessary editor foundation / medium-high. |
| **Color regions** | Fill closed shapes and create a new filled path from a clearly enclosed region of vector linework. Preview the boundary; show gaps. Keep outline strokes intact. | Essential to finish a drawing / high for region extraction. |
| **Organization** | Vector layer holds many objects; group, lock/hide, reorder and isolate selections; raster sketch below vector ink; named palettes and recoloring. | Essential / medium. |
| **Delivery** | Save/reopen editable `.capy`; SVG geometry import/export; PNG preview; explicit handling of unsupported content. | Release gate / high. |

For enclosed-region fill, start with a generated independent shape, not a fully live region graph that automatically tracks every subsequent stroke edit. The artist can edit the resulting shape. Add small-gap closure only with a visible tolerance/preview; avoid unpredictable zoom-dependent pixel flood-fill results in a supposedly geometric tool.

M1 should support a complete colored character illustration with basic geometric edits. It should not be advertised as covering ordinary graphic-design work until M2.

### M2: complete the common editor workflows

| Capability | Why it belongs in the recommended subset |
| --- | --- |
| **Union, subtract, intersect, divide; Shape Builder** | Shape-based logos and flat illustration without excessive node construction. Share geometry infrastructure with fill/trim where practical, while recognizing their different operations. |
| **Alignment, distribution, snapping and numeric transforms** | Make clean icons, logos, borders and repeated motifs. Snapping must have an obvious off switch. |
| **Expand stroke, offset/inset/outset, compound paths** | Sticker borders, lettering outlines, holes, cleanup and external delivery. Keep conversion explicit and undoable. |
| **Clipping and linear/radial gradients** | Finish a useful range of illustrations and preserve common imported SVG content. Full arbitrary filter editing can wait. |
| **Basic editable text and text-to-path export** | Labels, simple logos, stickers and diagrams. Include shaping/font fallback; warn about missing fonts. Do not pretend a text engine is a trivial last-minute addition. |
| **Simple tracing and cleanup** | Start with black/white silhouettes or high-contrast scans: threshold, speckle removal, smoothing and preview. Keep the original image. Offer manual tracing immediately; evaluate automatic tracing after editable path cleanup exists. |
| **Recolor matching objects and export selection/region** | Rapid variants and reusable assets. One canvas/export region is enough initially; full multi-artboard management is later. |

**Recommended broad first release = M1 + these M2 essentials.** M1 alone is a valuable inking pilot. If scope must shrink, preserve select/repair/fill/save/export and narrow the advertised use cases; do not remove the end of the artist's journey.

Automatic tracing needs a separate contract: outline tracing produces filled contours; centerline tracing produces open paths. Neither reconstructs the original pen pressure or stroke order. Inkscape's beginner guide cautions that detailed/color tracing can yield difficult-to-edit object piles. [Tracing guide](https://inkscape-manuals.readthedocs.io/en/1.3/tracing-an-image.html).

### Later, when demand justifies it

Text on path; symbols/repeat patterns; advanced art/scatter brushes; advanced width profiles; vector warps; multi-artboards; PDF delivery; more extensive SVG masks/filters; tracing in color or by centerline; UIM/InkML adapters. Defer mesh-gradient authoring, procedural node UI, print separations/spot-color/prepress suites, animation and CAD tooling from this initial program.

These omissions mean the first release will not replace specialized packaging, technical drawing, complex typography or print-production software. Artists doing that work need a reliable handoff.

## 6. A new artist journey worth shipping

### Entry choices based on intention

Offer three optional project starters: **Draw a character**, **Make a logo/sticker**, **Edit an SVG**. They choose a workspace and example, not incompatible document types. Include a blank-canvas option and an immediate skip. Use the same layer stack and core tools throughout.

Keep the initial toolbar compact: Select, Brush, Erase/Trim, Fill, Shapes, Pen and Nodes, with the existing color, undo and navigation controls. Add Text and gradient controls when relevant. Advanced path commands live in selection context actions and menus, with searchable names matching familiar editor terminology.

### First 15 minutes: draw, fix and share a character

These times are design targets, not measured results.

| Time | Action | What the artist learns |
| --- | --- | --- |
| 0–2 min | Open an original Capy exercise sketch with Sketch / Ink / Color layers prepared; draw using a good pressure-ink preset. | They can begin drawing immediately; layer purpose is visible. |
| 2–5 min | Deliberately cross two lines, then trim the excess with a labeled button and short animation. | Imperfect strokes are repairable. No hidden gesture is required. |
| 5–8 min | Select a stroke, adjust thickness and color, and nudge one curve. | A finished mark remains editable. |
| 8–11 min | Fill two enclosed regions, then fix one intentionally open boundary using a visible gap preview. | Region coloring differs from assigning a path fill. |
| 11–13 min | Duplicate/mirror an element, group the result, and zoom in. | Reuse and scale are practical benefits of vectors. |
| 13–15 min | Save `.capy`, export SVG, and reopen the export to inspect/edit an outline. | Native brush editing and exchange geometry are different, understandable outcomes. |

Give an optional second exercise that adds raster texture and shows how mixed SVG/PNG export differs. Never make texture silently invalidate an earlier “all-vector” promise.

### Second route: logo/sticker with a mouse

Construct a simple badge from circles and rectangles → align → subtract a hole → adjust two nodes → add a short text label → create an offset border → export SVG/PNG. This teaches grouping, holes, fill/stroke and clean delivery through a finished result. Introduce Bézier handles after the artist has made something useful.

### Third route: clean up an SVG

Import a deliberately messy sample → select within a group → identify an open path → join endpoints → simplify with before/after preview → expand one stroke → check holes and bounds → export. Include a second sample with an unsupported effect so the import explanation and fallback are learnable rather than surprising.

### Interaction details that matter

- Show a selected stroke's centerline and a clear selection highlight. Reveal detailed nodes only when editing nodes.
- Separate **fill a shape**, **fill an enclosed region**, and **raster bucket fill** in context and feedback.
- Label eraser behavior: **Whole stroke**, **Trim to crossing**, **Erase section**. Avoid invisible “eraser-colored” objects masquerading as removed geometry.
- Offer **Keep hand-drawn character** versus stronger smoothing through a clear control; more smoothing is not always better.
- Use consistent selection, isolation, undo and keyboard shortcuts across raster and vector work. One action should have one meaningful undo step.
- Make touch/pen operations usable without a keyboard, with visible alternatives for modifier actions and adequate handle targets.
- Reuse Capy's established toolbar/drawer/layer reordering behavior. Competitor canvas hold gestures do not override the project's UI convention. Canvas editing and UI reordering are distinct interactions. [Drag/reorder convention](../ui/drag-and-reorder.md).

### Validate the journey with people

Run formative sessions with approximately 8–12 participants across three groups: new digital artists, raster-first illustrators, and experienced vector users. Include both stylus and mouse workflows. This is a problem-discovery sample, not a statistical estimate of market coverage.

Measure task completion without intervention, time to first useful mark, ability to repair an overshoot, successful coloring, understanding of selection, and correct save/export/reopen. Ask participants to explain what remains editable in `.capy` versus SVG. Observe where they expect familiar tools to be. Iterate on the tasks before adding a larger brush catalog. Keep research notes local and consent based, consistent with Capy's no-tracking model.

## 7. Feasibility in this repository

### Verified current state

The checked source contains `StrokePoint` fields for position, pressure, tilt, twist and elapsed time, plus a `Stroke`/`BrushSnapshot` live-contact structure. However, `Stroke` is explicitly documented as bounded live data outside persistence and undo history. `LayerKind` currently has no vector layer variant. [Core definitions](../../crates/layer-core/src/lib.rs).

The current `.capy` reference describes `CAPYRASTER` storage, immutable sparse raster revisions, and historical brush contacts/operation recipes excluded from the project. Contact reconstruction is bounded to an active/recent correction window. [Project reference](../reference/project-format.md), [project transport](../../crates/layer-core/src/project.rs).

Consequently, older claims in `README.md`, `docs/internals/documents.md`, and crate overview documentation about saved stroke reconstruction should not be used for estimating this work. Existing rectangle/ellipse tools also do not establish persistent editable vector objects: the current format describes figures as transient raster-submission commands.

**Reusable:** device input normalization, sensor capture, live feedback/stabilization infrastructure, shared commands, layer composition, file transports and existing raster storage. **New work:** retained vector geometry and styles, object selection, geometric editing/rendering, persistent vector transactions, schema evolution, import/export and host UI.

### Proposed model and rendering boundary

```text
Native mouse / touch / pen input
                |
                v
Shared Rust tool + input processing
                |
        +-------+-------------------+
        |                           |
        v                           v
Vector layer objects          Existing raster layers
- editable paths              - committed tile revisions
- stroke width/style          - raster painting and effects
- fills and groups
- optional source samples
        |                           |
        v                           |
Geometric rendering/cache ----------+--> existing composition
        |
        +--> SVG paths / expanded outlines / declared image fallbacks

Extended .capy saves the editable vector model and raster state.
```

Proposed invariants:

1. **Geometry is authoritative after editing.** Retain source samples for provenance or an explicit refit operation, but never replay old raw input over a path the artist has edited. Store width controls separately and remap them consistently when splitting, joining or simplifying.
2. **Objects stay distinct until a deliberate combine/expand operation.** A vector layer should hold many strokes without creating a layer per pen contact. Assign stable IDs to objects and subpaths.
3. **Runtime render caches are disposable; saved fallbacks serve compatibility.** Keep tessellations and zoom-dependent raster caches outside the authoritative model. Rebuild at the necessary scale; avoid permanently baking geometric vectors at the canvas's initial pixel resolution. Persist the portable appearance described above, and preserve it when the original brush renderer is unavailable.
4. **Undo is transactional.** A trim or Boolean operation can affect several objects but should commit atomically. Extend memory accounting and immutable save snapshots to cover vector data.
5. **Raw input is optional provenance, not unbounded history.** Exclude predictions, accept final sensor corrections consistently, and set explicit point/object/asset budgets.
6. **Extend the project format deliberately.** Preserve raster layers as raster; add versioned vector records and compatibility handling. Old pixels cannot regain editable stroke history through migration.
7. **Shared behavior remains in Rust.** Hosts supply input, capture, native controls and file I/O. Geometry, validation, edits, SVG conversion and publication should not diverge by platform.

### Where engineering effort actually goes

| Area | Assessment | Main issue to prove |
| --- | --- | --- |
| Monoline, Bézier shapes, basic node edits | Straightforward building blocks, substantial integration | Selection, transforms and persistent object editing work consistently. |
| Pressure/chisel ink with SVG outlines | Feasible, medium-high effort | Clean joins/caps and self-overlaps; no density explosion; no opacity seams at crossings. |
| Trimming, Booleans and offsets | Feasible, high correctness effort | Tangencies, near-coincident paths, tiny loops, holes and degenerate contours. |
| Enclosed-region fill and Shape Builder | Feasible, high interaction/topology effort | Stable regions, understandable gaps and predictable results after edits. |
| SVG import with editable structure | Feasible as a declared subset | CSS/styles, transforms, clips, gradients, fonts and effects must not be silently misinterpreted. |
| Editable text | Feasible, independent substantial work | Font availability, shaping and export appearance/editability tradeoffs. |
| Textured independent stroke objects | Feasible as a later hybrid feature | Asset mapping, cache invalidation and honest raster export semantics. |
| Arbitrary wet-brush replay across other apps | No demonstrated interoperable solution | Compatible brush engines and earlier canvas/material state are missing from ordinary SVG exchange. |

Avoid treating robust geometry as a set of trivial utility functions. Start prototypes with self-crossing ink, sharp turns, very wide strokes and nearly touching contours, not only circles and rectangles.

### Libraries worth prototyping, not preselecting

- **`kurbo`** for curve geometry and manipulation. [Maintainer repository](https://github.com/linebender/kurbo).
- **`lyon`** for path tessellation into GPU geometry. It is a rendering building block, not a document model or a complete Boolean/brush editor. [Maintainer repository](https://github.com/nical/lyon).
- **`iOverlay`** for polygon Boolean operations. Evaluate flattening tolerance and curve reconstruction because polygon output is not automatically a compact Bézier path. [Maintainer repository](https://github.com/iShape-Rust/iOverlay).
- **`usvg` / `resvg`** for SVG parsing/preprocessing, reference rendering and fixtures. Rendering-oriented normalization is useful but may discard authoring distinctions; an editable importer may need to preserve additional source structure. [Maintainer repository](https://github.com/linebender/resvg).

These are candidates based on their documented responsibilities, not benchmarked choices. A short prototype should determine whether they fit the current renderer, WASM targets, curve-quality needs and dependency policy before an implementation plan selects them.

## 8. Interoperability and release gates

### Define an SVG support profile

Start editable import with paths/basic shapes, groups, affine transforms, solid fills, ordinary strokes, opacity and fill rules. Add linear/radial gradients and clipping for the broader release. Respect `viewBox` and units. Keep text editable only when supported; provide an explicit outline/rendered fallback when fonts or layout cannot be preserved.

Unsupported masks, filters, patterns, embedded imagery or foreign editor extensions need an import report and a deliberate fallback. Preserve the original source asset when useful. A render-only placement is a valid separate action, but must not be presented as editable path import.

SVG files can include active/external content. An image editor's import path should use bounded static parsing, omit script execution, and avoid automatic external-resource fetching. This is part of the importer design, not a reason to burden artists with technical prompts.

For the initial exchange profile, use portable sRGB output with an explicit conversion from Capy's document color space. Preserve richer native colors in `.capy`; do not infer print-production color support from SVG export.

### Qualification corpus

Use real exported files and purpose-built edge cases:

1. Monoline and pressure-tapered strokes, dots, cusps and chisel rotations.
2. Self-crossing opaque and translucent strokes; verify joins and overlap opacity.
3. Trimmed stroke ends, nearly tangent crossings and partially erased strokes.
4. Holes, compound paths, both fill rules and overlapping Boolean operands.
5. Nested groups, transforms and width-scaling choices.
6. Clips and gradients; mixed vector/raster art with declared fallbacks.
7. Text with missing fonts, text converted to outlines and non-Latin shaping.
8. A large illustration with many strokes; measure input latency, selection speed, zoom behavior, memory and reopen time on desktop and constrained mobile hardware.

For supported features, exercise **Capy → Inkscape → Capy**, **Capy → Illustrator → Capy**, and **Capy → Affinity → Capy**, as well as SVG display in major browsers. Grade appearance and editability separately. Use the vector editor destinations for geometry exchange; use CSP/Concepts imports only against their documented reduced contracts. A browser screenshot can verify appearance but cannot prove preserved editing semantics.

No compatibility or performance results were measured in this research. Release should require fixture tests plus human completion of the starter projects on the actual host ports. Geometric tests should include invariants such as retained holes, bounded outline error, stable trim identity, and complete undo restoration, rather than only screenshots.

## 9. Suggested decision and next experiment

Adopt this product direction: **editable geometric ink + a small capable path editor + SVG exchange + native mixed-media projects**.

The first engineering experiment should implement one narrow vertical slice: draw a pressure stroke, select it, adjust its width, trim a crossing, save/reopen it as an editable object, and export the same visible shape as standard SVG outlines. Compare the reopened SVG in Inkscape and one other editor. In parallel with subsequent product work, use the character and badge exercises to validate whether the interaction model makes sense to new artists.

If that slice succeeds, extend toward the M1 illustration loop and M2 graphic-making subset. If it fails on outline quality, performance or persistence, resolve those foundations before adding dozens of tools. The intended result is a useful drawing that the artist can repair, finish and take elsewhere.
