# Layer clipping and effect attachment plan

[Developer guide](README.md)

**Status: accepted design direction; implementation and visual qualification are
pending.** This plan separates paint clipping from effect attachment while
retaining one contextual button. It records the intended behavior, implementation
boundaries and acceptance criteria; it does not describe a completed feature.

The code baseline is `origin/main` at `2e54808f6`, including the authored-model
application cutover in `b3f6f8e51`. Implement against the live
[authored model](../reference/authored-model.md) and
[package contract](../reference/capy-package.md). The remaining qualification in
the [format plan](capy-format.md) is independent of this work. Recheck these
boundaries against current code when implementation starts.

When this work lands, move its lasting contracts into the model, rendering and
UI guides and retain the design rationale in history before deleting this plan,
as required by the [writing guide](writing.md).

## Code baseline

`Document` now owns `Artwork` and `WorkingState`. `Stack.entries` owns sibling
order, occurrences own presentation, and paint/coverage sources own pixels.
`SceneView` and immutable `SceneSnapshot` expose those records to consumers.
There is no editable flat layer array to extend or a future format cutover to
wait for.

| Boundary | Existing implementation and required change |
| --- | --- |
| Authored relationships | [Occurrence](../../crates/layer-core/src/authored/artwork.rs) has one `clipped` boolean. Separate clipping membership from effect attachment without adding a second document model. |
| Target resolution | [Layer queries](../../crates/layer-core/src/layers.rs) resolve the first unclipped artwork sibling, then require it to be paint. Groups cannot currently serve as bases. Clipped effect input includes the lower portion of the clipping run. |
| Scene scopes | [Scene access](../../crates/layer-core/src/authored/scene.rs) includes `SceneScope::Prefix` and `effective_clipped`; partial snapshots and queries depend on the existing clipping interpretation. |
| Composition | [Stack composition](../../crates/layer-render-wgpu/src/scene/stack.rs) walks bottom to top. A clipped adjustment processes the accumulated clipping run; it is not an effect owned by one content occurrence. |
| Alpha | [EffectAlpha](../../crates/layer-core/src/effects.rs) distinguishes preserving and filtering alpha, but the clipped path in [effect shader construction](../../crates/layer-render-wgpu/src/effects.rs) and [effect blending](../../crates/layer-render-wgpu/src/effects_color.wgsl) preserves input coverage. |
| Groups | `Occurrence::passes_through` excludes clipped groups. The compositor isolates a clipped Pass Through group while its stored blend mode still says Pass Through. Remove that mismatch. |
| Structural edits | [Occurrence edits](../../crates/layer-core/src/authored/occurrence_edits.rs) already use typed record batches, preserve placement during reparenting, and protect unrelated clipping bases. Extend this shared planner. |
| UI | [Layer commands](../../crates/layer-ui/src/art_layers.rs) expose one clip action and paint-only alpha lock. [LayerState](../../crates/layer-ui/src/lib.rs) publishes `clipped`, without effect-owner or rail-endpoint metadata. |
| Presentation | [GTK layers](../../apps/layer-linux/src/layers.rs) and [Web layers](../../apps/layer-web/layers.js) show separate per-row clipping bars. Group icons indicate expansion, and Normal is omitted from the shared subtitle. |
| Package | [Artwork record adapters](../../crates/layer-core/src/package/artwork_records.rs) write `capy.occurrence/1` with `clipped`. Nonempty explicit effect `inputs` or `bindings` are currently unsupported; their presence in the grammar does not mean the editor evaluates them. |

Classify effects by `EffectKind`, not just `LayerKind::Effect`. Generators,
including the ordinary Solid Color occurrence named Paper, produce content;
they are not attached filter operations. Preserve their content-side behavior
and keep unsupported target capabilities unavailable. Extending generator target
capabilities is not a prerequisite for the paint/group decisions below.

## Decisions

### 1. Paint clipping uses a common base

Consecutive clipped content occurrences in one stack share the first eligible
unclipped base below them. Attached effect rows belong to their owners and do
not become clipping bases. Hidden members retain their structural relationships.
Paint composites bottom to top, preserving the base's coverage through the run.

```text
Highlights   clipped to Base colors
Shadows      clipped to Base colors
Base colors
```

Highlights can appear anywhere inside Base colors, including outside Shadows.
An empty or hidden Shadows layer does not become a new mask for Highlights.
Use an isolated nested group to clip specifically inside Shadows. There is no
top-first/bottom-first setting or per-chain evaluation-order toggle.

### 2. Group isolation has a persistent visual state

Show a small through-arrow badge on a pass-through folder and a contained folder
silhouette for an isolated group. Expansion remains an independent open/closed
folder state. Display the actual group mode in the existing subtitle, including
Normal as well as Pass Through and other blend modes.

The badge and subtitle describe actual composition. A group must not display
Pass Through while being implicitly isolated by another property. Fit the badge
inside the existing group thumbnail; add no horizontal column.

### 3. Group right swipe toggles pass-through; alpha lock stays paint-only

On an eligible closed group row, right swipe toggles pass-through as a shortcut
to the visible group-mode action. Restore the group's previous isolated blend
mode when returning from Pass Through; use Normal when no other isolated mode
has been chosen. The retained mode belongs to the shared model and survives
undo/redo and save/reopen.

Groups do not acquire alpha lock. Alpha lock constrains edits to existing paint;
it neither propagates automatically to children nor freezes a group's combined
silhouette. Use clipping or masks for those constraints. Group edit-lock remains
available.

Keep the [drag convention](../ui/drag-and-reorder.md): right swipe that closes
Delete only closes it; cancellation and short swipes do not edit; release commits
one undo step. Locked groups and groups whose relationships require isolation
cannot toggle into Pass Through. Native timing and capture remain host-owned;
availability and the transition belong to Rust.

### 4. Attached effects can change alpha and expand coverage

Attachment selects an input object; it does not multiply the result by that
object's original alpha. Blur, glow, border and drop shadow can produce pixels
outside the original painted shape. Preserve the program's declared alpha
behavior: a color adjustment may preserve alpha while a spatial filter changes
it. Do not make every effect create coverage.

An effect returns a complete result image. A shadow effect normally includes the
input and its shadow. At Normal blending, effect opacity and its optional mask
interpolate premultiplied input and result, including alpha:

```text
result = mix(input, filtered_input, effect_opacity * effect_mask)
```

Full strength returns the filtered result once; it does not source-over another
copy of the input. Zero strength or a black effect mask restores the input.
Retain supported non-Normal effect blending, with defined alpha interpolation
and transparent-input tests. Effect masks remain optional and editable.

The owner's content mask applies before its attached effects. Effects may expand
that masked shape. Outer clipping and containing-group masks still constrain
the completed object at their own stages. Bounds, damage propagation, sampling
halos and exact output must include newly created coverage without clipping to
the old content bounds or exposing tile seams.

### 5. Effect chains run bottom to top

The effect nearest its owner runs first; each effect above receives the previous
effect's result. Newly appended processing appears above the existing chain.
Reordering effects changes processing order. A hidden effect is bypassed without
retargeting its neighbors.

```text
Panel order       Processing order
Glow              Paint -> Blur -> Glow
Blur
Paint
```

Use a noncommuting pair of effects to verify ordering; two effects with identical
or order-independent results do not establish this contract.

### 6. One button has contextual wording and imagery

Keep the existing button location. Use Attach below as an umbrella concept in
design terminology, and precise action text in the actual tooltip and accessible
label. Both the selected content type and resolved target determine the text.

| Context | Action wording |
| --- | --- |
| Unclipped paint or eligible group | Clip to {base} |
| Clipped content | Release clipping from {base} |
| Unattached effect | Apply to {owner} |
| Attached effect released to stack scope | Apply to layers below |

Use related clipping and attachment glyphs in that one button. Expose checked
state and the actual target through accessibility. Do not call an attached
effect a clipped effect. User-supplied target names are Fluent arguments, not
concatenated translated fragments. Mixed selections use shared capability rules;
hosts do not infer meaning from row icons.

### 7. Clipping stays left; effect chains link vertically between thumbnails

Extend the existing clipping rail through the run to the bottom of its base's
thumbnail. Its terminal distinguishes the base from clipped members without an
arrowhead or a wider gutter. Effect rows can be crossed by the rail without
being represented as clipped paint. Stop at the group header when the base is
a group; descendants are not additional members of the outer run.

Use a small vertical chain-link glyph in each existing gap between consecutive
attached effect thumbnails, then between the lowest effect and its owner.
An unattached effect has no such link. The glyph may slightly overlap thumbnail
borders if needed for legibility. Do not increase panel width, row height or
thumbnail indentation; do not introduce a right-side gutter, arrowheads or extra
`fx` labels. The glyph is an indicator, not a tiny new interaction target.

This schematic uses `:` for the vertical chain-link glyph:

```text
left rail     content thumbnails
    |          [Highlights]       clipped to Base
    |          [Curves]           attached to Shadows
    |              :
    |          [Blur]             attached to Shadows
    |              :
    |          [Shadows]          clipped to Base
    |          [Base]             rail ends at thumbnail bottom
```

There is no chain link between Highlights and Curves or between Shadows and
Base. The existing horizontal content-to-mask link remains distinct by position
and orientation. Selection/hover may emphasize an effect's chain and owner
border without adding permanent controls.

Evaluate a complement of the accent as the shared relationship color. This is
a color direction, not approval of an untested hue rotation. Derive a semantic
palette role in [theme.rs](../../crates/layer-ui/src/theme.rs), with lightness and
chroma adjusted for custom accents, light/dark surfaces, selection and hover.
Use a neutral fallback for neutral accents where needed. Both relationship
types may share this color; their geometry carries the distinction. Qualify
essential indicators against the [3:1 non-text contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).
Exact glyph geometry and color values remain subject to native-size review.

### 8. Direct targets must be paint or isolated groups

An isolated group has a defined combined image and alpha, so it can be a clipping
base or effect owner. Pass-through groups depend on the surrounding backdrop;
they cannot directly serve as either target in this design. Effects inside a
pass-through group retain the ordinary surrounding-stack scope.

An explicit Isolate group and attach action can perform the mode change and
attachment in one transaction. Do not silently isolate a group or silently
sample the external backdrop as its private image. Switching back to Pass
Through is unavailable while clipping or effect relationships require isolation;
the shared refusal identifies the relationship that must be released.

### 9. Effects process their owner before the owner enters clipping

There are three distinct placements, with deliberately different results.

**An effect on a clipped layer processes that layer alone.**

```text
Highlights   clipped to Base
Curves       attached to Shadows
Blur         attached to Shadows
Shadows      clipped to Base
Base
```

Evaluate Shadows, then Blur, then Curves; composite that result inside Base's
coverage. Highlights independently clips to Base. Neither effect processes Base
or Highlights. Effects between clipped content rows do not process an accumulated
portion of the clipping run.

**Effects on the base run before the upper content clips to it.**

```text
Paint        clipped to Base
Blur         attached to Base
Base
```

Evaluate Base and its content mask, then Blur. Use the blurred result's alpha as
the clipping shape for Paint, and composite Paint over that result. Paint can
appear in the expanded blur region with fractional coverage along soft edges;
Blur does not blur Paint. This order also applies to longer base effect chains.

**Effects on a containing isolated group process the completed clipping.**

```text
Blur         attached to Result
Result       isolated group
  Paint      clipped to Base
  Base
```

Evaluate the inner clipping, apply the group mask, then Blur. This is the way to
blur the finished combination or add an effect outside its clipping silhouette.

Owner visibility hides its whole attached output; hidden owners never cause
effects to retarget. An unattached effect remains a stack operation. Keep such
operations outside an uninterrupted clipping run: place them above the complete
run or use a group. Saved-selection rows do not supply an image or change an
effect's owner; connection metadata must handle their presence explicitly.

### 10. Dragging joins, releases and reorders relationships

| Drop | Result |
| --- | --- |
| Content into a clipping run | Join that run's base. |
| Clipped content into an ordinary stack position | Release clipping. |
| Effect onto an eligible owner's thumbnail | Attach to that owner; preview the exact position in its chain. |
| Effect within its chain | Change processing order. |
| Effect into an ordinary stack position | Become an effect on the stack below. |
| Owner with attached effects | Move its effects with it. |
| Clipping base | Move its clipping run and all member effect chains together by default. |

Preview the resulting rail, links and target before committing. Distinguish
attachment to a group's output from insertion inside its folder using the
existing thumbnail/body drop surfaces, without a new gutter. Keep an ordinary
unattached effect from splitting a clipping run. All structural changes from a
drop form one undo step; cancellation changes nothing.

Removing or moving a middle effect reconnects the remaining chain. Removing a
clipped member leaves the other members on their base. Preserve unrelated
relationships, world placement, masks, locks and selected roots. Grouping,
duplication, deletion and merging must use the same relationship closure;
unavailable partial operations keep their shared refusal until a correct atomic
operation exists.

### 11. Isolated groups can themselves clip down

Render the group's contents and internal clipping, apply its mask and attached
effects, then composite the resulting object into the outer clipping run. The
outer base can be paint or another isolated group. Preserve the group's supported
blend mode; isolation does not mean Normal is its only blend mode.

An unclipped isolated group can anchor a clipping run above it. A group that is
itself clipped joins the lower common base; it does not introduce an additional
immediate-neighbor mask for upper siblings. Use a nested group for that extra
boundary. This keeps group behavior consistent with decision 1.

## Implementation approach

The following representation and sequencing are implementation proposals for
the decisions above. They should be refined against the live code without
changing the artist-facing contracts.

### Shared authored state and resolution

Replace the overloaded `Occurrence.clipped` flag with a typed relationship such
as Independent, ClipToBase or AttachEffectBelow. Allow content-side clipping and
adjustment-side attachment according to kind; reject impossible combinations.
Keep stack order authoritative. A contiguous attached-effect run and its owner
form one content unit, even though their rows remain separate occurrences.

Resolve owners, preceding effect steps, common clipping bases, run boundaries
and structural dependents in the shared scene index at structural publication.
Cache typed handles; do not scan portable IDs or reconstruct layer objects on
each frame or dab. Attachment resolution ignores visibility and non-compositing
selection rows, stops at an unattached stack effect, and never crosses a stack
boundary. Moving a unit preserves ownership; only an explicit attach/release
or drop changes it. Derived owner handles are not a second serialized authority.

Represent group isolation and its retained isolated blend mode coherently so
Pass Through toggling does not lose Multiply or another mode. Define defaults,
record validation and portable spelling together. Remove the old implicit
clipped-pass-through coercion when this representation lands.

Replace clip-dependent boolean query scopes with input scopes that distinguish
an owner's pre-effect content, a particular effect step, a completed owner and
a stack backdrop. Update `SceneScope`, artwork queries, analysis inputs,
reference closure, thumbnails, merge inputs and partial snapshots together.
One resolver supplies both UI targets and rendered inputs.

### Persistence and atomic editing

Extend typed record changes and the known occurrence wire contract together.
Use a record/schema distinction that identifies the new semantics; do not
reinterpret an old clipped adjustment as a local attached effect. Follow the
pre-release rule: add no migration or old-format compatibility reader. Preserve
the existing unsupported-package outcome and original bytes where applicable.
Do not enable arbitrary effect `inputs` merely because the grammar reserves
them; this plan needs structured stacks, not a general node scheduler.

Extend `reparent_occurrence_edit` and related planners to compute the moved unit,
source repair, destination membership, placement changes and resulting selection
before publishing one validated `Edit::Batch`. Preview and commit use the same
planner, with release-time validation against the current document.

A release-to-stack action inside a clipping run needs an explicit placement
policy. Proposed behavior: move the released effect above the complete run in
the same transaction, keeping remaining attached effects with their owner. Show
that destination in feedback. Validate this interaction on GTK before porting;
never leave a structurally ambiguous global effect inside the run.

### Rendering and dependent consumers

Extend the existing stack evaluator to evaluate each content unit's own effect
chain before compositing that unit into its clipping run. Compose attached
effects once. Cache intermediate results by owner, effect order/values, source
and mask revisions, geometry, color domain and captured evaluation context.
Keep unaffected units reusable after a local edit.

Use the existing premultiplied filter path for alpha-changing output. Preserve
pointwise fusion where valid, and the existing image/fusion boundaries for
spatial or analysis effects. Apply each mask and contribution opacity at its
defined stage once. In particular, preserve soft clipping-base alpha and the
existing once-only application of base contribution opacity to its clipping
run; do not repeatedly multiply the base alpha as clipped members accumulate.

Update both tile and display-resolution composition, image-stage inputs and
checkpoints, damage/bounds queries, exact snapshots, Navigator, export and merge.
Audit retouch reference sampling, Frequency Separation and source-aware effect
analysis because they consume clipping/effect inputs. Captures retain immutable
owners and frozen effect phases; later edits cannot retarget an accepted job.
Expanded effects must retain out-of-frame source content and respect the output
frame without baking or copying pixels on the UI/input thread.

### Shared presentation and hosts

Publish resolved relationship kind/target, clipping-rail segments and endpoints,
effect-link neighbors, actual group mode, contextual command copy and swipe
availability through the shared row view. Native clients use their measured row
rectangles to draw those relationships; they do not rediscover chain ownership.
Account for expanded groups, scrolled-out neighbors, non-compositing rows, row
reuse, multi-selection, masks and temporary drag presentation.

Use the shared icon bank and semantic palette. Add all new Fluent messages and
named arguments to every registered catalog using the
[localization workflow](../ui/localization.md#adding-ui-text). Remove the old
fixed-color per-row-only bars and paint/effect-agnostic clip wording with their
replacements. No additional settings are needed for chain order or connector
layout.

## Delivery milestones

Each milestone must build and pass its applicable checks. Do not commit a
partially switched model/codec/renderer boundary. Add a regression that fails
without each behavior change and reuse existing harnesses.

| Milestone | Complete result and acceptance gate |
| --- | --- |
| M1: shared semantics | Change authored relationships, group state, codec/admission, scene scopes, compositor and affected shared commands as one coherent switch. Save/reopen, undo/redo, actual pixels and exact consumers agree on the three decision-9 examples. Paint/group target capabilities are shared. Remove superseded boolean semantics. |
| M2: structural editing | Relationship-aware reorder, attach/release, grouping, duplication and permitted merge/delete operations preserve unrelated owners and placement. A base moves with its run. Preview and commit match, and each completed operation has one undo step. |
| M3: GTK interaction | Implement contextual button copy/icons, group indication and swipe, extended rails and vertical chain links at existing dimensions. Review narrow/wide layouts in both themes with mouse, touch and pen. Follow the existing [GTK-first review gate](../ui/README.md#rules-for-ui-changes) before porting the visual implementation. |
| M4: host parity | Port the reviewed GTK presentation to Web, then Android, Apple and Windows through their shared view/action boundaries. Verify identical targets and pixels, native gesture arbitration, accessibility and retained row updates. |
| M5: qualification | Finish cross-host user journeys, exact-output/capture coverage and affected performance rows on reference hardware. Move completed contracts into current guides and retire this plan only after the acceptance gates pass. |

M1 may adapt host transport to the new shared view so every client still builds;
it does not bypass the M3 visual review. This plan does not authorize implementing
unrelated filter types, general graph editing, automatic recovery or additional
clipping-order modes.

## Validation

### Shared model, persistence and pixels

| Case | Required result |
| --- | --- |
| Multiple paint clips with empty/hidden middle content | All share the same base; intermediate content does not become a mask. |
| Paint above Blur attached to Base | Expanded, fractional blurred alpha clips Paint; Paint itself remains sharp. Compare analytically chosen pixels inside, outside and at the soft edge. |
| Effects attached to a clipped member | Only that member is processed, then outer clipping applies. Base and other clipped members retain their own content. |
| Noncommuting effects | Reordering changes pixels in the expected bottom-to-top order; hiding a middle effect bypasses it without changing ownership. |
| Isolated group as base, owner and clipped member | Internal composition and local effects precede outer clipping; Normal and supported non-Normal modes work, with group/base opacity applied once. |
| Pass-through | Direct attachment/clipping is refused or explicitly isolates; the visible mode matches pixels. Toggling restores the prior isolated blend and is refused when dependencies require isolation. |
| Masks and effect opacity | Zero strength restores input; full strength returns the result; partial strength mixes alpha as well as color. Content masks precede local effects, effect masks gate their result and outer masks apply at their boundary. |
| Soft/tiny alpha and color depth | Independent reference pixels cover U8/U16/F16/F32, supported blend domains, extended RGB and transparent input without dark fringes or repeated coverage multiplication. |
| Reorder/reparent/duplicate/delete/merge | Preserve unaffected bases, owner chains, masks, world placement and immutable source identity. Undo/redo restores order, relationships and group mode together. |
| Scope and capture | Selected/reference input, effect previews, analysis, thumbnails, Navigator, retouch, merge and export agree on the same owner/chain. Accepted snapshots survive later edits and retain animated phases. |
| Dirty bounds | Incremental and forced full renders match across tile seams, document edges, nested groups, changed blur radius and partial snapshots. Expanded output is not cropped to old content bounds. |
| Package lifecycle | Save/reopen preserves relationships and isolated mode; clone/imported immutable resources remain correctly owned. Invalid/dangling relationships fail admission, and unsupported schemas follow preservation policy. |

Extend the existing suites rather than creating parallel harnesses:
[occurrence edits](../../crates/layer-core/src/authored/occurrence_edits.rs),
[package semantic round trips](../../crates/layer-core/src/package/codec/roundtrip_semantics.rs),
[layer queries](../../crates/layer-core/src/layers.rs),
[UI session](../../crates/layer-ui/src/session.rs),
[adjustments](../../crates/layer-render-wgpu/src/tests/adjustments.rs),
[clipping pixels](../../crates/layer-render-wgpu/src/layer_tests.rs),
[pass-through pixels](../../crates/layer-render-wgpu/src/tests/pass_through.rs),
[display composition](../../crates/layer-render-wgpu/src/scene/scale/tests.rs)
and [snapshot consumers](../../crates/layer-render-wgpu/src/snapshot.rs).
Replace assertions whose intended behavior changes, including silent isolation
of clipped Pass Through groups, while retaining their unaffected coverage.

Run the [checks by change type](testing.md#checks-by-change-type) for every changed
crate and host, including shared model/UI tests, hardware GPU tests, GTK/Web
consumer checks, package fixtures and localization checks. New behavior tests
must exist before their names are used as command filters.

### Real user journeys and performance

On every affected host, create a painted base, add an expanding attached blur,
clip paint above it, add another clipped layer with its own effects, and reorder
both chains. Repeat with nested isolated groups, a group clipped to paint and a
group clipped to an isolated group. Toggle visibility, masks, opacity and group
mode; undo/redo; save/reopen; compare export. Verify pass-through refusals and
explicit isolation, including a non-Normal mode restored by the swipe.

Check light and dark themes, custom/neutral accents, selected/hovered rows,
narrow panels, increased text size, collapsed/expanded groups, scrolling and
content/mask thumbnails. Chain glyphs remain legible at native size without
changing panel width or row height. Rail endpoints identify the actual base.
Screen-reader labels identify the relationship and target independently of color.

Follow the [input validation matrix](../ui/drag-and-reorder.md#required-validation-when-implementing)
for mouse, touch and pen: ordinary scrolling, hold pickup, swipe/Delete closure,
drag near list edges, source removal, stale destinations, cancellation and capture
loss. Reserve devices and use private test installs/displays under the
[device rules](devices.md). Physical pen checks remain separate from injection.

Measure painting through attached chains, effect slider motion, navigation with
Navigator, nested-group composition, layer-list scrolling, swipes and drag
previews against [performance targets](../PERFORMANCE_TARGETS.md), following
[measurement rules](../performance/measuring.md). Qualify low/mid/top at their
12/24/61 MP canvases and 60/90/120 Hz targets with at least three 5-10 second
moving gestures. Separate fresh completed updates, presentation, input latency,
settling and memory; check that refinement yields to new input. Record current
hardware results in the tier tables. This plan claims no measured target passes.

## Research basis

These sources support the design direction; they do not establish that every
mixed-stack edge case behaves identically across applications.

| Source | Relevant precedent |
| --- | --- |
| [Photoshop clipping masks](https://helpx.adobe.com/photoshop/using/revealing-layers-clipping-masks.html) and [Procreate layer options](https://help.procreate.com/procreate/handbook/5.4/layers/layers-options) | Multiple clipped layers can share a base. Photoshop also adds a layer inserted into a clipping run to that run. |
| [Clip Studio nested clipping](https://support.clip-studio.com/en-us/faq/articles/20190046) | A nested folder expresses clipping inside an already-clipped shadow. |
| [Clip Studio layer settings](https://help.clip-studio.com/en-us/manual_en/180_layers/Other_layer_settings.htm) and [Krita alpha inheritance](https://docs.krita.org/en/tutorials/clipping_masks_and_alpha_inheritance.html) | Through folders are not ordinary clipping targets; pass-through changes the scope of alpha inheritance. Krita's accumulated-alpha model is distinct from the common-base model chosen here. |
| [Affinity live filters](https://affinity.help/photo2/en-US.lproj/pages/Layers/livefilters.html) | Filters can affect the stack below or attach to a layer/group through drag and drop, with editable masks. |
| [GIMP layer effects](https://docs.gimp.org/3.0/en/gimp-using-layer-effects.html) | Effects belong to a layer and evaluate bottom to top. |

Implementation is complete only when the model, all affected consumers and host
journeys agree on these decisions, required checks pass, and remaining failures
or unmeasured performance rows are reported explicitly.
