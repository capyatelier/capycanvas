# Photo editing: deferred work

[Developer guide](README.md) · [Photo editing research](../history/photo-editing-research.md) · [Performance targets](../PERFORMANCE_TARGETS.md)

The remaining photo epic is deferred. Resume individual features from this list
against current code; the historical research is a design record, not a current
inventory. P numbers identify the former M5–M6 execution sequence; research IDs
identify the broader M7/M8 backlog. They do not require separate builds or commits.

## Delivered foundation

GTK, Web and Android have the P01–P30 geometry and color implementation:
pixel-tight bounds, retained perspective/Warp, explicit transform baking,
group transforms, precise Properties controls, sampled White Balance and
Levels/Curves, Histogram and RGB Waveform, pointwise and local adjustments,
3D CUBE LUTs, Gaussian sigma through 85 and unified multi-stop gradients.
Export Again adds the repeated-export action from P44–P45 independently of the
remaining export redesign. Feature delivery does not close performance gates.

Current behavior belongs in [image commands](../ui/image-commands.md),
[transform controls](../ui/canvas-action-bar.md),
[numeric controls](../ui/numeric-controls.md),
[color sampling](../ui/color-picker.md),
[runtime filters](../reference/runtime-filters.md) and
[export](../ui/color-management.md#export-again).

## Inspection, presets and export

| Former milestones | Deferred feature |
| --- | --- |
| P31–P32 | Info panel and persistent document color samplers, then Web/Android interactions. |
| P33–P34 | Effects, Print Proof and SDR Before/After, split views and momentary comparison. |
| P35–P37 | Cross-document copy/paste of one effect's settings and a local saved-preset library. |
| P38–P39 | Authoritative shared export recipe controls; Long Edge, Short Edge, Percent and Megapixels size modes. |
| P40 | Bounded output sharpening after resizing. |
| P41–P43 | One frozen source per export dialog, actual encoded file-size calculation and artifact reuse on GTK/Web/Android. |
| P46 | Combined workflows, resource lifetime, current hardware qualification and retirement of superseded paths. |

**Info and samplers.** Reuse the bounded GPU artwork-query path. Store up to ten
stable-ID sample positions, sample widths and readout choices in the drawing;
values and active selection remain transient. Dragging makes one undo item and
Cancel restores the original. Whole-image geometry commands transform positions;
layer transforms leave them fixed. Readouts include named document RGB, linear
RGB/EV, OKLCH and alpha, retaining signed/HDR values. Distinguish pending, empty
and outside-canvas results. Query only while Info or a sampling gesture needs
values. Keep Info as an ordinary secondary panel beside Navigator/Proof, with
shared actions and native mouse, pen, touch and keyboard input.

**Comparison.** Keep Before/After entirely in view state, outside history and
export. Effects Before bypasses adjustments while preserving fills, geometry,
masks and blending. Proof/SDR compare existing output transforms. Use Before,
After, vertical and horizontal split layouts, initially vertical at 50%.
Divider motion reuses a bounded display-resolution pair with shared source
backing and animation time; it must not render the whole drawing twice per move.
Momentary comparison has no default shortcut and releases on key-up, blur,
modal opening or cancellation. Editing exits comparison. Keep HDR surface
selection correct for both branches; Navigator and numeric samples retain their
ordinary source meanings.

**Effect settings and presets.** Copy one complete effect instance, including
tagged colors, curves, gradients and LUT resources. Built-ins retain stable
filter IDs, parameter-data versions and every keyed value; current code and
controls come from the bundled catalog. Custom effects retain their immutable
program. Paste requires an unlocked target and changes only the effect, with
one undo item. Numeric values retain destination-working-space meaning; tagged
colors retain their defining space. Validate accepted bounds independently of
slider ranges, plus shaders and resources, before publication or application.
Custom programs compile separately, so matching IDs or WGSL declarations may
coexist; built-in IDs stay reserved. Catalog changes affect future insertions,
not stored presets or existing custom applications. Refusal leaves source and
destination intact. Follow the [package contract](../reference/capy-package.md).
The window-owned clipboard survives tab changes and source closure without
retaining live layers. A local library reuses Filter
Types and its bounded preview cache; no new library browser or Settings section.
The proposed library bounds are 64 entries and 64 MiB, with unique trimmed
1–80-character names. Revalidate under the existing host storage lock, preserve
old data on failed writes and reject stale target completions.

**Export controls.** Extend the existing ExportDraft/ExportForm and worker,
removing host recipe assembly as controls migrate. Add checked sizing with
half-up rounding for edge/percent modes and a pixel-budget-safe megapixel mode;
keep codec limits and the 32768-axis bound. PPI changes metadata only. Preserve
current color/depth/metadata normalization. Revisit ordinary Export's per-drawing
recipe defaults separately; Export Again already retains its concrete recipe.

**Output sharpening.** Proposed choices are Off/Low/Standard/High, default Off.
Apply a seven-tap Gaussian unsharp mask after the linear-premultiplied row
resampler, before tone/profile conversion, quantization and gain-map splitting:
sigma .75 output pixels, radius 3, amounts 0/.25/.5/1. Blur RGB and coverage
together, sharpen straight RGB, then restore original alpha. Preserve finite
extended values and admit at most seven input rows plus one output row. Off
keeps the exact bypass. Qualify transparent edges, HDR and codec passes before
exposing the control.

**Frozen export and file size.** Preview, size calculation and publication share
one immutable artwork revision/time per dialog. Calculate File Size performs an
actual metadata-complete encode on demand; Export reuses the result only while
owner and complete recipe match. Recipe changes invalidate prepared output;
slider motion does not trigger full encoding. One running preparation and one
replaceable pending recipe bound the work. Reuse GTK/Android temporary files
and Web worker OPFS, existing memory admission, cancellation and stale-job
cleanup. Never open or truncate the destination during preparation. Existing
Export Again continues to capture fresh artwork for each invocation.

## Masking and compositing: M7

| Research IDs | Deferred result |
| --- | --- |
| SEL-6 | On-canvas Refine Edge, uncertain-edge matting and optional color decontamination; output to selection, mask or a new masked layer. |
| SEL-7 | Edge-aware quick selection through the existing painted-selection tool. |
| SEL-8 | Channel selections, Color to Alpha and Select Similar. |
| LYR-3 | Blend If / Blend Ranges with matching cached, fused and exact composition. |
| LYR-4 | Mask density and live feather through the existing mask controls. |
| ADJ-7 | Large-radius, lens and tilt-shift blurs; the delivered Gaussian range is not this feature. |
| ADJ-8 | Noise Reduction, Median, Dust & Scratches and Smart Sharpen. |
| T-10 | Live Wand/Select by Color tolerance changes from a saved baseline, amending one undo step. |

## Advanced workflows: M8

| Research IDs | Deferred result |
| --- | --- |
| RET-5, RET-6 | Local content-aware removal/fill, Patch and content-aware move. |
| RET-8, RET-9 | Blur/sharpen brushes and History brush; Revert to Original already exists. |
| LYR-6–LYR-9 | Align/distribute/auto-align, stack modes, layer styles, panorama and focus merge. |
| IO-4–IO-6 | Batch processing, layer/selection export, RAW hand-off and New Drawing from Files. |
| ADJ-11, ADJ-12 | Remaining lens corrections and Match Color; vignette removal already exists. |
| SEL-9 | Subject/Sky selection research using on-device models with acceptable licenses, size and cost. |
| VIEW-4, VIEW-5 | Additional guides/grid/overlays and the History panel. |
| T-12, T-19, BAR-8 | Sampling below for Smudge/Blender, exposed brush blend modes and a decision on a layer action bar without a selection. |

Live Liquify and Reconstruct remain deferred. Preserve the existing baked tool;
a future live version is an effect, not another retained-placement variant.
Per-layer linear blending in Perceptual drawings and compact constant-color
storage are separate deferred decisions. RAW development and generative cloud
features remain outside this epic's agreed scope.

## Apple and Windows follow-up

Re-audit existing implementations before assigning ports. Shared Rust and bridge
compilation do not prove native parity; several Windows controls already exist.

| Former packets | Remaining parity/qualification scope |
| --- | --- |
| F01/F08 | Retained geometry, group transforms, bake and input/lifecycle controls. |
| F02/F09 | Apple: Properties pages, precise Curves, calibration, targeted adjustment and scopes. |
| F03/F10 | Apple: unified gradient editor and LUT file import. |
| F04/F11 | Info, persistent samplers and comparison after their shared implementation. |
| F05/F12 | Effect clipboard and saved presets after their shared implementation. |
| F06/F13 | Export recipe controls and output sharpening. |
| F07/F14 | Apple: frozen export, destination ownership and Export Again. |
| F15 | Retire the remaining Apple legacy Histogram route and qualify every supported host. |

Require macOS mouse/keyboard and physical-iPad pen/touch journeys, and Windows
native D3D12/input/device-loss journeys, in both themes. Keep capabilities honest
until native workflows work; never discard unsupported artwork silently.

## Open qualification and earlier follow-ups

Current measurements belong only in the [tier tables](../PERFORMANCE_TARGETS.md)
and [responsiveness record](../performance/responsiveness.md). The latest
[top-tier color/gradient results](../performance/top-tier.md) still have motion
and completion-latency misses. No hardware ceiling is established.
Reproduce the reported 8-bit gradient banding with its original colors and zoom
before treating that report as closed.

Before closing the epic, qualify active motion, exact analysis/Auto, guide
preparation, export/cancellation, first visible content and continuous process/
driver memory peaks on reference hardware. Include physical stylus input,
input-to-photon latency and low/mid reference tiers. Measure combined
transform/mask → adjustment → comparison/preset → export workflows when the
deferred features exist. Repeated Frequency Separation and merge/undo must
remain bounded; the earlier 24 MP memory gates were roughly 1.5 GB settled PSS,
2.5 GB during the operation and at least 1 GB device memory available.

Reproduce older reports before treating them as current bugs or allocating work:
clipping coverage in Copy to New Layer; transient undo/selection readiness;
hidden-pixel writes; crop-mask trimming; image-command recentering; isolated
group adjustment handling; Eyedropper tool switching; Leave Copy persistence;
disabled-action feedback; contact-brush release taper; IPTC/large XMP writing;
perceptual export-matte/resize and brush-preview consistency; grouped blend
choices; GTK Document Properties/Tool Options; tablet Clone bar positioning;
Android caption clipping, numeric focus and duplicated Crop choice; accessible
labels; Web tablet tile artifacts; and private GTK/Web test-runner limitations.
The [M2–M4 record](../history/photo-editing-m2-m4.md) preserves their original
context. Unrelated baseline failures remain in the [testing guide](testing.md).
