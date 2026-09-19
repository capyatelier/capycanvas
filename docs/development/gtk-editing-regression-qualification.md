# GTK fill and gradient regression qualification

2026-09-19. Base: fetched `origin/main` at
`caf46ebf320fd69e140452c024ee831085de0811`. Branch:
`gtk-editing-regressions`. Evidence: `artifacts/gtk-editing-regressions/`.

## Report, reproduction and repair

The user's `cargo run --release -p layer-linux` log reported
`Canvas failed: Invalid paint operation`, followed by
`Painting is unavailable. Save the drawing and reopen it.`
The same sequence reproduces on a fresh sRGB8 canvas with a portable P3 red.
Its document-linear RGB is approximately `[1.2249402, -0.042057, -0.019638]`.
These are legitimate color-conversion coordinates. The old fill/gradient
operation validator incorrectly required every RGB component to be in `[0,1]`.
The asynchronous bucket completion returned that validation error from the
frame path, causing GTK to suspend the canvas. The old gradient reproducer also
failed to commit pixels. This is a document-validation defect, not evidence of
a GPU driver failure.

Fill, gradient and figure validation now accepts finite signed/extended linear
RGB, matching portable colors and brush paint. Alpha still must be finite and
in `[0,1]`. Geometry, mask validation and native storage checks remain intact.
There is no preemptive RGB clamp, changed precision, renderer workaround, UI
redesign, or change to the reviewed Proof controls. No Float32 storage work was
added; upstream's existing mode remains as inherited from main.

The regression test failed before the fix (`core-before.log` and
`baseline-p3-fill.log`). Bounded sRGB fill/gradient controls had passed and did
not establish that portable colors worked. The earlier phase-4 signoff was too
narrow to qualify these basic editing workflows.

## Checks

- All shared core, engine and UI tests: 98 + 63 + 483 passed
  (`shared-tests.log`). NaN/infinite RGB and invalid alpha remain rejected.
- New native GPU reference matrix: four RGB spaces × U8/U16/F16 × seven paint
  operations (fill, four gradients, filled rectangle, outlined/filled rectangle).
  Checks both sides of a tile boundary, transparency, premultiplied interpolation,
  exact half-quantized references, and at most one native code of integer error.
  Every operation also passes exact undo/redo and save/reopen sample equality
  (`gpu-extended.log`). The first test iteration corrected its independent
  oracle: fill-only figures use foreground; outlined/filled figures use background
  for their interior. Numerical bounds were not relaxed.
- Existing native-raster GPU suite: 11 passed; layer GPU suite: 60 passed.
  Includes brush/selection isolation, alpha locks, masks, transforms, partial
  publication, invalid-output rejection and recovery. Fifteen explicitly ignored
  performance/large-document tests were not included in these correctness runs.
- Real Mutter → Wayland → GTK mouse contacts at 1× and 2×: sRGB8, sRGB16 and
  P3 Float16; bucket, four gradient modes and a rectangle; exact undo/redo; actual
  GTK Save/Open choosers and workers; continued painting after reopening
  (`native-1x.log`, `native-2x.log`). Screenshots and saved masters are retained.
- Existing native brush/selection/bucket, selected-brush, gradient and HDR
  failure/restart tests pass. The recovery test deliberately injects a GPU
  validation error and verifies the last backed checkpoint and resumed painting.
- The old native figure test also failed on unchanged main: it inspected
  discarded `pending_operations` after raster publication. Its repair checks
  the native Shift-constrained guide at the original 0.001-document-pixel bound,
  then checks that an actual host-backed raster edit was committed. Ellipse
  vertices are checked against the analytic circle rather than assuming a
  tessellated guide contains every cardinal point.
- The normal release package builds with the pinned GTK startup fix. Its
  relocated executable opens/captures the three saved editing masters and loads
  the packaged GTK library (`package-validation/report.json`). Native photo
  codecs are bundled but are not exercised by opening these `.capy` documents.

The GPU is the NVIDIA RTX PRO 6000 Blackwell Max-Q, driver 610.57.04, Vulkan;
GTK selects PCI `f1:00.0`. Native input runs use private 120 Hz Mutter displays,
1600×1000 at scale 1 and 3200×2000 at scale 2. These are correctness runs, not
new latency or sustained-memory measurements.

## Runnable review and scope

Run `artifacts/gtk-editing-regressions/review/launch.sh` for a fresh review
instance, or pass one of `Editing-U8.capy`, `Editing-U16.capy`, `Editing-F16.capy`
from that directory. The package is also at `dist/capycanvas-linux/bin/capycanvas`.
The ordinary `run 3` / Cargo release path is rebuilt with the same fix.

These results qualify the reported painting regression. They do not establish
that every feature or the entire HDR milestone is finished. The
[phase-4 qualification](color-management-m4-gtk-qualification.md) still records
the two outstanding JPEG fidelity assertions, animation-guide discontinuities,
and missing physical pen/touch, optical/display-transition and other-device
evidence. This patch does not change those contracts or reuse old measurements
as new evidence.
