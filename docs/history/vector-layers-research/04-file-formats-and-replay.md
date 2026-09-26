# Vector layers & brush strokes: file formats and interop for Capy Canvas

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Research date: 2026-09-25. Confidence notes are inline. Where I relied on my own knowledge and not a fetched source, I say so ("unverified").

## TL;DR

- **SVG 1.1 (with a few SVG 2 extras) is the interchange format for artwork. PDF is the interchange format for print.** Everything else is niche or proprietary.
- **SVG has no variable-width stroke, and nothing is on track to add one.** Every app that has pressure or width-tool strokes, including Illustrator, Inkscape PowerStroke, Concepts and Excalidraw, exports them as **filled outline paths**.
- **There is no widely adopted standard for replayable brush strokes.** W3C InkML is the only vendor-neutral standard. Microsoft Office uses a subset of it for ink, but painting apps don't use it, and its brush model is only an ellipse or rectangle tip. Wacom UIM, Microsoft ISF, Jetpack Ink protobufs and PencilKit are all tied to one vendor or engine. Each painting app stores strokes in its own format, and some store no strokes at all.
- **Recommendation:**
  - **Source of truth:** keep recorded samples plus a brush snapshot and seed. Fitted curves with a width profile are derived, and become the source of truth only after the user edits nodes.
  - **SVG export:** filled outlines by default, a centerline mode for cutters, laser cutters and plotters, and optional private `capy:` namespace data so Capy Canvas can round-trip its own files. This follows the Inkscape pattern: the `d` attribute holds the rendered result and a private namespace holds the editable source.
  - **Also export:** PDF via krilla, and InkML as an optional extra.
  - **Import:** SVG via usvg, reading private data in a separate roxmltree pass. PDF and PDF-compatible AI via hayro-interpret.

---

## A. Path and vector interchange

### A1. SVG versions and status
- **SVG 1.1 Second Edition** (W3C Recommendation, 2011) is the baseline that all tools implement.
- **SVG 2** is still the **04 Oct 2018 Candidate Recommendation** on /TR. Editor's drafts continued into Sept 2025.
  - https://www.w3.org/TR/SVG/
  - https://svgwg.org/svg2-draft/single-page.html
- **Mesh gradients and hatches were removed from SVG 2 in 2018.** Only Inkscape implemented them. They were moved to the SVG 2.1 / "next" drafts with no guarantee of returning.
  - https://librearts.org/2018/05/gradient-meshes-and-hatching-to-be-removed-from-svg-2-0/
  - https://wiki.inkscape.org/wiki/SVG2
- **Inkscape mesh gradients:** since 1.0, Inkscape embeds a JavaScript polyfill so they display in browsers. https://wiki.inkscape.org/wiki/index.php/Release_notes/1.0
- **SVG Tiny 1.2** (Recommendation, 2008) is a mobile subset. It has no clipPath, mask, pattern, filters or markers (unverified from memory). It is only relevant today through BIMI's SVG Tiny PS profile. Don't target it.

### A2. Variable-width strokes: confirmed absent
- **ISSUE-2271**, "variable stroke width (as in calligraphy)", was raised 2009-05-16 and still shows state RAISED. https://www.w3.org/Graphics/SVG/WG/track/issues/2271
- **2013 proposals went nowhere.** Tavmjong Bah (Inkscape) proposed `stroke-widths` / `<strokeProfile>` stop syntax, and there was a working-group action to write up a proposal. None of it was specified.
  - https://lists.w3.org/Archives/Public/www-svg/2013May/0041.html
  - https://www.w3.org/Graphics/SVG/WG/track/actions/2583
- **The SVG Strokes module Editor's Draft (14 Sep 2025) doesn't include it either.** It contains only `stroke-alignment`, `stroke-dashcorner`, `stroke-dashadjust` and the `arcs`/`miter-clip` joins. There is no width profile. https://svgwg.org/specs/strokes/
- **Consequence:** any pressure-varying stroke must be exported as a **filled outline**. That is how Inkscape PowerStroke works: the `<inkscape:path-effect effect="powerstroke" offset_points=... interpolator_type=CubicBezierJohan|CentripetalCatmullRom|... start_linecap_type ... linejoin_type ...>` element lives in `<defs>`, and the path carries `inkscape:path-effect="#…"`, `inkscape:original-d="…"` (the source), and `d` (the computed outline).
  - https://inkscape.gitlab.io/inkscape/doxygen/lpe-powerstroke_8cpp_source.html
  - https://wiki.inkscape.org/wiki/PowerStroke

### A3. What interoperates in SVG

**Safe (works everywhere that matters):**
- `<path>` with absolute or relative commands, plus basic shapes
- `<g>` with `transform`
- `fill`, `stroke`, `fill-rule`, `opacity` and `fill-opacity`, linecap, linejoin, miterlimit, dasharray
- `linearGradient` and `radialGradient`
- Embedded PNG via `<image>`
- `viewBox` with physical `width`/`height` in mm

**Mostly safe:**
- **`clipPath`, `mask`:** fine in browsers, Inkscape, Illustrator and Affinity. Figma loses masks and only clips simple shapes. Cutter apps reject both.
  - https://forum.figma.com/t/importing-svgs-with-masks-broken/4254
- **Gradients with `gradientUnits="userSpaceOnUse"`** trip up Graphite. https://github.com/GraphiteEditor/Graphite/issues/2524
- **`<text>`** depends on installed fonts. Convert it to outlines for anything headed to fabrication.

**Poorly supported:**
- Mesh gradients and hatches (Inkscape only).
- **Filters:** Figma is limited, and Cricut and LightBurn ignore them.
- **`mix-blend-mode`:** browsers support it. Inkscape's support is inconsistent, and blend modes get lost in Figma and in Adobe round-trips.
  - https://inkscape.org/forums/questions/blend-mode-not-showing-on-svg/
  - https://forum.figma.com/t/blend-mode-and-svg/387
- **SVG 2-only syntax** (unverified detail): `href` without `xlink:`, `paint-order`, `vector-effect`, CSS Color 4 `color(display-p3 …)`, `<style>` classes, and SVG 1.2 `flowRoot` / SVG 2 `shape-inside` text.
- **Rule:** emit SVG 1.1 syntax with presentation attributes. Cricut and Silhouette ignore `<style>` blocks. https://svgmaker.io/blogs/svg-wont-open-in-cricut-design-space-or-silhouette-studio-quick-fix

### A4. How the major apps handle SVG

**Illustrator**
- "Save As SVG" with *Preserve Illustrator Editing Capabilities* embeds an entire private AI/PGF payload (`i:pgf`), so the file is 2× or more larger and no one else can read that part.
- *Export As* produces clean SVG 1.1 with presentation attributes.
- Width-tool strokes are exported as outlines (unverified, but consistent with SVG's lack of variable width).
- Sources:
  - https://creativepro.com/understanding-illustrator-file-formats/
  - https://css-tricks.com/snippets/svg/abobe-illustrator-export-options/

**Inkscape**
- Its native format is SVG. Editing data lives in the `inkscape:` and `sodipodi:` namespaces:
  - `sodipodi:type` and `sodipodi:nodetypes`
  - `inkscape:groupmode="layer"`, `inkscape:label`
  - `inkscape:original-d` plus `inkscape:path-effect` for Live Path Effects (LPEs)
- **Design principle:** "Inkscape SVG should render identically with or without the Inkscape extensions." Caveat: on re-edit, Inkscape regenerates `d` from its stored parameters, so it can overwrite edits made in other apps.
- *Plain SVG* export strips the namespaces.
- Inkscape keeps unknown foreign-namespace XML. Ink/Stitch stores all its embroidery parameters as `inkstitch:*` attributes and depends on this.
- Sources:
  - https://wiki.inkscape.org/wiki/Inkscape_SVG_vs._plain_SVG
  - https://wiki.inkscape.org/wiki/Inkscape-specific_XML_attributes
  - https://inkstitch.org/namespace/

**Affinity**
- Good support for gradients, masks and clips. Effects that have no SVG equivalent are rasterized on export. Complex Inkscape-specific clips and filters sometimes fail to import.
  - https://imagetosvg.com/app/affinity-designer
  - https://logosbynick.com/import-edit-svg-files-affinity-designer/

**Figma**
- Weak on masks, filters and blend modes, and drops unknown data.
- `.fig` is a proprietary Kiwi binary that embeds its own schema. It has no public spec.
  - https://github.com/OpenFig-org/openfig-core/blob/main/docs/research.md

**Graphite**
- Native format is a `.graphite` JSON node graph. It is alpha software.
- Import and export are SVG, but gradients and filters are not preserved consistently on import.
  - https://graphite.art/features/
  - https://lwn.net/Articles/1051242/

**Krita**
- Vector layers are SVG inside the `.kra` zip, at `layers/<n>.shapelayer/content.svg`, since 4.0.
- It can export one vector layer as SVG.
- Paint strokes are always raster.
  - https://docs.krita.org/en/general_concepts/file_formats/file_svg.html
  - https://krita-artists.org/t/use-krita-api-to-live-update-vector-layer-content/147585

**Fabrication tools**
- **Cricut Design Space:**
  - Ignores gradients, filters, masks, patterns, clip paths and CSS.
  - A stroke-only path becomes a **double cut line** along both edges, so users must "outline stroke" first.
  - https://inkscape.org/da/forums/cutplot/converting-a-stroke-to-a-path-creates-a-fill-which-also-creates-2-cut-lines-in-cricut-design-space/
  - https://photo2vector.com/blog/cricut-svg-not-cutting-troubleshooting
- **LightBurn:**
  - Maps each unique color to a cut layer, and stroke color takes precedence over fill color.
  - Ignores fill/stroke semantics, gradients, patterns and images. Text is not imported.
  - https://forum.lightburnsoftware.com/t/lightburn-turns-fill-color-into-stroke-color-in-svg/115843
- **Glowforge:**
  - Stroked paths become cuts or scores, and filled shapes become engraves. Stroke width is ignored.
  - Sizing and DPI mistakes are common.
  - https://vectosolve.com/blog/glowforge-svg-requirements
- **Silhouette Studio:** SVG import requires Designer Edition. The free edition imports DXF only. https://www.silhouetteschoolblog.com/2016/10/silhouette-studio-file-types.html
- **Embroidery:** Ink/Stitch (Inkscape extension, SVG with `inkstitch:` attributes) is the open path. Commercial digitizers import SVG as outlines for auto-digitizing.
- **What this means for Capy:** fabrication users need **centerline strokes** (plotting, scoring, running stitch) or **clean filled outlines** (engrave, vinyl). They need **no clip, mask or style**, and **physical units**.

### A5. Embedding private editable data in SVG
- **Use a foreign XML namespace you control**, for example `xmlns:capy="https://capycanvas.art/ns/svg/1"` (URI illustrative).
  - SVG renderers must ignore unknown namespaced elements and attributes.
  - Inkscape preserves them.
  - Illustrator, Figma, Affinity and usvg drop them. That is acceptable, because the visible geometry is still correct.
- **Where to put it:**
  - Per-element attributes such as `capy:stroke="id"` and `capy:brush="#b3"`.
  - Bulk payload in `<metadata><capy:document>…</capy:document></metadata>`. `<metadata>` is intended for arbitrary non-SVG XML, most often RDF/Dublin Core.
- **Avoid `data-*` attributes.** They are valid in SVG 2 and HTML-embedded SVG, but SVG 1.1 tools treat them as unknown non-namespaced attributes and often strip them.
- **Avoid Illustrator's approach** of embedding a whole opaque document. It doubles the size and invites divergence.
- **Precedents for "viewable file plus embedded editable source":**
  - Windows Ink saves **GIF with embedded ISF** (the only save format for `InkStrokeContainer`). https://learn.microsoft.com/en-us/windows/uwp/ui-input/save-and-load-ink
  - Krita `.kpp` is a PNG with XML in a metadata chunk.
  - LibreOffice "hybrid PDF" embeds the ODF source (unverified detail).

### A6. PDF, AI and EPS
- **PDF is the print interchange format, and its vector model is a superset of SVG.** It has:
  - All 16 standard blend modes, soft masks, isolated and knockout groups.
  - Coons and tensor-patch mesh shadings (types 6 and 7), so mesh gradients *are* expressible in PDF.
  - ICC-based, CMYK and spot color.
  - Optional content (layers).
- **PDF also has no variable-width stroke,** so strokes still become outlines. For print hand-off, use PDF/X-4, which allows live transparency and ICC color (general knowledge).
- **AI:**
  - A modern `.ai` file is a PDF plus private PGF data under `/AIPrivateData`.
  - Other apps (Affinity, Inkscape) read **only the PDF-compatible stream**. That stream exists only if "Create PDF Compatible File" was on, which is the default.
  - Treat AI import as PDF import. Never try to write AI.
  - Sources:
    - https://www.datalogics.com/adobe-illustrator-and-pdf-compatibility
    - https://www.affinity.studio/help/import-ai/
    - https://alpha.inkscape.org/vectors/www.inkscapeforum.com/viewtopicc3c6.html?t=21992
- **EPS:** a legacy PostScript format with no transparency. Import requires a PostScript interpreter; Ghostscript is AGPL. Skip it.
- **Rust PDF writers:**
  - **krilla 0.8.2** (Aug 2026), MIT/Apache, built on `pdf-writer`, used as Typst's backend. It supports:
    - Paths, fills, strokes and clip paths
    - Alpha and luminosity masks
    - Blend modes and isolation
    - Linear, radial and sweep gradients, and patterns
    - Images, and SVG via `krilla-svg`
    - Font subsetting and tagged PDF
    - Validated **PDF/A-1…4 and PDF/UA-1** output

    PDF/X is not listed. Its MSRV is 1.92.
    - https://github.com/LaurenzV/krilla
    - https://docs.rs/krilla
  - **pdf-writer** is the low-level option, needed for things krilla doesn't expose, such as OCG layers and PDF/X OutputIntents.
  - **svg2pdf** converts a usvg tree to PDF. That is the cheapest route: build the same tree used for SVG export.
  - **printpdf 0.9** uses svg2pdf for SVG. https://lib.rs/crates/printpdf
- **Rust PDF reading: hayro (Apache/MIT).** `hayro-interpret` emits paths, fills, strokes, images and clips into an abstract `Device` trait, which is exactly what a vector importer needs. It handles more than 1,400 regression PDFs. Gaps: knockout groups and non-embedded CID fonts. https://github.com/laurenzv/hayro

### A7. Other container formats
- **PSD** (spec dated Nov 2019, public but incomplete):
  - Paths are image resources 2000–2997 (2999 is the clipping path name).
  - Path points are normalized 8.24 fixed-point Bézier knots.
  - Shape layers are a fill layer plus a vector mask (`vmsk`/`vsms`), with `vstk` stroke descriptors and `vogk` live-shape data.
  - Krita reads vector shapes and text from PSD but recommends ORA or TIFF instead. GIMP imports paths.
  - One vector mask per layer makes PSD a poor carrier for thousands of strokes. **Export vector layers to PSD rasterized.**
  - Sources:
    - https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
    - https://docs.krita.org/en/general_concepts/file_formats/file_psd.html
    - https://invent.kde.org/graphics/krita/-/merge_requests/1954
- **OpenRaster (ORA) 0.0.6:**
  - The layer `src` attribute may point to SVG; the spec's own example is `data/hw.svg`.
  - MyPaint 1.1+ wrote SVG vector layers. Drawpile rejected those files.
  - **Default to PNG layers in ORA,** with SVG as an opt-in.
  - Sources:
    - https://www.openraster.org/baseline/layer-stack-spec.html
    - https://github.com/drawpile/Drawpile/issues/184
- **DXF:** the lingua franca for CAD, laser and CNC.
  - Use POLYLINE (R12) or LWPOLYLINE (R14+) plus LINE, ARC and CIRCLE.
  - SPLINE support in CAM tools is uneven, so flatten Béziers.
  - DXF has no fills and no RGB (color comes from the AutoCAD Color Index). It is a centerline-only format.
  - Silhouette Basic and Concepts use it.
  - Rust: the `dxf` crate (https://docs.rs/dxf) or `acadrust` (R12–R2018).
- **Lottie:** JSON animation format. The spec v1.0 was published Sept 2024 by the Lottie Animation Community (Linux Foundation / JDF). It has paths, fills, strokes, gradients, masks and trim paths, but no variable width. It is only useful for exporting "draw-on" replays. https://www.linuxfoundation.org/press/lottie-animation-community-announces-lottie-v1.0-specification
- **EMF/WMF:** Windows metafiles. They only matter for the Office and Windows clipboard. Low priority.
- **Sketch** is a zip of JSON with a published schema. **Figma `.fig`** is proprietary (see A4). Import both via SVG or clipboard only.

---

## B. Stroke and ink data formats (the "replay" question)

### B1. W3C InkML: the only vendor-neutral standard
- **Status:** W3C Recommendation, **20 Sep 2011**. https://www.w3.org/TR/InkML/
- **Structure:** `<ink>` contains `<definitions>`, `<context>`, `<inkSource>`, `<traceFormat>`/`<channel>`, `<trace>`, `<traceGroup>`, `<brush>`/`<brushProperty>`, `<timestamp>` and `<annotation>`/`<annotationXML>`.
- **Reserved channels:**
  - `X Y Z`: position
  - `F`: tip force
  - `S`: tip switch
  - `B1..Bn`: buttons
  - `OTx OTy`: tilt
  - `OA OE OR`: azimuth, elevation, rotation
  - `C`/`CR CG CB`/`CC CM CY CK`: color
  - `W`: width
  - `T`: time
- **Trace text** supports explicit, first-difference and second-difference encoding (from memory).
- **Brush properties:**
  - `color`, `width`, `height`, `transparency`
  - `tip` (ellipse, rectangle or drop)
  - `rasterOp`, `antiAliased`, `fitToCurve`, `ignorePressure`
  - `brushRef` inheritance
- **There is no model for dynamics curves, spacing, textures or scatter.** A painting brush cannot be described in InkML.
- **Who uses it:**
  - **Microsoft Office (OOXML):** ink parts are `application/inkml+xml` (a supported subset) in Word, PowerPoint and Excel. https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/096dacae-0d2c-4861-bc4d-c8e4c6405ad3
  - **OneNote API:** reads with `?includeInkML=true` and writes a `presentation-onenote-inkml` part. It was in beta in 2017. https://devblogs.microsoft.com/microsoft365dev/onenote-ink-beta-apis/
  - **Handwriting-recognition datasets** such as CROHME. https://www.isical.ac.in/~crohme/CROHME_data.html
  - **Wacom's InkML→UIM converter.** https://github.com/Wacom-Developer/inkml-to-uim
  - I found no painting or illustration app that uses it.

### B2. Microsoft Ink Serialized Format (ISF)
- **History and licensing:** introduced in 2002 with Tablet PC. The spec is published as a PDF and covered by the Open Specification Promise.
  - https://learn.microsoft.com/en-us/uwp/specifications/ink-serialized-format
  - https://en.wikipedia.org/wiki/Ink_Serialized_Format
- **Contents:**
  - Packets: X/Y, pressure, tilt and custom GUID properties.
  - Drawing attributes: color, width/height, tip, transparency, raster op, fit-to-curve.
  - Transforms.
  - Compact delta/derivative compression.
- **How Windows uses it:** Windows Ink's `InkStrokeContainer.SaveAsync` writes only **GIF-with-embedded-ISF**, and `LoadAsync` also accepts raw and base64 ISF. https://learn.microsoft.com/en-us/windows/uwp/ui-input/save-and-load-ink
- **Adoption outside Microsoft is negligible.** Rnote cites its derivative encoding as design inspiration. https://github.com/flxzt/rnote/issues/1173
- **Relevance to Capy:** it may be worth *reading* on the Windows host for pasting ink from the clipboard.

### B3. Wacom Universal Ink Model (UIM) / WILL 3
- **Spec:** UIM **v3.1.0**, published on Wacom's developer portal. https://developer-docs.wacom.com/docs/specifications/intro/
- **Encoding:** a RIFF container (`UIM3` chunk with HEAD and DATA sub-chunks) holding a Protocol Buffers v3 message. https://developer-docs.wacom.com/docs/sdk-for-ink/uim/encoding/
- **Contents:**
  - **InputData:** raw sensor channels.
  - **InkData:** Catmull-Rom splines with per-point properties.
  - **Brushes:** vector brushes (polygon prototypes), and **raster brushes with PNG shape and fill textures, particle spacing, randomization seed and blend modes**.
  - An RDF/OWL knowledge graph for semantics.
- **The richest open ink model found,** and the only one that can describe a textured stamp brush.
- **Standardization:** Wacom's docs make **no ISO/IEC or W3C standardization claim**, and I could not confirm any.
- **Licensing:** the Python `universal-ink-library` is Apache-2.0. The terms for the WILL SDKs are on Wacom's developer portal and should be checked.
  - https://github.com/Wacom-Developer/universal-ink-library
- **Adoption** appears confined to Wacom's own ecosystem and SDK licensees (unverified).

### B4. Android Jetpack Ink (`androidx.ink`)
- **Releases:** 1.0.0 stable on **2025-12-17**. 1.1.0-alpha09 on 2026-09-23. https://developer.android.com/jetpack/androidx/releases/ink
- **Stroke storage:** `StrokeInputBatch.encode`/`decode` produce a compact binary described as "a tiny fraction" of naive storage. Float drift was fixed in beta02.
- **Brush storage:**
  - **`BrushFamily` serialization graduated to stable API in 1.1.0-alpha02** (Apr 2026), with a decode overload that takes a max version.
  - `calculateMinimumRequiredVersion` arrived in alpha06.
  - Custom families are defined by a proto, with textured brushes such as Pencil.
- **Documented persistence recipe:** store the brush family (or an enum for stock brushes), color, size, epsilon and the encoded inputs. Meshes are regenerated, not stored. https://developer.android.com/develop/ui/views/touch-and-input/stylus-input/ink-api-persistent-storage
- **Engine:** the C++ engine `google/ink` is Apache-2.0 and includes a protobuf `storage` module, but it gives "no hard guarantees about interface stability". https://github.com/google/ink
- **Assessment:** the closest thing to an open, replayable "stroke + brush" model with an open-source reference renderer. It is still a single-engine format.

### B5. Apple PencilKit
- **`PKDrawing.dataRepresentation()` is an opaque, undocumented "Apple Drawing Format".**
  - Public API exposes `PKStroke` (ink type, transform, mask) and `PKStrokePoint` (location, timeOffset, size, opacity, force, azimuth, altitude).
  - Newer inks, such as iOS 17 watercolor, broke loading with "Apple Drawing Format is from a future version". https://developer.apple.com/forums/thread/734632
- **Interop only happens through the API on Apple platforms.** The Apple host could convert PencilKit points into Capy samples.

### B6. App-specific formats

| App | Stroke storage | Notes |
|---|---|---|
| Excalidraw | JSON `freedraw`: `points`, `pressures[]`, `simulatePressure` | Rendered with perfect-freehand. SVG export is outline fills. https://plus.excalidraw.com/docs/api/scene-content-schema |
| tldraw | `draw` shape `segments` (free/straight). Points are delta-base64 encoded (first point Float32, then Float16 deltas). z = pressure, plus `isPen` and size s/m/l/xl | https://tldraw.dev/sdk-features/draw-shape |
| Xournal++ `.xopp` | Gzipped XML. `<stroke tool color width="nominal w1 w2 …">x y x y…</stroke>` gives one width per segment, in points (1/72 in) | https://xournal.sourceforge.net/manual.html |
| Rnote `.rnote` | Gzipped serde JSON. A new format (version header, zstd, pluggable serializer) was tried in PR #1177 (closed Nov 2025, moved to #1578). bitcode was rejected because it isn't forward-compatible | https://github.com/flxzt/rnote/pull/1177 |
| Concepts | Native vector strokes with textures | SVG, DXF and vector-PDF export: "single line weight per stroke", textures stripped. https://concepts.app/en/manual/export |
| Clip Studio `.clip` | Proprietary CSFCHUNK chunks plus embedded SQLite. Vector strokes in `VectorObjectList` with Bézier controls, brush radius, and per-point width and opacity factors | Clean-room MIT Rust parser: https://github.com/Aodaruma/clipfile-rs. CSP can export vector layers to SVG: https://tips.clip-studio.com/en-us/articles/3714 |
| Adobe Fresco | Vector brushes | No SVG export. The PDF export places a raster composite *over* the vector layers. https://community.adobe.com/t5/fresco-discussions/vector-layer-from-fresco-not-editable-in-illustrator/td-p/10921995 |
| Procreate | Zip containing `Document.archive` (NSKeyedArchiver) and per-layer raster `.chunk` tiles, plus time-lapse MP4 segments | **No stroke data.** Replay is video only. https://github.com/Avarel/silicate |
| Krita | Paint layers are raster; vector layers are SVG | The recorder docker records snapshots, not strokes (unverified detail) |

### B7. Brush-definition formats (all proprietary to their engines)
- **Photoshop ABR:** no public spec. GIMP and Krita import only the tip images, and dynamics are lost. Third-party converters approximate the dynamics. https://github.com/Pawel-9215/abr-to-krita
- **Krita `.kpp`:** a PNG thumbnail with the paint-op XML stored in a text chunk. https://docs.krita.org/en/reference_manual/resource_management/paintoppresets.html
- **Procreate `.brush`/`.brushset`:** a zip containing a plist plus Shape and Grain PNGs. https://convert.guru/brush-converter
- **CSP `.sut`:** SQLite-based (community knowledge, unverified).
- **MyPaint `.myb`:** documented JSON v3 (`settings → base_value + inputs` curves). The libmypaint engine is ISC-licensed and embedded in GIMP and Krita, making it the only portable engine plus brush format pair. https://github.com/mypaint/libmypaint/wiki/Using-Brushlib
- **Relevance:** exchanging samples alone doesn't reproduce a look. Replay fidelity requires *the same engine semantics* (spacing, dynamics curves, jitter PRNG, blending). So portable replay would require publishing Capy's brush model as a spec, which is out of scope.

### B8. Conclusion on "replayable brush strokes"
- **No practical cross-editor standard exists.**
  - InkML is standardized, and Office uses it, but its brush model is trivial and creative tools don't read it.
  - UIM and Jetpack Ink can express real brushes, but each is one vendor's format and engine.
  - Every painting app uses a private format, and several (Procreate, Krita paint layers) don't store strokes at all.
- **What "replay elsewhere" can realistically mean:**
  1. **Static geometry** as SVG or PDF outlines. Universal.
  2. **Replay as media:** time-lapse video, or an animated SVG that reveals each outline through a mask whose centerline is animated with `stroke-dashoffset`. This works in browsers but isn't editable.
  3. **Ink data for ink tools:** InkML (Office, OneNote, handwriting recognition).
  4. **Exact replay and re-editing** only in Capy Canvas, from its own data. This can be embedded in SVG or PDF as private data.

---

## C. Recommendations for Capy Canvas

Context from the repo:
- `.capy` is a CAPYRASTER container: JSON metadata plus digest-indexed LZ4 blobs, and brush and source assets already carry their own digests (`docs/reference/project-format.md`).
- Strokes already record real samples plus a `BrushSnapshot` and a deterministic seed for replay (`docs/internals/brushes.md`).
- Vector layers can reuse both.

### C1. Native storage (source of truth)
```
VectorLayer { strokes: [VectorStroke], blend, opacity, ... }
VectorStroke {
  id, transform: Affine,                           // local space; ancestor transforms separate
  samples: blob,                                   // x,y,t,pressure,tilt/azimuth/altitude,twist,device kind
  brush: digest -> BrushSnapshot (deduped table), seed,
  engine_version,                                  // dab generator, smoothing, outline algorithm
  edited_curve: Option<{ cubic Béziers, width(t) profile, opacity(t) profile }>,
  outline_cache: Option<blob, engine_version>      // baked fill geometry for export/fallback
}
```
- **Untouched strokes: samples are authoritative.** This enables exact replay, time-lapse, re-brushing after the fact, and resolution-independent re-rendering.
- **Encode samples compactly.** Store the first sample at full precision, then quantized deltas. tldraw's Float16 deltas and ISF's derivative coding are precedents. Put them in the existing blob store, not in JSON arrays.
- **Node editing:** fit centerline plus width and opacity profiles using kurbo's curve fitting and simplification. The **edited curve becomes authoritative.** To render it, synthesize a sample stream along the curve and run it through the same dab engine, so there is only one render path. Keep or drop the original samples according to a policy, marking them stale.
- **Imported SVG/PDF paths use the same model.** A stroked path becomes a synthetic constant-pressure stroke. A filled path becomes a "shape" object (fill plus optional stroke).
- **Always store `outline_cache`, versioned.** If the engine changes, old files still render identically. This matches Inkscape's "`d` is always the rendered truth" principle.
- **Restrict vector layers to self-contained brushes.** Destination-aware brushes (smudge, wet mix, watercolor, liquify) read the underlying pixels and can't be resolution-independent vectors. Disable them on vector layers or rasterize the stroke on commit.

### C2. Brush classes and how they export

| Brush class | Native | SVG/PDF export |
|---|---|---|
| Solid ink (round or elliptical tip, pressure→size) | exact | Union of swept tip outline as a filled path. Exact. |
| Pressure→opacity / flow buildup | exact | **Not expressible as a flat fill.** Options: split into opacity-banded segments (visible seams), a gradient-mask approximation, rasterize, or flatten to constant alpha. |
| Textured / stamped / grain / scatter | exact | Options: (a) raster `<image>` of the stroke, clipped by its outline, which gives a crisp edge but isn't vector; (b) outline filled with a `<pattern>` grain tile, a rough approximation that cutters and Figma ignore; (c) solid outline, lossy but universal (Concepts does this). (d) Fresco's approach: a PDF with a raster appearance layer over the vector layers. |
| Destination-aware | n/a on vector layers | rasterize |

**Default:** solid outline, with a per-export option of "raster appearance overlay" or "embed stroke rasters".

### C3. Export
1. **SVG "Capy editable"** (default for Save As SVG):
   - SVG 1.1 syntax: presentation attributes, `xlink:href`, no `<style>`.
   - `width`/`height` in mm plus a `viewBox`.
   - One filled `<path>` per stroke (nonzero fill, simplified, 2–3 decimals).
   - Layers as `<g id inkscape:groupmode="layer" inkscape:label>`, which Inkscape understands and others ignore.
   - Layer opacity on the group.
   - `mix-blend-mode` only when not Normal, with `isolation:isolate`.
   - Private data:
     - `xmlns:capy`.
     - On each path: `capy:stroke`, `capy:brush`, and a `capy:geom` fingerprint (bbox, node count, coarse hash of quantized geometry).
     - `<metadata><capy:document version>` containing the brush table and per-stroke sample blobs (deflate + base64). Samples are stored in the path's **local** coordinates, so moves and scales made in other editors via ancestor `transform`s carry over.
2. **SVG "Plain"**: the same file with no `capy:`/`inkscape:` data. This is the equivalent of Inkscape's Plain SVG or Illustrator's Export As.
3. **SVG "Cut/Plot"**:
   - **Centerlines**: `fill="none"`, stroke color per operation, nominal width. Variable width is dropped or reduced to the average.
   - Optionally, outlines boolean-unioned per color, with no clip, mask, gradient, image or text. For clip and eraser geometry, apply the booleans geometrically before export.
   - Also offered as **DXF** (flattened polylines).
4. **PDF** via krilla:
   - Outlines, native blend modes and soft masks.
   - ICC color. This matters because `.capy` supports Display P3, Adobe RGB and ProPhoto, while SVG realistically means sRGB, so convert on SVG export.
   - PDF/A optional.
   - Optionally embed the `.capy` file (or the private stroke data) as an attached file, so a PDF round-trips in Capy (hybrid-PDF pattern).
   - Use pdf-writer directly for OCG layers or PDF/X-4 if print shops ask for them.
5. **InkML** (optional, "for ink tools"):
   - Map channels X, Y, T, F, OTx/OTy (or OA/OE) and OR.
   - Brush maps to `width`, `color`, `transparency` and `tip=ellipse`. Capy's texture and dynamics are lost.
   - Target the Office/OneNote subset for maximum reach.
6. **Replay export:** time-lapse video from the existing replay, and optionally an animated SVG or Lottie "draw-on".

### C4. Import
- **SVG via usvg/resvg** (0.48.x, Aug 2026). What it does:
  - Resolves CSS, `use`/symbols, nested `svg` and markers.
  - Converts shapes to absolute M/L/Q/C/Z paths.
  - Resolves text layout and gradients, and keeps clip paths, masks, filters and images.
  - Drops animation, scripts, SVG fonts, external `use`, and `color-profile`.
  - https://docs.rs/usvg/
  - https://github.com/linebender/resvg/blob/main/docs/unsupported.md

  What is lost:
  - Primitive editability (rect and circle become paths).
  - Text editability, unless the text nodes are mapped.
  - CSS classes.
  - All foreign-namespace data (`inkscape:*`, `capy:*`).
  - SVG 2 extras such as mesh gradients and hatches, which are unsupported.

  **Do a second pass with `roxmltree`** (the XML parser usvg itself uses) to read `capy:` data, keyed by element `id`. usvg keeps ids.
- **Trust rule for private data:**
  - If the visible geometry still matches the `capy:geom` fingerprint, within a tolerance that allows for number reformatting, restore the full strokes. Then apply the element's current CTM.
  - Otherwise another editor changed the art. Import the visible path as plain vector geometry, and offer "restore original strokes".
  - This avoids Inkscape's "regenerate `d` and clobber edits" trap.
  - Treat the payload as untrusted: cap decompressed sizes and validate everything, as `.capy` already does.
- **PDF/AI via hayro-interpret:** walk the `Device` calls into shape objects, with images becoming raster layers. AI works only when it has its PDF-compatible stream; ignore `/AIPrivateData`.
- **Skip EPS** (or shell out to a user-installed Ghostscript). PSD vector masks and ORA SVG layers are low priority. Krita `.kra` vector layers are plain SVG and could be imported via the same SVG path.
- **Ink imports (cheap to add):**
  - InkML: Office and OneNote ink.
  - PencilKit through the Apple host API.
  - ISF through the Windows host.
  - Excalidraw, tldraw and Xournal++ JSON/XML, which all have points plus pressure.

### C5. Pitfalls checklist
- **Units and DPI:** use mm in `width`/`height`. CSS assumes 96 px/in, legacy Illustrator uses 72, and Glowforge and Affinity sizing complaints are common.
- **Stroke vs fill semantics** differ per fabrication target. Offer explicit centerline vs outline modes; never export both for one stroke.
- **Alpha:** a stroke's internal overlap buildup disappears in an outline fill. Stroke-level opacity maps to `fill-opacity`, and layer opacity maps to group `opacity`.
- **Vector erasers:** implement them as geometric booleans or stroke splitting (like Concepts' Slice), not as masks, which cutters, Figma and Cricut don't support.
- **Wide gamut:** SVG and most editors assume sRGB. Use PDF with ICC for P3, Adobe RGB and ProPhoto.
- **File size:** pressure outlines are node-heavy. Fit, simplify and round, and make embedded samples optional (a "Preserve Capy editing data" toggle, like Illustrator's).
- **Engine drift:** version the dab generator and outline algorithms, and keep baked outlines so old files never re-render differently.
- **Useful Rust crates:**
  - `usvg`/`resvg`, `roxmltree`
  - `quick-xml` (already a dependency)
  - `kurbo` (Béziers, stroke expansion, curve fitting)
  - A boolean-ops crate such as `i_overlay` or Linebender's `linesweeper`
  - `krilla`/`pdf-writer`/`svg2pdf`, `hayro-interpret`
  - `dxf`
