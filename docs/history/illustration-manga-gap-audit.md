# Professional illustration and manga: gaps and recommended behavior

[Design history](README.md)

Research date: **2026-10-04**. Current-code baseline: **`origin/main` at
`ad78ed40d27bd04bb2d6994374b67f24d22ad1f5`**. The initial illustration audit
used `24e00192bc70c33e6e8ece1aef68eb47f46fb73f`; this record reconciles its
recommendations with the newer main branch. Code links below refer to the
current-code baseline. Later changes require checking those claims again.

This is a research and product recommendation record. **Implementation remains
pending further direction.** Priorities 1 and 2 receive the detailed treatment:
daily illustration editing and professional flat-color/region workflows.
Priorities 3–5 retain the other gaps so they are not lost.

## Scope and conclusions

The audit covers sketching, inking, region and mask construction, coloring,
rendering, revision, collaboration, manga page production and delivery. It traces
artist-facing commands and options into shared implementation rather than
counting tool names. Sources include original Japanese professional walkthroughs,
official Japanese documentation, Japanese printer requirements and professional
coloring research.

**Present** means implemented in the inspected source, not qualified through a
complete artist journey on every host. **Partial** identifies a missing option,
target or interaction within an existing capability. **Missing** identifies a
new artist-facing capability. Acceptance examples below are proposed checks;
they were not executed as part of this source audit.

The highest-value open work is consistent geometric mask editing, accessible
brush customization, and a family of reference-aware enclosed-region tools.
Many general editing gaps from the initial audit are already covered on main:
clipboard operations, crop/resize, merges, selection refinement, broader blends
and Distort/Warp. They must not become duplicate implementations.

## Workflow evidence

The linked video is RiceBrush's analysis of Mogoon's workflow. Chapter markers
and sampled storyboard frames were inspected, including 9:03; captions were
unavailable, so this is a visual review rather than a full spoken-instruction
review. [Video](https://www.youtube.com/watch?v=JNfcnJBdel4&t=543s).

| Phase | Observed emphasis | Product implication |
| --- | --- | --- |
| Sketch, 1:00 | Large shapes, composition, pose and value/color structure. | Fast shape blocking, references, canvas revision and value inspection. |
| Line art, 3:42 | Defining forms without resolving every detail. | Adjustable inking behavior; support loose as well as clean closed contours. |
| Masking, 5:33 | Separating silhouettes and parts/materials. | Fast, reusable regions with direct mask editing. |
| Rendering, 7:40 | Lighting, materials and detail. | Personal brushes, clipping, adjustments and accessible references. |

Around 9:03 the frames show transparency/reflectivity references; later frames
show grayscale and threshold comparisons. The recommendation is to improve
reference and inspection workflows, not introduce a separate physical-lighting
tool. Enclosure filling complements manual silhouette/mask construction because
the artwork need not have perfectly closed linework.

Japanese walkthroughs give additional concrete evidence: Renta adjusts working
resolution, stabilization and line masks before flats; Wasabi creates a silhouette
base, separates part colors into clipped layers/folders and repairs small missed
regions with enclosure filling. [Renta](https://tips.clip-studio.com/ja-jp/articles/1507),
[Wasabi](https://tips.clip-studio.com/ja-jp/articles/662).

## Priority 1: complete daily illustration editing

### Covered requirements from the initial audit

These remain part of the desired workflow, but are **not missing tools on the
current baseline**. Retain the existing implementations and qualify their artist
journeys instead of scheduling replacement implementations.

| ID | Requirement | What we have today | Recommended state and change type | Evidence |
| --- | --- | --- | --- | --- |
| D01 | Copy/cut artwork and Copy Merged | Pixel clipboard captures active artwork or the composite. Paste, Paste in Place and Paste Into create a new layer; the internal clip retains precision and a PNG serves external applications. | **Covered.** Preserve existing clipboard behavior, color handling, cancellation and position. Qualify exchanges between drawings and external apps. | [Clipboard](../../crates/layer-ui/src/clipboard.rs) |
| D02 | Clear selected pixels and create layers from selections | Clear Selected, Clear Outside, Copy Selection to Layer and Cut Selection to Layer have shared implementations. | **Covered.** Reuse these commands for flat/ink revision; preserve soft coverage, lock rules and one-step history. Artwork clearing does not establish mask-target parity. | [Selection pixels](../../crates/layer-ui/src/selection_pixels.rs) |
| D03 | Ordinary layer merges | Merge Down, Merge Group, Merge Visible, Flatten Image and Stamp Visible exist, including clipping/effect-specific behavior. | **Covered for common merges.** Retain existing appearance-preserving rules. Direct arbitrary Merge Selected remains a convenience gap in D11. | [Merge UI](../../crates/layer-ui/src/merges.rs), [merge model](../../crates/layer-core/src/merge.rs) |
| D04 | Crop the master canvas | Crop provides ratio choices, overlays, straightening and a delete-cropped-pixels choice; Crop Canvas to Selection also exists. | **Covered.** Use the existing crop session for composition revision; preserve cancellation and the distinction between reframing and deleting pixels. | [Crop](../../crates/layer-ui/src/crop.rs), [canvas size](../../crates/layer-ui/src/canvas_size.rs) |
| D05 | Resize the canvas | Canvas Size supports units, relative dimensions and anchoring. | **Covered.** Retain canvas reframing independently of resampling artwork. Manga trim/bleed metadata remains separate. | [Canvas size](../../crates/layer-ui/src/canvas_size.rs) |
| D06 | Resample artwork and edit print resolution | Image Size supports physical units, resolution, proportional dimensions and Automatic/Bicubic/Lanczos/Bilinear/Nearest resampling. | **Covered.** Retain rough-to-working-size revision. Physical size/DPI in initial creation remains partial in D10. | [Image size](../../crates/layer-ui/src/image_size.rs) |
| D07 | Refine an existing selection | Grow, Shrink, Feather, Border and Smooth support preview/apply/cancel for current selections and editable selection masks. | **Covered.** Reuse existing refinement. Geometric Shrink is not the enclosure-selection feature F03. | [Selection refinement](../../crates/layer-ui/src/selection_refine.rs) |
| D08 | Transform only the selection boundary | Transform Selection Outline exists independently of transforming pixels; outline transforms are affine. | **Covered.** Preserve that distinction and its availability rules. It does not provide enclosure discovery or direct geometric painting into selection masks. | [Transform operation](../../crates/layer-ui/src/operation.rs) |

### D09 — direct geometric editing of masks

**Status: Partial. Change: improve existing tools and target routing.**

**Today:** artwork/effect masks support brush editing and selection-to-mask
operations. Quick Mask and saved Selection Layers have brush, bucket and gradient
paths. Artwork fill/figure operations use `drawing_content`, which rejects
coverage targets. Lasso Fill and Figure are rejected while editing a selection
mask, and geometric selection tools return to artwork.

**Recommended:** make Lasso Fill, polygon/rectangle/ellipse filling, bucket and
gradient operate on the selected mask, with meaningful grayscale reveal/hide
coverage. Allow geometric add/subtract/intersect edits of Quick Mask and saved
Selection Layers without leaving that target. Preserve each tool's existing
artwork behavior and distinguish editing mask content from making a canvas
selection. New enclosure tools in F01–F03 must share this destination policy.

**Acceptance:** select a clothing mask; add a polygon silhouette, subtract a
small opening, add a gradient and load the result as a selection. Repeat on
Quick Mask and a saved Selection Layer. Linked/placed masks remain correctly
aligned, cancellation changes nothing, and each gesture has one undo step.

**Evidence:** [drawing-target rules](../../crates/layer-core/src/layers.rs),
[artwork tool routing and operations](../../crates/layer-ui/src/art_layers.rs),
[painted selections](../../crates/layer-ui/src/painted_selections.rs),
[figures](../../crates/layer-ui/src/figures.rs).

### D10 — physical dimensions and resolution at document creation

**Status: Partial. Change: extend the existing New Document workflow.**

**Today:** New Document accepts pixel extent, working color/depth, background and
blend space, with saved presets. Image Size later offers physical units and PPI;
export also has resolution controls. Initial creation does not offer a combined
physical-size/PPI specification.

**Recommended:** allow pixel or physical dimensions plus resolution at creation,
reusing the existing unit conversion and validation. Save those values in custom
document presets. Keep pixel dimensions explicit so changing resolution has a
clear consequence. Manga trim/bleed templates are M07, not a second size system.

**Acceptance:** create the same artwork using equivalent pixel and millimeter/PPI
values; both produce identical pixel dimensions and declared resolution. A saved
preset restores the settings.

**Evidence:** [creation options](../../crates/layer-ui/src/document_creation.rs),
[size units](../../crates/layer-ui/src/canvas_size.rs),
[resolution behavior](../../crates/layer-ui/src/image_size.rs).

### D11 — merge the selected layers directly

**Status: Partial; lower priority. Change: optional command on existing merge machinery.**

**Today:** common merges exist, but no direct Merge Selected command is exposed.
Group Selected followed by Merge Group covers many ordinary cases.

**Recommended:** evaluate a direct selected-layer merge only if artist testing
shows the extra grouping step matters. Define admissible ordering, clipping,
effect, mask and blend combinations before adding it; reuse current bake/history
machinery. Do not merge separated layers if doing so changes their relationship
to intervening artwork without an explicit, understandable result.

**Acceptance:** ordinary selected adjacent layers merge in one action and undo;
unsupported combinations explain their specific limitation. Existing Merge Group,
Merge Down and Stamp Visible continue to preserve their established semantics.

**Evidence:** [merge commands](../../crates/layer-ui/src/merges.rs),
[grouping and layer actions](../../crates/layer-ui/src/art_layers.rs).

### D12 — artist-controlled stabilization and pressure smoothing

**Status: Partial. Change: expose existing engine capabilities.**

**Today:** the engine has streamline, stabilization, motion filtering, pressure
smoothing and pressure-fall controls. The brush Tool Settings definitions do not
expose these. A global pressure-response setting already exists.

**Recommended:** expose useful per-brush stabilization and pressure smoothing,
with compact common controls and an advanced editor for the remaining parameters.
Store edits with the brush definition. Keep stabilization distinct from pen
prediction and from the global pressure-response calibration.

**Acceptance:** save a clean-ink brush and a loose-sketch brush with different
smoothing; switching brushes and restarting restores each behavior. Changing a
control does not modify previously committed strokes.

**Evidence:** [engine brush model](../../crates/layer-core/src/lib.rs),
[Tool Settings](../../crates/layer-ui/src/tool_settings.rs),
[global input settings](../../crates/layer-ui/src/settings.rs).

### D13 — start/end taper and minimum stroke size

**Status: Partial. Change: expose existing dynamics with artist-facing controls.**

**Today:** the engine models start/end taper distance, size, opacity and tip
sharpness. Brush mappings can express pressure output floors. These are not
editable as brush properties in the current Tool Settings surface.

**Recommended:** provide independent start/end taper, minimum pressure diameter
and opacity controls where supported, with a stroke preview. Preserve the
difference between pressure-controlled size and a deliberate endpoint taper.
Use the existing engine representation rather than applying an unrelated
postprocessing effect.

**Acceptance:** an artist can create a blunt sketch pencil, tapered inking pen
and fixed-width detail pen, save each and reproduce the intended endpoints.

**Evidence:** [taper and mappings](../../crates/layer-core/src/lib.rs),
[exposed settings](../../crates/layer-ui/src/tool_settings.rs).

### D14 — per-brush pressure, tilt and other input mappings

**Status: Partial. Change: add a brush dynamics editor over existing mappings.**

**Today:** the brush model supports pressure, speed, direction, tilt magnitude/
direction, twist and other sensors, with sampled curves and multiple targets.
Artists have global pressure response but no individual brush mapping editor.

**Recommended:** first expose pressure-to-size/opacity/flow and tilt-to-shape/
rotation with editable curves and output limits. An advanced surface can expose
supported speed/direction mappings without forcing every artist to configure
them. Validate and persist all mappings with the brush.

**Acceptance:** the same light pen pressure produces deliberately different
results in two saved brushes; tilt shading can be configured without changing
global device calibration.

**Evidence:** [sensors, curves and mappings](../../crates/layer-core/src/lib.rs),
[settings surface](../../crates/layer-ui/src/tool_settings.rs).

### D15 — brush antialiasing and hard-edge control

**Status: Missing as an artist-facing brush option. Change: extend brush settings
and qualify the required renderer behavior.**

**Today:** selection antialiasing and fill edge smoothing exist. Brush hardness
exists for analytic tips, but there is no exposed brush antialiasing choice.
Hardness is not a guarantee of bilevel output.

**Recommended:** offer a deliberate hard/no-antialias mode for monochrome inking
and crisp flat-color boundaries, alongside normal smooth drawing. Inspect the
actual brush execution paths before deciding which settings can be exposed
directly and which need renderer changes. Coordinate with final-size tone output
in M04 rather than assuming brush settings alone guarantee print-safe dots.

**Acceptance:** selected monochrome brushes produce the intended hard edges at
actual pixels; smooth color brushes retain their current behavior. Verify erasing
and mask coverage as well as painting.

**Evidence:** [brush settings](../../crates/layer-ui/src/tool_settings.rs),
[selection antialiasing](../../crates/layer-ui/src/selection_tools.rs),
[brush rendering model](../../crates/layer-core/src/lib.rs).

### D16 — independently named personal brushes

**Status: Partial. Change: add a user brush library and editor workflow.**

**Today:** the catalog is built-in; per-workspace overrides preserve exposed
settings for known built-in preset IDs. Artists cannot independently name and
retain several variants of one preset or import/export personal brush definitions.

**Recommended:** create/duplicate/rename/delete/reset user brushes with stable
identities, searchable groups/favorites, import/export and complete validated
definitions. Define how a library brush relates to workspace-specific tool
memory so switching workspaces does not silently discard it. Reuse current
tool-family dispatch and settings validation.

**Acceptance:** duplicate G-Pen into two named brushes, give each different
dynamics, restart, switch workspaces, export/import and recover the same behavior.
Canceling an edit preserves the saved definition.

**Evidence:** [catalog and workspace tool memory](../../crates/layer-ui/src/tools.rs),
[tool settings validation](../../crates/layer-ui/src/tool_settings.rs).

### D17 — user brush tips, textures and advanced brush properties

**Status: Partial. Change: asset import/editing plus selective exposure of engine options.**

**Today:** the model supports analytic and textured tips, grain, dual brushes,
shape/scatter and color dynamics. The artist can adjust several numeric settings
such as spacing, angle, size/rotation variation, grain strength and mixing, but
cannot author/import the full tip/texture setup or configure many dynamics.

**Recommended:** attach imported tip/grain assets to personal brushes; preview
their scale, aspect/orientation and repeat behavior. Add medium-appropriate
advanced controls for scatter/stamp count, color variation, grain behavior and
dual-tip configuration. Support a native portable brush package first; competitor
brush formats require a separate compatibility decision, not a promise that their
engines can be reproduced exactly. Decoration/ribbon materials are R11.

**Acceptance:** import a monochrome tip and grain, save a textured brush, share
it with its assets and reopen it without missing resources. Applicable settings
appear for that execution class and unsupported combinations are rejected.

**Evidence:** [brush model](../../crates/layer-core/src/lib.rs),
[current controls](../../crates/layer-ui/src/tool_settings.rs),
[catalog](../../crates/layer-ui/src/tools.rs).

## Priority 2: professional flats and enclosed-region workflows

### Existing foundation and semantic distinction

Current tools include a seed-based bucket, noncontiguous color selection,
geometric selections and ordinary Lasso Fill. Sampling choices are visible
artwork, editing layer and marked reference layers/groups. Region options include
tolerance, gap closing up to 32 px, signed expansion up to 32 px and edge smoothing.
Current selection coverage limits artwork filling.

**Ordinary Lasso Fill paints the geometric enclosure. Enclose and Fill discovers
eligible line-bounded regions within that enclosure.** A polygon limit on one
seed fill does not supply component discovery or target-region policy.

Evidence: [tool definitions and lasso path](../../crates/layer-ui/src/art_layers.rs),
[region gesture/source handling](../../crates/layer-ui/src/region_tools.rs),
[region request and refinement](../../crates/layer-render/src/lib.rs).
Comparators: [Japanese closed-region filling](https://tips.clip-studio.com/ja-jp/articles/591),
[Krita Enclose and Fill](https://docs.krita.org/en/reference_manual/tools/enclose_and_fill.html).

### F01 — Enclose and Fill / 囲って塗る

**Status: Missing. Change: add a reference-aware fill subtool.**

**Today:** Lasso Fill fills its polygon; the bucket requests a single seed. Neither
enumerates the eligible enclosed components of reference artwork.

**Recommended:** draw an enclosure, discover enclosed eligible regions from the
chosen sampling source and fill them on the separate destination. Default to
closed regions wholly inside the gesture, with exterior background excluded.
Keep ordinary Lasso Fill for shape blocking. Share the region engine and options
with F02/F03 rather than maintaining separate algorithms per gesture.

**Acceptance:** one loose loop fills many enclosed hair/accessory regions beneath
untouched reference line art, leaves the exterior clear and undoes in one step.

### F02 — paint unfilled areas / すきま塗りペン

**Status: Missing. Change: add a region-repair brush subtool.**

**Today:** artists repair tiny holes with repeated bucket clicks or manual paint.
An ordinary brush does not discover closed regions under its footprint.

**Recommended:** let a brush gesture identify and fill eligible small closed
regions under its swept area using the same source, target and edge policies as
F01. Offer suitable defaults for repairing transparent/near-transparent holes;
avoid replacing existing flats by default.

**Acceptance:** sweep over several missed hair-tip regions; holes fill without
painting across the outlines or recoloring surrounding flats, with one undo.

### F03 — Enclose and Select / シュリンク選択

**Status: Missing. Change: add a selection-output sibling of F01.**

**Today:** the artist can make a geometric lasso or combine seed-based Wand
selections. Shrink contracts existing selection coverage; it does not discover
line-bounded components within a lasso.

**Recommended:** use enclosure discovery to produce a selection with New/Add/
Subtract/Intersect modes, then allow saving it or turning it into mask coverage.
Reuse F01's region policies and existing selection refinement.

**Acceptance:** loop around several clothing regions, obtain their union without
the surrounding background, save the selection and reuse it for shading.

Comparator: [Japanese selection tools](https://help.clip-studio.com/ja-jp/manual_jp/330_selection/選択範囲ツール.htm)
documents enclosure-based シュリンク選択 separately from geometric selection.

### F04 — adaptive expansion at line boundaries

**Status: Partial. Change: improve the existing Expansion option.**

**Today:** expansion is a uniform signed pixel-distance refinement. Increasing
it can cover an antialiased fringe but offers no darkest/most-opaque stop policy.

**Recommended:** retain distance expansion and add an adaptive option that stops
at the dark/opaque ridge of the reference line. Handle colored line art and
variable line width deliberately. Reuse it in bucket, enclosure and region
selection output.

**Acceptance:** expand flats beneath mixed thin/thick antialiased lines without
leaking across thin segments or leaving bright fringes.

### F05 — bucket drag-fill

**Status: Missing. Change: extend the existing bucket gesture.**

**Today:** the contact records a seed at pointer-down and submits it on release;
movement does not fill additional crossed components.

**Recommended:** support drag through multiple regions, with modes for any
eligible crossed region or only regions matching the first target color. Deduplicate
components, preserve source semantics throughout the gesture and commit one edit.

**Acceptance:** drag through separated shirt details to fill all of them; the
matching-color mode preserves already colored neighboring details. Cancel aborts
the whole gesture.

### F06 — additional sampling scopes and exclusions

**Status: Partial. Change: improve reference/source options and layer metadata.**

**Today:** visible/editing/reference sources exist, and groups can be marked as
references. There is no tool option for a temporary selected-layers/folder scope
or explicit draft/paper/locked/current-layer exclusions.

**Recommended:** support selected layers and a chosen folder as sampling scopes,
plus exclusions that matter to flats. Separate marked references from the current
editing selection. Reuse shared scene-scope composition and define how masks,
clipping and effects contribute. Draft/export metadata also supports R08.

**Acceptance:** sample a line-art folder while ignoring the rough sketch, paper
and current flat layer; adding shading does not alter the fill boundaries.

### F07 — target-region and boundary-color policies

**Status: Missing beyond seed-color matching. Change: add shared region policies.**

**Today:** the bucket matches a seed using tolerance. There is no artist-facing
transparent-only/white-or-transparent/specified-color policy for enclosure output,
or explicit fill-until-boundary-color policy.

**Recommended:** distinguish what forms a boundary from which regions are eligible
to receive paint. Start with transparent, white/transparent, specified color and
all eligible closed regions. Provide boundary-color matching where it supports
colored linework or gradients inside a shape. Define alpha tolerance explicitly;
do not make tolerance silently switch between unrelated meanings.

**Acceptance:** fill transparent holes while retaining colored flats; handle a
white-background scan; replace one flat color without repainting reference ink.

### F08 — enclosure-boundary, hole and partial-region behavior

**Status: Missing for enclosure discovery. Change: define and expose shared policy.**

**Today:** geometric Lasso Fill and a limited seed fill have different semantics;
there is no enclosed-component eligibility or partial-intersection policy.

**Recommended:** default to fully enclosed eligible regions, preserve intended
holes, and specify what happens where a reference component intersects the lasso
boundary. If a partial-region mode is offered, make it explicit. Test antialiased
contours and thin partially transparent fragments so they do not become accidental
tiny targets. Gap closing must not silently turn arbitrary open sketches into
reliable closed drawings.

**Acceptance:** a circle fully inside the loop fills; a partly intersected shape
follows the chosen policy; the surrounding open background does not fill.

### F09 — lasso snapping to reference line art

**Status: Missing. Change: improve lasso/region-boundary construction.**

**Today:** polygon and freehand selections are geometric; they do not attract
their boundary to reference ink.

**Recommended:** optional snapping with adjustable distance/strength and a
temporary bypass. Allow manual segments across larger intentional openings while
following existing contours elsewhere. Reuse the active source policy.

**Acceptance:** select a shaded region partly bounded by ink and partly by an
artist-drawn bridge, without tracing every contour manually.

### F10 — multiple enclosure gesture types

**Status: Partial foundation. Change: reuse selection gestures for the new subtools.**

**Today:** freehand, polygon, rectangle and ellipse selection gestures exist,
but none produces the reference-aware component discovery described above.

**Recommended:** begin with freehand and polygon enclosure; support rectangle/
ellipse and a combined freehand/polygon gesture where useful. Keep input geometry
separate from Fill/Select/repair output so each gesture does not become a separate
tool implementation.

**Acceptance:** enclose the same set of regions with each supported gesture and
obtain the same result; incomplete/canceled construction deposits nothing.

### F11 — destination and mask parity for the region family

**Status: Partial. Change: share target routing with D09.**

**Today:** ordinary bucket filling works on paint and dedicated selection-mask
paths; artwork coverage masks are rejected by the artwork fill path.

**Recommended:** let the family output artwork color, artwork/effect mask
coverage, a current selection or a saved selection as appropriate. Sampling and
writing must remain independent. Preserve mask grayscale semantics, selected
coverage limits, lock rules and alignment under placement.

**Acceptance:** the same reference enclosure can produce flats, a clothing mask
or a saved selection without modifying the source linework.

### F12 — direct noncontiguous fill

**Status: Partial. Change: add a bucket mode using existing selection capability.**

**Today:** noncontiguous Color Select exists, but ordinary bucket fill always
requests contiguous regions. Color Select followed by Fill Selection is a workaround.

**Recommended:** offer a direct similar-color/noncontiguous fill mode that reuses
the matching engine and selected-area limit. Keep it separate from enclosed-region
discovery: matching a color across the canvas does not imply closed shapes.

**Acceptance:** recolor separated matching flat regions in one operation while
respecting a limiting selection and excluding other colors.

### F13 — gap, tolerance and edge controls across the family

**Status: Present foundation; integration gap for new outputs. Change: reuse and extend.**

**Today:** tolerance, gap closing, expansion and smoothing already exist. They are
not missing bucket options. The current gap/expansion distance bound is 32 px.

**Recommended:** share recognition/refinement settings across F01–F03/F05/F12,
with appropriate defaults and independently understandable alpha/edge controls.
Evaluate the existing distance bounds on actual working-resolution art before
changing them. Store named fill/selection configurations if repeated artist
setups warrant it; retain current remembered settings.

**Acceptance:** thin and thick lines, small gaps, opaque scans and partially
transparent ink give predictable results at final working resolution. Raising
tolerance does not unexpectedly destroy boundary recognition.

### F14 — fill-hole inspection and local repair preview

**Status: Missing. Change: add a temporary inspection view.**

**Today:** ordinary zoom, layer visibility and color/effect changes can help
inspection, but there is no dedicated fill-hole/blacklight view.

**Recommended:** temporarily show chosen flats as solid dark shapes against a
contrasting background without dirtying the drawing. A later extension can
highlight likely holes and show a magnified local repair preview. Automatic color
prediction is optional and should follow manual repair quality, not gate it.

**Acceptance:** inspection exposes small missed regions, restores the exact prior
view on exit, changes no saved artwork and works with the repair brush.

### Region-family evidence and qualification

The Japanese CSP documentation distinguishes enclosure filling, a gap-filling
pen, target-color policies, reference exclusions, edge expansion and lasso
snapping. Krita documents independent enclosure gestures, target policies,
adaptive growth and drag-fill. These support a family with shared semantics rather
than a single additional icon. [CSP closed-region guide](https://tips.clip-studio.com/ja-jp/articles/591),
[CSP fill settings](https://tips.clip-studio.com/ja-jp/articles/590),
[Krita enclosure](https://docs.krita.org/en/reference_manual/tools/enclose_and_fill.html),
[Krita bucket](https://docs.krita.org/en/reference_manual/tools/fill.html).

GapFill reports professional use of blacklight inspection and enclosure/gap
repair. Its small, non-refereed 2025 study involved 13 professional anime colorists
and found improvement on dedicated missed-region detection/repair tasks. It
supports prioritizing inspection and manual cleanup; it does not establish a
universal productivity result for automatic coloring.
[GapFill](https://www.wiss.org/WISS2025Proceedings/data/demo/2-C15.pdf).

Qualify the family on separate ink/flat layers, multiple reference layers/groups,
colored and antialiased ink, tiny hair-tip regions, narrow contours, intentional
holes, small line breaks and larger manual bridges. Include white scans,
transparent art, placed layers/masks, selected-area limits, cancellation,
undo/redo and save/reopen. Compare one-gesture region creation with today's seed
clicks and manual tracing.

## Remaining gaps: priorities 3–5

These are retained recommendations outside the immediate focus. A missing tool is
not necessarily equally important to every artist: vector ink matters especially
to manga, whereas references and masks serve loose painterly illustration too.

### Priority 3: drawing assistance, inspection and collaboration

| ID | Gap | What we have today | Recommended state |
| --- | --- | --- | --- |
| R01 | Layered artwork interchange | Native editable `.capy` and flattened image delivery; palette interchange is broader than artwork interchange. | Layered PSD import/export with groups, masks, clipping, profiles and supported blends; explicitly handle unsupported effects. Consider PSB/ORA separately. |
| R02 | Reference board/subview | Imported images, separate documents, navigator and windows. | A persistent reference board/subview with pan/zoom/flip, sampling and multiple references, without requiring reference pictures in the artwork stack. |
| R03 | Independent second artwork view | Navigator and separate drawing sessions exist; no dedicated new view of the same artwork command is found. | Two synchronized views with independent zoom/pan, one for detail and one for composition. |
| R04 | Temporary value inspection | Black & White, Desaturate and Threshold effects exist. | View-only grayscale and adjustable threshold checks that do not dirty or alter the layer stack. |
| R05 | Perspective drawing guides | Straight, parallel and radial rulers. Distort/Perspective/Warp transforms are already available. | Linked horizon and one-/two-/three-point perspective rulers, grids and snapping. A radial guide alone is not a linked perspective system. |
| R06 | Symmetry, curve and ellipse guides | Straight/parallel/radial snapping; line, rectangle and ellipse figures. | Mirror/radial replicated drawing, curve/concentric/ellipse guides and curve/Bezier/polyline figure tools. |
| R07 | Editable vector ink | Raster painting and diagnostic stroke recording; no vector artwork layer kind. | Editable strokes, control points, width correction, simplify/connect and intersection erasing. Add vector-centerline fill stopping and reference-aware no-cross-line brushing when the needed line representation exists. |
| R08 | Layer organization and draft semantics | Groups, references, locks and visibility; no draft/export-exclusion or organizational color-label fields in the occurrence model. | Draft/export-exclusion flags, useful organizational labels and source exclusions (F06). Preserve existing reference-group behavior. |
| R09 | Scanned line extraction and outlines | Levels, Threshold, effects and masks exist; no dedicated color-to-alpha/luminance-to-opacity or border effect. | Line-background extraction and cleanup; editable border/outline effect for shapes, lettering and stickers. |
| R10 | Reusable gradient library | Both drawn gradients and gradient effects have multistop editing; gradient-fill effects exist. | Named reusable/shareable gradient presets. Do not schedule multistop editing or editable gradient fill as missing. |

Evidence: [export](../../crates/layer-ui/src/export.rs),
[commands](../../crates/layer-ui/src/lib.rs),
[rulers](../../crates/layer-core/src/rulers.rs),
[figures](../../crates/layer-core/src/figures.rs),
[layer kinds and brushes](../../crates/layer-core/src/lib.rs),
[occurrence metadata](../../crates/layer-core/src/authored/artwork.rs),
[gradient editing](../../crates/layer-ui/src/effects/gradient.rs),
[effects](../../assets/filters/manifest.json).
Workflow comparators: [Japanese vector guide](https://tips.clip-studio.com/ja-jp/articles/600),
[perspective guide](https://tips.clip-studio.com/ja-jp/articles/807),
[ruler types](https://help.clip-studio.com/ja-jp/manual_jp/510_ruler/定規の種類と作成方法.htm),
[professional line extraction](https://tips.clip-studio.com/ja-jp/articles/871).

### Priority 4: complete manga production

| ID | Gap | What we have today | Recommended state |
| --- | --- | --- | --- |
| M01 | Panels / コマ割り | Generic figures and groups, without panel objects or layout commands. | Editable borders, panel splitting, gutters, masked panel folders and templates. |
| M02 | Lettering / 写植 | No artwork text tool or text artwork kind is found. | Editable text with Japanese vertical layout, ruby, 縦中横, font fallback, spacing and alignment. Qualify native text entry and printed placement. |
| M03 | Balloons | No dedicated balloon object/tool. | Editable balloons, tails, border/fill styles and text association; hand lettering remains possible. |
| M04 | Print screentones / トーン | Artistic Halftone uses pixel spacing, angle, contrast and ink/paper colors with smoothed dot edges. | LPI tied to print resolution, percentage density, dot shapes, phase/angle, reusable tones, coverage-based gradients and mask scraping. |
| M05 | Monochrome final output | Grayscale export and Threshold exist; sample depths do not include 1-bit. | Reliable final-size bilevel rendering and 1-bit export where supported/required; preserve pure dots and avoid unintended resampling/gray fringes. Do not require every printer to accept the same format. |
| M06 | Page projects | Separate tabs/sessions are not an ordered manga page project. | Ordered pages, right-to-left binding, spreads, page numbers, shared assets/templates and batch output. |
| M07 | Print-page setup and preflight | Physical units/PPI in size editing, ICC handling, soft proofing and CMYK/grayscale delivery already exist. | Creation templates with trim/bleed/safe areas, configurable printer presets, crop marks and preflight. Reuse D10 and existing color/export systems. |
| M08 | Webtoon output | General raster canvas and export. | Phone viewport preview and configurable vertical slice/batch export if webtoon is a supported market. |

Evidence: [artwork model](../../crates/layer-core/src/authored/artwork.rs),
[commands](../../crates/layer-ui/src/lib.rs),
[Halftone and Threshold](../../assets/filters/manifest.json),
[sample depths](../../crates/layer-core/src/color/profile.rs),
[export](../../crates/layer-ui/src/export.rs),
[creation](../../crates/layer-ui/src/document_creation.rs).

Japanese sources describe dedicated panel/manga tools, Japanese typography and
page management. Tone instruction demonstrates editable frequency/density,
gradients and scraping. A printer's instructions supply concrete resolution,
bleed and output constraints, but those values are printer-specific.
[Manga tools](https://tips.clip-studio.com/ja-jp/articles/9950),
[text settings](https://tips.clip-studio.com/ja-jp/articles/851),
[page management](https://tips.clip-studio.com/ja-jp/articles/2158),
[tone workflow](https://tips.clip-studio.com/ja-jp/articles/9181),
[Neko no Shippo production instructions](https://www.shippo.co.jp/neko/making/clip.shtml).

### Priority 5: repeated production and optional expansion

| ID | Gap | What we have today | Recommended state |
| --- | --- | --- | --- |
| R11 | Materials and decoration | Built-in textured brushes and imported images, without a complete artist material library. | Reusable stamp/ribbon/pattern materials, layer templates and organized local assets, building on D16/D17. |
| R12 | Actions and batch operations | Workspace customization and ordinary commands; no artist action recorder/batch workflow is found. | Record reusable setup/edit sequences and process multiple pages/exports with clear history and cancellation. |
| R13 | 3D posing and background extraction | No posing/scene workflow or manga line-and-tone extraction suite; Edge Detect exists. | Consider pose/scene reference and configurable line/tone extraction after essential drawing/page workflows. |
| R14 | Artist timelapse | Stroke recording serves diagnostics and replay. | Dedicated artist-facing process capture and shareable timelapse output if demand warrants it. |
| R15 | Animation | Time-dependent effects exist, but not an artist animation timeline/cel workflow. | Separate optional animation workstream; not a prerequisite for static illustration or manga. |

## Existing strengths and corrected non-gaps

Current main provides raster ink/paint media, smudge/blend and liquify, clone/heal,
selection painting, Quick Mask and saved selections, clipping and masks,
reference groups, palettes/interchange, navigator and canvas navigation, editable
native packages, recovery infrastructure and color-managed output.

The baseline has 52 bundled effects, multistop drawn gradients and gradient-fill
effects, 24 ordinary blend modes plus group Pass Through, and a document blend-space
choice. Color Dodge/Burn, Linear Burn, Hard Light, Hue/Saturation/Luminosity and
Difference are **present**, not missing. Distort/Warp, richer interpolation and
retained group/multiple-layer transforms also exist; respect their existing
target-specific limits rather than claiming that the workflows are wholly absent.

Evidence: [blending](../../crates/layer-core/src/layers.rs),
[transforms](../../crates/layer-ui/src/operation.rs),
[gradient editor](../../crates/layer-ui/src/effects/gradient.rs),
[effect catalog](../../assets/filters/manifest.json),
[recovery](../../crates/layer-ui/src/recovery.rs).

## Delivery recommendation and acceptance boundary

1. Focus Priority 1 on D09, D12–D17, with D10 for setup convenience. D11 is lower
   priority; D01–D08 already have implementations and need qualification/reuse.
2. Build F01–F03 as a shared region-discovery family, with F04/F06–F08/F11/F13
   providing its essential semantics. Add F05/F09/F10/F12/F14 as the practical
   interactions that reduce repeated tracing, clicks and inspection work.
3. Keep layered handoff, references and drawing assistance visible in Priority 3.
   Make M01–M07 release requirements for a complete manga-production offering,
   with M08 conditional on webtoon scope.

Use shared Rust for source/destination rules, discovery/refinement, validation,
settings and undo. Hosts present the same capabilities using native controls and
input. Do not perform GPU readback, decoding or other blocking work on input/UI
threads. Future brush/region changes must satisfy the repository's
[performance targets](../PERFORMANCE_TARGETS.md) and
[testing rules](../development/testing.md). New brush/material reordering follows
the [drag convention](../ui/drag-and-reorder.md).

Before calling the focused work complete, walk a loose-line illustration through
sketch, ink, masks and rendering; color detailed ink with tiny holes/gaps; revise
the saved masks and flats; and exchange/reopen the resulting artwork. Measure
gesture/workaround counts and inspect edge correctness at actual pixels. Exercise
pen, touch and mouse, long sessions, undo, save/reopen and recovery on affected
hosts. The manga track additionally needs a multipage lettering/tone/print task.

This record does not establish runtime host parity, pen feel, large-document
performance or printer acceptance. Existing code coverage and future artist
qualification are separate claims. No product implementation is included.
