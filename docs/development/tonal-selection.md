# Tonal range selections

GTK exposes **Tonal range** in the Select family. It produces the same byte
coverage selections as the other selection tools: existing painting, Quick
Mask, saved selections, layer masks, and selection combination need no new
mask type. The GTK Tools panel and Tool Options bar consume the same shared
list, text, numeric, and command definitions.

The default controls are **Tones**, **Sample from**, **Softness**, and **Feather**,
plus the standard selection combination modes, Invert, and Apply/Cancel.
Choose **Visible image** (the default) or a specific artwork layer in **Sample from**.
The source stays pinned when changing the editing destination. Raw layers
are sampled before opacity and masks; composite sampling includes their rendered
appearance but excludes selection overlays, proofing, and display mapping.
Quick Mask and selection layers are destinations and never sampling sources.

Check one or more entries in **Tones**. **Edit range** reveals the active range,
its name, From/To bounds, edge widths, and saved-range controls. Editing a built-in preset creates
an enabled custom copy and disables that preset; other included bands remain.
Custom names and recipes survive workspace capture. **Save range** adds or
updates an application preset by name; **Saved ranges** copies one into the
current recipe. Up to 16 bands, including built-ins, can be in the recipe.

| Preset | Full-strength interval, stops relative to white |
| --- | --- |
| Shadows | below −5 |
| Mid-shadows | −5 to −3.5 |
| Midtones | −3.5 to −1.5 |
| Mid-highlights | −1.5 to −0.5 |
| Highlights | above −0.5 |
| Deep shadows | below −7 |
| Bright HDR | above +1 |

These are luminance intervals, not camera exposure metadata. The GPU computes
`log2(Y)` from unassociated linear RGB using the document primaries' XYZ Y row.
RGB 1 is reference white (203 cd/m²); values above 1 stay available in HDR.
Zero and negative luminance use the black endpoint. Transparent pixels supply
no selection coverage or sample weight.

The default smooth falloff extends 0.5 stops beyond each finite bound.
**Softness** scales every included range's falloff from 0% (hard threshold) to
200%, with 100% as the default. It does not modify named presets. Under Edit range,
linked **Edge width** edits both shoulders; unlinking allows independent values. Include
Darker/Brighter controls remove the corresponding bound. Bands combine by
maximum coverage, so overlap does not strengthen a mask. Invert applies to the
combined tonal criterion; source alpha still limits coverage. Spatial feathering
uses the existing GPU selection refinement in image pixels.

Mouse/pen hover reports a 5×5 linear average without changing the selection.
A click samples that footprint into the active band, initially one stop wide;
later clicks preserve a custom finite width. A dragged rectangle samples the
central 90% of its luminance distribution with 1/16-stop bins. Rotated and sheared
layers use the exact inverse-transformed sampling footprint. Matching tones
are selected throughout the source. To restrict the result spatially, make a
lasso selection first and choose Intersect. GTK retains its existing finger
navigation; touch controls use normal native widgets.

| Editing destination | Preview | Apply result |
| --- | --- | --- |
| Ordinary selection | Standard marching ants | Current selection |
| Quick Mask | Existing Quick Mask shading and overlay settings | Current selection, staying in Quick Mask |
| Selection layer | That layer's mask shading and overlay settings | That selection layer; current selection is unchanged |

Toggling Quick Mask preserves a ready current-selection preview and the artwork
source. Switching to a different selection layer restarts the preview against
that layer's mask. The source choice stays unchanged. A short destination label
and **Apply selection** / **Apply mask** identify where the result will go.

Previews retain an immutable starting selection or destination mask. Every recomputation combines
against that baseline, rather than repeatedly intersecting an already-softened
result. Apply commits one history edit. Cancel, Escape, and tool changes discard
the draft. Shift/Alt combination modifiers latch for a sampling gesture and its
subsequent preview adjustments; they do not replace the configured mode after
Apply. Sampling and parameter changes coalesce through the existing
single-flight GPU region queue; hover uses the bounded color-sampling queue.
The GPU classifies source tiles and returns packed coverage and small probe
summaries. It never downloads a full color image for CPU classification.

## Checks

- `cargo test --locked -p layer-ui tonal_` covers drafts, baseline combination,
  completion-frame display publication, single-step history, cancellation,
  mask destinations, global softness, saved bands, workspace capture, stale
  controls, and shared options.
- `cargo test --locked -p layer-render-wgpu tonal_` needs a hardware GPU. It
  compares HDR masks with a scalar luminance oracle, checks SDR paint and
  composites across tiles, and exercises transparent samples and percentiles.
- `tools/performance/workspace-motion.sh gtk --native-test=native_tonal_selection_input`
  exercises mouse sampling, touch presets, text/numeric entry, neutral ordinary
  previews, Quick Mask shading, pinned artwork sources, selection-layer writes,
  global mask coverage, cancellation, and undo/redo.
- `native_tonal_toolbar_input` checks horizontal and narrow vertical Tool Options,
  retained lists, text entry, pinned source choice, and the complete overflow form.
- `cargo test --locked --release -p layer-render-wgpu tonal_preview_latency -- --ignored --test-threads=1 --nocapture`
  measures complete 24MP HDR preview/readback and full-image sampling on an
  opaque uniform composite (seven bands, no spatial feather). Run serially.
- Run `native_tonal_selection_pen_input` through the same harness with `--tablet`
  for injected Wayland pen sampling in Quick Mask. This is not a physical-device test.

Use the isolated harness, not the user's desktop. Screenshots and machine-specific
logs belong in ignored `artifacts/` or `/tmp`.
