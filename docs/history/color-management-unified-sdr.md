# Unified SDR appearance — GTK review, 2026-09-18

The GTK Proof panel now exposes one SDR control set: **Brightness, Contrast,
Highlights, Highlight color**. There is no Browser/Photographic selector.
Controls, ranges, endpoint words and saved semantics belong to shared Rust;
GTK uses the existing numeric controls and flat panel rows. View → Proof still
switches Off ↔ the remembered SDR/Print mode and reveals the panel on enable.

## User flow and compatibility

For a new HDR document, select Proof → SDR and adjust the four live controls.
Auto measures the edited image's luminance range and resets brightness/contrast;
it retains Highlights and Highlight color. Reset restores the initial 1000-nit
range and neutral controls. Off changes viewing only. Each gesture is one
undoable document change. The HDR master and coverage are untouched.

An older document retains its exact saved recipe, including Browser,
Photographic, BT.2390, Scale or Clip. Its SDR page offers **Update controls**,
Auto and Reset. The new sliders are hidden until an intentional update; old
Exposure/HDR range numbers are never relabelled as the new controls. Updating
starts the new defaults. Undo restores the old recipe and compatibility view.
Simply opening, saving, exporting or enabling proof does not migrate it.

The same authored rendition supplies SDR exports, the regenerated SDR base of
HDR JPEG/AVIF, and the bounded input to ICC print proof/delivery. Export does
not get a second set of tone controls. Native HDR output keeps the HDR values.

## Rendering contract

`SdrMethod::Unified` is a distinct saved version, not an alias for either old
method. The existing 8-float renderer/cache record carries the new shoulder
parameter in slot 5. Missing fields and document defaults preserve old versions.

1. Measure D65 luminance in linear Rec.2020; zero/nonpositive luminance maps to
   SDR black. Reference white remains RGB 1 = 203 cd/m². Auto measures the
   input white endpoint in stops; Highlights does not change that endpoint.
2. Map luminance with the normalized-PQ Hermite shoulder used by BT.2390.
   Highlights `h` controls knee offset `2^-h`, from 2 (Detail) to 0.5 (Bright).
   Knee is `max(output - offset × (1-output), 0)`. Default `h=0` retains the
   previous photographic shoulder. The offset range retains monotonicity;
   shoulder and white endpoint are independently testable.
3. Brightness and Contrast operate **after** the shoulder in log odds. With
   `p=log2(.18/.82)`, output odds are
   `2^(contrast × (log2(y/(1-y))-p) + p + brightness)`.
   Converting odds back to luminance preserves black/white, with a stable
   display-linear 18% contrast pivot. Brightness's UI range ±100% is ±4 stops
   of odds; it is not scene exposure. This replaces, rather than renames,
   the old pre-tone exposure adjustment. Identity contrast has a cheap
   rational evaluation; default brightness/contrast leave the tone result.
4. Scale source RGB to the target luminance, then convert primaries. **White**
   compresses toward neutral at constant luminance. **Color** first moves any
   negative channel toward neutral, then scales RGB together to fit the actual
   destination cube. Positive saturated RGB can therefore retain its ratios
   by sacrificing luminance. Interpolate these bounded outputs with Highlight
   color. This is RGB-direction preservation, not guaranteed perceptual hue.
5. Apply tone/gamut mapping to straight RGB before alpha/matte compositing.
   Builtin RGB export uses its destination gamut. ICC proof and delivery use
   the same bounded working-RGB input domain. Gain maps are regenerated from
   the untouched HDR master against the authored SDR base; alpha is independent.

This is a display-referred tone/color pipeline, not an ACES/AgX transform or a
claim to implement a complete perceptual gamut mapper. Existing SDR documents,
print options, half-float storage and Float32 processing are unchanged.

## Reach of the controls

A reproducible exploratory fit uses the production Rust mapper on 97 exposures
from −8 to +3 EV for red, green, blue, orange, skin-like RGB and gray. With the
same measured headroom, Brightness −29%, Contrast 96.2%, Highlights +12.2% and
Highlight color 100% approximate the previous default Browser rendering with
**2.505/255 encoded-sRGB channel RMS**, versus **11.316/255** for the earlier
Photographic four-parameter fit. The maximum channel error is 23.001/255.

This supports useful appearance coverage without a second method. It is not
exact Browser equivalence, a perceptual error metric, a global optimizer result,
or evidence for every saved range/recipe. Old recipes still use the exact old
path. Probe source and output: `artifacts/color-m4/unified/fit.rs`, `fit.txt`.

## Why four sliders and no local-contrast slider yet

The research favored separate tone and color controls, as in
[Adobe SDR rendition editing](https://blog.adobe.com/en/publish/2023/10/10/hdr-explained),
[Affinity tone mapping](https://affinity.help/photo2/en-US.lproj/pages/HDR/hdr_tonemapping.html),
[darktable sigmoid](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/sigmoid/)
and [libplacebo's tone/gamut controls](https://libplacebo.org/options/).
A 2D pad is optional alternative input for Highlights × Highlight color, not
another rendering parameter. Sliders fit the existing compact panel and retain
keyboard/numeric access and established history behavior.

Local detail recovery is useful for compressed texture, but is unnecessary for
covering the useful Browser/Photographic range. It is **not implemented in this
review**, despite the initial implementation update overcommitting to it.
[libplacebo](https://libplacebo.org/options/) explicitly documents ringing risk
for contrast recovery. A bilateral base/detail reference and local-Laplacian
comparison remain necessary before choosing a default or exposing an amount:
[bilateral research](https://people.csail.mit.edu/fredo/PUBLI/Siggraph2002/),
[local Laplacian research](https://imagine.enpc.fr/~aubrym/projects/llf/index.html).

The audit found no reusable spatial SDR stage. Required follow-up:

- One bounded, cancellable document-space luminance guide, keyed by composite
  revision, animation time, working space and device generation; Float32 analysis.
- One coverage-aware edge-preserving base/detail implementation for canvas,
  navigator, SDR exports, gain-map bases and mapped print, with explicit resize
  ordering. Viewport-size kernels would change appearance when zooming.
- Keep a single worker and bounded guide/cache; test supersession and GPU loss.
  A full 60 MP Float32 luminance plane alone is 240 MB, so avoid full-frame
  temporary planes. A bounded guide needs explicit detail/scale qualification.
- Natural scenes, flat artwork, text, alpha edges and bright points; evaluate
  halos, noise, zoom/tile invariance, cancellation, latency and peak memory.

## Validation

The following passed on the Linux/NVIDIA Vulkan test host:

- 100 core, 84 portable color (87 with native HEIF support), and 474 shared UI
  tests; independent Float64 shoulder sweeps, control extremes, working-space
  invariance, alpha, old-recipe decoding and integer/ICC output consistency.
- GPU CPU-reference parity across SDR/HDR surfaces and mapped proof, 11 thumbnail
  tests, two filter-preview tests and lossless HDR snapshot storage.
- Six isolated GTK workflows: HDR edit/save/reopen/SDR export and explicit
  legacy migration with undo/redo; HDR JPEG/transparent AVIF and Auto migration;
  compact Paint/Photo layouts and floating tabs; remembered proof menu toggle;
  print profile/history/save/reopen/RGB export; cancellation/supersession/failure.
- Three pinned-codec tests: JPEG/AVIF HDR and alpha round trips, regenerated
  fallback and independent ISO/Ultra HDR metadata paths, and saturated HDR
  reconstruction from both White and Color SDR bases. The brightness fixture
  uses −50% (two stops of output odds); the former one-stop test assumed
  pre-tone exposure and its >0.1 fallback-difference assertion no longer fit
  the fixed-white response. Pixel-reference tolerances remain unchanged.
- Browser SDR proof/edit/history/save/reopen and GPU replacement, explicit HDR
  rejection with the SDR document still usable, shared U16/profile workflows,
  and independent Chrome JPEG/AVIF SDR-base decoding with preserved AVIF alpha.
- Release GTK and production Wasm builds. Native panel screenshots were inspected;
  all four sliders fit both default layouts without scrolling.

The 60 MP 8192×7324 ProPhoto half-float fixture with 20 effects was measured
with the unified default, same isolated 120 Hz desktop and retained workload as
the previous photographic baseline. These are single encoded-preview runs,
not final 60 MP publication or constrained-device qualification.

| Preview | Prior / new time | Largest UI heartbeat gap | Cancellation | Peak process-tree RSS | Temporary codec storage |
| --- | --- | --- | --- | --- | --- |
| SDR | 12.93 / 12.44 s | 26.62 ms | 152.10 ms | 1,348,472 KiB | 0 |
| HDR JPEG | 31.32 / 28.36 s | 41.67 ms | 134.28 ms | 2,401,108 KiB | 1,914,726,780 bytes |
| HDR AVIF | 41.38 / 40.15 s | 26.59 ms | 148.52 ms | 4,302,768 KiB | 2,121,015,906 bytes |

GPU residency peaked at 1,893 MiB. The fixture exceeds the supported HDR output
range: JPEG/AVIF preflight continues to report that and require explicit clipping.
No new image buffers or storage-depth changes were introduced by these global
controls. Timing differences from single runs should not be treated as a
performance improvement claim.

Current evidence is recorded under `artifacts/color-m4/unified/`. The review
bundle manifest records the exact executable and source checkpoint. Physical
HDR luminance, print matching, mobile/constrained hardware and subjective
natural-scene appearance still require qualification/user review.
