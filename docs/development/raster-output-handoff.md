# Raster output preparation investigation

[Developer guide](README.md)

Investigate the shared renderer failure `accepted raster output is not prepared`.
Two source-level inconsistencies are identified below; neither is yet tied to
the observed Windows failure by a failing frame or call stack. Delete this
temporary handoff when the investigation is resolved and regression tests carry
the evidence. This handoff is committed at the user's explicit request.

## Evidence and limits

- The failing Release app was built from `33ddb23eb9e684f30e17d1c08ee82acf1b7a77ff`.
  Its visible status was exactly:
  `canvas renderer failed: Renderer invariant failed: accepted raster output is not prepared`.
- An older diagnostic build based on `f368c5051` showed no such status, but it
  used a different profile and included DRED instrumentation. This is not a
  matched A/B reproduction.
- Code inspection compared the memory refactor `5e196eb79` with its parent
  `39e1a26b0`. The relevant planning and execution branches remain unchanged at
  `a83a55d3f`, checked before publishing this handoff.
- No reproduction test was run for either finding. The final review involved
  no app launches, builds, input injection or source changes. An earlier runtime
  probe failed in its own control lookup and supplies no reproduction evidence.
- The message comes from `prepared_raster_output` in
  [renderer lib.rs](../../crates/layer-render-wgpu/src/lib.rs). It covers both
  undeclared outputs and missing physical storage at several call sites, so the
  message alone does not identify the failing branch or prove memory exhaustion.

## Finding 1: the combined preview rectangle exceeds the accepted tile set

The mismatch is a source-level regression introduced by `5e196eb79`:

1. `raster_outputs` in [staging.rs](../../crates/layer-render-wgpu/src/raster/staging.rs)
   inserts `PreviewColor` for each preview batch's `brush_display_damage`
   rectangle separately when `sparse_preview` is false.
2. `submit_frame_inner` in [lib.rs](../../crates/layer-render-wgpu/src/lib.rs)
   accumulates `new_preview_damage` with `PixelRect::union`. This is an enclosing
   rectangle, not a union of tile sets.
3. With `new_preview_from_persistent == false` and no sparse contact set, the
   seed loop visits every page of that enclosing rectangle and calls
   `accepted_raster_output(layer_id, PreviewColor(coordinate))`.
4. A corner page absent from every individual batch fails that membership check,
   even if a leftover allocation happens to exist for the coordinate.

For example, one batch covers page coordinates `{0,1} x {0,1}` and another
covers `{1,2} x {1,2}`. Preparation accepts seven color pages. Seeding visits
nine, including unaccepted `[2,0]` and `[0,2]`.

This is not restricted to an artificial multiple-preview API call.
`build_predicted_preview` -> `push_active_preview` -> `push_batches` in
[canvas.rs](../../crates/layer-engine/src/canvas.rs) splits `BrushExecution::Wet`
previews at `MAX_WET_DABS_PER_BATCH == 3`, recalculating each chunk's damage.
Multiple nonempty previews disable `preview_from_persistent`; Wet execution also
disables `sparse_preview`. A tail with the geometry above therefore reaches the
mismatched traversal. Watercolor previews have a separate unlimited-batch rule;
do not assume the same three-dab splitting applies to them.

Before the refactor, `ensure_preview_pages(new_preview_damage, sparse)` allocated
the same enclosing rectangle that the seed loop traversed. The new preparation
uses the smaller per-batch set while the seed loop retains the old traversal.

## Finding 2: an empty contact plan still declares preview storage

This is a separate code-level invariant violation for valid renderer input.
Its production engine/UI trigger still needs to be demonstrated.

`brush_tiles::plan` in [brush_tiles.rs](../../crates/layer-render-wgpu/src/brush_tiles.rs)
uses per-contact planning for stable blenders: Smudge with Dither or Scatter.
However, `sparse_preview` returns false for those execution modes, and
`brush_work_planned` only checks dab count and rectangular damage for non-Liquify
batches. Consequently, an empty tile list can coexist with nonempty declared
`PreviewColor` output.

A concrete packet witness is one preview batch with two unswept round dabs:

- Centres `(-25, 0)` and `(0, -25)`, radii `[20,20]`, rotation `[1,0]`,
  `previous[0] == 0`, and no contact model or transport expansion.
- Use Smudge/Dither or Smudge/Scatter, with `edge_after_stroke == false`.
- Set batch damage to the union of the two dab bounds. Including the one-pixel
  margin in `Dab::bounds`, those bounds are `[-46,-21]..[-4,21]` and
  `[-21,-46]..[21,-4]`. Each clips to empty against the canvas, but their union
  clips to `[0,0]..[21,21]` on a canvas of at least 21 pixels per axis.

The contact planner returns no tiles, while `raster_outputs` declares
`PreviewColor([0,0])`. In `submit_frame_inner`, `tiles.is_empty()` skips the
batch, leaving `new_preview_layer == None`. Submission then clears preview
pages instead of calling `prepare_preview_outputs`. `validate_raster_outputs`
requires the declared preview color page and emits the same invariant error.

The empty-contact skip predates the refactor. The new output declaration and
validation make its disagreement with preparation fatal. This failure needs
only one preview batch and occurs before the seed loop in finding 1.

## Other explanations inspected

The inspected live restoration path uses `publish_prepared_restore`, preserving
the frame key; it does not replace it with a Direct restore key. The inspected
native-color eviction path protects current required coordinates and
destinations. Neither supports the earlier speculation that ordinary eviction
or restoration simply discards the active output plan. This is not a proof that
all resource-lifetime paths are correct.

Neither finding by itself explains ordinary single-batch, in-canvas Dry painting.
If that was the failing interaction, continue tracing the other callers of
`prepared_raster_output` instead of declaring this incident solved.

## Next work and completion criteria

1. Add focused regression cases to the existing shared Rust suites: the
   multi-batch Wet geometry, and the empty-contact/nonempty-bounds blender case.
   Prove they fail before the fix. Separately establish engine-generated input
   for the second case and identify which branch explains the reported failure.
2. Make the accepted output plan determine preview existence and every tile
   consumed by allocation, seed/copy, validation and encoding. Preserve required
   nonlocal inputs and the memory budget. Do not suppress the invariant, retry
   the frame or allocate unplanned pages opportunistically during encoding.
3. Keep planning assertions windowless and independent of a GPU where possible;
   use the existing renderer harness on hardware for pixel and material-state
   checks. Cover persistent-plus-preview work, sparse Dry previews, watercolor
   halo state, preview reuse and crossing tile/canvas boundaries as applicable.
4. Follow [testing](testing.md), [memory architecture](memory-architecture.md),
   [performance measurement](../performance/measuring.md) and the
   [diff review](../COMMIT_GUIDE.md#review-the-diff) for any eventual code change.
   Report the original incident as unresolved until its trigger is accounted for.

The current task authorizes this handoff only. The user requested code inspection
without new app launches or speculative UI testing; keep that scope unless the
user changes it. Keep the user's running apps and profiles untouched, and leave
the unrelated root files `effects.rs`, `layout.rs` and `recovery.rs` alone.
