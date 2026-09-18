# HDR highlight rendering review — 2026-09-18

The later [unified SDR controls](color-management-unified-sdr.md) supersede the
method selector described here; legacy recipes remain unchanged.

The initial audit below describes the pre-change renderer at `14c26bd0`.
The [implemented follow-up](#implemented-photographic-follow-up) records the
subsequent rendering change; neither change migrates saved SDR recipes on open.

## Why bright colors stay dark in SDR

The previous `crates/layer-core/src/color/hdr/sdr.rs` mapped the maximum channel in linear
Rec.2020 through the selected tone curve, then scales all channels by the same
factor. `crates/layer-render-wgpu/src/hdr_mapping.wgsl` mirrors it. The output
matrix is followed by component limiting. There is no highlight chroma roll-off
or perceptual destination-gamut compression.

This preserves RGB proportions before output limiting. Raising a saturated
highlight eventually stops making it brighter, without making it white. In
linear sRGB, full red has about 21% of white's luminance. An SDR screen cannot
show that same saturated red with white's luminance; adding green and blue
increases luminance but decreases saturation. This is a rendering tradeoff,
not an exposure value being discarded. Neutral highlights already approach
white; the missing behavior is most obvious in colored highlights.

Changing BT.2390's shoulder or estimating the peak more accurately does not
solve that color-rendering problem. The current “Perceptual” label describes a
tone curve, not a complete perceptual color-rendering transform.

## Established approaches

- **AgX:** Blender's photographic view transform deliberately moves bright
  colors toward white. It directly illustrates the appearance requested here.
  It is designed for scene-linear rendering, so adopting it for Capy's
  display-referred, 203-nit reference-white master requires an explicit input
  adaptation and reference-white policy. Simply applying a generic shader
  approximation could add an unwanted second rendering look.
  [Blender release documentation](https://developer.blender.org/docs/release_notes/4.0/color_management/).
- **ACES 2:** combines tone mapping with hue-preserving chroma compression,
  then destination-gamut compression and white limiting. The chroma stage
  varies with lightness and colorfulness; this is more sophisticated than
  globally reducing saturation. It supports the design principle, but full
  ACES/OCIO integration remains outside this phase's scope.
  [ACES chroma compression](https://docs.acescentral.com/system-components/output-transforms/technical-details/chroma-compression/).
- **libplacebo:** a useful reference for already rendered HDR. Its current
  default uses an adaptive spline plus a separate perceptual gamut mapper.
  It also documents constant-luminance desaturation toward white and controls
  for the balance between luminance and saturation preservation. These are
  distinct from the scalar BT.2390 curve currently implemented in Capy.
  [libplacebo options](https://libplacebo.org/options/).

## Recommendation for the next rendering change

Prefer a **Photographic** SDR appearance with a smooth path toward white in
colored highlights. Evaluate AgX as the visual reference and a display-referred
tone/color mapping pipeline against it; do not claim that adding an arbitrary
desaturation mix implements AgX or ACES. Keep ordinary SDR tones and reference
white stable, retain hue through the roll-off, and compress destination gamut
instead of relying on final channel clipping.

Use one **Highlight color** control, with **White** and **Color** endpoints, in
Proof → SDR. The default should favor bright, soft highlights. This controls
the authored SDR rendition, including the SDR base of gain-map exports and the
input to mapped print delivery. Export should reuse it rather than adding
another set of knobs. A legacy saved recipe must reopen unchanged; switching
the method or resetting is an intentional, undoable change.

Acceptance needs exposure ramps in red/green/blue and mixed hues, neutral ramps,
wide-gamut colors, skin tones, near-black and alpha edges. Check monotonic
luminance, highlight detail, hue drift and CPU/GPU parity, then compare proof,
ordinary SDR exports, gain-map SDR bases and print input. A visually pleasing
default on neutral ramps alone would miss the present defect.

## The gain map can retain the HDR color

The current encoder in `crates/layer-color/src/photo/gainmap.rs` uses an RGB
gain map, regenerated from the edited HDR master and authored SDR base. Each
channel has its own gain, including gains below one. A near-white SDR highlight
can therefore reconstruct a saturated HDR highlight; desaturating the SDR
fallback does not require desaturating the master. Alpha remains independent.

Lossy base/gain encoding, supported value ranges and partial-headroom display
interpolation still require validation. A monochrome gain map would not offer
the same independent color reconstruction. Artist control over the SDR fallback
should be separate from intentionally changing saturation in the HDR artwork.

## Implemented Photographic follow-up

**Photographic** is now the default for newly authored renditions. Its
**Highlight color** control runs from **White** (0, default) to **Color** (100%).
The common numeric control displays the endpoint words and still supports
numeric entry. Exposure, Contrast and HDR range retain their existing meaning.
Auto fits the edited image's luminance peak and resets exposure/contrast,
preserving the selected method and Highlight color. Reset deliberately selects
the new defaults. The method, Auto and Reset share a compact native control row;
the Off/SDR/Print selector and View → Proof behavior remain unchanged.

This is a display-referred pipeline, **not AgX, ACES or libplacebo's complete
perceptual renderer**. AgX supplies a visual design reference; an exact numerical
AgX comparison has not been performed. Its scene-linear input adaptation and
look would need separate qualification. The implemented choice combines the
already validated BT.2390 shoulder with constant-luminance desaturation, an
established display-rendering option described by
[libplacebo](https://libplacebo.org/options/). No external transform code or LUT
has been copied for the new color stage.

The shared Float32 CPU and WGSL implementations:

1. Convert to linear Rec.2020 for D65 luminance `Y` and positive channel peak `P`.
   Apply the saved exposure/contrast adjustment `A` before the scalar BT.2390
   curve `T`. White's target luminance is `T(A(Y))`; Color's target is
   `T(A(P)) × min(Y/P, 1)`. Interpolate these luminances with Highlight color,
   then scale the original RGB by target luminance / `Y`.
2. Convert to the destination primaries. At constant D65 luminance, compress
   color toward neutral into the destination cube. A rational shoulder begins
   at 98% of the boundary, has continuous first derivative and approaches the
   boundary without crossing it. This retains linear RGB hue direction, **not
   guaranteed perceptual hue**. Nonpositive luminance becomes SDR black;
   luminance at/above one becomes white. These are rendition decisions only.
3. Keep coverage independent; perform tone/gamut mapping on straight RGB before
   linear alpha/matte compositing and output encoding. The same bounded
   working-RGB input feeds ICC print proof and ICC delivery. Ordinary SDR
   documents and the old mapping methods keep their original paths.

Neutral ramps are unchanged from the prior BT.2390 method. For a linear sRGB
red `[8, 0, 0]` at the 1000-nit range, the former SDR result was `[1, 0, 0]`.
White produces approximately `[1, .781, .781]`; Color produces
`[1, .160, .160]`. Increasing brightness therefore has a visible route toward
white, with an explicit saturation tradeoff. Color still compresses out-of-gamut
values rather than promising the old channel-clipped result.

Saved Perceptual, Browser, Scale and Clip recipes retain their transforms. A
missing recipe retains the old document default; an old recipe missing the
method retains the earlier Browser interpretation. Selecting Photographic or
Reset is an undoable migration. Highlight color is included in save/reopen,
recovery, history, preview keys, thumbnails and the GPU uniform contract.

### Delivery and gain maps

Photographic gain-map exports author their SDR base in bounded **sRGB**, then
encode those same colors in the container's existing Rec.2020 application
space. This aligns its appearance with normal sRGB delivery on legacy viewers.
The RGB gain map is regenerated against that edited fallback, after selected
quality encoding where applicable. The original HDR color and independent alpha
remain the reconstruction target. Saved older methods retain their original
base-generation behavior. Other RGB destination profiles can produce different
SDR colors because their available gamut differs.

The codec test for `[8, 0, 0]` measured an HDR reconstruction of approximately
`[8.016, .00012, -.00108]` from JPEG's much whiter SDR base. AVIF measured
premultiplied `[4, .00009, -.00001, .5]` for 50% coverage. Those small errors are
codec/matrix error, not lost HDR saturation. Tests also compare the Color end,
regenerated ISO/Ultra HDR metadata and the ordinary edited-rendition round trip.
Partial-headroom interpolation and real sharing-service preservation remain
unqualified.

### Validation

Implementation checkpoint: `9e3bb4b1`. Baseline source: `14c26bd0`; retained build manifest and new logs are under
`artifacts/color-m4/photographic/`. `samples.json` and `highlights.svg` compare
red, green, blue, orange, skin-like RGB and gray over −4 to +8 EV, generated from
the actual Rust transform by `crates/layer-core/examples/sdr_highlights.rs` and
`tools/validation/sdr_highlights.py`. The chart is an SDR comparison, not a
physical HDR measurement or a photographic scene qualification.

- Core tests: luminance monotonicity, bounded output, neutral behavior, linear
  hue direction, coverage and cross-working-space invariance, including D50
  ProPhoto; prior Float64 BT.2390 oracle and legacy recipe tests remain passing.
- Shared UI: endpoint text/numeric editing, live edits, gesture undo/redo,
  saved settings and recovery. Native workflow exercises the real controls and
  HDR input → edit → native save/reopen → HDR and SDR output.
- CPU output: builtin destination/view parity and ICC proof input parity.
  GPU readback covers Photographic White/intermediate/Color and legacy methods,
  SDR/scRGB/PQ, mapped proof, source/layer thumbnails, filter previews and
  unchanged half-float snapshots.
- Native JPEG/AVIF codec tests validate reconstruction, alpha, regenerated
  fallback and dual JPEG metadata. Physical HDR, mobile/Apple/Windows behavior,
  constrained-memory devices and actual print matching remain unqualified.

Native workflow logs cover the HDR edit/save/delivery journey, opaque JPEG and
transparent AVIF recommendation/flattening/reopen, print setup/history/save/RGB
delivery, profile failure/supersession/cancellation, and Paint/Photo panel bounds
with immediate tab dragging. The compact panel uses shared native choice/action
rows and the existing numeric controls. Shared counts: 97 core, 87 color (21
optional tests excluded here), and 474 UI tests (473 plus the added endpoint
test). The optional native codec and GPU tests above ran separately.

Browser logs cover proof viewing/editing/history/reopen/device replacement,
explicit HDR rejection, and the existing shared SDR workflows. The broader
browser test initially clicked the first profile-row button, which now toggles
menu visibility; it was corrected to target **Remove** explicitly and passed.
This was a stale harness selector, not a change to profile-library behavior.
Workspace and Wasm checks and production GTK/browser builds pass.

`web-gainmap-interchange.log` and `browser-sdr-results.json` record independent
Chrome decoding of newly generated neutral/dark JPEG and AVIF bases, plus the
GTK-exported color JPEG (compared with system Pillow/LittleCMS) and transparent
AVIF. SDR channels agree within 2/255, and the AVIF alpha matches its source.
This uses a CPU canvas on an SDR compositor; accelerated-canvas and physical
HDR reconstruction limitations from the earlier qualification still apply.

### 60 MP measurement

Single runs on the same retained 8192×7324 F16 ProPhoto/20-effect document,
RTX PRO 6000 Blackwell, NVIDIA 610.57.04, Vulkan and isolated 1600×1000@120
Wayland. `LAYER_HDR_DEFAULT_RENDITION=1` selects Photographic in memory without
rewriting the fixture. `tools/performance/hdr-memory.py` records the process
tree, codec children, private staging and NVML at 0.5-second intervals.

| Encoded preview | Time | Max UI heartbeat gap | Cancel | Peak process-tree RSS | GPU residency | Private staging |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| SDR | 12.93 s | 27.42 ms | 132 ms | 1,342,024 KiB | 1893 MiB | 0 |
| HDR JPEG | 31.32 s | 35.49 ms | 136 ms | 2,393,272 KiB | 1893 MiB | 1.86 GB |
| HDR AVIF | 41.38 s | 31.95 ms | 143 ms | 4,301,200 KiB | 1893 MiB | 2.06 GB |

Logs are `photographic/large-{sdr,jpeg,avif}.{log,memory.json,time.txt}` under the
artifact root. Prior Perceptual SDR was 10.94 s / 22.45 ms / 133 ms and
1,302,360 KiB RSS. Prior gain-map previews used the fixture's saved Browser
recipe (19.73 / 29.62 s), so their time difference cannot isolate Photographic's
color-stage cost. New preview throughput is slower; main-thread work remains
bounded, and measured cancellation stays below 150 ms. Memory is similar to the
prior route, but AVIF still requires about **4.10 GiB** process-tree RSS. Sampling
can miss brief peaks; constrained hardware needs separate qualification.

These tests measure preview and cancellation, **not final 60 MP publication or
physical input latency**. The retained fixture exceeds the formats' HDR range;
the app reports this and requires explicit clipping to publish it. No precision
reduction or unannounced clipping was introduced. Real emitted luminance,
mixed-monitor behavior, perceptual hue on natural scenes, local contrast and
actual print matching still require user/hardware review. The implementation
adds no Float32 document storage, OCIO/ACES pipeline, RAW development or external
LUT importer.
