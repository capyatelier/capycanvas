# Layers: recommended initial design

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

Status: GTK, web and Android layer/masking review builds implemented; the broader workflow roadmap below is not all shipped. Updated 2026-09-09. These decisions supersede the earlier automatic flat-color-layer proposal. Evidence and screenshot limitations are recorded in [Layers research](layers-research.md).

### Current review builds

Implemented: compact virtualized layer rows; separate content/mask thumbnails and link control; paint layers, isolated groups, reorder/reparent, rename, duplicate/delete, visibility/opacity, locks, reference designation and seven blends. Lasso selections and Lasso Fill provide the initial selection path. Imported images become editable paint layers. Masks support selection initialization, incremental reveal/erase painting, independent/linked translation, inspection, inversion, clear, enable/disable, Delete and Apply with undo. Clipped siblings share the unclipped base alpha. Commands and gestures use shared Rust state; Hosts supply widgets and file decoding. Lasso selection and Move are shared toolbar tools, not layer-footer modes.

Checkbox multi-selection and multiple reference designations are implemented independently of the editing target. The lighthouse header button marks checked paint layers/groups, including locked linework, then restores sole selection to the original editing target without switching content/mask targets. Only a sole selected editing layer already marked as a reference toggles off. Any selected reference gives the header button a subdued grey state; the tooltip describes its actual next action. Selection checks take precedence over reference icons, which take precedence over the editing icon. The remaining header controls edit the drawing target's owner, not the checked set. The [context-menu audit](layers-context-menu-audit.md) adds bulk duplicate/delete, Group selected layers, safe Ungroup, mask copy/paste, explicit reveal/hide-selection masks, Clear layer and visibility commands. Still planned, not exposed as working tools in this build: reference-aware Bucket Fill; bulk property editing/mixed values and moving multiple selected rows together; selection add/subtract and painted-mask-to-selection; constraining ordinary strokes to temporary selections; group flatten/merge; project persistence for the new model. Reference designation records sources but does not perform bucket filling. Apply Mask is available on paint layers, including imported images, not isolated groups. This review is not a claim that every acceptance scenario below is finished.

The Layers minimum is **226 logical pixels**: six standard 36-pixel toolbar tiles and five 2-pixel gaps. The shared docking allocator enforces it for docked/floating groups, including when Layers is an inactive tab. Header/footer buttons use 24-pixel targets; fixed eye and selection columns are 24 pixels wide, aligned with Alpha Lock and Lock above. In GTK/web their click targets fill the row's 36-pixel interior height. Thumbnail artwork starts at 28 pixels, and controls keep 6-pixel horizontal inset. Thumbnail-to-name spacing is 8 pixels, matching panel-tab name padding. Names/metadata and the blend label ellipsize. Rows stay compact (about 40 pixels with two-line metadata). No extra mask row or unused mask slot is added.

GPU implementation lives in the existing renderer crate: `layer_masks.rs`/`selection.wgsl` maintain sparse R8 coverage, and `scene.rs`/`scene.wgsl` compose tiles. Aligned Normal paint+mask is one source-over shader draw. Translations, clipping, groups and other blends share reusable tile-sized scratch surfaces, not full-canvas buffers per layer. Export excludes inspection tint. Apply Mask resolves displayed watercolor pigment before clearing its latent material state. All hosts asynchronously download only 32×32 pixels for visible rows; drawing never waits for them. `thumbnails.wgsl` reduces actual nonzero-alpha bounds on the GPU (including after erasing), frames that content without stretching, and composites a neutral checkerboard. Paper uses its configured linear color/opacity; transparent Paper displays the checkerboard. The scan is requested for changed visible previews, not per dab; no full-layer readback or additional full-canvas allocation is used.

Validation includes shared history/group/minimum-width tests, GPU pixel tests for soft-alpha clipping, group opacity, mask application, translated masks, exports, imported images and both brush-preview forms, plus a private Wayland GTK scenario with 110 added layers and dark/light captures. Review images are generated under ignored `artifacts/ui/layers-gtk/`, `artifacts/ui/layers-web/`, and `artifacts/ui/layers-android/`. The native Wayland review, browser `--layers` scenario and Android `layersReferencesMasksAndTools` exercise real GPU hosts; GPU thumbnail tests separately assert framing, transparency and erase behavior.

Web keeps the downloaded 32px preview as a host-memory UI bitmap. This avoids
a second browser GPU Canvas2D upload path that returned transparent pixels for
solid-white Paper/mask previews in validation. All painting, masking, bounds
reduction and thumbnail rendering remain in the shared GPU renderer. The browser
test checks every visible preview for completed opaque pixels, not only paint.
Removed web rows invalidate their preview-cache entries so Undo reloads their
thumbnails. Shortcut remapping and typed context-menu hints are also checked.

The final GTK/web comparison measures layer rows, eye/selection columns, names,
thumbnails and opacity controls within one logical pixel, alongside the existing
workspace geometry and shadow checks. Both hosts use the same original layer
button SVG bank as Android. Run `native_web_parity_reference`, then the browser
`--package --parity` test; captures and measurements are under ignored
`artifacts/ui/parity/`. Packaged layer interactions and PWA offline/update tests
also pass; generated static assets remain under ignored `dist/capycanvas/`.

Release GPU-completion benchmark (Vulkan, discrete GPU; 120 measured frames after 20 warm-ups; one 128×128 damaged region, one composition tile; milliseconds):

| Stack | Median | p95 | p99 |
| --- | ---: | ---: | ---: |
| Plain paint | 0.050 | 0.056 | 0.087 |
| One masked paint layer | 0.078 | 0.098 | 0.208 |
| 24 overlapping masked layers | 0.233 | 0.322 | 0.374 |
| 100 overlapping masked layers | 0.842 | 1.071 | 1.649 |

Command: `cargo test --release -p layer-render-wgpu layer_composition_latency -- --ignored --nocapture --test-threads=1`. These are submission-to-completed-GPU-frame measurements, not input-to-display or whole-document timings. Larger damage and complex nesting need separate measurements; this is not a blanket 120 Hz guarantee. The single masked draw reduced the 100-layer p95 from 9.862 ms in the first implementation.

## 1. Decision

Build a compact layer list around three distinct workflows:

```text
New regular layer → draw/fill an element → Alpha lock → paint within its alpha
                         ↑
               separate line-art reference

Existing texture → selection → layer mask → edit/move content or mask

Existing painted base → new layer above → Clip to layer below → shade separately
```

The main element-building path uses ordinary raster paint layers. A layer may contain several disconnected pieces belonging to the same element. The artist decides when to create the next layer and when to enable Alpha lock; neither happens automatically after every stroke, fill or pen lift. Separate flat-color/fill layer types are a later option, not an initial requirement.

Layer masks primarily control the visibility of existing paint or texture. They have their own thumbnail and editing target, with a simple link toggle between the two thumbnails. Clipping instead derives coverage from the layer stack below without creating an attached mask bitmap. Alpha lock, an attached mask, a temporary pixel selection and clipping must not be presented as the same operation.

## 2. Initial feature boundary

| Area | Include initially |
| --- | --- |
| Content | Regular paint layers, imported artwork and nested groups; preserve existing background/suggestion handling |
| Basic management | Create, rename, duplicate, reorder/reparent, delete, group/ungroup, multi-select, undo/redo and project persistence |
| Composition | Visibility, solo/restore, opacity, Alpha lock, full edit lock, Clip to layer below |
| Blend choices | Normal, Multiply, Screen, Add, Overlay, Soft Light, Color |
| Masks | One single-channel, alpha-based mask per layer/group; create from selection, enable/disable, invert, reveal/hide all, Show mask area, select mask contents, Delete mask and Apply mask to layer |
| Mask linkage and movement | Linked by default; toggle between thumbnails; paint content and mask separately; translate either target, moving both when linked |
| Direct region building | Brush and freehand/polygon Lasso Fill on the active regular layer; Bucket Fill reads separate linework with tolerance and contiguous-region behavior |
| Supporting selections | Freehand/polygon selection, replace/add/subtract, invert/deselect/reselect, selection from content or mask, fill current layer, mask from selection |
| Region corrections | Unlock alpha to change a painted silhouette; edit an attached mask with paint/eraser or selection commands |
| Flatting to painting | Enable Alpha lock for same-layer painting, or create a clipped layer for separate shading; group related layers when useful |

The seven blend choices are a proposed useful shortlist, not a measured popularity ranking or a promise of exact Procreate/Photoshop color matching. They cover ordinary paint, shadows, highlights, contrast, and recoloring. [Procreate's blend descriptions](https://help.procreate.com/procreate/handbook/layers/layers-blend).

Region boundaries are initially crisp with antialiasing and no feathering; masks retain intermediate coverage values. Basic bucket tolerance and controlled expansion under linework are initial tool controls. Canvas translation of content/masks is now initial scope. Scale/rotation/free transform, full selection transforms, sophisticated refinement, automatic gap repair and the exhaustive blend catalog remain follow-ups.

## 3. Panel layout

Retain Capy's existing panel container, tab-group behavior, theme tokens, 11pt text, and standard button sizing. Artwork groups are unrelated to workspace tab groups. Use the shared competitor pattern of fixed selected-layer controls, an independently scrolling stack, and a fixed action footer. Borrow Photoshop's explicit editing targets and CSP's useful illustration state, not their full control density.

```text
┌ Existing Layers panel tab ──────────┐
│ [Normal      ▾]  ━━━━━━━━ 100      │  Equal blend/opacity halves
│ [α-lock] [lock] [clip] [reference] │
│                                    │
│ eye  □ [open folder] Character     │
│ eye  ⚑     [paint]   Linework   🔒  │
│ eye  □     ▏[paint]  Shading        │  Pastel-red clipping stripe
│                      Multiply 65%  │  Only non-default metadata
│ eye  □     [paint]   Skin       α  │
│ eye  ✎     [paint]─[mask] Texture  │  Link between edit targets
│ eye  □ [swatch] Background         │
│                                    │
│ [new layer] [group] [mask] [more]   │  Icons with tooltips
└────────────────────────────────────┘
```

The diagram is structural, not pixel-accurate; words and symbols stand for our own icons. Use the compact dimensions specified in the GTK review-build section. Paired thumbnails and their link control are functional hit targets, not decorations to hide when width runs out. Names can ellipsize, but the active content/mask target must remain visible.

Retain 6-unit outer/control spacing; this compact panel intentionally uses smaller buttons than the 36-unit tool ribbons. Blend and opacity split their row equally. Opacity uses the reusable inline numeric presentation: slider plus editable whole-number readout, no visible title, unit or step buttons; its tooltip identifies the control. Expressions and stored precision still use shared numeric policy. Enforce the shared minimum rather than clipping controls. These are Capy dimensions, not measurements of CSP or Photoshop.

Use one continuous list surface: no rounded card or permanent border around each row. Selection uses the same blue tint as the selected brush. Four contrasting corner marks identify the editing thumbnail independently, including single-thumbnail rows. These marks render above the preview, not behind opaque image pixels. The fixed header/footer keep their positions as the list scrolls. Secondary metadata uses the same 11pt panel font in a secondary color.

### Layer rows

- Visibility and checkbox controls form two fixed columns, including nested rows. Indent only the thumbnails and names. A hidden ancestor is an inherited state, not a rewrite of each child's visibility. Checked selection takes precedence over pencil and reference icons, except when the editing layer is the only selected layer. Otherwise show a pencil for the drawable editing target, a lighthouse for references, or an empty box. All remain clickable; reference/editing roles survive a temporary checkmark. Rust supplies the icon identity.
- Show content and mask thumbnails side by side when a mask exists. The selected target has a clear outline. A disabled mask has an unmistakable overlay mark. Do not add an empty mask slot or a second list row when no mask exists. A chain/link icon joins the thumbnails when linked; a subdued broken-link icon occupies the same slot when unlinked. Clicking that slot toggles linkage without changing the selected target or moving anything. Tooltips/accessibility labels say Link mask to layer or Unlink mask from layer; the mask menu exposes the same toggle.
- Let the name ellipsize, with the full name available in its tooltip and inline Rename editor. Indent thumbnail/name content by 8 pixels per level, capped at 24 pixels to preserve the 226-pixel minimum; eye/selection columns never move.
- Put non-default blend/opacity beneath the name, for example `Multiply · 65%`; omit `Normal · 100%`. Lock indicators belong on the right, references in the selection column, and clipping is a bold pastel-red stripe before the thumbnail—not metadata text.
- The thumbnail-sized open/closed folder is the single group disclosure button. It has no resting background and expands/collapses without changing the editing target. Header controls retain their target even when its containing group is collapsed.
- Content selection on a paint layer means paint; mask selection means boundary editing. While editing a mask, show a compact contextual label such as `Editing Skin mask` with a visible route back to content. This can reuse the contextual tool surface; it is not a permanent extra panel row.
- The mask thumbnail can use white for visible coverage and black for hidden coverage. This is a visualization of one alpha channel, not an instruction to paint with black/white: any paint color reveals, while transparent ink/erasing hides.
- Paint thumbnails show content over a subtle checkerboard before the owner's mask/opacity/blend, independent of visibility so hidden rows remain recognizable. Use a stable document-coordinate thumbnail frame initially; cropped-content thumbnail modes are deferred. Group thumbnails may use a folder glyph initially. Background and suggestion rows retain explicit kinds and core-defined editing restrictions.

### Selection and movement

Row selection, the active content/mask target and reference layers are independent. Tapping a row name selects that row and its content; tapping either thumbnail explicitly selects that target. Tapping the already-active, selected row's name leaves its current target unchanged. Double-click names to rename in place (Enter or focus loss commits; Escape cancels). Header blend/opacity always edit the drawing target's owner, never mask strength. Checkbox clicks toggle selection without changing that drawing target. Bulk property controls with mixed values are deferred.

The [2026-09-12 drag convention](../ui/drag-and-reorder.md) supersedes the original
mouse/pen grouping: mouse can drag a row immediately, while touch and pen require
a hold on the row body before reordering. Ordinary pre-hold touch/pen movement
remains available for scrolling. The trailing grip starts dragging without a
hold for every device. Thumbnail taps still select targets. Native gestures
handle device recognition and the translucent drag preview; shared Rust owns
selection and move validation/results. Ordinary state updates retain row widgets
so a selection change cannot interrupt double-clicks or drags. There is no
separate multi-selection mode. Current host gaps are in the
[source inventory](../ui/drag-inventory.md).

Drop hints distinguish above/below a row from inside a group. A successful drop into a collapsed group opens it. A drag moves its source layer (or group subtree), including owned masks; moving multiple checked rows together is deferred. Never reuse workspace floating-panel docking or tear-off rules for artwork layers. Invalid destinations are rejected before commit.

### Placement of actions

| Location | Controls / commands |
| --- | --- |
| Always visible above list | Blend, inline opacity; Alpha lock, edit lock, Clip to layer below, selected-layer reference toggle |
| Always visible in row | Fixed eye/selection columns, content/mask targets, mask link, clickable open/closed folder, right-side lock (edit lock takes precedence over alpha lock), clipping stripe |
| Bottom action strip | New paint layer, new group, add mask, more actions |
| More / row context menu | Creation, inline Rename, duplicate/delete (checked set when applicable), Group selected layers, safe Ungroup, protection/reference toggles, Mask/Selection/Visibility submenus, Move and Clear. Pixel-selection extraction and merge are still planned, not working entries. |
| Mask context menu | Edit content, Show mask area, Enable mask, Link mask; reveal/hide selection; Copy/Paste mask, invert/reveal-all/hide-all; Apply mask to layer and Delete mask in the final section |
| Selection action strip | Fill current layer, Mask from selection (Replace mask from selection when one exists), clear selection |
| Fill-tool controls | Lasso Fill / Bucket Fill, active destination layer, current color; bucket also shows Reference, tolerance and edge expansion |
| Separate Layer Properties panel | Deferred until additional effects, transform or mask-refinement controls justify it |

Use separators between creation, organization, protection, and destructive actions. The visible More button provides access without hover, right-click, or long-press. Context menus dispatch the same commands as direct controls. More and row context menus establish an editing target but preserve the checked set when that row is already checked. Otherwise they select the clicked row alone. The lighthouse header action operates on the checked set without changing the editing target.

Do not duplicate the header and mask menu in a separate inspector for this MVP. If a Layer Properties panel is added later, it edits selected artwork, not the existing drawer for configuring which controls a built-in workspace panel displays. Keep Delete and merge in the menu initially; no new filter strip, Fill-opacity field, effect buttons or unsupported placeholders. Mask linkage is now a required direct row control.

The bottom Add mask action follows the existing selection-aware semantics below: use the pixel selection if present, otherwise reveal all. If a mask already exists, select it, never overwrite it. New paint layer remains a direct one-tap action; alternative creation commands stay in More and the shared customizable toolbar catalog.

Paper is selectable for visibility and opacity, but never accepts strokes, masks,
clipping, reordering or deletion. Its context menu contains only applicable
commands. It remains the bottom anchor; inserting or dropping at the bottom puts
artwork above Paper. Shared controls expose this capability difference to hosts.
The inline opacity value reserves space for its full formatted range, including
text editing, so its track does not move between one-, two- and three-digit values.
Thumbnail previews are non-measuring overlays in fixed 28-pixel slots; loading a
32-pixel GPU thumbnail after reordering cannot widen the row's thumbnails.

## 4. Initial workflows

### A. Build each element on a regular layer

1. Keep the line art visible above the colors and optionally lock it. Choose it as the fill reference, independently of the active editing layer.
2. Create/select a regular paint layer for an element, for example Skin or Clothing. New content goes inside a selected group or above the active sibling; no mandatory group or layer-naming dialog.
3. Draw, lasso-fill or bucket-fill on that layer until its silhouette is ready. Keep using the same layer for related islands. Lasso Fill writes to the current layer on contour completion; Bucket Fill reads the separate reference and writes to the current layer. Neither creates an attached mask or a new layer implicitly.
4. Enable **Alpha lock** explicitly. Further color painting preserves that layer's existing alpha. Disable it when the silhouette needs to grow or be erased.
5. Create another regular layer for the next element. Use **New clipping layer** when shading should remain separately editable instead of painting on the base itself.

The reference role does not automatically constrain freehand brush strokes. Line art is a visual guide while drawing; reference-aware bucket filling actually reads its pixels. A future brush boundary constraint would need its own explicit feature, not a hidden side effect of choosing a reference.

Bucket controls show the named reference and active destination. Initially use one paint/imported-image reference, not all visible layers. Read its content and enabled mask at their document positions, independently of display opacity/visibility so dimming line art does not alter filling. Handle transparent ink and opaque paper-backed imports; missing references disable the operation rather than silently sampling the empty destination. Alpha lock, pixel selections and target editability still constrain writes. Reference assignment never changes the drawing destination.

### B. Mask existing paint or texture to a selection

1. Select the existing layer and make a pixel selection of the area that should remain visible.
2. Choose **Add mask** or **Mask from selection**. Create a mask whose visible coverage equals the selection, hiding everything outside it. Keep antialiased/partial selection coverage.
3. Select the new mask target, linked to its owner by default. Consume the temporary selection so subsequent mask editing is not accidentally confined to it. Creation, target change and selection consumption are one undo action; undo restores the previous state.
4. Paint on the mask to reveal, or erase/use transparent ink to hide. Toggle **Show mask area** to inspect the hidden region over the canvas.
5. Select the content thumbnail to paint or move the existing texture, or the mask thumbnail to edit/move the boundary. The link toggle determines whether canvas movement affects one or both.

The reverse preparation order also works: Add mask without a selection creates a reveal-all mask; after making a selection, choose **Replace mask from selection**. That explicit command replaces coverage as one undo action and retains linkage. Merely pressing Add mask again focuses the existing mask and never silently discards its edits. Selection-to-mask mapping accounts for current content/mask positions, so the visible area matches the selection on the canvas.

Expanding a mask reveals only pixels already present in its owner. An empty paint layer stays empty. This is intentional for a texture mask, not an error to solve with an automatic solid-color layer.

### C. Shade through the layer below

Create/select a regular layer immediately above the base and enable **Clip to layer below**, or use **New clipping layer** to create and enable it in one undo action. Paint on the upper layer normally. Its visibility is constrained by the base coverage, while both layers' pixels remain independently editable. A clipping marker shows the relationship in the list. Clipping does not create a second mask thumbnail; an independently attached mask can coexist with it.

### Discoverability and shortcuts

Keep fill/selection controls contextual and stable, clear of stylus contact. Expose the same actions in menus and the configurable command catalog; no required operation is gesture-only. Proposed defaults remain candidates: `N` new paint layer, `L` Lasso Fill, `G` Bucket Fill, `S` selection, `M` switch content/mask target, `F2` rename, and `Ctrl/Cmd+Alt+G` New clipping layer. Retain existing bindings and test browser conflicts before adopting new ones. In particular, `F` already fits the canvas and `Ctrl/Cmd+N` is browser-reserved. Do not retain a New color layer shortcut for a deferred feature.

All action enablement, reference/destination state, link toggling, target switching, selection consumption, movement and undo behavior live in Rust. Native hosts own gesture recognition and widgets. Text focus suppresses drawing shortcuts; Escape cancels an unfinished contour or move before clearing a completed selection.

## 5. Semantics that prevent subtle bugs

### Alpha lock

Alpha lock preserves a paint layer's existing alpha while recoloring, including partially transparent edges. Painting cannot grow the silhouette, and erasing cannot reduce its alpha while locked. Unlocking permits those edits again. Alpha lock uses the actual paint alpha, not a frozen snapshot, reference layer or separate bitmap, and does not prevent moving the layer. It applies to content edits, not to a separately selected mask.

A full edit lock blocks content/mask mutation, canvas/structural movement, merging and deletion; visibility and temporary inspection remain available. Ancestor protection is inherited. Its command restrictions are distinct from Alpha lock.

### Single-channel mask and editing convention

Store visible coverage `M` in one channel: `0` hides the owner, `1` reveals it, intermediate values partially reveal it. Effective premultiplied content is multiplied by `M`. The mask is separate from pigment alpha, watercolor wetness and temporary pixel selection.

Adopt CSP's alpha-based **editing** convention: colored paint reveals regardless of RGB (black paint reveals too); erasing or explicit transparent-ink painting hides. Do not force a grayscale palette or reinterpret black as a hide command. Zero brush opacity remains a no-op, distinct from choosing the transparent-ink/erase operation. With dab coverage/opacity `q`, the simple scalar operations are:

```text
Reveal: M' = M + q × (1 − M)
Hide:   M' = M × (1 − q)
```

Pressure, tip texture and brush opacity modulate `q`; wet pigment pickup/bleed does not run on the mask. The regular paint color is unchanged by entering or leaving mask editing. Thumbnail black/white visualization is only a display of `M`, not a second painting convention. This follows the interaction documented in [CSP's mask guide](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm); Capy keeps smooth coverage enabled without an extra threshold setting.

### Show mask area

The mask-thumbnail context menu has a checkable **Show mask area** entry. It toggles a tinted canvas overlay for the active mask, not a mask-only black/white replacement view. Use the hidden coverage `1 − M` as the overlay alpha, multiplied by a fixed preview tint opacity. Fully revealed regions have no overlay and show the normal underlying artwork; masked-out regions carry the tint. Partial coverage transitions smoothly.

Use a contrasting purple initially. The scene remains normally composited; the preview does not solo the layer or expose hidden source pixels. It is an inspection overlay drawn in the mask's current canvas position, independent of layer display opacity, never deposited paint, a fill source or exported content. Remember the toggle for the mask during the session; show it while that enabled mask is the active target, and hide it when switching targets. This target-scoped preview policy is a Capy default, not a claim about every CSP preference. No additional full-canvas overlay bitmap is needed.

### Linkage, ownership and movement

Every mask belongs to one layer/group even when unlinked. `linked` controls canvas movement only, not ownership or editing. A mask is linked when created. Clicking the inter-thumbnail link toggles the flag and is undoable; it never changes either image's current position. Re-linking preserves any relative displacement introduced while unlinked.

| Active target | Linked | Unlinked |
| --- | --- | --- |
| Layer content, paint/erase | Edit content only | Edit content only |
| Mask, paint/erase | Edit mask only | Edit mask only |
| Layer content, move on canvas | Translate content and mask by the same delta | Translate content only |
| Mask, move on canvas | Translate mask and content by the same delta | Translate mask only |

Use a Move tool/action operating on the active target; panel row dragging only changes stack placement. A complete move gesture is one undo action and Escape/cancel restores the starting positions. Store independent content/mask positions in their parent coordinate space and update metadata during a drag; do not rewrite the image pixels every frame. Painting and selection-to-mask commands map document coordinates into the selected target's coordinates.

Reordering, reparenting, duplicating, deleting and saving a layer always carry its owned mask, including the link flag and relative placement. Moving a parent group carries its descendant subtree; child masks' unlink flags do not detach them from ancestors. Resolve the movement target set once in Rust so selecting a parent and child cannot apply the same delta twice. Translation is the first implementation; scale/rotate can later reuse the same pairing rule.

### Mask menu and destructive actions

Group the context menu into inspection/protection, coverage operations and a separated final destructive section. Include the exact actions **Show mask area**, **Enable mask**, **Link mask to layer**, **Mask from selection / Replace mask from selection**, **Select mask contents**, **Invert mask**, **Reveal all**, **Hide all**, **Apply mask to layer**, and **Delete mask**. Selection-only actions are disabled without a pixel selection. Masks have no independent layer opacity control in this MVP.

- **Delete mask** removes only the attached mask and selects content. It reveals the owner's previously masked pixels; it does not erase them or delete the layer. Other clipping/opacity/visibility rules still apply.
- **Apply mask to layer** bakes enabled mask coverage, at its current placement, into the owner's raster content and then removes the mask. Preserve premultiplied color/alpha, the visible appearance and layer opacity/blend settings; do not apply those settings twice. Pixels outside the mask are actually discarded. The operation must keep brush/material state consistent so subsequent wet painting cannot resurrect discarded pigment.
- Apply is initially supported for editable raster content; applying a group mask requires explicit group flattening and is not silently substituted. Disable Apply for disabled masks and unsupported kinds, with a core-defined reason. Delete and Apply are distinct, undoable commands; undo restores pixels, mask data, linkage, placement and target as appropriate.

### Clip to layer below

Clipping is a live coverage dependency, not a copied mask. For a single clipped paint layer, use the paint/imported base immediately below in the same parent. A contiguous run of clipped paint layers shares the first non-clipped base below the run, allowing several shading layers over one silhouette. Groups are not clipping bases in the first cut; use a group mask instead. Reject clipping without a valid base.

Use the base's alpha and enabled mask at their current document positions. Opaque regions admit clipped paint, transparent regions hide it and partial coverage modulates it. The base's RGB does not define the boundary. The upper layer retains its own pixels, mask, opacity and blend; changing the base updates clipping immediately. No mask thumbnail or new persisted coverage channel is created merely by toggling clipping.

Resolve clipping as a coverage-preserving stack: blend upper contributions inside the base coverage, then apply the base coverage/mask/opacity once. Do not repeatedly source-over already-base-masked layers and inflate translucent edges. A hidden base hides the stack; the base blend mode applies to the resolved stack. This is the Capy compositing contract, not a promise of bit-identical CSP blend results.

Preview the resulting base when reordering. Do not silently unclip dependent paint if a base is removed or moved away: reject the operation unless it includes the dependent stack or the artist explicitly releases/reassigns clipping. Deleting a whole clipping stack must name that scope and remain undoable. Mask linkage does not imply movement linkage between separate clipped layers and their base.

### Groups and mutations

Groups initially composite in isolation, including child clipping stacks, then apply the group's mask, opacity, and blend mode once to the combined result. Do not multiply group opacity into each child before combining them. Pass-through groups are deferred, and grouping over an external backdrop may therefore change appearance; preview that change rather than promise otherwise. The isolated default follows the documented CSP option, whereas Photoshop defaults differently. [CSP folders](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_folders.htm), [Photoshop group blending](https://helpx.adobe.com/photoshop/using/layer-opacity-blending.html).

Move, duplicate, save, and undo carry masks with their owners. Multi-selection normalizes ancestor/descendant duplicates so the same subtree is not processed twice. Row drop hints distinguish above, below, and inside; never reuse workspace-panel docking rules for artwork layers. Touch scrolling and reordering need distinct recognition, and all operations have menu alternatives.

Ungrouping a masked, translucent, or specially blended group is not necessarily appearance-preserving. Current Ungroup accepts only unlocked, full-opacity, unmasked Normal groups whose direct children also use Normal blending; removing isolation otherwise changes child/backdrop blending. Positions, mask positions, order and references survive ungrouping. The merge milestone should start with simple Normal merges, whole clipping-stack baking, and isolated-group flattening when representable. Those merge operations are not implemented yet. Every destructive command is undoable and clearly named.

## 6. Core and renderer responsibilities

Extend the existing design rather than introduce another library layer:

| Existing owner | Responsibility |
| --- | --- |
| `layer-core` | Stable layer IDs, ordered children, content kind, owned mask/link flag, content/mask positions, clipping relations, locks, blend/opacity and validated document operations |
| `layer-ui` | Active content/mask target, selected layer IDs, independent reference/destination, mask-overlay state, selection/Move tool state, command enablement, menus/copy and shortcut mappings |
| `layer-engine` | Ordered edits, atomic history/move gestures, GPU damage scheduling, reference-version checks, selection/mask/fill/paint ordering and mask application |
| `layer-render-wgpu` | GPU mask/selection/paint operations, reference-based fill, translated content/mask sampling, clipping/group composition, thumbnails and mask-overlay presentation |
| Native / web hosts | Widgets, pointer/key normalization, focus, presenting the same semantic state, and platform surface lifecycle |

Use paint and group content alongside the existing special/imported kinds. A mask is attached coverage data plus enabled/link/position metadata, not an independent stack entry. No flat-color content type is required for the initial workflows. Do not add another crate, a separate region document, duplicated layer tree or per-platform masking controller.

Keep mask storage sparse and single-channel. A 256×256 R8 page is 64 KiB; a matching RGBA8 paint page is 256 KiB. A fully populated 4096×4096 R8 mask is 16 MiB, or 25% of one RGBA8 layer's 64 MiB, before allocator/history/temporary overhead. Store explicit default coverage: reveal-all starts at `1`, while selection-created coverage defaults to `0` outside its stored area. Sample missing regions using that default after coordinate conversion. Alpha lock and clipping reuse existing alpha instead of allocating another mask bitmap.

Selection coverage can transfer ownership to a new mask when its lifetime permits; avoid speculative storage aliasing with brush coverage or wetness. Undo and later selection edits must never mutate an attached mask accidentally. GPU masks, blend operations, and selection coverage stay off the CPU raster path. CPU geometry preparation, document validation, and input dynamics remain appropriate.

Keep ordinary Normal/unmasked painting on its existing fast path. Reuse persistent intermediate resources only where groups, clipping, or blend modes require them; avoid full-document allocation per group, per frame, or per thumbnail. Update visible thumbnails from damaged GPU content at a bounded rate. If native widgets require thumbnail bytes, read back only the small preview asynchronously, never full layers or on the input-critical path. This is an explicit small-preview exception to the current export-only readback policy and must be documented if adopted.

Do not claim the extension will meet 120 Hz merely because it uses the GPU. Rendering/backdrop sampling and deep isolated groups can dominate. Validate representative documents and report end-to-end frame timing as well as GPU/submission timings.

Lasso Fill, Bucket Fill and staged selections produce coverage for shared fill/mask commands. Their normal paint destination is the already-active regular layer; they do not need separate automatic layer-creation implementations. Bucket extraction is a bounded, scheduled GPU operation against a frozen reference revision; avoid a CPU image download or a synchronous document-wide loop on the UI thread. Large fills may finish over multiple frames while navigation remains responsive. Report fill completion latency separately from frame latency, and discard/cancel stale results after reference, destination, positions or document changes.

## 7. Initial acceptance tests

1. Build ten elements on explicitly created regular layers using brush, Lasso Fill and reference-aware Bucket Fill. Multiple strokes/islands stay on the active layer; no automatic masks, new layers or Alpha lock on pen lift.
2. Enable Alpha lock and recolor opaque/partial-alpha edges without changing alpha; an empty locked layer cannot gain paint. Unlock to grow/erase the silhouette. Editing an attached mask is not blocked by the content's Alpha lock.
3. Mask an existing texture from a multi-island selection with holes and antialiased edges. Only selected coverage remains visible; mask creation/selection consumption undo together. Also test Add mask first, selection second, then explicit replacement.
4. On a mask, black, white and colored brushes reveal identically for the same opacity/coverage; eraser/transparent ink hides. Zero opacity does nothing. Pigment, wetness and the artist's drawing color remain unchanged.
5. Show mask area tints hidden regions and leaves revealed artwork clear. Test partial masks, active-target changes, disabled masks and moved masks; thumbnails/fill references/export must not include the preview tint.
6. Test all four target/link movement cases, unlink → offset → relink without jumping, drag cancellation and one-step undo. Reordering/reparenting carries even an unlinked mask; parent/child multi-selection never doubles a movement.
7. Delete mask restores unmasked content. Apply mask preserves current appearance while removing the mask; undo restores coverage, positions and pixels. Test partial edges, moved masks, opacity/blends and later wet/smear edits so discarded paint does not return.
8. Test a layer with both an attached mask and clipping, several clipped siblings over one base, base mask/opacity/visibility, soft edges and live base movement. Reject invalid/missing bases and unsafe structural edits without silently exposing paint.
9. Verify group opacity/mask is applied after child composition; overlapping children must not produce doubled attenuation. Save/load and history preserve masks, links, target positions, clipping and groups.
10. Test thumbnail/link/grip hit targets, deep groups, multi-selection, mixed values, menus, shortcuts and text-focus exceptions on GTK/web/Android. Painting, canvas movement and row reordering must not be confused.
11. Fill transparent ink and opaque paper-backed references onto separate regular layers. Test dimmed/hidden/missing/moved references, partial edges, gaps, selections and Alpha lock; destination edits never alter the reference source.
12. Benchmark the existing brush cases plus masks, clipping, live linked/unlinked moves, Show mask area, mask application, fills and nested groups at 2K/4K. Report p50/p95/p99, memory, input-to-frame timing and hardware. Use 8.33 ms as the 120 Hz frame budget, not as a promise for whole-document operations.

## 8. Highest-priority follow-ups

| Priority | Feature | Why next / proposed home |
| --- | --- | --- |
| 1 | More robust reference fill: gap closure, enclose-and-fill, multiple/group references and unfilled-area cleanup | Extends the initial bucket to sketchier linework. Keep the same source/destination UI and region creation path; no separate automatic flatting document. |
| 2 | Better boundary refinement: grow/shrink, feather, narrow-gap cleanup, saved selections | Reduces halos and repeated cleanup. Selection tool and mask controls. |
| 3 | Scale/rotate/free transform for layers and masks | Extend the initial translation and link rules; do not defer basic movement or linkage behind this follow-up. |
| 4 | Masked adjustment layers, beginning with hue/saturation and curves; gradient maps next | Global/local recoloring and value correction without repainting. Creation menu and properties tab. |
| 5 | Pass-through groups, broader blends and more appearance-preserving merge/copy-visible operations | Compatibility and complex shading workflows. Blend menu and secondary actions. |
| 6 | Layer search, color tags, locate-layer-from-canvas, larger thumbnail option | Large flatting documents become hard to navigate. Search is collapsed until invoked; tags stay subtle. |
| 7 | Flat-color/fill layers and optional selection-to-color-layer creation | Useful later for recolorable shapes; not a replacement for the initial regular-layer plus Alpha lock workflow. |

Do not start with automatic semantic segmentation, vector masks, smart objects, animation, layer styles, or a full PSD compatibility promise. Those are larger projects and do not replace reliable lasso fill and reference-aware bucket workflows.
