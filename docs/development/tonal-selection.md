# Tonal range selections

GTK, Web and Android expose **Tonal range** in the Select family. It writes ordinary byte-coverage
selections and saved selection masks. The shared Rust model supplies the same
choice, numeric and selection-mode controls to each host's panel and Tool Options bar.
The panel presents five SDR tones (Shadows through Highlights, with Midtones in
the middle) and Custom in one horizontal segmented radio-button bar. Floating-point
HDR documents add Bright HDR before Custom; SDR wide-gamut documents do not.
Each segment has a distinct band-profile icon, ordered from dark to bright, with
a range-handle icon for Custom. Tooltips and accessibility labels give the preset
name and exact full-strength range in stops relative to reference white. Tool
Options uses the same segmented choice and icons, including in its overflow panel.
If the complete group cannot fit in a narrow toolbar, that existing overflow
exposes the horizontal bar without splitting or hiding individual presets.
Each host uses a reusable numeric row with the label, slider and editable value on one
line. The Tool settings panel has a minimum width of six small tiles and their
gaps inside the content insets (242 logical pixels total). The Select command
list also uses compact rows so it does not force the adjacent panel to be taller.
The shared `tonal-select` icon shows tonal bands inside a dashed selection border.

Selection mode comes first. Choose one tone, or click/drag on the canvas to sample
a custom interval. The result applies immediately. **Softness** and **Feather**
refine it. In the panel, horizontal Tool Options and its overflow, **Custom** adds one interval slider
with two movable ends, an editable low value on the left and high value on the
right. There are no visible bound labels or unit suffixes; tooltips specify stops
relative to reference white (0). Endpoint boxes fit their one-decimal readouts,
leaving the remaining row width to the track. Bounds stop at one another. The
existing numeric editors retain typed expressions, and each native range handle supports keyboard
and accessible adjustment. Escape during a drag restores that endpoint.
The track normally spans −12 to +6 stops and expands to include typed or sampled
values outside that interval, staying fixed during capture. Both ends remain
separately reachable when the interval is zero. The shared Tool Options schema
groups the bounds into one field, rendered by each host's reusable range component.
Its horizontal toolbar row reserves 280 pixels for the compact values and track;
the entire field moves into overflow when it cannot fit. The existing toolbar
slider-visibility preference hides the track while retaining both editable values.
Horizontal preset bars retain the standard 24-pixel form-control height.
There is no Apply/Cancel workflow, source
selector, range manager, destination label, or selection-actions menu in this
panel. Invert remains an independent selection/mask action in existing menus.

Opening the tool, changing combination mode, or changing destinations does not
modify a selection. A new tone choice or canvas sample starts an undo operation;
subsequent numeric refinements amend that operation, preserving its starting
coverage. Undo returns to that starting coverage and Redo restores the final
result. Refinement is allowed only while the exact document revision and target
still match. History admission checks retain the original inverse and account
for its memory. Tool changes retain the applied result and cancel pending work.

To combine named regions, choose **Add** and then another tone. To constrain tones
spatially, make a lasso selection first and choose **Intersect**. Refinements use
the operation's original baseline, so repeated softness/feather changes do not
repeatedly erode an already-softened mask. Shift/Alt modifiers latch for a sample
and its refinements; they do not change the configured mode of the next operation.

Sampling always reads **visible artwork**, before display mapping/proofing and
without selection or mask overlays. It does not silently switch to mask pixels
when entering Quick Mask or selecting a selection layer. Use the ordinary layer
visibility and Solo controls to isolate an exposure. Layer opacity, masks and
transforms participate in the visible artwork composite.

| Editing destination | Display | Updated coverage |
| --- | --- | --- |
| Ordinary selection | Standard marching ants | Current selection |
| Quick Mask | Existing mask shading/settings | Current selection, remaining in Quick Mask |
| Selection layer | That layer's mask shading/settings | Selected mask; current selection is unchanged |

Q changes the display of an existing current selection without restarting the
operation. Switching selection layers ends the operation; the new mask is changed
only by the next tone choice, numeric adjustment or canvas sample.

| Preset | Full-strength interval, stops relative to white |
| --- | --- |
| Shadows | below −5 |
| Mid-shadows | −5 to −3.5 |
| Midtones | −3.5 to −1.5 |
| Mid-highlights | −1.5 to −0.5 |
| Highlights | above −0.5 |
| Bright HDR (HDR documents only) | above +1 |

These are luminance intervals, not camera exposure metadata. The GPU computes
`log2(Y)` from unassociated linear RGB using the document primaries' XYZ Y row.
RGB 1 is reference white (203 cd/m²); HDR values above 1 remain available. Zero and
negative luminance use the black endpoint; transparent pixels supply no coverage
or sample weight. At 100% Softness each finite bound has a 0.5-stop smooth shoulder;
0% gives a hard threshold, and 200% gives a one-stop shoulder. Feather is spatial
smoothing in image pixels, using the existing GPU selection refinement.

A mouse/pen click samples a 5×5 alpha-weighted linear average and initially selects
a one-stop range around it; later clicks preserve the current custom width. A
rectangle samples the central 90% of its luminance distribution, with 1/16-stop
bins. Both select matching tones throughout the image, rather than spatially
clipping to the sample rectangle. GTK retains its existing finger navigation.
GPU requests coalesce through the existing single-flight region queue. The GPU
returns packed coverage and small sample summaries, never a full color image for
CPU classification. Hover performs no tonal sampling.

Workspace decoding accepts the original band-editor format, migrating the active
included band to a preset or Custom. Retired Deep shadows settings become Custom
with the same interval. Bright HDR becomes Custom if restored into an SDR document.
Retired command IDs remain decodable but unavailable, so old shortcuts/layouts
cannot activate the removed editor.

## Checks

See [photographic-size performance](tonal-performance.md) for memory bounds,
61 MP Huion measurements and the recovery regression.

- `cargo test --locked -p layer-core refinement_` checks original Undo/final Redo,
  current and saved masks, checkpoint identity and stale-history rejection.
- `cargo test --locked -p layer-ui tonal_` covers direct edits, baseline
  combination, refinements, mask destinations, sampling, stale results, shared
  controls and workspace compatibility.
- `cargo test --locked -p layer-render-wgpu tonal_` needs a hardware GPU and checks
  HDR/SDR coverage, composites, transparent samples and percentiles.
- `tools/performance/workspace-motion.sh gtk --native-test=native_tonal_selection_input`
  checks the single-row preset bar, its height, ordering and range tooltips,
  native numeric edits, automatic updates,
  actual outline/shading pixels, both mask destinations, sampling and undo/redo.
- `native_tonal_toolbar_input` checks horizontal and vertical Tool Options and
  the same segmented bar and numeric form in overflow.
- `native_tonal_toolbar_range_input` checks the inline interval's mouse/touch
  handles, keyboard and numeric edits, Quick Mask context, narrow-bar overflow,
  and retirement on tool changes. `native_tonal_toolbar_range_pen_input --tablet`
  checks immediate pen adjustment of both inline ends.
- `native_tonal_range_input` checks both slider ends with mouse and touch,
  keyboard adjustment, drag cancellation, typed
  bounds outside the usual track domain, coincident handles and one-step history.
- `native_tonal_range_pen_input --tablet` separately checks both ends through
  the injected Wayland pen fixture; numeric editing uses the unproxied journey.
- `native_tonal_selection_pen_input --tablet` checks injected Wayland pen sampling
  in Quick Mask; this is not a physical-device test.
- `apps/layer-web/device.test.mjs --tonal-selection` runs against an isolated
  browser origin through `LAYER_DEVICE_CDP` and `LAYER_WEB_URL`. It checks both
  presentations, mouse/touch/pen endpoints, compact numeric edits, cancellation,
  Quick Mask pixels, saved masks, sampling and the HDR preset. The desktop Web
  harness accepts the same flag.
- Android `AndroidInteractionTest#tonalRangePanelsAndToolbarAcrossDevices`
  checks compact panel/toolbar geometry, both endpoints with mouse, finger and
  stylus, keyboard/numeric edits, cancellation, Quick Mask pixels and saved masks.
  Pass `-e systemInput true` to replay through Android's OS input dispatcher.
  `AndroidRasterTest#tonalHdrCoverageAndSamplingOnDevice` checks actual Vulkan
  mask coverage above reference white and sampling through the overlay.
  Build/install with a separate `capyApplicationId` to isolate device tests.

Host tests replay input events on the device; they do not verify physical pen
pressure or the feel of moving the attached pen by hand.

Use the isolated harness, not the user's desktop. Screenshots and machine-specific
logs belong in ignored `artifacts/` or `/tmp`.
