# Illustration filters: proposed behavior and controls

[Developer guide](README.md) · [Runtime filters](../reference/runtime-filters.md) · [Workspace and UI](../ui/README.md)

This proposal covers illustration, comic, lettering and texture effects. It
describes the gaps, the intended results and the controls an artist needs.
Algorithms and rendering passes remain open. Controls and starting values are
recommendations for prototyping; accepted data ranges must be settled before
each filter's release freeze. All named families are GA candidates, subject to
demonstrated quality and performance and the explicit release cut rule below.
No family is removed or deferred by this proposal.

Layer Color and Expression Color are outside this work. Grayscale and binary
conversion are ordinary reversible effects here, not restrictions on what a layer
can store or what a painter can draw.

## Evidence and scope

The starting point is the 52-entry bundled catalog and its shared controls:

- [Catalog and parameter declarations](../../assets/filters/manifest.json).
- [Spatial and artistic shaders](../../assets/filters/filter_library.wgsl).
- [Color conversion shaders](../../assets/filters/effects.wgsl).
- [Existing brush-owned watercolor material](../../crates/layer-core/src/raster.rs).
- [Shared effect properties](../../crates/layer-ui/src/effects.rs).

"Missing" means absent as a built-in effect or dedicated workflow. A similar
result may be possible through manual layer composition. "Improve" means extend
the existing effect rather than add an overlapping implementation. This is a
proposal based on code and reference documentation; host behavior and rendering
performance have not been qualified.

Reference products inform familiar behavior, not exact pixel equivalence. The
control lists below are proposed Capy controls, not complete copies of another
product's dialogs. Each filter citing a commercial product compares at least
two distinct tools; a second page from the same product does not count. Research
and open-source references can stand alone. A comparison supplies perspective,
not a requirement to reproduce either tool's algorithm or complete dialog.
Source links accompany the relevant proposals.

## Release policy: small saved controls, evolving artistic rendering

Artistic filters may change appearance after their initial release. Improving
the look from feedback takes priority over reproducing every previous pixel.
Reopening an artwork can therefore render these effects differently in a newer
Capy release. This is an intentional product policy, not an approximation error.

Keep the authored interface small: filter identity, the few meaningful controls,
their values and units, colors, selected resources, and any random seed. Preserve
useful control intent: larger marks, more detail, stronger grain, or fewer tonal
levels. Do not save kernel variants, sample counts, octave counts, tensor settings,
lighting constants, algorithm revisions or reserved future controls. Algorithms
and artistic response curves may improve without preserving a legacy renderer.

This policy covers Painterly/Oil Paint, Pencil/Charcoal, Watercolor Look and
Border, Cartoon, Extract Lines, hatching/engraving/stipple, Crystallize, Satin,
and the appearance synthesis in Paper Texture. Color Transfer's matching
algorithm, Film Grain and procedural noise synthesis can also improve.
Artist-selected images, palette entries and computed
results explicitly saved by Update/Match remain authored data, not disposable
caches. A saved seed makes one release repeatable; it does not freeze the noise
algorithm across releases. Built-in artistic textures can evolve with the app;
imported texture pixels must remain intact.

Precise operations still need precise meanings: border width, displacement
distance, threshold class, blur scale, palette membership and alpha handling
must remain understandable and testable. An artistic label does not excuse tile
seams, lost input data, invalid colors, flicker or device-dependent semantics.
Keep geometric and color conversion primitives dependable while allowing the
artistic composition and tuning around them to change.

The [package contract](../reference/capy-package.md#adding-controls-after-ga)
and [object-layer design](../history/object-layer-ga-design.md#114-rendering-semantics-and-mime-identity)
record this scoped policy. Their current renderer tables and tests still describe
the implemented baseline. When enabling the policy for a filter, separate stable
geometry, alpha and data invariants from artistic appearance comparisons in its
tests. Data versions remain for actual changes to saved data meaning; do not
archive artistic shader revisions. Visual baselines for evolving looks become
reviewed release comparisons, with intentional differences documented; invariant
tests still fail on bugs.

Navigation: [implementation decisions](#decisions-before-implementation),
[filter inventory](#inventory-and-implementation-order),
[claim-by-claim review](#assessment-of-the-review),
[algorithm research](#algorithm-research-and-recommended-prototypes),
[per-filter performance](#per-filter-performance-and-incremental-rendering),
[GA work and acceptance](#revised-ga-work-and-acceptance).

## Decisions before implementation

The latest review correctly identifies missing extension, geometry and resource
decisions. These are design commitments, not claims that the current code already
implements them. The filter list and artist-facing controls remain unchanged.

### Adding controls after GA

Choose **option (a): a parameter-data version bump with a concrete converter**.
The [package rule](../reference/capy-package.md#adding-controls-after-ga) now
specifies validation against the old schema, insertion of the value preserving
earlier intent, and writing all values in the resulting version. No later field
is reserved now. A newly exposed control's conversion value need not equal its
new-insertion default; changing an internal artistic constant is not itself an
addition to the authored interface.

The review's missing-value claim is correct:
[`decode_values`](../../crates/layer-core/src/package/effect_records.rs) requires
all current parameters, including defaults. The existing guide already requires
versions/conversions for released data-meaning changes; this makes adding a
control explicit. Its description of older apps as opening new files read-only
is too strong. Unsupported-file preservation and an optional validated cached
preview do not guarantee that an old renderer can evaluate the artwork.

### Crop compensation and source boundaries

Choose **crop compensation**, coordinated with
[object-layer sections 5.3 and 7](../history/object-layer-ga-design.md#53-portable-built-in-effects).
Keep removal of the composition origin. Existing frame-relative effects remain
composition-space operations. Do not reinterpret all of them as owner-local.
New owner-relative inputs, such as Displacement's map placement, retain their
declared origin, independent of changing painted bounds.

Frame-dependent effects need an authored **spatial reference**: an origin and
reference extent in composition units, initialized from the frame on insertion.
It supplies pattern phase and the basis for percentage centers/radii. Use finite
F64 geometry, with positive extents, for this new binding; existing scalar
controls keep their types. This is not a visible control, an image boundary or
a renderer cache. Include it only for effects whose evaluation consumes it.

For a crop whose new top-left was `d` in the old frame, content moves to `p-d`
and each frame-relative origin changes from `o` to `o-d`. Keep the reference
extent. Thus `(p-d)-(o-d) = p-o`: Halftone, Hatching, Mosaic, Dither and grain
keep phase, while normalized centers and radii keep their geometry even when
the crop changes aspect ratio. Updating center sliders alone would not preserve
implicit pattern phases or elliptical radii. These origins are composition-space
values, so compensate each effect once, including those in groups, without
also inheriting the root translation. Owner movement still moves the source
behind a frame-relative effect. Save/reopen and undo retain the reference.

Crop does not clamp centers into the new frame. Whole-image resize, rotation and
flip need explicit per-filter remapping of the reference and directional/length
values, atomically with artwork; camera transforms do not edit them. Growing or
rebasing paint storage must not reset pattern phase. Qualify high/negative
coordinates using renderer-relative arithmetic. This adds real authored geometry
for implemented behavior, not empty future graph fields.

**Neither the frame edge nor a temporary tile/ROI edge is an image boundary.**
Filters must request retained off-frame content throughout their dependencies:
an off-frame mark can cast a shadow or blur into the frame. Transparent extension
begins at a true finite source edge, with any explicit map extension handled
separately. Capture windows must include upstream halos; cropping an intermediate
to the output frame is insufficient. The current
[effect dependencies](../../crates/layer-render-wgpu/src/effects.rs) and
[scene windows](../../crates/layer-render-wgpu/src/scene/windows.rs) are bounded
by frame extent, so this is renderer work, not merely a documentation correction.

An intentionally frame-defined generator can retain its specified output domain.
An explicitly frame-based global analysis, such as the existing local tone guide,
can change its statistics after crop. Neither exception permits treating the
frame as a sampling edge for retained source content. Test crop equivalence on
the spatial filters above, and separately test each declared analysis domain.

### One shared image record and per-use sampling roles

Use **`capy.image/1` from object-layer Milestone 1** for image objects, paint bases
and image inputs to filters. It is specified but not implemented; the earlier
claim that no record design was settled was incomplete. Keep immutable sharing,
tile/profile references, import limits, save/copy/undo and dependency lifetime
in that single model. Pattern, Paper, Displacement and Color Transfer depend on
Milestone 1; their rendering also needs Milestone 2's correct source evaluation.
They do not need to wait for the entire object manipulation UI.

An actual filter image binding carries a `color` or `data` sampling role.
`color` uses the image's declared color interpretation and the existing working
color pipeline. `data` reads stored channel values, normalizing integer samples
by their declared depth, without profile/transfer conversion. Float channels
retain their raw values subject to the filter's specified range handling.
The role belongs to the use, not to the shared image: one image can serve both
roles. Map channel selection, interpolation, alpha and neutral-value handling
remain part of the consuming filter's contract. Do not discard profile metadata
just because one use ignores it.

For Color Transfer, test the existing
[`capy.lut3d/1` resource](../../crates/layer-core/src/lut3d.rs) first. It already
stores finite float RGB samples in cubes of side 2…65 with a declared domain.
The [current lookup shader](../../assets/filters/color-lookup.wgsl) interpolates
within a bounded encoded-color domain and carries the boundary correction onto
out-of-domain values. This is not an exact representation of an arbitrary Oklab
affine transform over extended RGB. Prototype a sampled saved match; measure
interpolation error, gamut/HDR behavior and alpha preservation. Reuse the LUT if
it meets the intended match behavior. Add a distinct authored representation
only for a demonstrated limitation. Save one authoritative mapping, not two
redundant representations that can disagree; future Match algorithms may improve
without recomputing an already saved match on open.

### Seeds, assets and accepted ranges

- **Seeds:** use the existing Number parameter with Count dimension and inclusive
  range **0…16,777,215 (`2^24-1`)**. Require an integer before conversion to f32,
  since a fractional wire value can otherwise round to an integer. Zero is a
  valid explicitly saved initial value. Randomize chooses and saves a value
  in this range as one undoable edit. No numeric seed control or u32-wide promise.
  Check endpoints, fractions, overflow and unchanged seeds on unrelated edits.
- **Bundled assets:** every serialized asset ID is permanent. Its artistic pixels
  may improve under the evolving-rendering policy. If an asset is retired, keep
  a total, cycle-free mapping from its saved ID to an available replacement and
  test loading that ID. Content hashes can check integrity; they are not stable
  authored identity. Internal rank masks with no saved selection need no invented
  portable ID. Imported image pixels are retained, not silently replaced.
- **Accepted ranges:** before each filter's first release freeze, record every
  parameter's type, units, inclusive bounds, integral constraints, choices and
  cross-parameter constraints in the contract and validation fixtures. Never
  narrow a released accepted range; widening later is allowed. UI slider limits
  are separate and can be tuned from measurements. Admission/work budgets can
  refuse an operation atomically but must not silently clamp stored values.
  Numeric size maxima are prototype exit decisions, not arbitrary estimates in
  this behavior proposal. No new filter reaches freeze with an unspecified range.

### Release cut rule

Keep all listed families as GA candidates. Prototype high-risk paths early:
connected-component cleanup, preserved thinning, oil marks and large anisotropic
smoothing. Qualify the actual filter under the tier rules, including permitted
moving previews and settled/export quality, rather than assume native 61 MP
recomputation at 120 fps is required for every intermediate pass.

At the release cut, a filter that fails quality or its
[performance gate](#per-filter-performance-and-incremental-rendering), including
any explicitly qualified slower or baseline-relative acceptance, is deferred to
1.1 with its missing evidence recorded. A justified slower artistic result can pass that gate without
being reported as meeting the ordinary tier FPS target. Ship no unusable
stub, hidden unqualified mode or CPU painting fallback. This rule leaves the
candidate list and controls intact today; it prevents one difficult algorithm
from blocking otherwise qualified filters. Shared foundations still gate every
delivered feature that needs them.

Settle intended IDs, controls, units and value kinds early; freeze the complete
accepted interface only when it can be qualified for its release. Do not reserve
unused fields to advertise a future filter. A deferred filter can often use the
same package envelope, but the review's “no format cost” claim is conditional:
its new built-in ID/data version or value kind is unsupported by an older GA
reader. Reusing the envelope does not make old apps able to edit new filters.

The accepted mathematical corrections also need two qualifications. Sparse 3×3
quadrature is an approximation, but has no demonstrated small-error bound for
full-region statistics; the checkerboard example below disproves that bound.
The `π/4` stipple limit assumes a square cell and nonoverlapping circular dots
with maximum radius half its spacing, not every square-root radius algorithm.
The other corrections—continuous faint coverage, medial information for thin
strokes, full engraving width, explicit 8-bit midpoint, jitter-aware search and
Oklab range/gamut handling—remain requirements as described below.

## Control conventions

- **Keep the first view small.** Main controls should be enough for the usual
  result. Later controls are candidates, not requirements for the first version.
  Add them only when examples demonstrate a useful distinction. They are not
  reserved saved parameters with frozen absent values. Adding a control after
  GA uses a parameter-data version and a concrete converter, as specified below.
- **Use familiar names.** Thickness, Size, Blur, Strength, Density, Distance,
  Angle, Color and Opacity describe visible changes. Avoid algorithm names,
  kernel choices and separate technical quality settings.
- **Use familiar units.** Lengths use pixels, amounts and coverage use percent,
  angles use degrees and limited-color counts use whole numbers. Tone frequency
  can use lines per inch when document print resolution is available. Sliders
  retain numeric entry through the existing shared controls.
- **Reuse existing properties.** Enable, mask, ordering and overall effect
  blending belong to the existing effect workflow. Do not add a second Strength
  control that does the same thing as existing effect opacity.
- **Distinguish generated marks from source artwork.** Border, shadow and glow
  color/opacity affect their generated marks; the original artwork remains
  visible. If a style needs a blend control, it applies to those marks rather
  than changing the source artwork's blend behavior.
- **Show relevant controls only.** Selecting a mode reveals its additional
  controls and retains the values of hidden controls. A disabled option must
  not conceal an unrelated active adjustment.
- **Prefer direct manipulation when it helps.** Shadow offsets and effect
  centers can be dragged on the canvas, with equivalent numeric fields. This
  does not require a new permanent tool or panel.
- **Start with useful defaults.** Black outlines and shadows, transparent paper
  for extracted lines, round tone dots and neutral color conversions make a
  predictable starting point. Exact values follow visual trials.
- **Keep previews faithful.** Patterns stay anchored in artwork coordinates;
  pan and zoom do not change the authored pattern. Preview, saved artwork and
  export agree on its position, colors and transparency.
- **Keep randomness simple.** For looks with visibly random marks or grain, use
  a secondary Randomize action where useful, with undo. Save the seed without a
  numeric seed field. Choosing another unrelated control must not reshuffle it.

## Inventory and implementation order

First, Next and Later below describe implementation sequence within the GA goal.
All listed filter families are candidates, including the formerly optional group.
The separate **Later** paragraphs under each filter describe possible extra
controls or modes that are not required for GA. Feasibility is still unproven.

| Work | Current coverage | Suggested order |
| --- | --- | --- |
| Border and Drop Shadow | Missing | First |
| Watercolor Border | Brush material only | First |
| Extract Lines | Basic Edge Detect | First |
| Tone / Halftone | Basic circular Halftone | First |
| Decrease Color | Threshold supports color classes and binary transparency | First |
| Brightness to Opacity | Implemented without additional controls | Next |
| Color to Alpha | Missing | Next |
| Adjust Line Width and Remove Dust | Missing artwork filters | Next |
| Palette reduction and creative dithering | Posterize only; delivery dithering exists | Next |
| Cartoon and paper/canvas texture | Some building blocks | Next |
| Outer Glow | Bloom exists; silhouette glow missing | Next |
| Inner Shadow, Inner Glow, Long Shadow and Bevel | Missing; color-based Emboss exists | Later |
| Hatching, engraving and stippling | Basic Crosshatch | Later |
| Pencil, Charcoal, Watercolor and Oil Paint looks | Basic Pencil and Painterly | Later |
| Pattern fills/overlays and procedural textures | Solid Color and Gradient Fill only | Later |
| Stable random variations | Film Grain exists; no authored seed control | Alongside stochastic filters |
| Pixel Mosaic | Cell-center sampling | Later |
| Crystallize, Zoom/Spin Blur and Lens Blur | Missing | Later |
| Image-driven Displacement and Color Transfer | Missing | Later |
| Satin | Missing | Later |
| Effect presets/stacks and separated lines/tones | Dedicated workflows missing | Alongside relevant filters |

## First additions and improvements

### Border

**Gap:** no live outline effect around painted content.

**Behavior:** outline the visible silhouette, including separate islands and
holes. On a group, use the group's combined silhouette rather than outline each
child. The border follows edits and masks. Transparent empty input stays empty.

**Main controls:**

| Control | Meaning and starting behavior |
| --- | --- |
| Thickness | Border width in pixels. Start with a modest visible width. |
| Color | Border color; black initially. |
| Position | Outside, Center or Inside; Outside initially. |
| Anti-aliasing | Smooth edges on by default; off for crisp pixel artwork. |

**Later:** gradient fill, a second outline and border-only output. Stacking the
same effect should already cover many multiple-outline uses. Do not begin with
separate corner, cap and join panels for raster silhouettes.

References: [Clip Studio border effects](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_properties.htm),
[Affinity Outline](https://affinity.help/photo2/en-US.lproj/pages/LayerFX/layerFX_outline.html).

### Watercolor Border

**Gap:** wet/burnt watercolor edges exist as brush-owned raster material, not as
an independently editable effect on any suitable artwork.

**Behavior:** add a soft stain or darkened pigment rim whose color follows nearby
paint. Preserve the source interior. It works on imported art and dry paint
without requiring a watercolor brush or simulating paint transport.

**Main controls:** Width in pixels, Opacity in percent, Darkness in percent and
Blur in pixels. Darkness starts with a subtle rim; increasing Blur softens it.
Avoid a separate edge color picker because this effect follows the source color.

**Later:** variation for an uneven natural rim, only if the basic effect looks
too regular. Do not expose brush wetness or physical simulation controls here.

Research perspective: [Bousseau and colleagues on watercolor stylization](https://www.matt-kaplan.com/npr/watercolor/watercolor.pdf).
This motivates pigment-edge behavior without prescribing a product dialog.

### Extract Lines

**Gap:** Edge Detect exposes sampling radius, strength and invert. It provides
basic grayscale edges, not a complete clean-line illustration workflow.

**Behavior:** extract contours from an image as dark ink. Suppress insignificant
texture while retaining selected detail. Gray mode produces soft ink coverage;
Monochrome produces hard black marks. Default to transparent paper so colors
underneath remain visible. An opaque image's rectangular boundary should not
become an unintended frame; a deliberate cutout silhouette can contribute lines.

**Main controls:**

| Control | Meaning and starting behavior |
| --- | --- |
| Mode | Gray or Monochrome; Gray initially. This controls the result, not layer storage. |
| Thickness | Width of the extracted ink, independent of how much detail is detected. |
| Detail | Higher values retain more small contours; lower values simplify. |
| Remove fragments | Suppress short isolated marks; low initially. |
| Background | Transparent or White; Transparent initially. |

**Later:** Smooth Lines for continuity, Black Fill with a darkness threshold,
and Simplify Colors before extraction. Expose directional detection only if
common examples require it; hide technical detector selection. Ink color can
follow later if black extraction and existing recoloring are insufficient.

Prefer improving/replacing Edge Detect's illustration behavior, with its simple
edge view retained only if that view serves a distinct use. Avoid two nearly
identical extraction filters.

References: [Clip Studio lines and tones](https://help.clip-studio.com/en-us/manual_en/390_filters/Convert_to_lines_and_tones_%28EX_only%29.htm),
[Krita Edge Detection](https://docs.krita.org/en/reference_manual/filters/edge_detection.html).
Compare the full illustration workflow with a simpler edge/alpha filter; Capy
combines only the controls needed for clean extracted ink.

### Tone / Halftone

**Gap:** Halftone offers Size, Angle, Contrast, Ink and Paper. It has one circular
screen, opaque ink/paper parameters and source-alpha preservation. Density
control, transparent paper, additional patterns and opacity screening are missing.

**Behavior:** represent shading with dots or other repeating marks. Selected
density should correspond to the amount of ink coverage. Transparent gaps reveal
lower artwork. Retain Halftone as the existing effect and make Tone/Screentone
discoverable through naming/search rather than introduce a duplicate effect.

**Main controls:**

| Control | Meaning and starting behavior |
| --- | --- |
| Pattern | Circle initially; also Square, Diamond, Ellipse, Line, Cross and Noise. |
| Size / Frequency | One spacing control, presented as pixels or lines per inch. Unit changes preserve the pattern. |
| Angle | Screen rotation; a conventional diagonal screen initially. |
| Density | Image or Fixed. Image derives coverage from shading; Fixed reveals a percentage. |
| Contrast | Changes the shading-to-density response; neutral initially. Hidden for fixed density. |
| Ink | Mark color; black initially. |
| Background | Transparent or Color; reveal Paper color only for Color. |

**Later:**

- Offset X/Y to align screens across layers.
- Use Opacity for Density: incorporate source coverage and, optionally, owner
  opacity into mark size instead of merely fading marks. Default it off until
  the intended manga workflow is demonstrated. Handle each opacity once.
- Posterize Density: an adjustable set of shading bands, starting with a simple
  level count; custom band boundaries only if needed.
- Anti-aliasing: Smooth or Hard, without arbitrary quality levels.
- Noise Size and Randomize for the noise pattern. Save the chosen realization.
- Decorative shapes only after the basic geometric patterns work well.

**Color halftone extension:** Mode offers Single Ink, Color or Alpha. Color
screens channels separately; Alpha screens transparency while preserving source
color. Keep Single Ink as the ordinary comic workflow. Color can later offer RGB
or simulated CMYK and per-channel angle pages. Simulated CMYK is an artistic
screen effect, not a document color-mode change or a print-separation workflow.

**Accuracy improvement:** inspect flat shading ramps and small features. The
current circle formula predicts about 78.5% geometric ink coverage at a nominal
50% density before anti-aliasing. This code-derived observation requires pixel
verification. Calibrate coverage and reduce dependence on one sample at the cell
center. Do not prescribe the correction algorithm in this proposal.

References: [Clip Studio Tone](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_properties.htm),
[Krita Halftone](https://docs.krita.org/en/reference_manual/filters/artistic.html),
[GIMP Newsprint](https://docs.gimp.org/3.0/en/gimp-filter-newsprint.html).

### Decrease Color: grayscale and monochrome

**Gap:** grayscale conversion is already covered. Desaturate, Black & White and
Channel Mixer's Monochrome mode retain gray shades. Threshold now supports
binary RGB and optional binary alpha; single-color choices make the other class
transparent. Posterize is per-channel reduction, not a fixed
total palette size.

**Behavior:** reuse the grayscale effects. Improve Threshold to cover hard
black/white/transparent illustration conversion. "Monochrome" here describes
that output, while the existing photographic Black & White effect stays a
grayscale mixer. A new combined Decrease Color entry is unnecessary unless
discovery trials show the existing entries are confusing.

**Main controls for improved Threshold:**

These controls are implemented in Threshold parameter-data version 2. Equality
selects white and accepts the alpha cutoff; zero source coverage remains empty.
The [runtime contract](../reference/runtime-filters.md#photo-color-adjustments)
records the exact units, accepted ranges and saved choice IDs.

| Control | Meaning and starting behavior |
| --- | --- |
| Threshold | Boundary between dark and light; midpoint initially. |
| Colors | Black & White, Black or White. Single-color choices make the other class transparent. |
| Transparency | Keep or Threshold. Keep preserves the ordinary Threshold workflow; Threshold enables binary coverage. |
| Alpha threshold | Visible for thresholded transparency; rejects faint source coverage. |

**Later:** Use Layer Opacity when deciding the alpha threshold, off initially.
A Monochrome preset selects thresholded transparency. Layer/effect opacity
applied afterward can still fade the result; binary conversion describes the
filter's output before those outer properties.

Avoid adding duplicate grayscale channel sliders, layer drawing restrictions
or document color-mode controls. Palette reduction is separate below.

References: [Clip Studio decrease color](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_properties.htm),
[Krita Desaturate and Threshold](https://docs.krita.org/en/reference_manual/filters/adjust.html).
Compare grayscale, binary color and transparency separately; their packaging
need not become a combined Capy dialog.

### Drop Shadow

**Gap:** no silhouette-based shadow effect.

**Behavior:** cast a displaced shadow behind the artwork, using its alpha rather
than its brightness. Editing the artwork updates the shadow. Zero blur gives a
crisp graphic shadow. A group's shadow follows its combined shape. Start with
ordinary source-over behavior for translucent artwork; do not create an opaque
fill that hides the original.

**Main controls:** Color, Opacity, Distance in pixels, Angle in degrees, Blur in
pixels and Spread in percent. Start with black, a short offset, modest softness
and zero spread. Angle and Distance can also be edited by dragging the shadow.

**Later:** Shadow Only output, generated-shadow blend mode and Hide Under Source
for artists who want to prevent a shadow showing through translucent fill. Avoid
global-light controls and contour graphs in the first version.

References: [Affinity Outer Shadow](https://affinity.help/photo2/en-US.lproj/pages/LayerFX/layerFX_outerShadow.html),
[Photoshop Drop Shadow](https://helpx.adobe.com/ie/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html).

## Cleanup and limited-color artwork

### Brightness to Opacity

**Coverage:** Brightness to Opacity is implemented as a control-free black-ink
conversion. Its saved parameter-data version is 1 with an empty values map.

**Behavior:** white becomes transparent, black becomes opaque black and gray
becomes partially transparent black. Multiply by existing coverage so transparent
areas stay transparent. Preserve soft scanned edges instead of simply deleting
all pixels above a cutoff.

**Main controls:** none. This should work as a familiar single conversion.
Existing Levels/Curves can prepare a faded scan before it. Do not duplicate
Threshold's sliders in this filter.

Related open-source workflow: [Krita Color to Alpha](https://docs.krita.org/en/reference_manual/filters/colors.html).
The proposed control-free black-ink conversion is narrower than general color
removal; it does not reproduce that filter's full behavior.

### Color to Alpha

**Gap:** no general removal of a chosen paper/background color.

**Behavior:** make the chosen color transparent, including its contribution to
mixed edge pixels, while retaining the remaining color. A gray edge against
white should become partially transparent dark ink rather than a pale opaque
fringe. This also covers colored scans that Brightness to Opacity simplifies.

**Main controls:** Color, initially white, with the existing color sampler;
Tolerance, controlling how widely paper color variations are removed. Exact
matches disappear; unrelated saturated colors remain useful.

**Later:** softness only if Tolerance cannot provide a predictable edge. Keep
Brightness to Opacity as the simple grayscale conversion, with shared behavior
where appropriate rather than duplicate code paths.

Reference: [Krita Color to Alpha](https://docs.krita.org/en/reference_manual/filters/colors.html).

### Adjust Line Width

**Gap:** selection grow/shrink machinery exists, but no dedicated artwork filter.

**Behavior:** thicken or thin ink on transparent artwork. Retain its color and
soft edge character. Narrowing should avoid destroying fine strokes when the
artist requests preservation. This is stroke cleanup, distinct from adding a
separately colored Border.

**Main controls:** Process: Thicken or Thin; Amount in pixels; Keep Thin Lines,
shown for Thin and on initially. Prefer this familiar pair over a signed radius
whose direction is easy to misread.

**Later:** white-paper input mode if paper removal plus width adjustment does
not cover common scans. Do not expose neighborhood geometry by default.

References: [Clip Studio Adjust Line Width](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Dilate](https://docs.gimp.org/3.0/en/gimp-filter-dilate.html) and
[Erode](https://docs.gimp.org/3.0/en/gimp-filter-erode.html).
Compare stroke-oriented controls with basic morphology. Their light/dark
conventions are not a definition of Capy's alpha-based Thicken/Thin behavior.

### Remove Dust

**Gap:** no size-limited removal of isolated marks or filling of small holes.
Edge-Preserving Smooth does not express either operation.

**Behavior:** remove small isolated flecks while protecting connected larger
strokes. Size describes a feature's spatial extent, not its ink area or path
length; a tightly coiled mark may still count as small. A second mode fills
tiny enclosed holes while preserving large intentional holes. Default
to transparent-background artwork; support white scans as a mode.

**Main controls:** Mode: Remove Marks or Fill Gaps; Size in pixels, representing
the largest feature to change; Background: Transparent or White. For Fill Gaps,
use nearby ink color automatically.

**Later:** a chosen fill color only if neighboring color is insufficient. Avoid
separate despeckle and gap-fill dialogs with the same controls.

References: [Clip Studio Remove Dust](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Despeckle](https://docs.gimp.org/3.0/en/plug-in-despeckle.html).
GIMP's median cleanup supplies a contrasting approach; it does not establish
the proposed component-size or enclosed-hole semantics.

### Reduce Colors

**Gap:** Posterize limits levels per RGB channel, not the total number of colors
or a selected palette. Four levels can yield up to 64 RGB combinations.

**Behavior:** map artwork to a small palette for pixel art, printed looks and
limited-color illustration. Preserve transparency. Automatic mode chooses a
representative palette when created, when Colors changes, or on Update Palette;
painting alone keeps that palette. Palette mode uses the artist's chosen colors.
This is an appearance effect, not a new indexed document format.

**Main controls:**

| Control | Meaning and starting behavior |
| --- | --- |
| Palette | Automatic or a selected existing palette. |
| Colors | Total color count; shown only for Automatic. |
| Update Palette | Recompute Automatic from the current input; one undoable change. |
| Dithering | None, Ordered or Blue Noise; None initially. |

**Later:** error-diffusion dithering and palette editing through the existing
palette UI. Save the selected colors or frozen palette result with the effect so
reopening cannot silently select a different palette.

Keep Posterize for the distinct per-channel graphic look. Do not label its Levels
control as the total number of colors.

Reference: [GIMP palette conversion](https://docs.gimp.org/3.0/en/gimp-image-convert-indexed.html).

### Dither

**Gap:** delivery/gradient dithering exists, but there is no creative filter for
binary or reduced-level illustration.

**Behavior:** distribute discrete marks to suggest intermediate shades. Patterns
remain stable across tiles, view changes and export. Applying noise alone is not
enough: the output must use the selected discrete levels or colors.

**Main controls:** Mode: Black & White, Gray or Color; Levels, shown for Gray or
Color and labeled as per-channel levels for Color; Method: Ordered or Blue Noise;
Size in pixels for the visible dither pattern. Start with Black & White.

**Later:** Error Diffusion, noise Randomize and binary alpha screening. Share
dithering choices and behavior with Reduce Colors; avoid a second palette editor
inside Dither. Do not expose named matrix variants in the main view.

Reference: [GIMP Dither](https://docs.gimp.org/3.0/en/gimp-filter-dither.html).

## Illustration looks and textures

### Cartoon / Cel Shading

**Gap:** Painterly, Posterize and Edge Detect provide separate building blocks,
not coordinated flat colors and outlines.

**Behavior:** simplify small color variations into readable regions while
retaining important boundaries. Optionally add dark contours. The result should
look like simplified illustration rather than a noisy thresholded photograph.

**Main controls:** Color Levels, Simplify, Lines toggle and Line Thickness
when Lines is on. Start with a small number of shading levels and restrained
outlines. Color Levels describes tonal simplification, not a guaranteed total
palette count; Reduce Colors provides that guarantee.

**Later:** Line Detail, opacity or color when examples require them. Initially
derive line selection from Simplify, keeping the panel small. Reuse extraction
and smoothing helpers where their purpose matches; the combined appearance may
evolve without freezing that exact pipeline into saved artwork.

References: [Clip Studio Artistic](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Cartoon](https://docs.gimp.org/3.0/en/gimp-filter-cartoon.html).

### Paper / Canvas Texture

**Gap:** no independent paper/canvas appearance effect on finished artwork.
Brush texture alone does not cover imported or already-painted images.

**Behavior:** give existing paint the visible grain or relief of a surface,
preserving the illustration's colors and transparent background. Choose textures
the project owns or has permission to distribute.

**Main controls:** Texture, Size and Strength. Start with a subtle paper texture.
Texture is a short preset selector, with a user image choice when supported.

**Later:** Angle, lighting Direction and Depth for relief-style textures. Do not
show lighting controls for a plain grain overlay. This is appearance only; it
does not change brush pigment transport or physical canvas behavior.

Reference: [GIMP Apply Canvas](https://docs.gimp.org/3.0/en/gimp-filter-apply-canvas.html).

### Hatching, Engraving and Stippling

**Gap:** Crosshatch provides four straight hatch families and opaque ink/paper
colors. Independent shading/detail controls and transparent paper are limited;
engraving and stippling looks are missing.

**Behavior:** translate shading into line or dot marks. Lighter regions have
fewer marks; darker regions have denser coverage. Use one family of controls
where possible rather than create several almost identical filters.

**Main controls:** Pattern: Lines, Crosshatch, Engraving or Stipple; Spacing in
pixels; Width for line patterns or Dot Size for Stipple; Angle for line patterns;
Contrast; Ink; Background: Transparent or Color. Start with simple straight
crosshatching. Engraving gives close parallel bands; Stipple gives irregular dots.

**Later:** curved contour-following lines, line-family count and mild variation.
Prefer sharing stipple/noise behavior with Halftone where the results coincide.
Do not duplicate a tone mode under a second name without a distinct drawing use.

Reference: [GIMP Engrave](https://docs.gimp.org/3.0/en/gimp-filter-engrave.html).

### Pencil / Charcoal

**Gap:** Pencil supplies a basic light/dark sketch with blur, contrast and
opaque ink/paper colors. Charcoal texture and transparent paper are missing.

**Behavior:** produce soft drawing marks from an image, with a clean pencil look
or broader grainy charcoal. Preserve useful contours without filling the whole
rectangle with gray haze. Allow the sketch over lower colors.

**Main controls:** Style: Pencil or Charcoal; Detail; Contrast; Grain; Ink;
Background: Transparent or Color. Grain starts low for Pencil and higher for
Charcoal. Hide it until the selected style provides visible grain behavior.

**Later:** mark direction. Improve Pencil and add Charcoal as a style before
adding separate filters with duplicated controls.

### Painterly / Oil Paint

**Gap:** Painterly performs limited edge-preserving region smoothing. It does
not provide a developed oil-paint or visible brush-mark look.

**Behavior:** simplify an image into painted regions. An oil-paint extension adds
visible marks while keeping forms readable and source colors recognizable.
Preserve alpha; do not create arbitrary paint outside the source silhouette.

**Main controls:** Style: Painterly or Oil Paint; Brush Size and Detail. Larger
Brush Size makes broader painted marks; more Detail retains finer source forms.
Oil Paint additionally exposes Texture to vary visible mark relief, with zero
giving flat paint. Reuse existing effect opacity to mix with the original.
Do not add Smoothness alongside Detail until trials show an independent need.

**Later:** lighting Direction only if artists need to orient visible relief.
Tensor parameters, sector counts and stroke-placement methods stay internal and
may change. Do not promise physical paint simulation.

Reference: [Krita Oilpaint](https://docs.krita.org/en/reference_manual/filters/artistic.html).

### Watercolor Look

**Gap:** live watercolor brushes exist, but there is no image-to-watercolor
appearance filter. Watercolor Border covers only the rim, not the full wash.

**Behavior:** soften colors into washes, retain important shapes and add subtle
pigment variation while keeping source alpha. This is an image stylization,
independent of live watercolor painting and its saved wet-paint state.

**Main controls:** Wash Size, Detail and Grain. Reuse Watercolor Border in a
stack when a stain rim is wanted; do not duplicate its full controls here.

**Later:** edge strength as a coordinated preset parameter if stacking proves
awkward. Paper choice belongs to the shared Paper / Canvas Texture workflow.

### Pattern Fill / Pattern Overlay

**Gap:** only Solid Color and Gradient Fill are available as built-in generators.
There is no dedicated repeating image pattern or overlay.

**Behavior:** repeat a chosen pattern over a fill layer or over the existing
painted shape. Fill can cover the composition with its mask; Overlay preserves
the source coverage. Keep those two uses clear without duplicating the pattern
chooser and transform controls.

**Main controls:** Pattern, Scale, Angle and Offset X/Y. Overlay uses existing
effect opacity and blend controls. Patterns start at their native proportions.

**Later:** mirror tiling and direct dragging of pattern placement. Automatic
seam repair is separate work; repeating an imported image need not make its
edges seamless.

References: [Photoshop Pattern Overlay](https://helpx.adobe.com/ie/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html),
[Krita Pattern Fill](https://docs.krita.org/en/reference_manual/layers_and_masks/fill_layer_generators/pattern_fill.html).
Compare clipping a texture to content with filling a layer from a repeated image.

### Procedural Texture Fill

**Gap:** noise helpers exist in shaders, but noise/clouds and basic repeating
patterns are not exposed as fill generators.

**Behavior:** generate a reusable texture independent of input paint. Start with
Noise/Clouds, Stripes, Checkerboard and Grid. Use existing fill masks and blending
to place them in artwork.

**Main controls:** Pattern, Size and two Colors. Noise/Clouds reveals Contrast,
Detail and Randomize; Stripes/Grid reveal Width and Angle where meaningful.
Save the random choice. Do not animate static illustration textures by default.

**Later:** seamless noise and Offset X/Y. Share simple repeating patterns with
Pattern Fill if one selector can serve both uses without confusion.

References: [Clip Studio Perlin Noise](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Solid Noise](https://docs.gimp.org/3.0/en/gimp-filter-noise-solid.html).

### Existing Film Grain and random variations

**Gap:** Film Grain already has Amount, Size, Color and animation controls, but
no authored seed. New stipple, charcoal, watercolor and procedural textures also
need a random choice that survives undo, reopen and export within a release.

**Behavior and controls:** retain Film Grain's existing look controls; add the
shared secondary Randomize action and save its result. For static illustration,
animation stays off. Existing animation remains a distinct supported use. Do
not add a duplicate grain filter, a seed-number field or per-octave sliders.
Use the same action for new looks where another random realization is useful.

The seed's value is authored; the artistic hash/noise realization may improve
between releases under the scoped policy above. Temporary previews must not
introduce a fresh random pattern on every frame.

### Pixel Mosaic

**Gap:** the existing effect samples the center of each square cell. It does not
average the cell or provide rectangular cells and grid alignment.

**Behavior:** reduce an image to a stable grid of flat cells. Average sampling
better represents detailed photographs; Center retains the distinct sampled
pixel-art look. Define treatment of partial coverage without introducing dark
fringes at transparent edges.

**Main controls:** Size and Sampling: Average or Center. Keep the existing
center-sampling appearance available.

**Later:** linked Width/Height for rectangular cells and Offset X/Y. Pixel
Mosaic changes appearance; document resizing and palette reduction remain
separate operations.

Reference: [Krita Pixelize](https://docs.krita.org/en/reference_manual/filters/artistic.html).

## Shadow, glow and relief additions

### Outer Glow

**Gap:** Bloom glows from bright image content; no glow follows the full painted
silhouette regardless of its brightness.

**Behavior:** add soft colored light outside the shape, retaining the original.
Dark ink can glow. Use group coverage for a group's glow.

**Main controls:** Color, Opacity, Size and Spread. Size controls reach;
Spread makes the inner part more solid. Start with a soft modest halo.

**Later:** generated-glow blend mode and Glow Only output. Leave noise and
contour graphs out of the first version.

References: [Affinity Outer Glow](https://affinity.help/photo2/en-US.lproj/pages/LayerFX/layerFX_outerGlow.html),
[Photoshop Outer Glow](https://helpx.adobe.com/ie/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html).

### Inner Shadow

**Gap:** no directional shadow within a silhouette.

**Behavior:** shade just inside the edge to suggest a recessed shape or cutout.
The shadow does not enlarge the object's coverage.

**Main controls:** Color, Opacity, Distance, Angle and Blur. Start with a shallow
dark edge. Offset can use the same drag interaction as Drop Shadow.

**Later:** Choke to make the shaded rim more solid and a generated-shadow blend
mode. Reuse Drop Shadow's offset language rather than invent a light-position UI.

References: [Affinity Inner Shadow](https://affinity.help/photo2/en-US.lproj/pages/LayerFX/layerFX_innerShadow.html),
[Photoshop Inner Shadow](https://helpx.adobe.com/ie/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html).

### Inner Glow

**Gap:** no soft colored lighting confined to the source shape.

**Behavior:** light the inside edge without adding exterior coverage.

**Main controls:** Color, Opacity and Size. Start at the edge with soft falloff.

**Later:** Source: Edge or Center, Choke and a generated-glow blend mode. Start
without a contour curve or several interchangeable falloff sliders.

References: [Affinity Inner Glow](https://affinity.help/photo2/en-US.lproj/pages/LayerFX/layerFX_innerGlow.html),
[Photoshop Inner Glow](https://helpx.adobe.com/ie/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html).

### Long Shadow

**Gap:** no graphic extrusion shadow.

**Behavior:** extend the silhouette continuously in one direction behind the
source. Unlike Drop Shadow, this creates a connected trail rather than one
displaced silhouette. Keep it bounded for ordinary illustration work.

**Main controls:** Angle, Length in pixels, Color, Opacity and Fade. Zero Fade
gives a solid extrusion; higher values fade toward its end.

**Later:** Shadow Only output. An unbounded/infinite mode is optional and should
not complicate the first controls or output bounds.

Reference: [GIMP Long Shadow](https://docs.gimp.org/3.0/en/gimp-filter-long-shadow.html).

### Bevel / Relief

**Gap:** Emboss derives relief from color differences. It does not provide
lit raised or recessed edges based on the painted shape.

**Behavior:** give the silhouette a raised or recessed rim while preserving
its base color. Treat Bevel as a distinct use from the existing image Emboss.

**Main controls:** Style: Inner, Outer or Emboss; Size; Depth; Direction:
Raised or Recessed; Light Angle; Softness. Start with a modest inner bevel.

**Later:** Light Height, highlight/shadow colors and strengths. Avoid custom
profile graphs and global lighting unless simpler controls prove insufficient.

References: [Affinity Bevel/Emboss](https://affinity.help/photo2/en-US.lproj/pages/LayerFX/layerFX_bevelEmboss.html),
[Photoshop Bevel & Emboss](https://helpx.adobe.com/ie/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html).

## Further GA additions

### Crystallize

**Gap:** Pixel Mosaic uses a regular grid; no irregular cell mosaic exists.

**Behavior:** replace regions with representative flat colors in irregular cells
for stylized backgrounds. The same settings and random choice reproduce the
same result.

**Main controls:** Cell Size, Randomness and Randomize. Low randomness approaches
a regular arrangement. Prefer representative cell averages over one-point
sampling, while preserving source alpha per pixel. The prototype must validate
cost and appearance; do not expose a cell-color algorithm selector.

**Later:** seamless output for texture-making and cell borders. Do not add a
separate seed-number field when Randomize and undo are sufficient.

References: [Clip Studio Crystallize](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Mosaic](https://docs.gimp.org/3.0/en/gimp-filter-mosaic.html).
Compare irregular cell color/size behavior; GIMP's decorative joints and
lighting are not required for Capy's flat-cell look.

### Zoom Blur / Spin Blur

**Gap:** directional Motion Blur exists, but zooming and rotational blur do not.

**Behavior:** Zoom Blur streaks toward/away from a center; Spin Blur follows arcs
around it. Both are useful for illustrated action. A shared Radial Blur entry
with two modes is sufficient if users can readily distinguish them.

**Main controls:** Mode: Zoom or Spin; Amount; Center X/Y with a draggable center.
Use a straightforward numeric amount and keep sampling quality automatic.

**Later:** Direction for Zoom: Both, Outward or Inward. Elliptical spin and extra
falloff controls are optional.

References: [Clip Studio radial and spin blur](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Zoom Motion Blur](https://docs.gimp.org/3.0/en/gimp-filter-motion-blur-zoom.html) and
[Circular Motion Blur](https://docs.gimp.org/3.0/en/gimp-filter-motion-blur-circular.html).

### Lens Blur

**Gap:** no blur with shaped highlight/bokeh behavior.

**Behavior:** soften a background with lens-like highlights while respecting
alpha. Keep this a simple appearance filter before considering depth maps.

**Main controls:** Radius, Highlight Strength and Shape: Round or Polygon.
Polygon reveals Blades and Rotation. Round is the ordinary starting shape.

**Later:** Highlight Threshold and a depth-map input. Leave technical aperture
simulation settings out of the first version.

References: [Clip Studio Lens Blur](https://help.clip-studio.com/en-us/manual_en/390_filters/Filters.htm),
[GIMP Lens Blur](https://docs.gimp.org/3.0/en/gimp-filter-lens-blur.html).

### Displacement Map

**Gap:** Domain Warp and other distortions use procedural controls; no authored
image map drives displacement.

**Behavior:** distort artwork using a selected map, for example to follow cloth
or an uneven illustrated surface. A neutral map leaves it in place. Make the map
an explicit saved input so a missing external file cannot change the result.

**Main controls:** Map, Horizontal Amount and Vertical Amount in pixels, and
Edges: Transparent, Repeat or Clamp, applying to sampled source artwork outside
its defined bounds. Missing map coverage means neutral displacement. Zero amounts
preserve the source. Start map placement at native scale with a stable artwork
origin; dragging the map can be added when placement trials establish the need.

**Later:** linked amounts, separate horizontal/vertical maps and polar mode.
Separate channels and sampling kernels do not belong in the first view.

Reference: [GIMP Displace](https://docs.gimp.org/3.0/en/gimp-filter-displace.html).

### Color Transfer

**Gap:** no reference-image color matching effect.

**Behavior:** bring an illustration's color relationships closer to a chosen
reference while keeping its forms and coverage. This is palette harmonization,
not Layer Color or replacement of every pixel with one color.

**Main controls:** Reference, Strength and Match. Choosing a reference performs
an initial match; Match refreshes it from the current input. Retain the computed
mapping so ordinary painting does not continually change the entire layer's
colors. Start with a moderate application; zero Strength preserves the original.
Reference and the matched result are saved, and Match is one undoable action.

**Later:** Preserve Lightness only if the initial matching behavior cannot
protect established values adequately. Its absent state is not a reserved
parameter. The initial prototype should compare full color-statistic matching
with chroma-focused matching. Avoid algorithm/working-space selectors.

Reference: [Krita Color Transfer](https://docs.krita.org/en/reference_manual/filters/colors.html).

### Satin

**Gap:** no interior folded/glossy shading effect for decorative graphic shapes.

**Behavior:** add soft interior shading suggestive of satin or a glossy finish,
without replacing the underlying artwork or enlarging its coverage.

**Main controls:** Color, Opacity, Angle, Distance and Size. Start with a subtle
dark contribution. Its narrower illustration use places it late in the GA work.

**Later:** Invert. Custom contour graphs are outside the initial scope.

## Related workflow gaps

### Reusable effect presets and stacks

Save one effect's settings or an ordered stack so artists can reuse a comic
screen, ink cleanup or lettering treatment. Preserve authored colors, pattern
choices and resources. Reuse the existing deferred
[effect settings and preset work](photo-editing-roadmap.md#inspection-presets-and-export)
rather than start a second preset system.

**Main controls:** Preset selector, Save Preset and Reset. Saving asks for a name.
Applying a preset is one undoable change. Rename/Delete belong to preset
management, not every effect's primary controls. Copy/paste settings should work
without first saving a preset.

Reference: [Krita layer style presets](https://docs.krita.org/en/reference_manual/layers_and_masks/layer_styles.html).

### Separate lines and tones

Provide a conversion command that creates a group with independently editable
line and tone results while retaining the source. Use the existing extraction
and tone controls rather than a new parallel dialog with different meanings.

**Main controls:** Lines and Tone toggles; Tone Output: Screentone or Gray;
Create Layers. Show the relevant effect controls with the normal preview.
Create all results in one undo step and name them clearly.

Whether results initially retain live effects or are rendered into editable
raster layers remains open; the command must clearly communicate which result
the artist will receive. Do not silently replace or flatten the source.
Vector tracing and extraction from 3D geometry are separate projects.

References: [Clip Studio separated lines and tones](https://help.clip-studio.com/en-us/manual_en/390_filters/Convert_to_lines_and_tones_%28EX_only%29.htm),
[Krita edge extraction](https://docs.krita.org/en/reference_manual/filters/edge_detection.html) and
[halftone filtering](https://docs.krita.org/en/reference_manual/filters/artistic.html).
These compare a packaged separation workflow with independently composable
passes, not two claims of the same one-click command.

### Advanced style controls to defer

Several reference tools offer effect contours, shared/global light, independent
fill opacity, gradient or pattern borders and elaborate noise settings. Keep
these recorded as possible improvements, not mandatory controls for every
style. Existing per-effect stacks and masks should cover common combinations
first. Independent source Fill Opacity is useful for effects-only lettering,
but changes layer contribution behavior and needs its own design before being
added as a filter slider.

## Implementation boundaries and completion criteria

The proposal chooses artist-visible behavior and researched starting algorithms,
without making artistic implementation details a permanent contract. Implementers
may extend the catalog, reuse existing filters, coordinate an effect stack or
add shared renderer support where needed. New image-map/pattern inputs require
the shared `capy.image/1` record and per-use color/data roles from object-layer
Milestone 1, followed by the off-frame evaluation work in Milestone 2. These are
prerequisites, not a second image-resource design. Preserve authored resources
without exposing GPU layouts in artwork.

Keep document rules, controls, history and validation shared across hosts.
Follow the existing [runtime filter contract](../reference/runtime-filters.md),
[package contract](../reference/capy-package.md), and
[UI development workflow](../ui/README.md#rules-for-ui-changes).
Update localization when controls become actual UI.

For each delivered feature, check:

- The common journey with its main controls, including reset and undo/redo.
- Transparent input, partial alpha, holes, disconnected shapes and grouped art.
- Masks and clipping, expanded effect bounds, source edges and tile seams.
- Stacked effects and edits that change either the source or the parameters.
- Stable pattern placement through pan, zoom, rotation and document geometry.
- Save/reopen, preview and exact export with retained choices and resources.
- Small ink details and flat ramps for extraction, tone and color reduction.
- Real affected host journeys in both themes, and the checks required by the
  [testing guide](testing.md).
- Frame production and control-drag performance against the
  [performance targets](../PERFORMANCE_TARGETS.md), measured by the
  [measurement rules](../performance/measuring.md).

Record implementation evidence separately. A proposal, screenshot or successful
compile does not establish host parity or a reference-hardware performance target.

## Assessment of the review

The review identifies real gaps in resources, invalidation, alpha treatment and
testing. Its blanket requirement to freeze artistic formulas is rejected under
the release policy above. Bounded math alone also does not establish that every
filter can ship at the required speed. Treat all filters as the scope to deliver,
not as an already demonstrated feasibility result.

Only the supplied summaries of C2–C9 and the current review are available here.
The earlier review's unspecified definitions cannot be treated as approved or
complete. The decisions below assess the claims actually supplied.

### Cross-cutting claims

| Claim | Assessment and revised decision |
| --- | --- |
| C1: every filter needs a parameter-derived maximum local radius | **Revise.** Every evaluator needs correct dependencies, output bounds and invalidation. These may be a finite neighborhood, a transformed sampling region, or whole-input analysis. A small analysis image still depends on the whole input. Store explicit Update/Match results where useful; asynchronous global evaluation is also valid. Do not add an artist-facing Size control solely to disguise global analysis. |
| C2: freeze absent values for future parameters | **Choose the versioned alternative.** Reserve no future fields. All implemented values, including hidden/default values, are saved. When a new control ships after GA, a concrete converter inserts the value preserving earlier intent under a new parameter-data version. Internal artistic tuning remains separate from adding an authored control. |
| C3: anchor everything to the owner | **Choose crop compensation for frame-relative effects.** Preserve existing composition-space meaning using the authored spatial reference described above. Crop shifts that reference with artwork; moving an owner does not silently make a frame-relative effect object-local. Explicit owner-relative image inputs keep their declared origin. Painted-bound growth never resets either origin. |
| C4: all content beyond the source is transparent | **Distinguish boundaries.** The frame edge and temporary tile/ROI edges are never image boundaries. Retained off-frame content must reach every filter dependency. True finite source edges use each input's declared extension: new silhouette/image effects generally use transparency, while displacement map extension has its own choices. Current frame-bounded capture must change before GA. |
| C5: image and palette values | **Accept and reuse the specified design.** `capy.image/1` is not implemented yet, but is already specified by the object-layer design. Use that same record for image objects, paint bases and filter inputs, with interpretation selected per use. Ordered portable palette colors remain a separate authored value. |
| C6: saved seeds | **Accept with scope.** Randomize must be undoable and repeatable within a release, across tiles, preview and export. Use the existing Count dimension with a validated integer in 0…16,777,215. Neither fixed procedural algorithms nor unchanged generated pixels across artistic releases are promised. |
| C7: one silhouette pipeline | **Accept shared input/coverage plumbing, reject one universal threshold.** Use the same owner/group/mask semantics. A 50% contour does not represent all visible paint: a uniformly 25%-opaque stroke has none. Binary contour extraction and continuous coverage processing serve different purposes. |
| C8: improve before GA | **Accept.** Correct current tone coverage, transparency and poor artistic output before presenting the expanded set as finished. This does not mean freezing every first-release artistic implementation forever. |
| C9: minimal choice lists | **Accept.** Keep distinct artist-facing modes; remove algorithm selectors and overlapping sliders. Painterly initially drops separate Smoothness; Cartoon initially drops separate Line Detail. |
| C10: one immutable shared operator library | **Accept reuse, reject the universal/immutable formulation.** Share identical primitives and preparation for identical inputs. Different artistic purposes can require different smoothers or edge responses. Artistic helper improvements need review across dependents, not automatic data-version bumps. |
| C11: conservative permanent maxima selected from the lowest tier | **Qualify.** Measure conservative UI ranges, processing budgets and admission on every tier. Accepted data bounds and slider bounds are separate. Finalize every accepted range at the filter's release freeze and never narrow a released range. Existing pixel lengths can exceed sliders and resize must retain their value or refuse atomically. A low-tier measurement cannot qualify larger canvases, stacks, exports or every accepted length. |

C1's spatially bounded analysis is valid when Size really limits the sampled
region and that local result serves the filter's intent. Reducing the resolution
of a full-layer analysis only bounds its storage/work; it does not make its
dependencies local. The existing asynchronous analysis path is a third option
alongside a saved Update result and a genuinely local algorithm.

Code evidence: [required effect values and current auxiliary kinds](../../crates/layer-core/src/package/effect_records.rs),
[sampling dependencies and accepted lengths](../../crates/layer-core/src/effects.rs),
[analysis ownership and scheduling](../reference/runtime-filters.md#shadowshighlights-and-clarity),
[package semantics](../reference/capy-package.md#evaluation-meaning).

### What should actually be shared

| Building block | Useful sharing boundary; gap in the review |
| --- | --- |
| Distance | Share coverage preparation and distance services where appropriate. Specify the represented boundary, reconstruction, empty input, ties, anisotropic transforms and required accuracy. An exact distance to binary sample centers is not exact subpixel distance to a reconstructed coverage contour. A capped distance field can support local styles; an unbounded field has wider dependencies. |
| Gaussian | Reuse the existing meaning: sigma in pixels, normalized weights, support through `ceil(3*sigma)`, zero identity. Gaussian has infinite mathematical support until truncation is specified. Multi-stage footprints include all upstream stages and sampling support. |
| Smoothing | Existing Denoise is a small bilateral-like filter, while Painterly uses quadrant statistics. Classic and anisotropic Kuwahara are themselves different. Share moment/gradient/tensor/blur helpers; do not force denoising, cel regions and brush marks through one Kuwahara variant. |
| Edges | Extract Lines and Cartoon can share preparation and an extraction implementation. Pencil can share part of it without being restricted to identical output. An edge-response operator alone does not define line selection, width, cleanup or source-alpha handling. |
| Noise | Share seeded coordinate hashing and useful noise functions. Coordinate origin, negative coordinates, frequency/scale and empty input need tests. Artistic noise spectra and octave mixes remain tunable. Randomize is a small action, not a technical seed editor. |
| Blue noise | Use a threshold/rank mask suitable for halftoning, not an arbitrary image with a blue spectrum. Test uniform tone coverage and wrap boundaries. Repeated sampling must be stable within a release; an internal mask is not automatically a new authored resource kind. |
| Color distance | Oklab is a reasonable palette-matching candidate. Convert from the document's actual primaries/transfer function, define ties and handle extended values. Perceptual nearest color and physically meaningful mixture proportions are different calculations. |
| Lightness | Do not use one ambiguous function for everything. Linear luminance, encoded luma and perceptual Oklab lightness have different purposes. Name the domain internally; reuse the established color infrastructure. No working-space selector is needed in the filter panel. |

The Oklab author's [space definition](https://bottosson.github.io/posts/oklab/)
and [gamut analysis](https://bottosson.github.io/posts/gamutclipping/) support
separating perceptual operations from output-gamut handling. Neither establishes
one universally best metric for every illustration effect.

### Painterly / Oil Paint

**Assessment: useful direction, over-specified permanence, incomplete oil look.**
The current four 3×3 quadrant samples are visibly different from full-region
statistics. On a one-pixel checkerboard, radius 4 samples offsets 0, 2 and 4:
all nine samples can be white, while the corresponding full 5×5 quadrant has
mean 13/25 = 0.52. This is not a harmless approximation under a small tolerance.

Prototype anisotropic Kuwahara for coherent painted regions, with classic
quadrants as a comparison. Tensor construction, smoothing support, flat-region
fallback and bounded anisotropy all affect cost. Do not make full-quadrant means
or a specific tensor algorithm the saved artistic contract.

Smoothed lightness is not paint thickness. Lighting it can emboss the photograph's
tonal features rather than create oil marks. Compare mark-derived relief and a
subtle texture modulation visually. A bundled bitmap is optional, not necessary
for an oil look. Expose Oil Texture if relief is visible; leave its synthesis and
light tuning internal. Brush Size describes mark scale and Detail describes
retained structure. Their exact response curves may evolve; a redundant
Smoothness formula is not a GA requirement.

### Cartoon / Cel

**Assessment: sensible pipeline family, not a uniquely correct recipe.**
Edge-aware smoothing, tonal grouping and optional ink are a strong starting
point. Bilateral or guided smoothing may suit flat cel regions better than
Kuwahara's brush-like regions. The exact smoother is internal.

Quantizing Oklab L while preserving a/b can leave the output gamut. For example,
red's chroma at L = 0.5 gives approximately linear sRGB
`(0.622, -0.040, -0.019)`. Specify behavior for the document's output range;
do not accidentally clamp all HDR artwork. Decide the tonal endpoints and test
small level counts. Soft band edges also mean Color Levels is not a count of
unique RGB values.

Use lines from an appropriate stage of the simplified image. Applying a second
source-over layer of ink with the source alpha can increase coverage; this
effect should normally preserve the original coverage and mix ink within it.
Reuse extraction controls where exposed, but start with only Line Thickness in
the combined filter. The exact pipeline can change without a cascade of artistic
data-version changes.

### Watercolor Look

**Assessment: accept appearance stylization and stacking; reject “single pass”
and generic brightness noise as sufficient definitions.** Shader pass count is
an implementation choice. Granulation should resemble pigment density variation,
not just mottled brightness or colored noise. Preserve alpha, test white paper
and saturated paint, and let Grain zero remove granulation.

Keep Wash Size, Detail and Grain. Edge staining and paper can be reusable stack
components. Watercolor Border must cover alpha silhouettes; internal boundaries
between opaque colors are a distinct question for the combined look. Merely
stacking an alpha-edge filter does not reproduce every internal pigment edge.
Share helpers where appropriate while allowing the look to improve.

### Bevel / Relief

**Assessment: plausible distance-height approach, not a complete formula.**
The proposed smoothstep omits the zero-size case, outer height direction and
inner/outer join. Outer and Emboss styles also need an explicit coverage policy.
Normals need a derivative neighborhood, and height blurring adds its own support.
Subtract or otherwise account for flat-surface illumination so a bevel does not
unintentionally dim the entire interior.

Raised/Recessed can reverse the relief sign; Depth should visibly strengthen
relief. Light Angle is attached to artwork, not the camera. Viewing a rotated or
flipped canvas must not rewrite it. Authored rotate/flip/resize operations need
defined coordinate transforms, including reflection. Fixed initial altitude and
colors are reasonable defaults, not reserved absent parameters.

### Satin

**Assessment: viable Capy-specific starting look.** The absolute difference of
two displaced, blurred coverage fields can produce interior folds. Specify the
offset vector and clip the shading to source coverage exactly once. Start with
a Multiply contribution for familiar dark satin shading; colored/light examples
must test this choice before implementation is settled.

For a truncated Gaussian candidate, each axis needs up to the corresponding
absolute offset plus `ceil(3*sigma)` and interpolation support. Distance zero
should give no folds for this candidate. Invert can wait. This is an evolving
artistic definition whose Gaussian-difference shading can improve over time.

### Lens Blur

**Assessment: accept normalized aperture gather as reference; qualify speed.**
A round disk or regular polygon is a useful reference. Define whether Radius
measures the polygon's vertices or inscribed circle, and preserve normalized
energy with premultiplied color/coverage. Shape rotation belongs to artwork.
Highlight gain needs an explicitly chosen lightness domain, reference white and
safe zero-alpha handling; it must not brighten alpha. Make Radius zero a complete
bypass even when Highlight Strength is nonzero.

Gaussian separability does not make a disk separable, and a hexagon is not a
close substitute for every polygon. Approximation quality must be checked with
isolated highlights, not only smooth photos. Compare bounded aperture gathers
and suitable disk approximations, with display-resolution motion previews and
native refinement. A uniform Lens Blur does not need a center point. A depth
map is an additional feature and dependency model, not an absent current input.

### Displacement Map

**Assessment: accept explicit data sampling; correct ambiguous edge semantics.**
Raw encoded R/G channels are a conventional useful map interpretation. Import
the map as data for this role without color-profile conversion; preserve its
original samples. For grayscale maps, use the gray channel for both axes rather
than introducing an undefined color-managed “lightness.” Transparent or missing
map coverage should be neutral, not maximum negative displacement.

The suggested `(v - 0.5) * 2 * Amount` is sound for normalized values in [0,1],
but the displacement sign and sampling direction must be stated. Float maps
outside that range need an explicit policy or the claimed bound fails. Eight-bit
128 is not exactly 0.5: that formula produces `Amount/255`, so neutral 8-bit
fixtures and the chosen midpoint convention matter.

Keep the familiar Edges control for the **source image**, consistent with the
[GIMP Displace dialog](https://docs.gimp.org/3.0/en/gimp-filter-displace.html).
Use neutral map exterior initially. A later map-repeat control would be separate.
The footprint is per-axis `abs(Horizontal/Vertical Amount)` plus interpolation;
a scalar Euclidean radius is larger for diagonal displacement. Map sampling has
its own extent/placement dependencies. Native map pixels, owner-local pixels and
composition units cease to be interchangeable after layer scaling.

### Color Transfer

**Assessment: explicit Match is a good default; the statistical recipe remains
an experiment.** Store the matched result and reference so per-pixel evaluation
does not reanalyze the layer after every stroke. Compute a match from the input
before this effect to avoid feedback, with alpha weighting and hidden RGB ignored.
Use a defined finite input domain, including its off-frame content where retained;
an unbounded procedural input cannot be analyzed “in full.”

Mean/standard-deviation matching needs zero-variance and empty-image handling.
Oklab channel scaling is a candidate adaptation, not the original Reinhard
paper's lαβ formulation. Compare preservation of important values and hue against
aggressive full-statistic matching. Do not assume Preserve Lightness must default
off merely because the reviewer proposes it.

Save one authoritative mapping representation, rather than both redundant
statistics and a transform that can disagree. Evaluate whether the existing LUT
resource is sufficient before adding a new representation. Match/reference
changes must publish atomically, be cancellable and undo together. The algorithm
that creates a new match can improve without adding user-visible algorithm modes.

### Crystallize

**Assessment: bounded jittered sites are suitable; “a second pass” proves little.**
Define maximum jitter and search enough neighbors to include every possible
winner; an arbitrary 3×3 search is not valid for arbitrary jitter. Deterministic
ties and seed/origin handling prevent seams and flicker. Cell-average coloring
requires gathering/reducing the whole contributing region, not merely assigning
site IDs in another pass.

Prefer coverage-weighted cell color with original per-pixel alpha. Compute the
representative straight color from associated sums and reapply source alpha once.
Test cells with no paint and partially covered sites. One-point site sampling is
a useful inexpensive comparison but can miss thin colored features. Site layout
and averaging details remain free to improve under the same three controls.

### Remove Dust

**Assessment: size-bounded connectivity is promising, but the stated margin and
soft-fringe behavior are incomplete.** A component whose bounding box fits in
Size × Size can be classified locally only with enough extra boundary evidence
to reject a continuing component. A tightly coiled line can fit that box too;
this is not automatically “remove dots but preserve all long lines.”

A hard 50% coverage test ignores faint ink. Define foreground classification
for transparent versus white paper, including the color domain. Choose consistent
foreground/background connectivity to avoid diagonal ambiguity. Fill Gaps means
enclosed holes, not automatically bridging open breaks. Soft-fringe attribution
needs a bounded rule; following arbitrarily faint connected pixels can defeat
the promised local margin. Nearby fill color needs a specified neighborhood and
tie behavior. Reuse the cleanup engine for Extract Lines where useful, without
claiming bounding-box size and fragment length are identical controls.

### Adjust Line Width

**Assessment: morphology is a good basis; the proposed preservation formula is
not established.** Maximum/minimum coverage over a disk is meaningful, but an
“anti-aliased disk” does not by itself define fractional-radius grayscale
morphology. A weighted maximum is not automatically a coverage-preserving
subpixel dilation. Test shallow alpha ramps and subpixel amounts.

Distance from each pixel to the nearest edge is not the local stroke half-width.
At a thick stroke's boundary that distance is near zero; using it directly to
limit erosion can protect the very boundary that should move inward. Width needs
additional ridge/medial information or another preservation method. Even a
medial axis does not by itself prove a fast local, topology-preserving algorithm.
The [morphology documentation](https://scikit-image.org/docs/stable/api/skimage.morphology.html)
distinguishes distance, medial axes and thinning for this reason.

Keep Thin Lines should aim to retain recognizable fine strokes and junctions,
not promise an exact one-pixel width for every rotated, faint or branching shape.
Preserve existing colors where coverage remains; define extension-color ties
for thickening rather than copying a transparent contributor during erosion.
Measure the preserved-thinning path separately from ordinary morphology.

### Reduce Colors and Dither

**Assessment: adopt saved generated palettes and blue-noise dithering; reject
mandatory destructive error diffusion.** Automatic palette creation/Update is
global analysis, while applying the saved palette is local. Changing Colors
recomputes the palette as one completed authored change; painting does not.
Median cut is one candidate, not automatically the best perceptual palette.
Once the palette is saved, its generation algorithm can improve independently.

True Floyd–Steinberg propagates error in scan order; independent tiles cannot
reproduce it. That is a dependency/scheduling problem, not proof that it can never
be a saved live effect. It could use whole-input cached evaluation if justified.
The GA Dither methods remain Ordered and Blue Noise; Error Diffusion is a later
method, explicitly not an alias for Blue Noise. “All filters” does not require
every proposed later mode or control.

Blue Noise needs a suitable rank distribution. Test gray ramps, endpoints,
palette membership, alpha and pattern scale. Color dithering also needs a color
mixture rule; simply picking the two nearest Oklab entries need not reproduce the
intended average color. A moving random pattern or a frame-dependent seed is
unacceptable even for an evolving artistic filter.

### Engraving and Stipple

**Assessment: useful candidates with a tone-calibration error to avoid.**
Engraving Width should mean full line width; the proposed half-width formula
otherwise doubles its intuitive meaning. Darkness/Contrast mapping is internal
artistic tuning. Use pixel-footprint-aware antialiasing rather than promising a
one-pixel ramp will work at every zoom/export scale.

For fixed square-grid sites and nonoverlapping circular dots, radius proportional to square-root
darkness gives proportional area only up to a scale factor. With maximum radius
Spacing/2, full darkness covers only π/4 ≈ 78.5% of a square cell. Larger dots
overlap and break the simple area argument. This is not a universal bound on
square-root radius modulation or on jittered layouts. Prefer density-modulated blue-noise
sites for a distinct stippled drawing, with Dot Size controlling mark diameter
and Spacing the available site density. Calibrate the usable range; do not promise
exact solid black at every combination of artistic dot size and spacing.
Halftone remains the calibrated screen when precise ink coverage is needed.

### Pencil / Charcoal

**Assessment: reject freezing the current shortcut as the artistic target.**
The code uses a stabilized ratio, a contrast exponent, and its existing color/
blur ordering. It is only related to an ideal color-dodge construction. At
gray 0.1 and blurred gray 0.2 with contrast zero, current coverage is about
0.4762; the simple ratio model gives 0.5. Alpha and nonlinear color conversion
introduce further differences. Neither formulation alone guarantees pencil marks.

Compare a fast improved ratio sketch against coherent lines plus tonal stroke
texture. Charcoal needs broad textured deposits and useful dark tones; multiplying
the same thin edge map by fbm cannot create missing tonal structure. Keep Detail,
Contrast and meaningful Grain. Make transparent paper multiply ink coverage by
source coverage. A hard haze floor can erase faint strokes; prefer smooth tuning
and test low-contrast sketches. Grain zero must remove texture modulation.

### Value kinds and assets

| Proposed kind | Decision |
| --- | --- |
| Image input | Use the planned shared `capy.image/1`, with per-use `color` or `data` role, immutable ownership and discoverable references. Pattern, Paper, Displacement and Color Transfer depend on object-layer Milestone 1. |
| Palette | Save ordered tagged colors and define empty/duplicate handling. Reuse existing palette UI. Preserve the selected colors; they are authored choices. |
| Point / spatial reference | Keep simple paired center controls; uniform Lens Blur has no center. Frame-dependent filters save spatial reference geometry so crop preserves phase and normalized centers/radii. This is authored geometry, not a new visible control or a universal graph input. |
| Seed | Existing Number with Count dimension, inclusive 0…16,777,215, integral before f32 conversion. Randomize writes the value with one undo action; no numeric seed UI. |
| Bundled assets | Every serialized asset ID is permanent and resolves to an available asset or explicit replacement mapping. Artistic pixels may evolve; IDs are not content hashes. Internal rank masks need no saved ID unless selected/serialized. Imported image pixels remain authored resources. |
| Derived authored results | Useful for palettes and Match. Save authoritative results produced by explicit actions; keep temporary tensors, distance fields, histograms and previews out of artwork. Their reuse keys must include the actual source and parameters. |

### Groups the review leaves “unchanged”

These stay in scope, but referring to an earlier reply does not close their gaps.

| Filter or workflow | Remaining decisions and checks |
| --- | --- |
| Border | Faint/soft coverage, holes, grouped silhouettes, outside/center/inside width, antialiasing and expanded bounds. No implicit 50% cutoff for all paint. |
| Drop Shadow / Outer Glow | Offset, spread and softness meanings; source kept visible; alpha compositing once; generated-mark opacity independent of owner opacity; transparent exterior. |
| Inner Shadow / Inner Glow | Confine the result to source coverage without applying alpha twice. Define edge handling and interior falloff. |
| Long Shadow | Connected directional sweep, finite length, endpoint/fade, fractional angles and dirty/output bounds. A repeated gather over every pixel of Length is only a reference, not a speed solution. |
| Watercolor Border | Source-following rim color, separate Darkness/Opacity roles, faint coverage and zero-width behavior. Brush wet-paint state remains separate. |
| Extract Lines | Coherent contours, thickness independent of detail selection, gray/binary outputs, transparent paper and no unintended rectangular frame. Cleanup semantics need their own acceptance examples. |
| Halftone | Actual tone/area calibration, 0%/100% endpoints, partial alpha, rotated screens, no moiré caused by preview sampling, print-resolution units and transparent gaps. |
| Threshold | Gray-to-binary class definition, exact equality at threshold, single-color transparency and independent alpha threshold. Reuse existing grayscale filters. |
| Pixel Mosaic | Coverage-aware Average versus Center, cell phase, partial boundary cells and incremental damage covering entire affected cells. |
| Brightness to Opacity | State the brightness domain; white/black endpoints, preserved source coverage and black ink output. |
| Color to Alpha | Remove the selected color's edge contribution, not merely low-distance pixels. Define tolerance, alpha reconstruction and handling outside SDR. |
| Reduce Colors with dithering | Saved palette membership, alpha, tone accuracy, stable pattern and explicit palette refresh behavior. |
| Paper / Pattern / Procedural Texture | Resource ownership, image versus data sampling, stable placement and scale, transparent overlay versus filled coverage, saved random choice. Avoid view-dependent texture detail. |
| Zoom / Spin Blur | Center space and dragging, direction/amount meaning, normalized coverage-aware sampling and variable spatial dependencies. Maximum displacement depends on position and domain as well as Amount. |
| Presets | Can reuse effect records, but resource collection, copying IDs, storing presets and undo still need design. “No format impact” is conditional, not proven. |
| Separate Lines and Tones | Use ordinary duplicated sources/effect stacks where sufficient; linked live outputs would introduce dependency questions. Decide live versus raster output clearly and make creation one undoable operation. |

## Algorithm research and recommended prototypes

These are recommendations from primary papers, author explanations and official
technical documentation, combined with the code audit. They are **engineering
inferences for Capy**, not benchmark results. There is no single best artistic
algorithm independent of subject matter, desired marks, scale and hardware.
Choose the smallest approach that produces the intended look, then improve it
behind the same small set of controls.

The references are research inputs, not dependencies to import. In particular,
some research implementations are GPL; reuse published ideas through Capy's own
implementation and follow the repository's dependency/publication rules. No
third-party code, assets or dependencies are added by this plan.

### Extract Lines: flow-guided difference of Gaussians

**First candidate:** smoothed structure-tensor orientation, a difference-of-
Gaussians response across contours, smoothing along them, and a soft threshold
for gray ink. Apply final width/cleanup independently enough that increasing
Thickness does not simply detect more texture. Compare an isotropic XDoG path
as the inexpensive baseline.

[Structure-adaptive filtering](https://www.kyprianidis.com/p/tpcg2008/)
uses separated directional filtering to improve contour coherence.
[XDoG](https://www.kyprianidis.com/p/cag2012/) provides useful threshold and
stylization variants. These support a cleaner illustration starting point than
just increasing a Sobel-like edge magnitude.

**GPU tradeoff:** gradients and tensor smoothing are regular passes; following
curves adds irregular reads. Bound integration length and iteration count
internally and include guide preparation in dependencies. Reuse a guide only
when the source stage and scale actually match. Tensor resolution may be reduced
for preview, but retain thin strokes and intersections in the final result.

**Decision images:** faces, architecture, small lettering, scanned pencil,
equal-lightness color boundaries, textured foliage, transparent cutouts. Choose
the algorithm by coherent useful ink and lack of unwanted frames; neither
“detect every edge” nor absolute agreement with a photographic detector is the
artistic goal. Detail controls retained contour complexity, not a saved DoG ratio.

### Cartoon: edge-aware simplification plus tonal grouping

**First comparison:** a guided filter against orientation-aligned bilateral
smoothing, followed by perceptual tonal grouping and optional Extract Lines ink.
Guided filtering has a local linear model and radius-independent linear-time
formulation; its box-statistic structure is attractive for GPU processing.
Fast guided filtering estimates coefficients at reduced resolution, trading
detail for cost. These are alternatives to test, not a claim of identical output.
See [Guided Image Filtering](https://people.csail.mit.edu/kaiming/eccv10/index.html)
and [Fast Guided Filter](https://arxiv.org/abs/1505.00996).

**Why not one Kuwahara everywhere:** its region-selection character is useful
for paint but may create unwanted facets in cel shading. A bilateral grid is
another candidate for wide smoothing, with extra grid storage and a choice of
range guidance; the [original bilateral-grid work](https://groups.csail.mit.edu/graphics/bilagrid/)
establishes a GPU approach. Test equal-luminance color edges before using only
scalar guidance.

The [domain transform](https://doi.org/10.1145/2010324.1964964) offers efficient
iterated one-dimensional edge-aware filtering. Its recursive realization has
scan dependencies; it should not be selected merely because a paper calls it
linear time. Capy's tiled incremental work and WebGPU portability matter as much
as full-frame throughput. Keep it as a comparison if the first pair fails.

**GPU/quality gate:** broad flat regions without halos, direction bias or
staircasing; stable fine contours; limited intermediate storage. Prototype
perceptual banding with range-aware gamut handling. Simplify maps to a coordinated
internal amount/scale; do not expose filter radius, regularization and iteration
count as three extra controls.

### Painterly and Oil Paint: distinguish abstraction from brush marks

**Painterly first candidate:** anisotropic Kuwahara with smooth sector weighting.
The [polynomial-weighting paper](https://www.kyprianidis.com/p/tpcg2010/) replaces
texture-evaluated weights with efficiently evaluated polynomials.
[Multi-scale anisotropic Kuwahara](https://www.kyprianidis.com/p/npar2011/)
addresses noisy/low-contrast regions and large-scale abstraction through a
pyramid. These are stronger candidates than sparse four-quadrant sampling when
the intent is coherent painted regions.

**GPU tradeoff:** a direct neighborhood gather still grows with area. Polynomials
do not make radius free. Try cached orientation and limited sector gathers; use
the multi-scale approach for broad marks rather than thousands of full-resolution
taps. Compare full quadrant statistics as a simpler reference, not a permanent
requirement. Watch flat-region instability, alpha contamination and seams between
levels or tiles. Half-precision variance can lose important small differences.

**Oil first comparison:** the same regional abstraction with mark-related relief
versus deterministic oriented brush stamps on the GPU. Actual stamps better
match the intent of visible strokes than lighting image brightness.
[Lu, Sander and Finkelstein](https://gfx.cs.princeton.edu/pubs/lu_2010_ips/index.php)
demonstrate local stroke placement and textured GPU sprites. Their reported
512² workload is evidence of an approach, not a 12–61 MP mobile qualification.

Stroke rendering adds overdraw, placement, ordering and renderer-integration
work. Bound mark size/density, keep source alpha and avoid changing all stroke
positions on a small edit. Choose the simpler abstraction/relief path only if it
looks convincingly like oil on the test set. Brush Size, Detail and Oil Texture
are enough controls for either implementation; Direction and stroke algorithms
need not become saved fields.

### Watercolor: pigment modulation over simplified washes

**First candidate:** edge-aware color abstraction with multi-scale pigment-density
variation. [Bousseau and colleagues](https://www.matt-kaplan.com/npr/watercolor/watercolor.pdf)
separate abstraction from appearance effects, using texture-driven pigment
variation rather than fluid simulation. Their shader formulation changes colors
in a pigment-like way instead of adding arbitrary RGB noise.

For Capy, compare a bounded color-density response with a simple power/optical-
density-inspired response on saturated washes. Treat their domain and endpoint
behavior carefully; an SDR formula must not silently clip extended paint. Keep
large wash variation and fine grain visually distinct internally, with Wash
Size controlling the wash and Grain the visible deposit texture.

**GPU tradeoff:** simplification dominates cost; noise or sampled grain and
pigment modulation are inexpensive per-pixel stages. Cache the simplified input
when only Grain changes. Source alpha stays intact; watercolor edges and paper
remain reusable effects. Do not add optical flow, time integration or physical
water transport to a static image filter.

**Decision images:** flat colored washes, saturated red/blue, very pale paint,
ink over washes, opaque photos and cutouts. Reject “blur plus noise” if the result
looks dirty rather than painted.

### Pencil / Charcoal: coherent strokes plus tone

**First comparison:** improved ratio/dodge sketch as the fast baseline, versus
a coherent line map with a separate tonal mark texture. The
[Lu, Xu and Jia pencil paper](https://www.cse.cuhk.edu.hk/~leojia/projects/pencilsketch/pencil_drawing.htm)
separates strokes from tone; its full method includes global tone adjustment and
optimization. Borrow that visual decomposition, not an assumption that the whole
published pipeline is a cheap local filter.

For a GPU prototype, compare bounded directional convolutions or the shared
flow-guided line preparation, combined with a small tonal texture bank. Keep
stroke direction automatic. A locally evaluated tone response is the first
choice; use asynchronous global preparation only if it materially improves the
look. Transparent background converts the combined ink density into coverage.

Charcoal should emphasize wider broken marks, broad dark deposits and paper
interaction; give it a different tone/texture response while reusing useful
preparation. Multiplicative grain is a low-cost component, not the entire style.
Test faces, hair, smooth shadow ramps and faint sketches. Detail governs retained
structure, Contrast the tonal range, and Grain the roughness; do not expose the
number of directions, histogram parameters or a solver tolerance.

### Hatching, Engraving, Stipple and Halftone

**Straight hatch/engraving:** start with analytically evaluated periodic lines,
document-space phase and footprint filtering. Vary the active line families or
line width from tone; use a perceptual contrast response. This is a cheap local
starting point. Curved surface-following hatching is a different, more demanding
goal and remains a later enhancement.

**Textured hatch upgrade:** [Real-Time Hatching](https://gfx.cs.princeton.edu/proj/hatching/)
uses nested tonal art maps and mip levels to retain coherent marks across tone
and scale. Capy can test 2D tonal textures for more drawn marks. The paper's
surface parameterization is not available from an ordinary image; do not promise
3D-following strokes from it. Avoid mip transitions that make marks pop or turn
the entire drawing into an unwanted gray veil.

**Stipple first candidate:** progressive blue-noise point sets with density
driven by image tone and explicit Dot Size. This avoids the visible lattice of
a simple jittered grid and distinguishes stippling from a dot screen.
[Recursive Wang Tiles](https://graphics.uni-konstanz.de/publikationen/Kopf2006RecursiveWangTiles/index.html)
provide deterministic local point generation and spatially varying density.
Prototype a small progressive point-tile set first; use a more elaborate tiling
only if repetition is visible. Keep dot size in artwork units: the paper's
zoom-adaptive point-count behavior is not the desired saved illustration behavior.

**Halftone first candidate:** area-calibrated analytic screens with filtered
tone input at the relevant cell scale. Solve pattern-specific coverage, including
overlap/saturation; do not use one circle-radius shortcut for every pattern.
For smooth rotated screens, account for pixel footprint in pattern coverage.
Hard pixel-art output must still use discrete marks at final resolution.

**GPU tradeoff:** regular patterns have cheap independent pixels; stipple needs
bounded nearby sites or batched dots and can become overdraw-heavy in shadows.
Cell-level tone aggregation adds dependencies. Check ramps, dense dark areas,
tiny highlights, repeated tiles and exports at several scales.

### Dither and Reduce Colors: separate palette analysis from mapping

**Dither first candidate:** ordered thresholds and a blue-noise rank tile,
evaluated in stable artwork coordinates. Void-and-cluster is a suitable offline
mask-generation family; [Ulichney's paper](https://cv.ulichney.com/papers/1993-void-cluster.pdf)
describes the construction. A runtime threshold fetch is much cheaper to schedule
than propagating error across the full artwork. Generate/choose the tile once
for the app implementation; do not run its construction while dragging controls.

**Automatic palette first comparison:** GPU-reduced, alpha-weighted color
statistics followed by variance-based splitting, compared with a small bounded
number of weighted k-means refinement iterations. Median cut is a simpler
baseline. [Wu's statistical quantization work](https://experts.mcmaster.ca/scholarly-works/3899043)
is a relevant starting reference for variance-based palette construction.
The Oklab weighting and refinement proposed here are Capy experiments, not claims
that those historical methods already specify them.

Palette extraction is an Update operation. Keep pixel analysis on the GPU;
small authored result publication does not justify a CPU readback of the image.
Set deterministic initialization/ties, handle empty clusters and transparent
images, and compare rare accent colors as well as average color error. A
downsampled image can lose small accents even when its mean error looks good;
compare stratified samples or a bounded histogram before choosing preparation.

**Mapping:** direct nearest-palette search is a good baseline for small palettes;
larger palettes may need an acceleration structure or palette-index lookup.
Interpolating a color LUT can invent colors outside the palette, so final output
must still select a palette entry. Oklab distance helps perceptual matching;
dither mixture proportions need their own tone/gamut assessment. Preserve alpha
independently unless an explicit alpha-screening mode is selected.

### Border, shadows, glows, bevel and line width

**First building blocks:** continuous alpha morphology, normalized blur,
directional displacement/sweeps and, where useful, distance fields. Keep the
coverage pipeline shared; use different primitives where their semantics differ.
Use alpha-only intermediates until color is needed to reduce bandwidth.

For distance, compare [jump flooding](https://www.comp.nus.edu.sg/~tants/jfa.html)
with [parallel banding](https://www.comp.nus.edu.sg/~tants/pba.html).
Jump flooding is approximate and normally needs a sequence of decreasing jump
sizes; its speed is not an exact-distance guarantee. Parallel banding computes
exact Euclidean distance for binary samples, with additional implementation work.
Neither automatically gives continuous subpixel coverage contours. A CUDA result
does not establish a portable WGSL implementation or mobile speed.

**Border/line width:** direct local morphology is an inexpensive small-radius
baseline; large radii need better methods. The
[van Herk algorithm](https://research.manchester.ac.uk/en/publications/a-fast-algorithm-for-local-minimum-and-maximum-filters-on-rectang/)
reduces min/max work for rectangular or decomposable kernels. It does not make
an exact circular grayscale disk separable. Compare decomposition error on
corners/diagonals, or use a suitable distance-based method where a shape boundary
is actually the right model. Preserved thinning remains its own prototype.

**Shadow/glow:** blur and move coverage, then compose generated marks with the
original using the chosen policy. Reuse the Gaussian implementation where Blur
means Gaussian sigma. Spread/choke require defined coverage growth. Inner styles
must operate within the original alpha. Cache coverage preparation across
color/opacity edits.

**Long Shadow:** investigate directional prefix/sliding reductions or hierarchical
segment combinations instead of sampling the entire length at every output
pixel. This is an engineering candidate; fractional directions, fade and partial
alpha composition may prevent reuse of a simple max reduction. Compare against
a straightforward directional sweep reference before calling it equivalent.

**Bevel/Satin:** a reusable distance/height/normal path fits bevels; displaced
blurred alpha fits an initial satin look. Match the intended rim/fold appearance,
with no unintentional whole-shape darkening or coverage change. No need for a
general lighting engine or artist-facing contour editor.

### Remove Dust: bounded classification, GPU connectivity

**First candidate:** size-bounded connected-component classification with
explicit boundary rejection, followed by bounded fringe cleanup or hole filling.
Use overlapping tiles large enough to decide eligibility, and classify continuing
components as protected. This can keep small cleanup work local, but Size and
fringe support must be accounted for together.

[Block-based GPU union-find](https://www.federicobolelli.it/media/publications/pdfs/2019iciap_labeling.pdf)
is a concrete research basis for GPU connected components. It motivates parallel
labeling, not a claim that every union/atomic pattern ports directly to WebGPU.
Compare with fixed-window component exploration at small allowed sizes; worst
case branching and repeated work can dominate a naïve local implementation.

Do not substitute a median filter or morphological opening and call it the same
operation: those can alter long connected strokes. Test diagonal links, holes
touching tile borders, faint fringes, multicolored ink and huge connected shapes.
Cap memory and work per dispatch; no CPU flood fill over canvas pixels.

### Lens, Zoom and Spin Blur

**Lens first candidate:** normalized deterministic aperture gather with
multi-resolution acceleration for broad radii. Use a high-quality gather as the
reference. Test a complex-separable disk approximation as an alternative for
Round only. [Wronski's implementation analysis](https://bartwronski.com/2017/08/06/separable-bokeh/)
shows the ringing and extra intermediate storage involved; more components trade
quality for bandwidth. This is not a free substitute for all polygon kernels.

[AMD FidelityFX DoF](https://gpuopen.com/manuals/fidelityfx_sdk/techniques/depth-of-field/)
uses strategies including multi-resolution sampling and ring merging. Those
ideas are relevant, but its depth-aware game renderer, API requirements and
assumed image inputs are not drop-in matches for Capy's uniform transparent
layer filter. Avoid frame-random sampling or temporal accumulation that makes
static exports disagree with live previews.

**Zoom/Spin first candidate:** deterministic gathers along the intended radial
path/arc, with sample spacing tied to image distance and a hierarchical input
representation for large motion. The shared renderer already has linear sampling
and Motion Blur to reuse as building blocks. A fixed tiny tap count aliases long
arcs; a naïve sample for every traveled pixel scales badly. Preserve normalized
color/alpha, verify the center singularity, and transform the dependency window
for the actual path. Do not add a technical quality slider to fix undersampling.

### Crystallize, Pixel Mosaic and displacement

**Crystallize first candidate:** bounded jittered-grid Voronoi assignment with
cell-color reduction. Regularly bounded sites suit local evaluation better than
repeated global centroidal relaxation. Compare representative average coloring
against site sampling for cost/quality. Account for alpha in both reduction and
final output. Jump flooding is an alternative for an explicitly stored arbitrary
site set, not automatically faster than a short proven neighbor search.

**Pixel Mosaic:** aligned per-cell GPU reductions for Average; existing sampled
Center mode stays available. Cache cell statistics until the input/size/phase
changes. Use source-alpha-aware sums and a clear partial-cell policy. A global
summed-area image is another option, but its preparation/invalidation and numeric
precision can outweigh benefits for small local edits.

**Displacement:** a data-texture lookup plus an appropriately filtered source
lookup is the direct GPU approach. Strong/minifying warps need footprint handling
to avoid aliasing; input edge policies and raw data decoding matter more than a
complicated artistic algorithm. Reuse the transform/sampling infrastructure
without color-managing the map as paint.

### Color Transfer, paper, patterns and basic conversions

**Color Transfer:** start with alpha-weighted channel moments and a restrained
perceptual affine match, comparing chroma-focused and full matching. The
[Reinhard paper](https://home.cis.rit.edu/~cnspci/references/dip/color_transfer/reinhard2001.pdf)
motivates mean/spread transfer; the Oklab variant here is an adaptation. Global
moments are GPU reductions; applying the retained match is cheap. More elaborate
distribution transport or semantic style transfer is not the first prototype
for a small familiar Reference/Strength/Match interface.

**Paper:** sample a reusable grain/height image, with optional local gradients
for relief. Preserve coverage; grain should modulate paint rather than expose a
rectangular paper patch. Compare simple texture modulation with lighting only
when the selected texture benefits from it. Texture, Size and Strength are enough.

**Pattern:** repeat a retained image under an affine pattern transform. Use
premultiplied, color-correct filtering/mips and separate filled versus source-
clipped output. Reuse that path for bundled image patterns.

**Procedural fills:** analytic stripes/checker/grid and seeded gradient/value
noise with a few internal octaves for clouds. The
[GPU Gems noise chapter](https://developer.nvidia.com/gpugems/gpugems2/part-iii-high-quality-rendering/chapter-26-implementing-improved-perlin-noise)
discusses computed noise versus texture storage. Benchmark modern WGSL choices
instead of copying its old hardware restrictions. Filter frequencies too fine
for the pixel footprint without moving the underlying pattern; keep octave
counts, hash variants and technical noise names out of saved controls.

**Threshold/Brightness to Opacity:** direct per-pixel conversion with explicit
color domain and alpha semantics. Existing grayscale operations remain distinct.
No elaborate algorithm is necessary.

**Color to Alpha:** reconstruct remaining foreground color and coverage so the
chosen background's contribution disappears from mixed edges. Compare against
recompositing the result over the removed color, allowing the intended Tolerance
behavior. The [GIMP manual](https://docs.gimp.org/3.0/en/gimp-filter-color-to-alpha.html)
illustrates why color removal needs more than making nearby colors transparent.
Use a direct per-pixel solution where possible; test saturated and extended colors.

## Per-filter performance and incremental rendering

Every delivered filter must pass the
[live-filter performance gate](../performance/measuring.md#live-filter-performance-gates),
including painting **into its changing input**, incremental correctness,
algorithm efficiency, memory and responsiveness. Fast/Medium/Slow describe
expected cost and tolerance for slower feedback, not permanent artistic contracts
or controls shown to the user. Every class still aims for the tier rate.

Cheap filters aim for the normal target at ordinary settings. When the matched
disabled baseline already misses, an efficient filter can pass
[baseline-relative acceptance](../performance/measuring.md#acceptance-when-the-disabled-baseline-misses):
measure a small, algorithm-explained increment without unnecessary regeneration,
and defer the shared baseline gap. Record absolute misses and added
throughput, age, gap, settle/resume and memory costs; this is not a tier-target
pass or a claim of global algorithmic optimality.

Otherwise, a slower result requires the gate's hardware model and comparison
with adequate alternatives. A class name grants no exception. Stale redraws do
not count as fresh output; preserve native detail and every authored input.

### Implementation pitfalls

1. **Alpha behavior is not a global dependency.** Do not route every coverage
   change through full-image staging. Keep pointwise damage local; evaluate
   nonlinear conversions on native samples before reducing their output.
2. **Algebraic equivalence is not numerical equivalence.** Averaging first or
   rewriting conversion math can change threshold classes and faint coverage.
   Check native color/alpha, extended values, masks and signed spatial boundaries.
3. **Warm damage is not cold initialization.** Retain initialized output and
   track freshness separately. Initialize cold pages fully; update warm reduction
   cells and old/new prediction damage only. Keep islands separate and account
   for every writer.
4. **Bound passes as well as pixels.** Per-page materialization and flushes can
   dominate local math. Reuse admitted output and bounded contiguous batches;
   test Fit, 100%, islands and both sides of batch limits with actual pass counts.
5. **Avoid redundant intermediates and inputs.** Fuse paint, effects and backdrop
   only where original-input, mask, ordering and lifetime rules permit it. Do not
   prepare reduced sources that a native-only consumer never reads.
6. **Count the optimization's own work.** A uniformity rescan adds reads before
   fallback reduction. Compare complete workloads, track changes during required
   writes where cheaper, and delete failed alternatives instead of layering them.
7. **Bound resource lifetime through teardown.** Pool counts do not bound driver
   bytes. Reuse only completed work, budget idle refinement, let new input resume,
   and finish retirement before device destruction. Test long contacts and idle.
8. **Measure what the artist receives.** Match source/build hashes, raw timing
   records and actual process exits. Distinguish physical passes from logical
   effects, processed from resident pages, and fresh ink from repeated output.
   Wait for required pipelines and fresh output; report startup, queue age and
   resume separately from warmed shader time.

### First-pass filter performance classes

These are design expectations, not benchmark findings. “Ordinary” means the
initial/default look and the main useful slider range, not zero strength or a
tiny handpicked radius. Before prototyping, record concrete ordinary and demanding
values for each mode; cover them on every tier. A more costly mode gets its own
row and explanation. Never lower the expectation of a cheap mode to match a
slower sibling. Large admitted values need a measured envelope, not silent
clamping to the slider maximum.

| Filter / mode | First-pass class | Why artists should expect this speed | Incremental strategy and expensive cases to qualify |
| --- | --- | --- | --- |
| Grayscale / improved Threshold / Decrease Color | Fast | Basic value conversion should feel immediate. | Pointwise mapping of changed pixels; fuse compatible adjustments. Binary alpha adds no global analysis. |
| Brightness to Opacity | Fast | A simple paper-removal conversion should follow every stroke. | Pointwise color/coverage conversion. No whole-layer scan. |
| Color to Alpha | Fast | Removing a chosen color should feel like a color adjustment. | Pointwise reconstruction; include extended-color and tolerance handling in the cost. |
| Halftone / Tone | Fast | Drawing with a screen should feel like drawing with ink. | Stable analytic/rank patterns; update affected density cells and their outputs. Image-density aggregation and unusually large cells need separate rows. |
| Dither, ordered or blue noise | Fast | A fixed screen is expected to update immediately. | Local rank lookup and color choice; retain rank assets. Error diffusion is outside this live gate until separately designed. |
| Reduce Colors, fixed or stored automatic palette | Fast for small palettes; Medium candidate for large palettes | A chosen palette should remain responsive while drawing. | Local mapping/dither with exact palette membership; qualify small and largest ordinary palette counts. Compare an accelerated lookup with a full palette scan. |
| Reduce Colors, Automatic Update | Slow preparation; then the mapping class above | An explicit palette calculation can take time; every stroke should not redo it. | Cancellable global reduction/quantization, atomic saved palette publication. Colors/Update changes invalidate mapping; ordinary strokes do not re-estimate the palette. |
| Paper / Canvas Texture | Fast | A paper overlay should not slow the pen. | Retain image/mips; shade changed coverage. Texture import/preparation is separate from the live pass. |
| Pattern Fill / Pattern Overlay | Fast | Repeated patterns behave like fills. | Reuse retained pattern samples; overlay updates changed source coverage. An unchanged standalone fill needs only dependent compositing, not regeneration. |
| Procedural Texture Fill | Fast | Simple stripes, checks and clouds should behave like fills. | Stable coordinate evaluation or retained generated tiles. Seed/scale changes can invalidate the fill; painting on another layer cannot. Qualify clouds' octave cost. |
| Film Grain / random variations | Fast | Grain is an overlay, not a new simulation per stroke. | Local deterministic noise and source modulation; no reshuffling, temporal history or full-image regeneration on input. |
| Pixel Mosaic, Center and Average | Fast | Pixelation is a basic graphic operation. | Center invalidates cells whose sampled input changed; include any per-pixel coverage dependencies. Average updates aggregates for touched cells and republishes those cells. Very large cells can expand damage and need a Medium row if justified. |
| Border | Fast at ordinary widths | Outlines on lettering should track editing immediately. | Continuous-coverage local morphology and cached coverage preparation. Wide borders may need a Medium case; avoid rebuilding an unbounded distance field per dab. |
| Drop Shadow | Fast at ordinary blur/spread | A familiar layer shadow should not make ordinary drawing sluggish. | Local alpha growth/blur, displaced dirty output and composition. Broad spread/blur qualifies separately as Medium; color/opacity edits reuse alpha work. |
| Outer Glow | Fast at ordinary size; Medium for broad glows | A small glow is a familiar layer style; a large glow touches more area. | Reuse alpha growth/blur and correct expanded bounds. Measure large soft halos and stacked glows. |
| Inner Shadow | Fast at ordinary blur; Medium for broad shading | Simple inset shading should follow the shape quickly. | Bounded displaced/blurred coverage clipped once to the source; reuse preparation. |
| Inner Glow | Fast at ordinary size; Medium for broad glows | A soft inner rim should feel like a layer style. | Bounded alpha falloff/blur; source coverage and masks determine damage. |
| Long Shadow | Medium | A long graphic sweep reasonably costs more than a short shadow. | Directional dirty sweep and reusable/hierarchical reductions. Large Length may justify Slow; repeated full-length gathering per output pixel is not the default. |
| Bevel / Relief | Medium | Height, normals and lighting involve more work than an outline. | Bound height/distance/normal preparation and blur support; cache across light/color-only edits. Broad relief needs a separate cost row. |
| Satin | Medium | Interior folds combine multiple spatial stages. | Shifted blurred-alpha dependencies; share reusable blur work and recomposite color changes. |
| Watercolor Border | Medium | A pigment-following rim needs more than a flat outline. | Local coverage/color preparation and rim finishing; test wide/soft rims. Target Fast if ordinary trials show simple morphology is enough. |
| Adjust Line Width, ordinary Thicken/Thin | Fast for small Amount | Small ink cleanup should remain interactive. | Bounded grayscale morphology; reuse unchanged tiles. Wide disks need a separate Medium case and a radius-aware method. |
| Adjust Line Width, Keep Thin Lines | Medium | Stroke preservation adds structural analysis. | A validated bounded preservation method or declared wider ridge/medial dependencies. Long connected strokes expose hidden global work; do not assume an ordinary morphology halo suffices. |
| Remove Dust, Remove Marks / Fill Gaps | Medium | Small-mark cleanup should feel lighter than restyling a painting. | Size-bounded connectivity, fringe and hole-color dependencies. Qualify dense connected ink and boundary rejection; a whole-layer flood fill on each dab does not justify a Slow relabeling. |
| Extract Lines | Medium | Coherent contour extraction can involve smoothing and line cleanup. | Reuse bounded orientation/smoothing/edge stages; invalidate through line thickness and fragment cleanup. Strong abstraction/cleanup needs a separate demanding row. |
| Cartoon / Cel Shading | Medium | Color simplification plus lines is more than posterization. | Cache simplification and edge preparation; banding/color edits reuse them. Lines-off mode must not execute line stages. |
| Hatching / Engraving | Fast | Regular graphic marks should feel like screens. | Analytic patterns with bounded tone input and antialiasing. Curved contour-following hatching is a later algorithm with its own gate. |
| Stipple | Fast for ordinary dots; Medium for dense/large marks | A stable dot pattern should follow tone without an optimization pause. | Local progressive sites or binned dots; update affected tone regions. Count overlap/overdraw and site-query cost; no whole-image point relaxation on a stroke. |
| Pencil / Charcoal | Medium | Coherent drawn marks and tone need several stages. | Reuse bounded lines/tone preparation and retained grain; grain/color-only edits do not rebuild structure. Compare the cheap sketch baseline before accepting a costlier look. |
| Watercolor Look | Medium; Slow candidate for broad washes | Wash abstraction has visible value beyond blur and noise. | Bounded multi-scale simplification and pigment modulation; retain paper/grain resources. Large wash support must justify any Slow case. |
| Painterly | Slow | Large coherent painted regions require substantial smoothing. | Bound tensor/orientation/smoothing footprints and cache by scale. A local edit updates their full composed support, not all canvas tiles. Small Brush Size should approach Medium. |
| Oil Paint | Slow | Convincing brush marks and relief can require expensive preparation or overdraw. | Local abstraction or spatially binned marks with deterministic ordering; regenerate only affected marks/tiles. A whole-canvas stroke restamp per dab is wasteful. |
| Crystallize | Medium | Irregular cells need neighborhood search and representative color. | Keep site layout stable; update touched cell aggregates and complete affected cells. Derive search and cell extents from maximum jitter; do not re-sum each cell for every output pixel. |
| Zoom / Spin Blur | Medium; Slow for long paths | Long radial paths legitimately read distant content. | Backward sample paths and forward dirty influence, with hierarchical inputs where useful. Small local input damage can still affect a large output region; measure that region honestly. |
| Lens Blur | Slow for broad bokeh; Medium for small radii | Broad shaped highlights need more work than an ordinary soft blur. | Aperture-support dirty expansion, normalized gather and bounded multi-resolution preparation. Compare adequate approximations; do not claim every pixel needs a brute-force aperture scan. |
| Displacement Map | Fast for modest amounts; Medium for broad/minifying warps | A retained map usually means a small number of lookups. | Retain the raw map; source edits invalidate its possible preimage, map edits invalidate the mapped output. Large displacement expands dependencies even when sampling is cheap. |
| Color Transfer, saved match | Fast | Once matched, painting should feel like a color adjustment. | Apply the saved LUT/transform only where source pixels changed. Never re-match on ordinary painting or on open. |
| Color Transfer, Match | Slow preparation; then Fast | An explicit reference match can be a background job. | Cancellable source/reference reductions, mapping construction and atomic publication. Reference import and analysis have separate completion budgets. |

Preset application takes the cost of its actual filter stack, including cold
resource preparation. Separate Lines and Tones is an explicit creation job;
subsequent painting through live outputs inherits their combined gate. Neither
workflow is a free performance class or a reason to execute identical source
preparation twice. Qualification includes useful combinations such as Border +
Drop Shadow, Extract Lines + Halftone, and Pencil + Paper; two separately passing
filters do not automatically make a passing stack.

### Dependency and cache design before shader selection

Start each prototype with the dirty-region plan. Record the forward affected
output and backward required input for every stage, cache keys and invalidating
controls, whether analysis is local or global, and the worst admitted working
set. Reuse the renderer's existing
[pass dependencies](../../crates/layer-render-wgpu/src/effects.rs) and
[bounded windows](../../crates/layer-render-wgpu/src/scene/windows.rs), extending
them where required for directional/mapped footprints and retained off-frame
sources. Bounded windowing is a memory mechanism, not proof of good incremental
cost or an excuse to keep current frame clipping.

- **Pointwise:** repaint changed input/coverage regions and fuse compatible work.
  Static pattern fills and imported texture preparation survive unrelated strokes.
- **Neighborhood pipelines:** propagate support through every pass and update only
  the necessary intermediate regions. Changing a finishing control reuses valid
  preparation. Finite dependencies can still be expensive when halos overlap.
- **Cell-based effects:** invalidate all output pixels whose shared cell statistic
  changed; update/reduce each cell once. Region-only output is incorrect when the
  rest of that cell still displays the old average.
- **Warped/directional effects:** derive both sample requests and dirty influence.
  A displaced source edit cannot be handled by copying its unshifted dirty box.
- **Global/derived data:** retain explicit palette/Match results. If an artistic
  algorithm truly requires automatic global analysis, declare it, keep its
  preparation cancellable, and measure a valid update strategy; do not pretend
  that its low-resolution guide has local dependencies.

For an output tile of side `T` and a local support radius `r`, one required
input window has area `(T + 2r)^2` before clipping to true source bounds. At
`T=256`, `r=16` expands input area by about 1.27×; `r=128` expands it by 4×.
This is input area for an already selected output tile. Expanding a source edit
into affected output is a separate step; stacked passes compose both footprints.
Measure duplicate halo work and tile/dispatch overhead before choosing a tile
size. Reuse overlapping preparation where worthwhile; never eliminate required
support to improve the timing.

### First hardware estimates and reject criteria

Use the cost model and efficiency checks in the
[measurement guide](../performance/measuring.md#hardware-cost-and-algorithm-efficiency).
The figures below use the exact tier canvas dimensions and planning peaks from
[the hardware sheet](../performance/hardware.md). They assume all listed bytes
reach external memory and arithmetic reaches the listed FP32 ceiling. They are
optimistic lower bounds for those stated assumptions, not runtime forecasts.

| Hypothetical whole-canvas work | Low | Mid | Top |
| --- | ---: | ---: | ---: |
| One RGBA16F read + write, 16 bytes/pixel | 13.37 ms | 22.46 ms | 14.34 ms |
| One RGBA32F read + write, 32 bytes/pixel | 26.73 ms | 44.91 ms | 28.67 ms |
| 100 ordinary FP32 FLOPs/pixel, arithmetic only | 13.22 ms | 18.75 ms | 3.56 ms |

Do not add the arithmetic and traffic rows as though they cannot overlap, or
assume every tap fetches uncached DRAM. Conversely, a bilinear lookup, atomic or
transcendental is not a single ordinary FLOP. The current RGBA32Float page format
cannot be budgeted as RGBA16F without a validated intermediate-format change.
Precision reduction must preserve the relevant color/coverage contract.

For contrast, a single 256×256 output tile with one RGBA16F read + write moves
about 1.05 MB, an ideal 0.073 ms at the low tier's peak bandwidth, before halos,
other stages, dispatches and painting/compositing. That difference explains why
pointwise/short-support filters should be Fast despite a large canvas. A naïve
whole-canvas implementation being bandwidth-bound is a reason to replace it,
not evidence for a slower class. A tile count alone does not predict FPS either.

All canvas pixel processing stays on the GPU. Shared Rust owns controls,
validation, history and scheduling. Use asynchronous preparation with bounded
caches and revision-safe publication; retain only authored results in artwork.
No CPU canvas fallback, required subgroup-only/CUDA path, synchronous GPU
readback or unbounded refinement queue belongs in the painting path.

Accept a slower artistic implementation only when its useful visual result,
correct dependency work and measured cost justify it against simpler candidates.
Reject avoidable full-frame rebuilds, redundant passes, repeated per-cell sums,
large brute-force kernels with adequate cheaper alternatives, and unexplained
cost beyond the calibrated model. Hitting a relaxed FPS floor does not excuse
waste. Conversely, a slightly costlier method can be justified by better quality,
less memory or lower latency; record that concrete tradeoff.

### Per-filter release evidence

Before a filter ships, its tier rows must identify the filter/mode and values,
source graph, brush/size, input and affected-output areas, actual evaluated
resolution/format, pass/tap/operation/byte counts, retained and peak memory,
calibrated prediction, measured fresh-filtered rate/age/gaps, ordinary and broad
settling, and interrupted-input behavior. Include the disabled baseline and a
simpler adequate candidate. Record the verdict as target met, qualified slower
filter, incremental efficiency accepted with the baseline gap deferred, or open
failure; keep raw traces in `artifacts/`.

Validate incremental/full equivalence and preview quality independently from
speed. Include source/mask/seed/resource edits, undo, tile crossings, negative
coordinates, retained off-frame content, and cache pressure. Hardware-qualified
ordinary settings do not prove the entire accepted numeric domain is fast. Record
expensive admitted cases explicitly; they must remain correct and responsive.

If a filter misses its class expectation, first fix dependency waste, caching,
shader scheduling or algorithm choice. A justified Medium/Slow exception follows
the shared gate; an unexplained filter regression or unusable feedback beyond
the accepted baseline gap invokes the release cut rule. No new filter is
performance-qualified by this design document.

## Revised GA work and acceptance

1. **Implement the settled shared foundations.** Apply the scoped artistic
   policy to tests, implement object-layer image records/roles and off-frame
   evaluation, saved palette results, exact seeds and crop-compensated spatial
   references. Establish silhouette ownership and per-input invalidation.
   Do not add unimplemented future fields or a shader-version archive. Define each
   filter's expected performance class, dependency/invalidation plan and hardware
   cost model before selecting its production algorithm. Extend the painting
   benchmark to attribute fresh filtered results from a changing filter input.
2. **Close existing correctness and usability gaps.** Improve Halftone, Threshold,
   Pixel Mosaic, Edge Detect/Extract Lines, Pencil, Painterly and Crosshatch.
   Build the independent edge/alpha/coverage examples that will also exercise
   new styles. Remove superseded implementations as replacements land.
3. **Deliver core illustration styles and cleanup.** Border, Drop Shadow,
   Watercolor Border, alpha conversions, line width, dust, palette/dither and
   glow/inner/long-shadow/bevel. Develop risky preservation/connectivity work
   early even when its UI ships later in the sequence.
4. **Prototype and deliver the full artistic set.** Cartoon, Pencil/Charcoal,
   Painterly/Oil, Watercolor, paper/pattern/procedural fills, hatching/engraving/
   stipple, Crystallize and Satin. Compare the researched candidates on one
   representative image set and reference hardware. Keep the best result behind
   the agreed small controls, with later improvement explicitly allowed.
5. **Deliver image-driven and motion/lens filters.** Displacement, Color Transfer,
   Zoom/Spin and Lens Blur; resolve their resource/global/spatial dependencies.
   Add presets and Separate Lines and Tones using the same effect definitions.
   Apply the explicit release cut rule to any family still failing qualification.
   Record a deferral and its missing evidence; do not ship a placeholder or silently
   remove a filter from the release list.

For every filter, keep a concise behavior note that states input/output coverage,
main controls and units, coordinate/edge behavior, source dependencies, neutral
states, authored resources/results, and tested operating ranges. Record the
current algorithm, performance class and qualified operating envelope as
implementation documentation. Apply the fresh-filtered drawing, incremental
correctness and efficiency gates above to every new and improved filter. Precise
utility operations get mathematical references; evolving looks get visual intent and
quality examples rather than permanent equations for every aesthetic choice.

Update the actual [built-in data contract fixture](../../crates/layer-core/src/package/codec/fixtures/builtin-contracts.json)
for saved IDs, types, choices and units, not hidden kernel details. Add non-default
saved values, hidden-mode retention, resources and seeds to fixed package
fixtures. Exercise save/reopen, duplication, preset application and undo; missing
resources or keys must not silently invent a different artwork.

Use independent numerical/geometry references for stable primitives and tests for
invariants such as alpha, palette membership, endpoints, bounds, incremental/full
equivalence and tile independence. Use hardware renders and a curated visual set
for artistic review. An intentional artistic improvement may update those visual
baselines with before/after evidence and a reason; it does not waive invariant
failures. Shared-helper changes require reviewing every affected filter, but not
automatically changing every filter's saved-data version.

The visual set should include opaque photos, comic ink, colored line art,
translucent washes, flat fills, gradients, saturated/extended colors, paper scans,
tiny islands/holes, diagonal strokes, hairlines, distant/off-frame content and
several scales. Include light/dark host journeys and all applicable checks from
the repository guides. Keep source/input snapshots for fair comparisons.

**Qualification status:** this design supplies behavior, format decisions and
researched prototype directions. It does not establish implementation, GPU
baselines, host journeys or tier performance. Record that evidence with the
implementation using the repository's measurement and testing guides.
