# HDR highlight rendering review — 2026-09-18

This is an audit and recommendation, not an implemented rendering change.
The Proof toggle change does not migrate saved SDR recipes or alter pixels.

## Why bright colors stay dark in SDR

`crates/layer-core/src/color/hdr/sdr.rs` maps the maximum channel in linear
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
