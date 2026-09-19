# Proof lens and retained rendering

Follow-up to the [Proof control polish](color-management-proof-polish.md).

The glass field has pronounced spherical refraction, a small fluid twist and a
narrow grazing rim. It has no surface glow or drop shadow. Rounded liquid cells
become finer to the right; contrast follows the original screen-space
vertical coordinate, so the bottom remains the low-contrast direction. This is
an illustration, independent of the document's tone mapping.

## Cache audit and change

The previous implementation cached the computed pixels in a Cairo surface, but
painted that surface into the DrawingArea's new full-size Cairo node whenever
the dot moved. Both arc widgets were also explicitly invalidated on every recipe
change, and all four percentages were painted into another full-size Cairo node.
The procedural function was not rerun on each drag, but the rendered control did
unnecessary raster work.

GTK now uses one immutable 512×512 RGBA texture shared by the Proof controls on
its UI thread. One bounded background job generates it; weak widget references
request repaint when it completes. Layout size, display scale, document content,
recipe values and dragging do not invalidate that texture. It occupies 1 MiB of
pixel data; renderer residency is managed by GDK. Generation failure leaves the
neutral field and logs the failed worker, without a retry loop.

The marker uses native GSK borders separate from the retained texture. Unchanged
arc snapshots are reused. Each percentage has a small retained render node,
invalidated only by its text, size or theme ink; changed values no longer repaint
the entire dial. Repeated input that quantizes to the same recipe is ignored.
The full-document asynchronous local tone guide and HDR processing are unchanged.

## Validation

Evidence is under `artifacts/color-m4/proof-lens/`. The build manifest in the
review bundle records the exact source commit and binary hash.

- Shared control defaults/range check passed (`shared-check.log`); production
  and native-test release builds passed (`release-build.log`,
  `native-build-final.log`).
- Native hit regions and cache checks passed at 128, 160, 226, 320 and 400 logical
  pixels (`native-cache.log`). All five controls reused the same texture object.
  Its single background generation took 23.21 ms. Across 600 changed-value
  updates, neither arc was repainted and unchanged arc readouts reused their
  nodes. Repeated identical contacts dispatched one edit per gesture. These
  checks exercise widget picking as well as rendering; actual GTK screenshots
  are in `controls/`.
- Native mouse and Mutter virtual-touch checks passed (`pointer.log`): circle
  and arcs remain independent, including drags across their boundaries,
  double-click reset and one-step Undo. Physical touch and pen are not qualified.
- The 60 MP HDR document check passed (`60mp-controls.log`). Sixty updates took
  1,134.26 ms including deliberate event pumping and a final drain. Maximum
  10 ms timer heartbeat gap was 14.70 ms. Neither the glass texture nor the
  document's local-tone analysis was regenerated. Undo restored the recipe and
  the HDR layers were unchanged.
- For that run, `/usr/bin/time` reported maximum RSS of 1,308,580 KiB;
  500 ms sampling observed up to 1,323,848 KiB for the application process tree
  and 1,893 MiB graphics residency. The sampling can miss brief peaks and GPU
  residency includes driver allocations (`60mp-controls.memory.json`,
  `60mp-controls.time.txt`). Hardware: NVIDIA RTX PRO 6000 Blackwell Max-Q,
  Vulkan, isolated Mutter 1600×1000 at 120 Hz.

The earlier control-polish run recorded a 24.55 ms heartbeat gap, 1,300,840 KiB
maximum RSS and 1,893 MiB graphics residency. These single-run measurements do
not establish a latency distribution or a general speedup. The assertions above
verify the eliminated redraws directly. No input-to-present latency, constrained
hardware, long-session memory, physical HDR luminance, browser or other-host
qualification is claimed. Tone mapping and export code did not change; their
previous validation and limitations remain in the archived review manifests.

## Quieter finish

The follow-up removes the GSK outset shadow, removes the broad lens reflection,
reduces the narrow glint and internal fold reflection, and halves the rim
lighting. Convex refraction is slightly stronger, with the same fluid twist.
This only changes the static illustration; caching, control mapping and tone
processing are unchanged.

The release and native-test builds passed. The five-size hit-region/cache check
passed again: one shared texture, 600 changed-value updates, no unchanged arc
redraws. GTK screenshots were visually inspected. Evidence is under
`artifacts/color-m4/proof-lens-quiet/`; the earlier pointer and 60 MP results above
were not rerun for this illustration-only adjustment.

## Spherical edge refraction

The next revision replaces the polynomial bulge with a hemisphere ray mapping.
The research reference is [PBRT's specular transmission chapter](https://www.pbr-book.org/4ed/Reflection_Models/Specular_Reflection_and_Transmission):
Snell's law controls the ray direction, while reflectance depends on the angle
at the surface. Its glass IOR range is 1.5–1.6. [Apple's Liquid Glass design
discussion](https://developer.apple.com/videos/play/wwdc2025/219/) emphasizes
lensing that bends and concentrates light. The [POV-Ray sphere reference](https://news.povray.org/povray.binaries.images/thread/%3C3D695DF1.8040507%40faricy.net%3E/)
was inspected for the tightly curved image near its perimeter. No reference
image is used as an application asset.

For this static guide, an orthographic ray enters a hemisphere at IOR 1.52 and
intersects its base plane. The surface normal turns sharply near the silhouette;
we amplify the resulting edge displacement by 2.8 to make that curvature visible
at panel size. A narrow angle-dependent rim blends toward reflection at the
silhouette. This is an art-directed illustration, not a complete physical glass
simulation. It keeps the smooth-left/fine-right and gray-below/contrast-above cues.

The broad inner reflection and off-axis glint have been removed. Fold transitions
are crisper, with less tint. Four samples per pixel in the tightly compressed
perimeter reduce aliasing. All of this is baked once by the existing bounded
worker into the same shared 512² texture; it adds no work during dragging and
does not change the tone-mapping algorithm or HDR image data.

Validation: release and native-test builds passed. The GTK hit-region/cache check
passed at 128, 160, 226, 320 and 400 logical pixels, with one texture build,
600 changed-value updates and no unchanged arc redraws. One-time generation took
34.15 ms on the background worker. Native screenshots at 128, 226 and 400 pixels
were visually inspected, including the compressed rim. Evidence is under
`artifacts/color-m4/proof-dome/`. The earlier real-pointer and 60 MP checks were
not repeated; this revision changes only the cached illustration.

## Liquid cells

Rounded, irregular cells replace the striped folds. A gently warped cellular
field has clean centers and narrow connected channels. Small neutral shade
variations distinguish adjacent cells without adding surface glow. A complex
exponential coordinate mapping progressively increases density toward the right,
so cells stay rounded as they get smaller. Contrast still fades toward gray at
the bottom. The spherical edge refraction and narrow grazing rim are retained.

The pattern is deterministic shared Rust code, with nine nearby cell sites per
sample and no assets or new dependencies. The existing one-time background
generation, shared 512² texture, edge antialiasing and retained rendering remain
in place. Tone mapping, control values, document data and exports are unchanged.

Validation: release and native-test builds passed. The five-size GTK
hit-region/cache check passed again, reusing one texture across 600 changed-value
updates without repainting unchanged arcs. The cell texture took 71.21 ms to
generate once off the UI thread. Screenshots at 128, 226 and 400 logical pixels
were visually inspected. Logs and screenshots are under
`artifacts/color-m4/proof-cells/`. Prior real-pointer and 60 MP evidence remains
archived; those checks were not repeated for this pattern-only change.
