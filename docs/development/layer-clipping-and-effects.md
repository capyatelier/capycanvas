# Layer clipping and effect attachment plan

[Developer guide](README.md)

**Status: accepted design direction; implementation and visual qualification are
pending.** This plan separates paint clipping from effect attachment while
retaining one contextual button. It records the intended behavior, implementation
boundaries and acceptance criteria; it does not describe a completed feature.

The code baseline is `origin/main` at `b5c96cbce`, including the authored-model
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
| Incremental filters | [Image stages](../../crates/layer-render-wgpu/src/scene_images.rs) already track input dependencies, parameter-based sampling radii and pass work. Structural changes set a shared reset, and changed effect metadata can invalidate the entire evaluation window. Retain the dependency machinery while narrowing invalidation to the changed chain. |
| Sparse composition | [Frame submission](../../crates/layer-render-wgpu/src/lib.rs) permits its sparse `composite_tiles` path only when every effect is nonanimated and has no image boundary. An unrelated spatial effect therefore disables this path; transformed paint can then fall back to full-document damage. Attachment alone does not fix this. |
| Display caches | The [display graph](../../crates/layer-render-wgpu/src/scene/scale/graph.rs) invalidates retained pages by dependency damage and expands filter footprints. Its damage unions and image-stage changes use enclosing rectangles, which can include untouched space between distant edits. Preserve sparse regions through the affected paths. |
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

On an eligible group row, right swipe toggles pass-through as a shortcut
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
retargeting its neighbors. Hiding an owner also hides its attached effects while
preserving each effect's own visibility setting. The layer list shows this
inherited hidden state with a dimmed eye; showing the owner restores effects
that were not individually hidden. Hidden paint remains the owner. Each attached
effect chain and its owner form a contiguous unit; saved Selection rows remain
outside it.

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
thumbnail. Keep the rail straight, with no terminal notch, foot or arrowhead.
Effect rows can be crossed by the rail without
being represented as clipped paint. Stop at the group header when the base is
a group; descendants are not additional members of the outer run.

Use a small vertical chain-link glyph in each existing gap between consecutive
attached effect thumbnails, then between the lowest effect and its owner.
An unattached effect has no such link. The glyph may slightly overlap thumbnail
borders if needed for legibility. Do not increase panel width, row height or
thumbnail indentation; do not introduce a right-side gutter, arrowheads or extra
`fx` labels. The glyph is an indicator, not a tiny new interaction target.
Use the existing content-to-mask link's neutral color and stroke weight with
an upright, symmetric chain glyph. Adjustment effects show their icon without a
thumbnail background in the existing thumbnail slot. Preserve the hit area and
selection/focus indicators; content generators retain their content thumbnails.
Saved Selection rows never interrupt this connection. A drop inside an attached
effect chain snaps above its top effect. Attaching across saved selections moves
those selections above the resulting chain in the same undo step; a deliberate
drop below the owner remains possible. Preview the normalized position before
committing it.
The saved Selection's Use Selection control is an ordinary icon button with
squircle corners, a transparent idle background and standard hover/pressed
states. Keep its existing position and hit area; do not style it as a mask
thumbnail.

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

Use a darker shade of the accent for the clipping rail. Derive a semantic
palette role in [theme.rs](../../crates/layer-ui/src/theme.rs), with lightness and
chroma adjusted for custom accents, light/dark surfaces, selection and hover.
Neutral accents produce a darker neutral rail. Apply this role to the clipping
rail; effect links use the neutral link color. A strictly darker accent cannot
always meet the [3:1 non-text contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)
on dark selected rows. Preserve the requested shade rather than silently
brightening it, and assess those rows at native size. Relationship labels and
resolved targets remain available independently of the line's color.

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
effect's owner; structural edits keep them outside attached effect chains.

### 10. Dragging joins, releases and reorders relationships

| Drop | Result |
| --- | --- |
| Content into a clipping run | Join that run's base. |
| Clipped content into an ordinary stack position | Release clipping. |
| Effect onto an eligible owner's thumbnail | Attach to that owner; preview the exact position in its chain. |
| Effect within its chain | Change processing order. |
| Effect into an ordinary stack position | Become an effect on the stack below. |
| Saved Selection inside an attached effect chain | Insert above the top effect; preview that normalized position. |
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
Track source and mask damage within those dependencies; a revision change does
not by itself evict every cached page. Keep unaffected units reusable after a
local edit.

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

### Incremental composition contract

Local edits must cost work proportional to their dependency footprint and
required sampling support, rather than the whole document or every attached
chain. This is an acceptance condition for M1, not an optimization deferred
until host qualification. Reuse the existing scene, page and effect machinery;
do not add a second compositor or allocate a full-canvas image for every owner.

For Paint clipped to a Base with an attached Blur, the dependencies are:

```text
Base + its mask -> Blur -> processed Base ----+
                                             +-> clipping composition -> stack
Paint + its mask -> Paint's own effects ------+
```

Painting Paint does not dirty Base or rerun its Blur. Painting Base updates its
Blur and the dependent clipping output, including newly changed clip coverage;
it does not dirty Paint's source or invalidate Paint's own cached effects.
Clipping may need to reblend several members in the changed region without
reprocessing their unchanged effect chains. Blending into the surrounding stack
is a separate downstream dependency; backdrop-dependent effects still receive
the changes they actually consume.

Implement these rules in shared rendering and damage planning:

- **Follow actual dependencies.** Carry the changed source/mask identity and
  regions into owner inputs, effect steps, clipping runs and containing stacks.
  Duplicated layers may share immutable backing but retain independent editable
  source handles and damage. Preserve
  cache identity for unrelated owners across sibling insertion and reordering;
  stack indices and a global document revision are not sufficient cache keys.
  A local edit must not set a document-wide reset simply because attachments
  exist. Rebuild relationship metadata on structural edits, not on every dab.
- **Separate output damage from input reads.** Pointwise filters preserve the
  dirty footprint; spatial filters expand it by their declared current support.
  Propagate that expansion only along dependent paths. Reconstruct the input
  halo needed to calculate changed output separately, using existing pass
  dependency planning. Reading an unchanged halo does not make that input dirty.
  Chain support accumulates through successive filters, including pass and
  reduced-resolution sampling support. Account for both offset and blur support
  if a shadow is implemented. Unknown/document-wide sampling may legitimately
  invalidate the effect's full evaluation domain and its downstream consumers.
- **Keep separate regions separate.** Preserve disjoint damage regions or sparse
  page sets through spatial effects, composition, mip updates and Navigator.
  Expand pixel regions before mapping to destination pages; do not repeatedly
  enlarge tile-rounded damage at each effect. Deduplicate overlapping pages.
  Batching may merge nearby work where measured dispatch savings justify it,
  but must not silently turn widely separated contacts into one large rectangle.
  Transforms include interpolation support and world placement without promoting
  ordinary local transformed paint to full-document damage.
- **Invalidate old and new output.** Moving, deleting, hiding, reattaching or
  shrinking a blur requires removing old output as well as drawing new output.
  Use conservative old/new rendered bounds, including effect expansion, and
  propagate both through their old/new dependents. Parameter changes can affect
  an owner's entire output even when there is no paint damage; this is distinct
  from a small source edit. Include changed effect order, mask state, opacity,
  group isolation and undo/redo. Unknown output bounds require a safe fallback;
  cached input bounds must never crop expanded output.
- **Retain unaffected stages.** Reuse valid upstream spatial results after a
  downstream parameter edit, and independent owner results after a structural
  edit. Preserve pointwise fusion rather than requiring a texture per effect.
  Treat warm-cache invalidation separately from cold start, eviction, changed
  view resolution and renderer recreation. Keep memory bounded by the existing
  cache/window budgets, with no blocking GPU readback to discover dirty bounds.
- **Propagate to every consumer.** Apply the same dependency rules to native
  evaluation, reduced-resolution previews, mip levels, exact windows, thumbnails
  and Navigator. A global animated effect can update its dependent domain each
  frame; an unrelated animated chain must not invalidate static owner caches.
  Selection, renaming, expansion and connector presentation alone do not dirty
  artwork pixels, although visible UI and thumbnail requests can require work.

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
| M1: shared behavior | Switch authored relationships, group state, codec/admission, scene scopes, compositor, incremental invalidation and structural editing together. Save/reopen, undo/redo, pixels and exact consumers agree. Reorder, attach/release, grouping, duplication and merge/delete preserve unrelated owners and placement atomically. Pass focused pixel/work gates and measure changed rendering paths. Adapt host transport enough to keep clients building; delete superseded behavior. |
| M2: GTK interaction | Complete shared contextual commands/copy, group indication and swipe, extended rails, chain links and drag feedback at existing dimensions. Complete the GTK journey in both themes and narrow/wide layouts with relevant mouse/touch/pen checks. Follow the [GTK-first review gate](../ui/README.md#rules-for-ui-changes) before porting visuals. |
| M3: ports and completion | Port approved presentation to Web, then Android, Apple and Windows. Complete host journeys and native gesture checks, remaining affected performance measurements and final documentation. Apple presentation is included; Apple test runs are excluded from this implementation's validation scope. Move lasting contracts into current guides and retire this plan after the acceptance gates pass. |

Use a small set of authored fixtures across shared semantics, editing, persistence,
rendering and host journeys. Add targeted variations at the boundary they test;
do not multiply every variation across all hosts, color depths and devices.
Run focused checks while editing and the required changed-area checks at each
complete milestone. Capture baseline performance once, measure stable rendering
in M1 and reuse those results while rendering remains unchanged. In later
milestones measure newly affected paths; repeat other checks when changes,
failures or unresolved concerns justify it.

M1's transport adaptations do not bypass the M2 visual review. This plan does
not authorize unrelated filter types, general graph editing, automatic recovery
or additional clipping-order modes. Existing unrelated performance misses remain
open and do not expand this work into a general renderer optimization project.

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

### Incremental pixels and work

Pixel equality alone cannot catch a correct renderer that recomputes everything.
For each fixture, compare incremental output with an independent full render,
then assert affected regions and actual work. Record source uploads, composed
pages/pixels, effect input updates and per-pass pixels/dispatches, cache reuse,
mip work and Navigator work separately. Existing image stages expose
`input_updates`, `pass_updates` and `pass_pixels`; extend existing diagnostics
where owner/page attribution is missing rather than adding test-only production
switches. Count temporary intermediate writes and input reads separately from
final output damage.

Work assertions use warmed resident caches and a fixed viewport/level, with
unrelated branches fitting the cache budget. Specify expected regions from the
fixture geometry and declared filter support, independently of the production
damage helper. Permit documented sampling/alignment conservatism, not arbitrary
full-window invalidation. Run cold/evicted-cache cases separately for correctness
and bounded memory; they cannot establish the warm-cache work bound.

Include a numerical sparse-work fixture at native resolution: on a 2048 × 2048
canvas with 256 × 256 pages, change `[120,136) × [120,136)` and
`[1656,1672) × [1656,1672)`. For a filter with known single-pass support of 16 px,
only output pages `(0,0)` and `(6,6)` require updating, rather than the 49 pages
in their enclosing tile rectangle. Assert the actual scheduled output pages;
account for input reads and any intermediate pass work separately.

| Fixture or edit | Required work and edge coverage |
| --- | --- |
| Paint above blurred Base | Zero Base blur dispatches after painting Paint. The changed Paint region reaches clipping and final composition; no unrelated owner is invalidated. |
| Paint Base under clipped members | Base's effects update within their supported footprint. Clipping uses the new alpha there; independent member source uploads and cached member-effect dispatches remain zero. Include erasing to empty and fractional edge alpha. |
| Two distant contacts in one frame | Dirty output covers the two supported regions without filling their untouched gap. Include a fast curved stroke and multiple source targets in one frame. |
| Unrelated spatial effect | Adding a resident effect in another isolated group does not broaden the edited owner's damage or trigger that effect. Repeat with translated, rotated and scaled paint/masks to detect the current full-document fallback. |
| Tile and document edges | Test inside a page, at an edge and at a four-page corner, odd canvas sizes, out-of-frame source pixels and partially captured windows. No stale seam, missing halo or write outside the supported output pages. |
| Long local chain | Test one, two and four effects, including noncommuting pointwise/spatial combinations. Support expands only along the chain; changing a late effect preserves cached upstream spatial results. Fused pointwise steps need no artificial intermediate cache. |
| Radius/offset/visibility changes | Increase and decrease support, bypass a middle effect, hide/show the owner, detach/delete and undo/redo. Old output disappears and new output appears; no unrelated chain resets. Test offset support when a shadow is available, without adding a filter just for this milestone. |
| Mask and group edits | Paint owner and effect masks separately; change opacity, nested isolation and clipping-base coverage. Correct downstream scope and old/new bounds without invalidating independent groups. |
| Structural edits and shared backing | Reorder/reattach/reparent within and between groups, move a base with its run, and edit one duplicated layer sharing immutable backing. Its independent source changes without invalidating the other copy. Shifted row indices do not invalidate unrelated effects. Reused editable source handles remain outside the admitted subset and retain the unsupported-package outcome. |
| Preview correction and refinement | Replacing/cancelling predicted strokes removes old predicted halos. Settled output matches the exact render. New input interrupts refinement without losing damage or reusing stale mip/thumbnail/Navigator pixels. |
| Stroke backing publication | After canonical stroke pixels are composed, publishing their backing must not dirty the same source/output pages again or discard completed refinement. Resume input while publication and refinement are pending; undo, external restores and capture without composition must still invalidate changed pixels. |
| Global/animated dependency | A genuinely document-wide filter updates its required domain; independent owners retain their caches. Freeze phases when comparing pixels. Hiding animation stops its pixel work without retargeting attachments. |
| No artwork change | After settling, a no-op frame, row selection, rename or group expansion causes zero artwork recomposition and effect dispatches. Keep UI drawing and requested thumbnail work separate. |

### Real user journeys

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

### Performance qualification gates

Follow [performance targets](../PERFORMANCE_TARGETS.md) and the
[measurement rules](../performance/measuring.md); they remain authoritative.
The attachment workloads must meet the same motion gates:

| Reference device | Canvas | Brush completed updates/s | Display-paced presented frames/s on the tier-rate panel | Maximum p99 completion/presentation gap |
| --- | --- | --- | --- | --- |
| TCL TAB 11 Gen 2 | 12 MP, 4248 × 2832 | 60 | 57 (95% of 60 Hz) | 33.3 ms |
| Wacom MovinkPad 11 | 24 MP, 6000 × 4000 | 90 | 85.5 (95% of 90 Hz) | 22.2 ms |
| Wacom MovinkPad Pro 14 | 61 MP, 9504 × 6336 | 120 | 114 (95% of 120 Hz) | 16.7 ms |

Keep fresh input-consuming update rates and p99 gaps visible alongside total
completed updates; prediction/refinement updates cannot stand in for new input.
Use the existing CPU input/update/submission budgets of 4/3/2 ms and GPU
painting/composition/presentation budgets of 12/8/6 ms as engineering guides,
not substitutes for completed and presented frame measurements.

Extend the existing [brush and motion harnesses](../performance/measuring.md#how-to-measure)
with reproducible authored fixtures for the cases below. The benchmark document,
owner being edited, chain order, effect parameters, active sampling support,
brush size, camera and cache state must be recorded; the default photo-plus-paint
benchmark alone does not exercise attachment.

| Workload | Comparison and required observation |
| --- | --- |
| No effects and ordinary clipping | Preserve the existing brush guarantees at each tier's sizes. Compare matched baseline/new builds to detect overhead imposed on unaffected documents. |
| Paint above blurred Base; paint Base itself | Separate the two dependency directions. Use a small 32 px brush diameter to expose excess dirty work and the tier's guaranteed simple-brush size for sustained load; include seam crossings and distant contacts. |
| Owner-local chains | One, two and four supported effects, including mixed pointwise/spatial chains. Exercise small and broad valid radii, recording actual declared support rather than assuming slider radius equals total halo. |
| Independent owner scaling | Compare 1, 10 and 100 small visible isolated owners with resident local chains. Only one owner receives input; dirty-page sets and unrelated-effect dispatches must not grow with owner count. Measure CPU traversal/submission separately. Run cache-pressure variants separately. |
| Nested clipping/groups and structural motion | Paint inside a clipped isolated group; drag/reorder/reattach a chain and its base; scrub opacity, masks and spatial parameters. Measure motion and commit latency, including removal of old expanded output. |
| Navigation and refinement | Fit and 100% zoom, pan/zoom/rotate, Navigator open, warm and evicted caches, then resume painting during pending refinement. Measure mip/Navigator work, memory peaks and fresh-input delay. |
| Layer presentation | Scroll long layer lists, swipe groups and preview drops with the new connectors. Isolate UI frame cost from canvas recomposition and ensure presentation-only changes do not dirty artwork. |

Use matched release/benchmark profiles, hardware, thermals, inputs and viewport;
warm up, then run at least three 5-10 second gestures per measured case. Compare
before/after fresh-update throughput, p99 gaps, CPU/GPU time, dispatches,
source/output/pass pixels and resident/peak memory. Equivalent workloads must
have no reproducible regression beyond measured run-to-run variation; rerun
borderline comparisons rather than inferring a pass from averages alone. New
alpha-expanding behavior can require additional legitimate work: compare an
equivalent grouped reference where possible, otherwise account for the extra
pixels/passes explicitly and still meet the applicable tier gate.

No work-count bound is waived merely because a fast desktop meets its frame
target. Conversely, correct bounded tile work does not prove a hardware target.
Investigate costs above 1.5 times the calibrated workload estimate under the
existing measurement rules. Full-domain effect changes may use the established
display-resolution preview and exact refinement; the full-screen-filter soft
target applies only with its required hardware arithmetic and absence of a valid
approximation. Refinement must yield to fresh input.

Record measured values, date and commit in the tier tables, with raw traces in
`artifacts/`. Existing baseline misses remain open and cannot be relabeled as
passes because this change is no slower. Missing reference-hardware evidence
leaves qualification open. This plan defines gates and claims no measured passes.

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
