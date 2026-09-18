# Local SDR rendition and Tone × Detail review

September 2026. GTK review implementation; physical display qualification remains
separate. Supersedes the four global controls described in
[color-management-unified-sdr.md](color-management-unified-sdr.md).

## User behavior

Proof keeps its Off / SDR / Print selector. SDR now has a **Tone × Detail pad**:

- **Right: more Tone compression.** Reduce differences between broadly bright
  and dark areas, around middle gray. This can reveal a room while retaining
  detail through its windows. Left retains more of the original lighting contrast.
- **Up: more Detail.** Emphasize texture and smaller luminance differences.
  Down softens them. The default 100% keeps the extracted detail at its original
  strength; the local illumination adjustment still changes the overall image.
- **Brightness** changes the final SDR midtones while holding black and white.
- **Highlight color** trades bright, increasingly white highlights for more of
  their color. This affects only the SDR rendition.

Drag the pad, use its arrow keys, or edit either numerical value below it.
Shift-arrow moves farther. Escape cancels an active adjustment; double-click
resets both pad axes. A drag is one undo step. These are live document settings,
with no Apply or Revert. Auto measures the edited HDR range; Reset restores the
starting recipe. Existing documents keep their previous rendering until the user
chooses **Update controls** or Auto. Undo restores the previous recipe.

The same settings produce SDR exports, the SDR base of newly generated gain-map
files, and the HDR-to-SDR stage before print proofing. Gain maps are regenerated
from the actual delivered SDR base and the edited HDR master. Local contrast
therefore changes the gain map spatially; the master and alpha are not baked with
that local contrast. JPEG still measures gains after encoding/decoding its base.

## Algorithm and shared implementation

Independent sampled local-Laplacian implementation based on
[Paris et al. (2011)](https://people.csail.mit.edu/sparis/publi/2011/siggraph/) and
[Aubry et al. (2014)](https://imagine.enpc.fr/~aubrym/projects/llf/index.html).
This is our bounded approximation and rendering policy, not a claim of pixel
identity with those authors' implementations or with a commercial application.

1. Traverse the complete linear, premultiplied document in bounded row bands.
   Build a fixed document-space, coverage-weighted log-luminance guide with a
   maximum edge of 768. Hidden RGB does not contribute. No viewport-dependent
   analysis and no full-resolution luminance allocation.
2. Build coverage-normalized Gaussian pyramids and interpolate remapped Laplacian
   coefficients at intensity anchors no more than half a stop apart. The smooth
   remapping has unit slope at zero and approaches ±1.5 stops across strong edges.
   Reconstruct detail; illumination is original log luminance minus that detail.
3. Gather the guide with coverage and luminance edge weights. In log2 units,
   `output = input − Tone × (illumination − log2(0.18))
   + (Detail − 1) × (input − illumination)`.
   Tone runs 0–0.85, presented as 0–100%; Detail runs 50–200%.
4. Apply a BT.2390 shoulder for the corresponding compressed input range, then
   bounded SDR brightness and destination-gamut compression. Color/alpha
   semantics and the existing ICC delivery stage are preserved.

A white anchor made the initial natural-image tests washed out; the middle-gray
anchor preserves a useful starting midtone level. Default Tone is 0.6 and Detail
is 1. This is a starting rendition, not a guarantee of an artist's preferred look.

All image arithmetic, guide storage and GPU uniforms are Float32. Editing storage
remains half-float. The guide's spatial reduction is explicit: fine texture is
retained in the full-resolution pixel residual, but its illumination estimate is
approximate. Large-scale halos, noise emphasis and extreme-range banding still
need subjective review. This is not an image-independent LUT; a 3D LUT cannot
represent a neighborhood-dependent transform.

`layer-core` owns the guide and recipe; `layer-color` owns row delivery;
`layer-render-wgpu` uses the same guide/equations for canvas and navigator and
shares full-document analysis across snapshot preview/export sizes. Native HDR
presentation and full HDR export bypass local SDR mapping. Isolated color swatches
and per-layer thumbnails have no document neighborhood and use the point-color
stage. They are not authoritative SDR delivery previews.

GTK owns pointer capture, accessibility, cancellation, scheduling and device
publication. The pad's axes/ranges/defaults are shared `layer-ui` configuration;
its number fields reuse the existing common control. One superseding worker per
window analyses changed artwork after a short debounce. Pad/brightness/color
changes reuse its immutable guide. Closing the window, replacing the document or
changing device invalidates work. A visible status identifies preparation or
failure. Animated views refresh at most twice a second, keeping the last completed
guide between frames; export analyses the captured frame exactly. Temporal
flicker on animated scenes is not qualified.

## Five actual HDR fixtures

[Poly Haven's CC0 assets](https://polyhaven.com/license) supply five 2048×1024
Radiance RGBE panoramas. They are lighting captures, not tone-mapped JPEGs:

| Source | Stress case | Largest decoded channel |
| --- | --- | ---: |
| [Abandoned Hall](https://polyhaven.com/a/abandoned_hall_01) | Interior/windows and broad tonal transitions | 80.5 |
| [Venice Sunset](https://polyhaven.com/a/venice_sunset) | Sun, sky gradient and foreground texture | 8,320 |
| [Neon Photostudio](https://polyhaven.com/a/neon_photostudio) | Saturated lights and dark surroundings | 110 |
| [Kiara Dawn](https://polyhaven.com/a/kiara_1_dawn) | Foliage, shadows and sky | 118 |
| [Small Studio](https://polyhaven.com/a/studio_small_09) | Emissive softboxes and neutral surfaces | 548 |

Linear Rec.709/D65 is an explicit assumed interpretation for these RGBE fixtures.
The importer reports every half-float rounding, underflow and exposure adjustment.
All five decoded originals were represented exactly: zero rounded or underflowed
channels, zero exposure adjustment. Their original RGBE quantization is unchanged.
The fixture-only reader does not add a production Radiance import feature.

Reproduce from the repository root:

```sh
cargo build --offline --release -p layer-color --example local_tone_review
python3 tools/color/local-tone-fixtures.py
```

The downloader verifies pinned SHA-256 values. Originals, native `.capy` documents,
source/license manifest and global/local comparison PNGs live under
`artifacts/color-m4/local-tone/images/`. They are review artifacts, not repository
binaries. Source traversal and analysis of these resident 2 MP images initially
measured about 85–129 ms each, with 768×384 guides (4,718,608 bytes); these are CPU
fixture timings, not GTK cold-open timings or constrained-device benchmarks.

## Validation and qualification

Results are recorded under `artifacts/color-m4/local-tone/`. The core numerical
oracle checks uniform illumination against a Float64 power response, independent
alpha, hidden RGB, recipe persistence, compatibility and cancellation. Spatial
GPU comparisons exercise SDR and print mapping, scRGB, working-space adaptation,
texture changes, guide inheritance and unchanged master pixels.

Native workflow results and large-document measurements are appended after the
review build has passed its checks. Physical HDR luminance/transport on the user's
monitor, touch/pen hardware, browser/mobile host integration and constrained
hardware are not qualified by an isolated desktop test.
