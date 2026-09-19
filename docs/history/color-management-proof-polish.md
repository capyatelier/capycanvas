# Proof control polish

September 2026. Follow-up to the reviewed [contrast model](color-management-contrast-dial.md).

## Defaults and controls

The center reads 100% Contrast and 0% relative fine-texture balance. Its gains
now equal the previous 130% Contrast / +30% fine-texture setting:

```text
C = 2^vertical
macro = 1.3 × C × 2^(−(horizontal + 0.3)/2)
micro = 1.3 × C × 2^( (horizontal + 0.3)/2)
```

The fixed local illumination analysis, HDR range fitting, shoulder and log-odds
pivot are unchanged. Color intensity defaults to 30%; reset and double-click on
the color arc restore 30%. Brightness uses half its previous control range:
−50% to +50%, corresponding to −2 to +2 in the log-odds exposure coordinate.
CPU and WGSL use the same baseline; SDR export, gain-map bases and mapped print
preparation share it. No compatibility renderer or migration was added.

Contrast and balance percentages are horizontal, with the existing contrast and
grain icons above. Brightness and Color reuse existing symbolic icons beside
their curved readouts. Shared geometry positions the bottom text outside the
track, allowing for glyph height and the marker radius.

The center is a procedural rippled-glass illustration: smooth folds to the left,
fine ripples to the right, stronger contrast at the top and near-gray at the
bottom. It is a direction guide, not a preview or a measurement of the image.
Shared Rust produces the bounded, at-most-512² disposable SDR texture; GTK caches
it by device-pixel size. Parameter changes do not regenerate it. Removed the old
128-edge document thumbnail, readback, retained source and asynchronous preview
worker. The full-image local tone guide remains asynchronous and unchanged.

## Input defect

GTK's picker visits children before calling the parent's `contains()` method.
The circular GtkScale subclasses still contained invisible linear trough/slider
children. A real GTK pick at (64,54) in the 128-pixel dial selected `GtkGizmo`
inside the center circle; the retained before-fix regression fails there.

Those linear child subtrees are now non-targetable. GtkScale retains native
range accessibility and keyboard behavior. Both click and drag recognizers check
the visible circle/arc before claiming a sequence; only the owning control can
end or cancel its gesture. Decorative icons never receive pointer events.
Real input also caught a second-press ordering defect: the grouped drag handler
could overwrite a double-click reset. It now preserves the reset until release,
regardless of click/drag callback order.

## Validation

Evidence is in `artifacts/color-m4/proof-polish/`:

- 655 shared tests pass (93 core, 84 color, 478 UI); 7 external ICC fixture tests
  remain explicitly ignored. The numerical reference verifies the new center
  against the old 130% / +30% formula and keeps the vertical contrast guarantee.
- Two GPU tests pass for CPU/GPU point and spatial agreement, alpha, HDR master
  preservation and mapped print proofing (`gpu-local.log`, `gpu-point.log`).
- `hits-before.log` reproduces the original GTK child-picking bug.
  `hits-final.log` passes a two-pixel circle grid and both complete arc tracks at
  128, 160, 226, 320 and 400 logical pixels. `controls/` retains the screenshots.
- `pointer-final.log` passes actual Mutter mouse and virtual touch down/move/up
  delivery across the formerly stolen center strip, circle-to-arc crossing,
  independent arc changes, one-step Undo and color double-click reset to 30%.
  The earlier double-click failure remains in `pointer.log` for comparison.
- Five real HDR photographs, cardinal positions, both arcs, keyboard, Escape,
  exact guide reuse and source preservation pass (`photos.log`). Compact Paint,
  Photo, floating and print layouts pass (`layout.log`); screenshots are under
  `native/` and `layout/`. HDR editing/save/reopen/SDR delivery and actual JPEG/
  alpha AVIF gain-map export/reopen pass (`hdr.log`, `gainmap.log`).
- GTK release build and package check pass. No background compilation ran during
  the performance measurement below.

On the same Fedora 44 / RTX PRO 6000 Blackwell Max-Q / GTK 4.22.4 environment as
the previous review, the 8192×7324 ProPhoto F16 / 20-effect document completed
60 control updates plus 16 ms event pumping and a 100 ms drain in **1,125.95 ms**.
The largest 10 ms heartbeat gap was **24.55 ms**. Peak sampled process-tree RSS
was **1,300,840 KiB**, GPU residency **1,893 MiB**, codec staging **0 bytes**
(`60mp-controls.log`, `60mp-controls.memory.json`, 9 samples). The guide was
reused, Undo restored the recipe, and source layers were unchanged. This is one
warm run, not input-to-present latency or p95/p99; 0.5 s sampling can miss peaks.

The launcher uses copies with the new default settings; source documents remain
preserved, with byte-identical HDR payloads in all six copies. The preparation
script and hashes are in `prepare-review-fixtures.py` and `fixture-record.json`.
Physical touch/pen, HDR output calibration, mixed displays, constrained/mobile
hardware and other native hosts are unqualified. Previous browser workflow
failures are not claimed fixed. No new full-60-MP export or device-recovery
qualification is claimed; prior measurements remain in the preceding manifests.
Broader phase-4 gates remain open.

## Glass guide clarity follow-up

The directional illustration now uses one continuous family of glass folds.
Fold spacing tightens smoothly to the right, where the reflective creases also
sharpen. The top has clear black/white separation; a separate amplitude envelope
fades the bottom toward neutral gray. This replaces the superimposed fine waves
that made the first version's directions harder to read at small sizes.

Evidence: `artifacts/color-m4/proof-guide/`. GTK renders and hit-region checks pass
at 128, 160, 226, 320 and 400 logical pixels (`native-controls.log`, `controls/`).
The existing shared default/texture check and GTK release build pass. This is a
cached UI illustration change; the previous tone-mapping, export, input and
performance qualification remains recorded above, not claimed as newly rerun.
