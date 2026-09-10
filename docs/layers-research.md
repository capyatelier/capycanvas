# Layers research: illustration workflows and panel design

Research date: 2026-09-09. Status: research, not an implementation specification.
The recommendation is in [Layers initial design](layers-initial-design.md).
The subsequent [layer context-menu audit](layers-context-menu-audit.md) inventories
command families, implemented additions and remaining engine gaps.

## Scope and evidence

Compare the official Procreate, Clip Studio Paint (CSP), and Photoshop documentation, including their illustrated panel guides. Prioritize drawing, flatting, shading, and non-destructive boundary editing rather than photographic compositing, animation, or page layout.

The user's clarified primary workflow is explicit regular-layer creation per element, drawing/filling against separate line art, then Alpha lock when the silhouette is ready. Related disconnected regions may share one layer. Lasso Fill and reference-aware Bucket Fill write to that active layer; automatic flat-color layers and automatic attached masks are not the default. Attached masks instead primarily reveal a selection of existing paint/texture, with independent content/mask editing and linked or unlinked movement.

The manuals establish capabilities and documented access paths, not usage-frequency statistics. Priority judgments below are product recommendations for this workflow, not measured claims about what most artists use.

Screenshot follow-up: CSP's official full-palette and tablet-row PNGs and Adobe's desktop mask-editing GIF were downloaded and visually inspected on 2026-09-09. An Adobe Community desktop-panel screenshot was also inspected; that example is from a 2022 post, not evidence of the latest release's precise styling. Adobe's current web-panel bitmap still returned HTTP 403, and Procreate's bitmap was not inspected. See the screenshot audit below. Competitor images are temporary research references outside the repository, not application assets; no competitor code or icons are imported. No measurements of competitors' logical padding or target sizes are claimed.

## 1. Procreate: a useful illustration baseline

### Panel and secondary interactions

The illustrated guide puts creation in the header and keeps the list focused on thumbnail, name, selected state, blend-mode abbreviation, and visibility. Opacity is reached through the blend controls. A separate background-color row supports a transparent document. Holding visibility temporarily isolates a layer and can restore the previous visibility state. The current handbook also documents bulk commands behind the Layers heading. This is a compact layout, but several actions depend on knowing where to tap or hold. [Panel guide and figures](https://help.procreate.com/procreate/handbook/layers/layers-interface), [blend controls](https://help.procreate.com/procreate/handbook/layers/layers-blend).

Organization includes a primary editing layer, additional selected layers, duplication, locking, reordering, and nested groups. Secondary selection and row swipes reduce permanently visible controls. For Capy Canvas, retain the compactness but give touch users an explicit multi-select command and a visible menu entry point. [Organization](https://help.procreate.com/procreate/handbook/layers/layers-organize).

The layer options include renaming, content selection, copying, filling, clearing, alpha protection, masks, clipping, reference-based coloring, and merging. Reference-based coloring is particularly relevant: boundaries can come from linework while color is deposited elsewhere. [Layer options](https://help.procreate.com/procreate/handbook/layers/layers-options).

### What “minimal” does not mean

Procreate distinguishes full locking, alpha protection, grayscale masks, and clipped paint. Masks appear as attached entries above their owners; moving or duplicating the owner carries its mask. This is functionally richer than a basic list of raster layers. The documentation reviewed does not establish support for masks on ordinary layer groups; do not assume that capability just because individual layers support masks. [Mask handbook](https://help.procreate.com/procreate/handbook/layers/layers-mask).

Our baseline should be its illustration-layer capabilities, not every Procreate feature: drawing assistance, animation, 3D painting, text, and the complete effects catalog are separate projects.

## 2. Clip Studio Paint: fast access, more visual density

The annotated palette is divided into a layer list, palette menu, selected-layer properties, and a command strip. Rows expose visibility, selection/status, thumbnail, name, and compositing information; tablet layouts also document a reordering grip. Properties include blending, opacity, clipping, reference/draft flags, locks, mask display, and layer-color controls. Commands include raster/vector creation, folders, downward transfer/merge, mask creation/application, and deletion. Several bars can be hidden or repositioned. [Palette diagram and control inventory](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm).

This is a useful inventory, but not a suitable default density for Capy's narrow, touch-capable panel. Reference flags and mask access have direct illustration value; rulers, draft flags, vector creation, and destructive mask application do not all need permanent buttons.

CSP's separate properties palette contains specialized appearance controls, including border effects, monochrome/tone-related settings, and display recoloring. Keep that distinction between everyday stack operations and detailed appearance editing. [Layer properties](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_properties.htm).

Important behavioral differences:

- CSP's mask painting is alpha-based: transparent paint/erasing conceals, while colored paint reveals regardless of RGB. Show Mask Area overlays the hidden region rather than replacing the canvas with a grayscale image. New masks are linked; a control between thumbnails changes linkage. Delete mask and Apply mask are distinct actions. These are the requested Capy interaction references, rather than Photoshop's black/white painting convention. [Masks](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm).
- Folder masks are explicitly supported and demonstrated for clothing and other multi-layer components. Linking allows content and mask to move together; unlinking separates their transforms. [Official folder-mask tutorial](https://tips.clip-studio.com/en-us/articles/717).
- New folders normally isolate their children. Through blending makes children interact with content outside the folder and has restrictions on clipping. Grouping therefore has rendering semantics, not just indentation. [Folders](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_folders.htm).
- A flat-color layer can keep color separate from its boundary mask. This remains a possible later addition, not a requirement for Capy's regular-layer/Alpha-lock workflow. [Fill layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Fill_layers.htm).

## 3. Photoshop: explicit targets and non-destructive properties

Desktop documentation establishes the core stack interactions: creation, visibility, reordering, and renaming. Additional documentation covers layer/group management and separate protection modes for transparency, pixels, position, and everything. We need alpha protection and full edit protection initially, not a permanent row of every lock type. [Desktop panel](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/get-started-layers/work-with-the-layers-panel.html), [management](https://helpx.adobe.com/photoshop/using/layers.html), [locking](https://helpx.adobe.com/photoshop/using/moving-stacking-locking-layers.html).

Photoshop on the web has its own current illustrated panel guide. It exposes layer creation, masks, adjustments/effects, ordering, deletion/actions, blend mode, opacity, and a properties entry point. This is a web reference, not evidence that the desktop panel has identical geometry. Its useful lesson is to keep the stack alongside a contextual properties surface rather than expose every property in every row. [Web panel and screenshot](https://helpx.adobe.com/photoshop/web/edit-images/manage-layers/layers-panel-overview.html).

Mask and layer thumbnails select different editing targets. Grayscale controls concealment, and group masks are supported. A clear target border is crucial: otherwise an artist can unintentionally paint the picture when trying to edit its boundary. [Mask editing](https://helpx.adobe.com/photoshop/using/editing-layer-masks.html), [mask creation](https://helpx.adobe.com/photoshop/desktop/create-masks/layer-masks/add-layer-masks.html).

Photoshop separates overall opacity from fill opacity, and its groups default to pass-through blending. These are not the same semantics as CSP's default isolated folders. Advanced clipping/blending options add further interactions. Capy should document one initial interpretation rather than imply exact Photoshop compatibility. Separate fill opacity is unnecessary without the associated advanced effects. [Opacity and blending](https://helpx.adobe.com/photoshop/using/layer-opacity-blending.html), [clipping](https://helpx.adobe.com/photoshop/using/revealing-layers-clipping-masks.html).

Masked fill and adjustment layers provide a future direction: change a region's color or correction without repainting its shape. They are not required for the current regular-paint-layer workflow. [Adjustment and fill layers](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/work-with-adjustment-and-fill-layers.html).

## 4. Comparison and implications

This table synthesizes the sources above; “initial” is the recommended Capy scope, not a claim of existing implementation.

| Capability | Procreate lesson | CSP / Photoshop lesson | Capy recommendation |
| --- | --- | --- | --- |
| Select, inspect, show/hide | Compact rows | Explicit visibility and target state | Direct in every row |
| Create, duplicate, rename, reorder | Fast secondary actions | Buttons plus menus | Creation direct; other actions in row menu and shortcuts |
| Opacity and blend | Small entry point | Selected-layer controls | Direct selected-layer controls |
| Alpha lock | Useful silhouette painting | Separate from full locking | Direct toggle, explicitly enabled after building an element |
| Full edit lock | Prevent accidents | More granular locks exist | One full lock, menu plus visible state |
| Clipped paint | Independent shading over a base | Dedicated clipping state | Direct toggle and “New clipping layer” action |
| Layer masks | Separate mask target | Adjacent thumbnails are compact | Adjacent owner/mask targets; CSP alpha-based painting |
| Mask linkage and movement | Owner carries mask | Linked and independent movement | Initial; default linked, inter-thumbnail toggle, separate target positions |
| Group masks | Not verified in reviewed handbook | Explicitly supported | Initial; one silhouette for several paint layers |
| Groups / multi-selection | Essential organization | Hierarchical operations | Initial; include touch selection without keyboard modifiers |
| Flat color + mask | Raster flatting is available | Dedicated editable fill content | Deferred; regular paint plus Alpha lock is the initial path |
| Selection reuse | Recall and select content | Selection/mask interoperability | Select contents/mask and reselect initially |
| Lasso fill | Selection coloring combines tracing and paint | Region-oriented fill tools | Initial; trace directly into the active regular layer |
| Reference-guided bucket | Separate linework from fill target | More source and gap controls | Initial closed-region fill; advanced gap repair follows |
| Layer effects / corrections | Broader editing tools | Powerful non-destructive stacks | Later; do not reserve a row of empty controls |
| Layer search / tags / comps | Compact simple organization | Large-document organization | Later, before adding obscure effects |
| Merge / flatten | Useful but irreversible without undo | More variants | Context menu; appearance-preserving cases only |

The panel needs to distinguish Alpha lock, a temporary pixel selection, a persistent visibility mask, and clipping paint to another layer. “Selected layer” must also remain distinct from “selected pixels.” Mask linkage governs movement, not which target receives painting; an unlinked mask still belongs to its layer.

## 5. The actual masking / flatting workflow

### Research relevant to reducing repetitive work

Procreate's freehand selection can combine drawn curves with straight segments. It allows navigation during selection, so tracing does not require abandoning the operation. Its selection interface provides region combination and color filling; selections can also be recovered from existing content or recalled. These capabilities matter more to flatting speed than another property in the layer menu. [Freehand](https://help.procreate.com/procreate/handbook/selections/selections-freehand), [selection interface](https://help.procreate.com/procreate/handbook/selections/selections-interface), [selection reuse](https://help.procreate.com/procreate/handbook/selections/selections-advanced).

CSP's selection launcher puts actions next to the selected area, including combining selection work with filling and creating content. That supports a contextual action strip in Capy rather than forcing every region through the main menu. The cited launcher manual is a legacy official guide; use it for the interaction precedent, not current pixel styling. [Selection launcher](https://www.clip-studio.com/site/gd_en/csp/userguide/csp_userguide/510_tool/510_tool_selct_launcher.htm).

Automatic region filling needs more than a generic flood fill. CSP documents boundary tolerance, gap handling, region growth, and reference sources. Higher gap settings can cost more processing, and different settings solve different failure cases. A faint sketch with open contours is fundamentally less reliable than enclosed linework. [Fill tool controls](https://help.clip-studio.com/en-us/manual_en/810_subtools/F.htm), [official fill workflow](https://tips.clip-studio.com/en-us/articles/1263).

### Tasks and friction to remove

| Artist's task | Common friction | Required design response |
| --- | --- | --- |
| Keep sketch/ink visible but safe | Paint accidentally lands on the sketch | Visible lock state; explicit destination; no source-layer mutation |
| Trace skin, clothing, or another region | Selection and filling are unnecessarily separate | Brush/Lasso Fill writes to the active regular layer |
| Fill an enclosed region in the linework | Bucket reads the empty destination and fills the whole canvas | Separate, visible Reference and Destination controls |
| Make an independently colorable element | Unnecessary mask creation and implicit layer changes | New regular layer → draw/fill → explicitly enable Alpha lock |
| Build several disconnected pieces of one material | One new layer per island becomes unwieldy | Continue painting/filling on the same active layer |
| Keep fingers, straps, or hair holes clear | Cutouts are missed or filled accidentally | Subtract selection or erase unlocked paint; use mask erase for existing texture |
| Check the flatting under linework | Transparent holes and seams are hard to see | Solo/restore, toggle linework visibility, checkerboard; Show mask area for attached masks |
| Correct a boundary later | Alpha lock prevents silhouette changes; revealing an empty layer adds no paint | Unlock paint alpha to grow/erase; edit an attached mask to reveal/hide existing pixels |
| Recolor or begin shading | Repeat selections or accidentally alter the silhouette | Alpha-locked painting or one-action clipped paint creation |
| Fit texture to a selected shape | Editing source pixels destroys reusable texture | Selection → attached mask; selected region stays visible |
| Adjust texture inside its boundary | Moving content unintentionally moves its mask too | Separate thumbnails; obvious link toggle and independent movement when unlinked |
| Organize a character | Reparenting loses masks or changes clipping | Groups carry ownership; explicit drop targets and validated clipping relationships |

### Workflow priorities

Lasso Fill and basic reference-aware Bucket Fill belong in the initial release. For clean closed linework, a bucket should not require manual tracing at all. Both feed the same current-layer fill operation; the artist creates layers explicitly. Lasso selection remains useful for staging complex regions, combining contours, recovering boundaries, and masking existing artwork, but is not an obligatory intermediate step for painting an element.

Keep the attached-mask workflow separate: existing texture → selection → mask, with the selected area revealed. Add mask without a selection reveals all; an explicit Replace mask from selection supports making the mask first and selecting later. Newly created masks start linked, and content/mask are separate edit targets. Painting reveals regardless of color, transparent ink/erasing hides, and Show mask area inspects hidden coverage with a tint. These policies are specified in the initial design rather than delegated to each frontend.

For sketchy or open lines, lasso fill is the reliable manual alternative. Advanced gap closure, enclose-and-fill, and automatic semantic segmentation are different levels of assistance; do not confuse basic reference-aware bucket filling with those larger projects. Region extraction must also work with an opaque imported sketch, not assume that every linework source has a transparent background.

Do not automatically make one layer per connected island: two hands may belong to one skin region, while adjacent pieces may need independent layers. The artist chooses the semantic grouping. Nor should every region require a new group: a regular paint layer with Alpha lock or clipped shading is enough until multiple independently managed layers need a shared group mask.

## 6. What to expose, and what to keep secondary

Recommendation, based on the workflow rather than presumed popularity:

- Permanent panel controls: visibility, target thumbnails and their link toggle, group expansion, blend/opacity, Alpha lock, clipping, new layer, new group, add mask, and a discoverable actions menu.
- Fill-tool controls: Lasso Fill / Bucket Fill, explicit destination, and a named reference source for bucket fill; current color stays visible. Creating a region must not require a layer menu every time.
- Contextual selection controls: fill current layer, mask from selection (explicit replacement when a mask exists), and deselect. Selection tools also expose add/subtract coverage. No automatic color-layer creation is needed.
- Context menus: rename, duplicate, locking, grouping/ungrouping, selection from content, solo, merging, and deletion. The mask menu adds Show mask area, enable/link/invert, selection operations, Apply mask to layer and Delete mask. Include shortcuts; never make a gesture the sole route.
- Selected-layer properties: blend/opacity in the panel, remaining operations in the layer/mask menu. Defer a separate inspector until it contains useful additional controls; the workspace panel-configuration drawer must not edit artwork.
- Deferred: dedicated flat-color/fill content, broad correction stacks, blend-if, vector masks, smart objects, linked external assets, animation, and exhaustive import compatibility. Basic content/mask translation and linkage are initial, not deferred with free transforms.

The initial design should be compact in its idle state and explicit about paint destination, mask target and reference. Direct drawing/filling avoids a mandatory selection-and-mask sequence for each painted element. New layers and Alpha lock remain deliberate artist choices. Texture masking is a separate, discoverable flow rather than an implicit side effect of flatting.

## 7. Local implementation gap

At the time of inspection, [the core layer model](../crates/layer-core/src/lib.rs) contains paint/imported-image/suggestion/background kinds, visibility, opacity, and content references. [The shared UI row model](../crates/layer-ui/src/lib.rs) exposes basic selection/editability/visibility/opacity. [The renderer documentation](rendering-program.md) describes normal-alpha layers and GPU-resident brush/material state, not the proposed group/mask/blend/selection system.

This is therefore an engine-and-workflow extension, not just additional panel buttons. Keep ownership in the existing crates and shared command catalog; do not implement separate native layer trees or flatting sequences. Existing watercolor wetness is material state and cannot double as an editable visibility mask.

## 8. Screenshot audit and panel recommendation

These are screenshots actually inspected, not image-search captions:

| Reference | Visible pattern | Recommendation for Capy |
| --- | --- | --- |
| [CSP full palette, official PNG](https://help.clip-studio.com/en-us/manual_en/images/180_layers_0004.png) | Property/command strips above a hierarchical list; adjacent mask thumbnail; names and compositing metadata on separate lines | Retain the hierarchy and direct targets, but omit default `100% Normal` text and unsupported controls |
| [CSP tablet row, official PNG](https://help.clip-studio.com/en-us/manual_en/images/180_layers_0006.png) | Separate eye/status columns and a trailing reorder grip | Separate visibility from selection and give touch reordering a distinct drag target |
| [Photoshop mask editing, official GIF](https://helpx.adobe.com/content/dam/help/en/photoshop/using/editing-layer-masks/jcr_content/main-pars/image/edit-layer-mask.gif) | Content and mask share one row; the editing target is outlined; creation commands are below the stack | One row per owner, not a separate mask row; persistent compact footer |
| [Photoshop desktop close-up, Adobe Community](https://community.adobe.com/questions-712/layer-mask-not-removing-everything-outside-of-canvas-1138879) ([image](https://uploads-us-west-2.insided.com/adobedme-en/attachment/393682i0DB1750532E54DA9.jpg)) | Eye on the left, paired thumbnails, name, occasional lock; top filter/lock/fill controls consume extra height | Borrow row clarity, not the filter strip, every lock type, or separate Fill opacity |

The [CSP palette guide](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) confirms its configurable command-bar placement and reference/status meanings. Adobe's [mask guide](https://helpx.adobe.com/photoshop/using/editing-layer-masks.html) confirms that choosing the thumbnail changes the editing target. Screenshots establish placement and visual relationships, not which operations artists use most often.

The resulting recommendation is a compact Photoshop-like stack with CSP's useful illustration state: a fill-reference badge, clipping relationship, and non-default blend/opacity metadata. Use Capy's existing colors, icons, panel shell and 11pt text, not competitor styling.

Three refinements to the earlier proposal matter most:

1. Budget width for paired targets and the inter-thumbnail link control. Prototype around 300 logical units instead of forcing a mask-heavy stack into the current narrow sidebar. Test 240/280/320 widths and deep groups; dimensions are proposed Capy values, not extracted competitor measurements.
2. Distinguish selected rows, the single paint/mask target, and the independent fill reference. Row highlight alone cannot communicate all three. Mask editing needs a small explicit target label as well as the thumbnail outline.
3. Do not create a second inspector that duplicates the header and menus. Add it when future effects, transforms or mask-refinement controls actually need it. Keep fill reference/destination and tolerance in the fill-tool surface, not permanently above the layer stack.
