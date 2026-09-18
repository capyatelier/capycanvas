# HDR → SDR mapping and compact Proof panel

Review update, 2026-09-18. Source: `~/code/capycanvas3`, branch `capycanvas3`.
Checkpoints are pushed to `origin/capycanvas3` for review; main is not updated.
Launch `artifacts/color-m4/review/launch.sh` for user review.
[Float32 layers are separately scoped](../development/float32-hdr-scope.md).
This update does not implement Float32 documents, RAW, PSD, CMYK layers or OCIO.

## Findings from the code and browser research

The initial shoulder spent nearly all SDR range below HDR reference white:
neutral RGB 1, 4 and 16 became sRGB8 240, 253 and 255. EV was present, but its
highlight differences were compressed almost away. The next candidate in
`e43105b1` reserved more highlight range, but its luminance-preserving gamut
compression pulled saturated highlights toward neutral. That produced the flat,
pastel result reported in review. Both paths are replaced, not retained as modes.

There is no single mandatory browser HDR-to-SDR algorithm. The relevant path
also depends on whether the asset is PQ/HLG, contains an authored gain map or
adaptive tone metadata, and whether rendering happens through a video overlay.
A browser's SDR rendition of a gain-map image may simply be its authored SDR
base, not a generic tone map. The [W3C ColorWeb overview](https://github.com/w3c-cg/ColorWeb-CG/blob/main/hdr-big-picture.md)
explains the distinction between gain maps and gain curves.

Primary implementation sources checked:

- [Current Chromium ToneMapUtil](https://github.com/chromium/chromium/blob/main/cc/paint/tone_map_util.cc)
  passes HDR reference white, content-light, mastering and optional AGTM metadata
  to Skia's `Metadata::makeToneMapColorFilter`.
- [Skia RWTMO implementation](https://github.com/google/skia/blob/12dcafc24c49f369b1df66efd17369fc1c3d5474/src/codec/SkHdrAgtm.cpp)
  supplies **Reference White Tone Mapping Operator (RWTMO)** when an authored
  headroom-adaptive curve is absent. It uses linear Rec.2020 max RGB, applies one
  gain to all channels, and leaves alpha alone. Source peak comes from MaxCLL,
  then mastering luminance, then a 1000 cd/m² fallback. Reference white defaults
  to 203 cd/m². Current source was pinned on 2026-09-18; the numerical oracle
  uses the same curve construction from `bc94efd2229aad1048edbf892a6b2e7db28b22c4`.
- [Chromium 142's earlier fallback](https://github.com/chromium/chromium/blob/142.0.7444.97/cc/paint/tone_map_util.cc)
  uses an extended Reinhard gain, `(1 + T*M/S²)/(1 + M/T)` for source peak S,
  destination peak T and max channel M, again in linear Rec.2020. It is useful
  historical context, but is not the current Skia default.
- [Apple's HAGC documentation](https://developer.apple.com/documentation/colorsync/headroom-adaptive-gain-curve)
  also identifies RWTMO as the recommended fallback for ISO 22028-5 imagery.
  CoreGraphics/CoreImage/CoreAnimation can consume HAGC metadata. This supports
  the operator choice, not a claim that every Safari version or media route
  produces byte-identical pixels.

Other relevant families:

| Approach | Appropriate use / tradeoff | Decision |
| --- | --- | --- |
| RWTMO | Display-referred HDR, explicit reference white and source range; linear lower tones and a smooth highlight shoulder. | Default. |
| Scale | Divide linear RGB by a chosen peak. Preserves ratios and linear contrast, but darkens the whole image. | Explicit alternative. |
| Clip | Keep linear brightness, then limit out-of-range destination components. Brighter, with intentionally lost highlight/color detail. | Explicit alternative. |
| Extended Reinhard | Small global operator, formerly Chromium's fallback. Compresses lower tones continuously and depends on source peak. | Researched; not another redundant UI choice. |
| ITU-R BT.2446 | Multiple HDR/SDR broadcast conversion methods, with defined encoding/color assumptions. | Valid future comparison, not one universal browser default. [ITU report](https://www.itu.int/pub/R-REP-BT.2446-1-2021) |
| ACES output transforms | Scene-linear ACES → a specified display, including rendering and gamut behavior. | Requires a scene/working-space contract that differs from Capy's display-referred HDR. Not a drop-in default. [ACES documentation](https://docs.acescentral.com/system-components/output-transforms/) |
| Local/adaptive tone mapping | Can expose local detail but needs spatial analysis, halo controls and more memory/work. | Not added to the simple global rendition panel. |

## Implemented transform and controls

Default **Tone map** adopts the RWTMO curve and fixed Rec.2020 gain domain.
The source range defaults to 1000/203 = 4.926×, or **2.30 EV** above reference
white. It is an authored rendition parameter, not the current monitor's peak
and not a claim to have measured the edited document. Imported content-light
metadata may become stale after edits; automatic edited-image peak analysis
is not implemented. The range control deliberately makes the assumption editable.

For peak P = 2^H and neutral settings, SDR reference white is
`W = 1 - 0.5 * min(H / log2(1000/203), 1)`.
Below RGB max 1, use the gain W. Between 1 and P, use the quadratic Bezier through
`(1,W)`, `(.35 + .65/W, .35*W + .65)` and `(P,1)`. Above P, the tone curve holds
at SDR white, retaining channel ratios before destination gamut limiting.
A zero-EV range reduces to clipping. This is a delivery/view derivative only.

**Deliberate numerical difference from Skia:** evaluate the underlying Bezier
analytically, with stable quadratic inversion, instead of its eight-point
Hermite approximation of log gain. An independent build of the pinned Skia C++
functions found reversals at 8 stops and a maximum 67.998 output at 16 stops.
The analytic curve remains bounded and monotonic throughout 0–16 stops.
At 203–1000-nit source ranges it differs from the browser approximation by less
than 0.0006 linear RGB (less than one 8-bit SDR code). This is an RWTMO default,
not a claim of exact browser framebuffer reproduction or full ST 2094-50 support.

The three controls have distinct effects:

- **Exposure:** independent linear gain in stops, after the contrast adjustment.
- **Contrast:** adjusts the Rec.2020 max-RGB tone scale around linear 0.18 before
  mapping; it darkens shadows and raises highlights without adding desaturation.
- **HDR range:** input endpoint in stops above reference white. Increase it to
  retain brighter highlight distinctions; lower it for a brighter SDR rendition.
  It also sets the divisor in Scale mode. Hidden in Clip mode, where it has no effect.

One common gain scales signed RGB. Final out-of-gamut destination channels are
limited to SDR; there is no extra compression toward gray. This retains more
color but cannot preserve every HDR/wide-gamut color in the smaller SDR volume.
Built-in delivery reports output clipping through the existing statistics; ICC
proof input limiting is counted too. HDR master samples, alpha, history,
reference white and precision are untouched. No automatic precision downgrade.

| Neutral HDR input | Initial sRGB8 | Rejected candidate | RWTMO default |
| --- | ---: | ---: | ---: |
| 0.18 | 118 | 118 | 85 |
| 1 | 240 | 202 | 188 |
| 2 | 250 | 224 | 224 |
| 4 | 253 | 238 | 249 |
| 16 | 255 | 250 | 255 |

RWTMO intentionally lowers diffuse tones to make SDR room for HDR highlights;
Exposure controls the desired brightness. Raise HDR range above the default
when the picture contains meaningful values beyond 1000 nits. For the saturated
linear sRGB sample `[4,1,.25]`, the rejected candidate produced `[255,207,193]`;
the new default produces `[255,148,77]`. Numerical results are retained in
`artifacts/color-m4/browser-mapping-numerical.csv`.

Rust CPU delivery, WGSL canvas/thumbnails, native picker previews, SDR export and
mapped print proofing share this contract. RGB delivery limits after destination
primary conversion. ICC delivery and print LUTs use the same bounded working-RGB
preparation before the CMM. Ordinary SDR documents and HDR delivery retain their
existing transforms. The saved recipe includes method, exposure, contrast and
headroom. Previous review settings retain exposure/contrast and map the old
shoulder setting to headroom around the same neutral point; their SDR appearance
intentionally changes with this correction. No obsolete mapper is retained.

## Compact Proof and user flows

**Proof** shares Color's tab group in the Paint and Photo defaults. Customized
placements are preserved; opening an unplaced Proof panel joins Color when
available. Existing tab/title drag behavior supplies floating and docking.

**SDR:** Method plus three single-line labeled slider/value rows. Preview,
Revert and Apply form a fixed footer. The overflow menu contains Compare saved
and Reset SDR settings. Clip hides HDR range. Clicking a value still permits
precise entry. The panel contains no tutorial paragraphs or extra +/- buttons.
Drafts preview live without changing pixels or history; Apply saves one undoable
recipe; Revert discards the draft. Preview compares against the HDR master.

**Print:** printer/paper and simulation remain prominent. Rendering intent,
black point compensation and gamut warning live in the Print options overflow menu. Preview,
Revert and Apply remain fixed. Preparation uses a bounded cancellable worker;
failed or cancelled work keeps the old proof. SDR documents show this page
without the unnecessary SDR tab.

**From Export:** SDR Appearance closes the modal export surface and opens the
operable Proof panel beside the canvas. Apply/Revert returns to the retained
export choices with a fresh artwork snapshot. Return to Export handles visits
without a pending draft. HDR and SDR output previews keep their explicit labels.

An independent picker boundary fix bounds derived HLS saturation at 100% when
Float32 rounding produced 100.000015. This prevents invalid workspace state
without changing the HDR paint values or the accepted color-panel layout.

## Future importable transformations

There is no single universal “HDR-to-SDR ICC profile” shared by all editors.
Two interchange directions are useful; neither is implemented in this update:

| Format / system | What it carries | Proposed role |
| --- | --- | --- |
| **CLF, `.clf`** | XML processing graph with matrix/range/log/exponent and 1D/3D LUT nodes; floating-point input/output and HDR shapers. | Best first portable, static SDR transform import. Keep explicit input primaries/transfer/reference white, output profile and supported node/domain contract. [CLF specification](https://docs.acescentral.com/clf/specification/) |
| **CTF, `.ctf`** | OCIO's richer transform serialization, including operators beyond CLF. | Consider after CLF, with an explicit supported subset. |
| **`.cube`** | Widely used sampled LUTs; conventions differ and domain/color interpretation is not a full color-management contract. | Optional convenience import with a required input domain/shaper and color-space interpretation. Never sample HDR as a bare 0–1 cube and silently clamp it. |
| **OpenColorIO** | Transform/configuration engine with color spaces, looks and display/view transforms, plus LUT readers. | An implementation option, not itself one file format. Full configuration integration remains separately scoped. Its tools recommend CLF/CTF for stronger shaper support. [OCIO guide](https://opencolorio.readthedocs.io/en/stable/guides/using_ocio/using_ocio.html) |
| **HAGC / AGTM, SMPTE ST 2094-50** | Reference white, baseline headroom and adaptive gain curves for different display headrooms; Apple can carry the binary payload in an ICC HAGC tag for PQ/HLG/linear profiles. | Most direct future exchange of an authored HDR display/rendition policy. Requires bounded parsing, format/version support and CPU/GPU conformance. [Apple HAGC](https://developer.apple.com/documentation/colorsync/headroom-adaptive-gain-curve) |
| **ISO 21496-1 gain maps** | Image-specific relationship between two renditions, with metadata. | Paired HDR/SDR delivery, not a reusable global LUT. Regenerate from edited HDR and authored SDR images. [Apple gain-map workflow](https://developer.apple.com/videos/play/wwdc2024/10177/) |

ST 2094-50 is no longer merely a proposal: SMPTE lists **2026-08**, available
**2026-08-28** in its [published-document table](https://www.smpte.org/standards/recently-updated-documents).
The older pinned Skia source references a draft; this implementation has not been
qualified as a parser/writer for the published standard.

A future imported transform should appear as an additional **Method → Custom…**
with a concise name, not in the printer profile selector. Import validation must
cover HDR/signed domains, unsupported operators, bounded file/LUT sizes, alpha
preservation, cancellation, embedded resources for reopen/recovery, and identical
preview/export evaluation. Controls that an imported transform cannot meaningfully
support should disappear. A baked LUT alone cannot represent local spatial tone
mapping or all headroom-adaptive behavior.

## Validation and limitations

Current-run evidence and review build hashes are recorded in
`artifacts/color-m4/review/build-manifest.json`. Numerical source, comparisons
and the independent C++ oracle are under `artifacts/color-m4/browser-tonemap-research/`;
`tools/validation/rwtmo_reference.cpp` regenerates the checked-in reference CSV.
The Skia attribution/license is in `THIRD_PARTY_NOTICES.md` and copied into the
review package.

Current checks passed:

- `browser-mapping-core.log`: 10 tests, including the independent browser curve
  comparison, Float64 parametric reference, monotonicity, effective controls,
  primaries, alpha, previous review settings and native archive/history.
- `browser-mapping-color.log`: 82 passed, 7 existing fixture tests ignored.
  Includes SDR preservation, actual encoded output landmarks and ICC consistency.
- `browser-mapping-gpu.log`: 6 passed, 1 local-profile test ignored. All three
  mapping methods match CPU on Float32 targets; HDR presentation, mapped proof,
  SDR surfaces, thumbnails and exact artwork sampling pass.
- `browser-proof-ui.log`: 462 shared UI tests passed. The earlier
  `proof-panel-workspace.log` records 86 native workspace persistence/default
  tests for the unchanged Proof layout integration.
- `browser-proof-hdr.log`: real GTK HDR input/edit, live draft/method/range,
  compare/reset/revert/apply, undo/redo, save/reopen and HDR/SDR export.
- `browser-proof-layout-final.log`, `proof-panel-compact/`: Paint/Photo placement,
  all SDR controls visible without scrolling, compact Print page and immediate
  tab dragging. Popovers are separate native surfaces and do not appear in the
  ordinary window PNG captures.
- `browser-proof-print-final.log`, `browser-proof-profiles-final.log`,
  `browser-proof-cancel.log`: print setup/history/save/reopen/RGB export;
  profile add/reuse/remove/simulation; cancellation, supersession and failed
  profiles preserving the current proof.
- `browser-proof-workspace-check.log`, `browser-proof-wasm.log`,
  `browser-proof-release-build.log`: all-target workspace check, browser check
  and runnable GTK release build. Browser HDR remains explicitly unsupported.
- `browser-proof-desktop/test.log`: actual HDR desktop master preview retains
  linear red 4.470145 (GSK capture 4.444563 with checker interpolation), while SDR
  output is bounded. Injected SDR/HDR capability changes update the preview.
  Reported compositor headroom is 49.261×; this is not measured emitted luminance.

Development failures were resolved before review: the first GPU run found a
16-byte minimum binding size after extending the HDR uniform to 32 bytes; the
Print tests initially assumed the old expander/subtitle presentation. The
updated GPU and native tests pass. The RWTMO reference approximation issue led
to the analytic implementation rather than weakening monotonicity checks.

The retained 8192 × 7324 ProPhoto/F16 document contains about 60 MP and 20 effects.
On NVIDIA RTX PRO 6000 / Vulkan 610.57.04, isolated 1600 × 1000 Mutter at 120 Hz:

| Preview case | Worker wait | Largest UI heartbeat gap | Cancel | Peak process RSS |
| --- | ---: | ---: | ---: | ---: |
| Retained pre-Proof HDR baseline | 7.79 s | 18.33 ms | 152.90 ms | 1,398,932 KiB |
| First Proof candidate HDR | 9.05 s | 21.07 ms | 150.39 ms | 1,394,952 KiB |
| Current HDR | 8.95 s | 22.01 ms | 129.97 ms | 1,394,708 KiB |
| First Proof candidate SDR | 8.53 s | 17.63 ms | 149.31 ms | 1,456,828 KiB |
| Current SDR, matching instrumentation | 6.90 s | 20.57 ms | 145.36 ms | 1,342,948 KiB |

Logs: `sdr-mapping-baseline/large-*`, `proof-panel-large-*`,
`browser-proof-large-hdr*`, `browser-proof-large-sdr-matched*`.
These are single runs, not statistical before/after qualification. The matching
HDR preview remains about 15% slower than the retained pre-Proof baseline;
that difference needs controlled repeated measurement before performance
acceptance. No claim of 120 Hz or p95/p99 input-to-present qualification follows
from these heartbeat samples.

A separate run with 0.5-second NVML/RSS polling (`browser-proof-large-sdr*` without
`-matched`) observed 1,892 MiB peak process GPU residency and 1,404,808 KiB peak
RSS. Its wait/heartbeat/cancel were 8.40 s / 43.01 ms / 171.60 ms. Polling can miss
allocation peaks and this instrumentation differs from the baseline. Driver
residency is not exact renderer allocation accounting. Environment and the
uncontrolled power/thermal conditions are recorded in `browser-proof-environment.json`
and `browser-proof-gpu-environment.txt`.

Physical pen/touch, 2× panel ergonomics, calibrated HDR/print appearance,
mixed-monitor movement, constrained/mobile hardware, other native hosts,
sustained load and per-device memory/latency budgets remain unqualified.
Continue using the bundled GTK runtime: the system GTK tablet-pad startup crash
requires the retained null-event-surface guard. Neither these checks nor the
new UI complete the entire phase-4 hardware gate.

The proposed gain-map export expansion is documented separately in
[HDR export proposal](../ui/hdr-export-proposal.md). It is not implemented by this
review build.


## Integration with origin/main (2026-09-18)

Merged `origin/main` at `ab6d93700ba51d15dd709637a9daf95bb8caf1dd` into
`capycanvas3`. This brings in the shared proof workflows on other hosts, the
shared filter-preview scheduler, and upstream renderer optimizations. The
review branch is pushed to `origin/capycanvas3`; neither main nor master is
updated. The two local handoff-document edits are excluded from the merge.

Conflict resolutions preserve dynamic HDR rasterization memory limits,
generation-tagged preview cancellation, display-headroom updates, and the
new platform command availability. Retained viewport captures now update HDR
uniforms before the upstream shared-proof-buffer shortcut; a GPU regression
exercises changes to the rendition and display headroom on that path. The Web
Navigator uses the updated inheritance API. Windows keeps its new import and
recovery workflows and explicitly rejects unsupported HDR documents/exports.
Incoming Windows tests were updated for `SampleDepth` and the shared preview
scheduler API; the native transport still checks cache bounds and packet lifetime.

Post-merge evidence under `artifacts/color-m4/`:

- `main-merge-workspace-check.log`: all-target workspace check passed.
- `main-merge-shared-tests.log`: core 94, color 82, shared UI 469 passed;
  seven existing color fixture tests ignored. This first combined run also
  records a Windows test assertion that assumed a GPU was attached. The final
  `main-merge-windows-tests.log` passes all 104 tests, with the real D3D12 journey
  still ignored on this Linux host.
- `main-merge-gpu-view.log`: six presentation/color/proof tests passed, one
  local-profile test ignored. `main-merge-gpu-hdr.log`: four tests passed for
  half-float publication, flattened storage, exposure/curves and HDR presentation.
- `main-merge-hdr.log`, `main-merge-hdr-journey/`: native HDR input/edit,
  rendition controls, undo, save/reopen and HDR/SDR delivery passed.
- `main-merge-layout.log`, `main-merge-print.log`, `main-merge-cancel.log`:
  compact panel placement/dragging, print/history/save/reopen/export and
  cancellation/supersession passed. The refreshed `proof-panel-compact/` captures
  show the merged build; the Photo SDR panel was visually inspected.
- `main-merge-desktop/test.log`: actual HDR desktop export master preview and
  capability changes passed. Master red remains 4.470145 linear (GSK capture
  4.444563 with checker interpolation). This is not luminance calibration.
- `main-merge-web-build.log`: runnable Wasm/browser build passed.
  `main-merge-web-unit.log`: workspace storage wake-up regression passed.
  `main-merge-web-hdr-wayland.log`: real Chrome rejects HDR photos/masters while
  preserving the active SDR drawing. `main-merge-web-proof-wayland.log`: profile
  preservation, proof viewing/Canvas/Navigator, editing/history, save/reopen,
  unchanged exported PNG bytes, GPU replacement and cancellation passed.
- Headless Chrome failed with the already-recorded NVIDIA/Dawn external-instance
  error and a CDP timeout (`main-merge-web-hdr.log`). The successful browser runs
  used a private Wayland desktop and GTK 3 Chrome; headless presentation remains
  unqualified. These are isolated profiles, with no user browser state changed.
- `main-merge-native-build.log`, `main-merge-release-build.log`: native test and
  runnable GTK release builds passed. `review/build-manifest.json` identifies
  the packaged binary and source commit.

The post-merge 60 MP checks ran sequentially after all other test GPU workloads
finished, with the same retained fixture, private Mutter display and time/RSS
instrumentation as the earlier comparison:

| Export preview | Worker wait | Largest UI heartbeat gap | Cancel | Peak process RSS |
| --- | ---: | ---: | ---: | ---: |
| HDR | 9.52 s | 18.20 ms | 145.37 ms | 1,364,184 KiB |
| SDR | 8.91 s | 19.56 ms | 152.44 ms | 1,313,440 KiB |

Both passed (`main-merge-large-hdr*`, `main-merge-large-sdr*`). These remain single
runs with uncontrolled power/thermal conditions, not performance acceptance or
a claim of improvement over the earlier baseline. GPU residency was not sampled
again. The hardware and calibration limitations above remain open.
