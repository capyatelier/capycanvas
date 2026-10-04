# Layer clipping and effect attachment

[Design history](README.md)

This design separates a content layer's clipping shape from an effect's input.
The current contracts live in the [authored model](../reference/authored-model.md),
[renderer](../internals/rendering.md#filters), [layer UI](../ui/shared-ui.md#layer-relationships)
and [package format](../reference/capy-package.md). This record preserves the
decisions and their rationale.

## Composition decisions

Consecutive clipped content shares one unclipped paint or isolated-group base.
An intermediate empty or hidden layer does not become a new mask. Composition
runs bottom to top; nested isolated groups express additional clipping boundaries.
There is no order toggle.

Attached effects process their owner alone, nearest effect first. The order is
owner content, content mask, attached effects, outer clipping, then contribution
opacity and blend. An effect on a clipping base therefore changes the alpha used
by upper clipped content: blur can expand that shape while the upper paint stays
sharp. Effects on a clipped member process that member before clipping. Effects
on a containing isolated group process the finished inner composition.

Attachment does not multiply the result by the owner's original alpha. Effects
declaring filtered alpha can expand coverage; normal effect opacity and masks
interpolate premultiplied input and result. Masks remain optional. Adding new
glow, border or shadow filter types is independent of this relationship change.

Isolated groups can own effects, act as clipping bases and themselves clip into
another base. Pass Through groups depend on the outside backdrop and cannot have
those relationships. Explicit isolation and attachment form one undo step.
Returning from Pass Through restores the previous isolated blend mode. Alpha
lock remains a paint-edit constraint; right swipe on a group toggles its mode
when its relationships permit it.

## Presentation and editing decisions

One existing button serves both operations, with contextual wording: Clip to the
base, Release clipping, Apply to the owner, or Apply to layers below. Shared
metadata supplies its icon, target, availability and action.

A straight rail in the existing left gutter extends to the base thumbnail's
bottom. It has no notch or arrowhead and uses a darker shade of the accent,
replacing the initially considered complementary color. Small upright neutral
chain links occupy the gaps between adjacent effect thumbnails and their owner.
The links use the existing mask-link ink; neither indicator adds horizontal space
or row height. Adjustment effects use backgroundless icons; generators retain
content thumbnails. Pass Through has a badge inside the folder and a blend-mode
subtitle. Use Selection is an ordinary flat squircle icon button.

Hiding an owner suppresses its effects without changing their individual eye
settings or retargeting them. Inherited hiding uses a dimmed closed eye. Saved
Selections stay outside the contiguous effects-and-owner unit; a drop inside it
snaps above its top effect. A separate drop below the owner remains valid. This
avoids routing connectors around rows that are not part of the chain.

Thumbnail drops attach effects; row gaps reorder or detach them; group bodies
insert inside the folder. Moving an owner carries its effects. Moving a base
carries its clipping run and each member's effects. Shared planning normalizes
both the preview and the atomic committed edit, including locks, cancellation,
placement and undo.

## Implementation boundaries

The authored attachment enum replaces the old clipping flag in occurrence/2
records. This does not change the package container version or add an old-format
reader. One owner evaluator replaces the clip-specific reconstruction path.
Stable owner identities retain cached local results independently of their outer
contribution. Sparse damage follows actual dependencies and declared sampling
support; refinement batches remain bounded and new input interrupts them.

Delivery is split into shared semantics and renderer, approved GTK interaction,
then Web and native presentation with obsolete transport removed. Focused pixel
oracles also assert actual page and effect work: sparse contacts must not dirty
the rectangle between them, and independent owners must retain their caches.
The [tier tables](../PERFORMANCE_TARGETS.md) record measured hardware results and
remaining qualification limits; a correct work bound alone does not establish
a frame-rate target.

## Research basis

| Source | Design precedent |
| --- | --- |
| [Photoshop clipping masks](https://helpx.adobe.com/photoshop/using/revealing-layers-clipping-masks.html), [Procreate layer options](https://help.procreate.com/procreate/handbook/5.4/layers/layers-options) | Multiple clipped layers share a base. |
| [Clip Studio nested clipping](https://support.clip-studio.com/en-us/faq/articles/20190046) | A nested folder adds a clipping boundary. |
| [Clip Studio layer settings](https://help.clip-studio.com/en-us/manual_en/180_layers/Other_layer_settings.htm), [Krita alpha inheritance](https://docs.krita.org/en/tutorials/clipping_masks_and_alpha_inheritance.html) | Pass-through scope differs from an isolated image; Krita's accumulated-alpha model is distinct from the common-base choice. |
| [Affinity live filters](https://affinity.help/photo2/en-US.lproj/pages/Layers/livefilters.html) | Effects can address the stack or a layer/group through drag and drop. |
| [GIMP layer effects](https://docs.gimp.org/3.0/en/gimp-using-layer-effects.html) | Owner-local effects run bottom to top. |

These precedents guide the design; they do not establish identical behavior for
every mixed-stack edge case in those applications.
