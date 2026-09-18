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

Drag the pad, use its arrow keys, or edit either numerical value beside it.
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

### Recorded GTK results

Feature checkpoint `8b2c81ed`; main sync `207ec78f` includes origin/main
`bdcfac3d`. Linux reference: Fedora 44, kernel 7.1.10, NVIDIA RTX PRO 6000
Blackwell Max-Q, driver 610.57.04, Vulkan. Isolated Mutter Wayland desktop,
1600×1000 at 120 Hz, bundled GTK 4.22.4. These are single desktop qualification
runs, not power-controlled or constrained-device measurements.

- Shared tests: 104 core, 84 color, 475 UI pass (663 total; seven unrelated
  color tests require external fixtures and remain ignored).
- CPU/GPU local comparison and legacy SDR/HDR/print comparison pass. All 17
  snapshot tests pass, including storage preservation, band limits, placement,
  resizing, cancellation and shared capture lifetimes.
- Four real codec tests pass. Spatially different SDR bases reconstruct the HDR
  master in both JPEG and transparent AVIF. The local test's sampled maximum HDR
  channel error was 0.04641 for lossy JPEG and 0.00547 for AVIF, in linear reference-
  white units. These small fixtures do not establish worst-case image-wide bounds.
- GTK HDR open/edit/exposure/curves/painting/sampling/histogram, live proof,
  undo/redo, save/reopen and actual SDR PNG/HDR PNG export pass. Native HDR JPEG
  and transparent AVIF preview/export/reopen pass. Explicit format interaction
  now prevents a late transparency recommendation from replacing that choice.
- Five downloaded native HDR projects load. Pad changes reuse the completed
  guide, gestures undo in one step, Escape restores the previous value and reset
  preserves artwork. Thirty scripted pad updates, including 5 ms event pumping
  between updates, took roughly 194–209 ms in the initial run; this is not
  input-to-present latency. The guide was already ready after the test's normal
  startup wait, so its reported zero additional wait is not a cold-open timing.
- Paint/Photo panel geometry and immediate tab tear-off pass. Numeric readouts
  sit beside the pad so both lower sliders fit even the tight Photo group.
- Simulated GPU failure, replacement device, exact surviving artwork, undo/redo
  and rebuilding a current local guide on the replacement device pass.
- Launching all five photos on the actual HDR desktop exposed an early display-
  hint race: the worker attempted presentation before the first frame configured
  its swapchain. HDR transform updates now schedule a redraw only after a frame
  exists. A native regression test holds back all frames while delivering an HDR
  hint; it passes, as do repeated five-photo and device-recovery workflows.
  The corrected production build opens all five canvases with negotiated PQ
  presentation and no GPU errors. This checks transport/startup, not emitted
  luminance or calibration. Logs: `gtk-early-hdr-hint.log`,
  `gtk-photos-surface.log`, `gtk-recovery-surface.log` and
  `review-desktop-fixed.log`.

A same-executable comparison of global versus local SDR encoded previews used
the retained 8192×7324 ProPhoto F16 fixture with 20 effects:

| Measure | Unified global baseline | Local rendition |
| --- | ---: | ---: |
| Preview completion | 14,075 ms | 13,700 ms |
| Largest 10 ms UI heartbeat gap | 34.96 ms | 23.50 ms |
| Cancel/close | 148.38 ms | 135.26 ms |
| Peak sampled process-tree RSS | 1,333,384 KiB | 1,408,440 KiB |
| Peak NVML graphics residency | 1,889 MiB | 1,893 MiB |
| Temporary codec staging | 0 | 0 |

The approximately 73 MiB RSS difference includes analysis and renderer workers;
it is not just the final guide. Half-second memory sampling can miss brief peaks.
The small time differences do not establish a speed improvement. Full 60 MP
JPEG/AVIF publication, long-session behavior and p95/p99 input-to-present latency
remain unmeasured for this local change.

### Web check limitations

The complete workspace check and release Wasm build pass. The Wasm build reports
an unused native-only local-guide cache field; an existing Apple dead-code warning
also remains. Chrome 152.0.7977.64 / hardware WebGPU reached application readiness,
but this run did **not** pass the browser workflow gates:

- The HDR-boundary harness attempted Open while the command remained disabled;
  it timed out before fetching the test file. A diagnostic captured that state.
- Print setup, profile retention, cancel and preservation-failure/retry passed,
  but the proof-versus-normal presented-canvas screenshot assertion failed.
- Chrome also reported `A valid external Instance reference no longer exists`.
  This warning alone does not establish the cause of either workflow failure.

Logs are `web-hdr-limits*.log`, `web-diagnostic.log` and `web-proof.log`. No baseline
comparison establishes whether these browser failures predate this change, so
browser workflow qualification remains unresolved. The new interactive panel
and asynchronous view-guide integration are GTK only; Web HDR editing remains
explicitly unsupported. The broader phase-4 common gates are not declared complete.
