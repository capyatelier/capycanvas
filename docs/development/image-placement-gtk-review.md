# GTK photo workflow: approved build

[Updated plan](../ui/image-open-import-proposal.md) ·
[Detailed evidence](image-placement-gtk-progress.md)

Built from `bef744d3` plus the local implementation on 2026-09-16.
The clipped-photo transform/paint slowdown is fixed and qualified. The user
explicitly approved the Open/Import/Drop workflow and large-photo responsiveness
of the package below. The GTK implementation, verification and approval goal is
complete. Restart an older running instance to load this renderer.

## Run and review

The implementation was subsequently committed as `8de027b7` and pushed to
`origin/main` with merge `522db8dc`. The merge retains upstream watercolor batching
and layer-local coordinates; 33 selected GPU checks and the GTK compile check pass.
The package hash and native timings below identify the original approved build.
Next-platform work is described in the [Web/Android handoff](image-placement-web-android-handoff.md).

From the repository root:

```sh
./dist/capycanvas-linux/bin/capycanvas
```

1. Open a JPEG from disk. It should create a document at the oriented source
   dimensions, with the complete photo fitted in the view.
2. Create a 2000×1500 drawing and import or drop a larger photo onto the canvas.
   It should become a new layer with scale/rotate handles and Apply/Cancel.
3. Resize or move it and Apply. Paint, Undo/Redo, save a `.capy` file and reopen it.
   Include a photo slightly larger than the canvas and at twice its fitted size;
   translate it while clipped, then Apply and draw on it.
4. Select Scale/Rotate and Original Size. The layer should return to native pixel
   scale with the original detail and aligned paint. Content outside the canvas
   remains stored; Apply does not resize the source into the canvas.
5. Import two files together. Verify two layers and one Apply undo step. Repeat
   and Cancel; the entire provisional batch should disappear.

The package includes the optional photo codec libraries and their notices/sources.
Package SHA-256:
`802f9a65cf7ed8b1ad091f111e285ea31036534ccbf17f64ef952940cc7d0183`.

## Verification

The clipped-photo follow-up passes on the new instrumented release build:

- Four real 61 MP GTK journeys: 1.1×, 1.2× and 2× fit, plus 1.2× with a second
  24 MP photo. Native translation and drawing, Apply, exact source retention,
  Undo/Redo, save/reopen, Original Size, cancellation and Open all pass.
- GPU regressions pass for previews above the former 128 MiB cap, reuse across
  the scale boundary, reduced memory allowance, paint/prediction/history and
  sharing the admitted memory among visible photos.
- The rebuilt package passes staging checks and an actual relocated launch of
  the 61 MP JPEG. Its production source manifest matches the native test build.

The preceding package also passed the broader qualification below. The only
subsequent production change is preview-memory admission; readers, drop handling,
placement transactions and file persistence are unchanged.

- **19 targeted GPU regressions pass**, covering source-cache changes, placement,
  off-canvas edits, masks, material sampling, exact export and retained sources.
  Two separate hardware performance tests remain explicitly ignored in that run.
- **Native multiple-selection Import passes** using keyboard input delivered
  through Mutter/Wayland. Ordering, Cancel, Apply, save/reopen and one Undo pass.
- Native external mouse/touch drops pass for canvas batches and Layers destinations,
  including cancellation, stale destinations and malformed second-file rollback.
- BMP/GIF/WebP and HEIC/AVIF Open/Import/Paste pass on the current GTK build.
- **Nine relocated application launches pass**: the 24/61 MP JPEGs and PNG,
  TIFF, BMP, GIF, WebP, HEIC and AVIF. Bundled codec paths are confirmed for
  HEIC/AVIF; the other readers correctly avoid loading those optional libraries.
- Real **24 MP and 61 MP** native workflows pass: Drop, transform, Apply, ordinary
  paint and material strokes with exact history, save/reopen, Original Size,
  Import cancellation and source-sized Open. Full-resolution source samples and
  exact artwork are checked, not just the displayed preview.

## Measured large-photo behavior

Isolated NVIDIA RTX PRO 6000 / Vulkan, 3200×2000 at 2×/120 Hz, 8 ms delivered
pointer events. No compiler, encoder or other GPU test ran during measurement.
The instrumented release test and package have separate binary identities and
application-launch evidence. The clipped cases use the new cache fix:

| 61 MP layer size | Translation GPU p95 / max | Drawing GPU p95 / max |
| --- | ---: | ---: |
| 1.1× fitted size | 3.62 / 7.15 ms | 4.37 / 7.41 ms |
| 1.2× fitted size | 4.27 / 7.66 ms | 4.34 / 7.64 ms |
| 2× fitted size | 3.55 / 7.06 ms | 4.44 / 6.90 ms |
| 1.2× with another 24 MP photo | 5.99 / 12.37 ms | 6.95 / 12.66 ms |

Single-photo delivered-pose-to-presentation p95 is 8.19–8.22 ms. The two-photo
case is 16.47 ms. Drawing presentation-gap p95 is 8.41–8.45 ms for one photo
and 8.82 ms for two. These runs exclude the separate large-smudge stress and
peak at 1,800–1,842 MiB process memory for one photo, 2,412 MiB for two.

The previous cap caused repeated source-tile fetches above ~1.19× fit: isolated
renderer completion at 1.2× improved from 108.16 ms median to 2.34 ms. Preview
memory now follows the renderer's existing measured GPU allowance. For this
photo it uses about 172 MiB more, preserves Float32 precision and leaves source
resolution untouched. An exhausted allowance can still require the slower
fallback; low-memory preview paging remains a follow-up.

### Earlier fitted-photo measurements

The following numbers belong to the preceding `b493d4bf` package's production
sources and include the separate material stress. They do not qualify the
clipped scale boundary; the new cases above cover that gap.

| Measurement | 24 MP, 4000×6000 | 61 MP, 9504×6336 |
| --- | ---: | ---: |
| Drop → first presented photo | 1.17 s | 1.65 s |
| Drop → full-photo thumbnail | 1.53 s | 2.11 s |
| Scale GPU queue-span p95 | 3.73 ms | 4.13 ms |
| Ordinary paint GPU queue-span p95 | 4.10 ms | 4.34 ms |
| Delivered transform pose → presentation p95 | 8.24 ms | 9.67 ms |
| Save | 293 ms | 328 ms |
| Whole-workflow peak process memory | 1,885 MiB | 2,537 MiB |

The workflows include two material strokes and temporary additional photo imports.
Peak process memory includes caches and driver allocations; it is not isolated
GPU memory. Queue span includes submission waiting. These measurements describe
this hardware and workload, rather than physical pen latency or a continuous
120 Hz guarantee.

## Known limitations for this review

- **Very large smudge was slow in the preceding qualification:** a 1024 px brush on the placed 61 MP photo
  has 71.55 ms median / 175.67 ms maximum presentation gaps. Ordinary painting is
  measured separately above. The 240 px twirl case has 8.90 ms p95 / 37.89 ms max.
  A subsequent native-size comparison records 21.32 ms smudge p95, showing the
  effect of source-space brush size; this is not a historical regression A/B.
  Broader smudge optimization is deferred as requested.
- Pan produces roughly 60 changed-camera presentations/s on the 120 Hz test display.
- Format support covers JPEG, PNG, TIFF, BMP/DIB, GIF, WebP, HEIC and AVIF with
  explicit variant restrictions. Unsupported HDR and some animation/container
  variants report errors; this does not promise every valid variant or RAW/PSD.
- Physical pen/provider breadth and sustained heavy-material or many-large-layer
  workloads remain further qualification, outside the focused core review.

The user approved the delivered GTK workflow and responsiveness with these limits
disclosed. Further qualification remains tracked separately. The approved
implementation is committed and published as recorded above.

## Evidence and captures

Artifacts are in `artifacts/image-placement/gtk-closeout/`, including input/source/
binary hashes, native logs, raw frame statistics and relocated-package records.

- [Clipped 61 MP placement at 1.2× fit](../../artifacts/image-placement/gtk-closeout/clipped-photo/native-1.2/active-placement.png)
- [Paint retained at Original Size after the 2× case](../../artifacts/image-placement/gtk-closeout/clipped-photo/native-2/original-size.png)
- [Rebuilt package opening the 61 MP JPEG](../../artifacts/image-placement/gtk-closeout/clipped-photo/package-smoke/0-61mp-DSC02494.png)
- [61 MP active placement](../../artifacts/image-placement/gtk-closeout/native-61mp/active-placement.png)
- [61 MP Original Size after paint and save/reopen](../../artifacts/image-placement/gtk-closeout/native-61mp/original-size.png)
- [24 MP source-sized Open](../../artifacts/image-placement/gtk-closeout/native-24mp/opened-photo.png)
