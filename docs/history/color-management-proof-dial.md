# Circular SDR Proof controls — GTK review

September 2026. Replaces the rectangular Tone × Detail pad described in
[local tone mapping](color-management-local-tone.md). The local-Laplacian
algorithm, master storage, export transforms and gain-map representation are
unchanged. This is a control mapping and GTK presentation update.

## Layout and behavior

- Inner circle: **up increases Strength**; **right favors texture**, left favors
  smoother tone compression. It shows a live, reduced central crop of the image's
  SDR rendition. The main canvas remains the place to judge full-image detail.
- Top 120° arc: Brightness, with a dark-to-light ramp.
- Bottom 120° arc: Color intensity, from white to a color sampled from the scene.
  The existing highlight-color semantics remain: retaining color can lower the
  brightness of intense highlights. A neutral image has a neutral ramp.
- Small curved values identify amounts without permanent labels. Tooltips and
  accessible names identify each control. Strength is on the left of the circle;
  signed Balance is on the right. Positive Balance favors texture.
- Tiny refresh icon at the top right, matching Color's edit-button size/style.
  It resets the four appearance controls and preserves the stored HDR range.
  Auto, the old sliders, numeric rows and Update controls button are removed.
- Drag starts immediately, as for color wheels. Arrow keys adjust values, Shift
  takes larger steps, Escape/focus loss/unmapping cancels an active gesture.
  Double-click resets the contacted control. One completed gesture is one undo.

The circle uses the color picker's invertible square-to-disc mapping, so the
entire Strength/Balance range is reachable, including its corners. Shared Rust
owns the ranges, defaults, mapping, arc hit geometry and saved recipe. GTK owns
native capture and rendering. Arcs inherit GtkScale's range/accessibility
behavior; reset uses the existing Color utility button. Off / SDR / Print and
the print-profile controls retain their behavior.

## Parameter mapping and compatibility

For Strength `s` in `[0,1]`, Balance `b` in `[-1,1]`:

```text
b < 0:  tone = s × (0.8 − 0.05b), detail = 1 + 0.5sb
b ≥ 0:  tone = s × (0.8 − 0.35b), detail = 1 + sb
```

Default Strength 75%, Balance 0% reproduces the previous default exactly:
Tone 0.6, Detail 1. Strength zero leaves local illumination/texture processing
neutral and retains the basic global SDR mapping. The texture end still applies
useful illumination compression. Maximum strength reaches Tone 0.85 / Detail 0.5
at the left and Tone 0.45 / Detail 2 at the right.

The native file still stores the existing tone/detail values. Opening old files
does not migrate or quantize them. For a legacy method or a tone/detail pair
outside this envelope, the circle shows dashes and no invented position. The
user can adjust either arc without altering that saved tone/detail recipe;
moving the circle or resetting explicitly selects the new local controls.
At zero strength, Balance has no rendered effect and is retained within the
active control; a reopened neutral recipe starts with centered Balance.

## Preview work and precision

The existing cancellable source-analysis worker also creates a native-space,
linear Float32 thumbnail with a maximum edge of 128. This is a separate bounded
preview cache; it never replaces master samples. It adds a bounded source-row
pass when the artwork changes. Moving any appearance control reuses the full-
document guide and thumbnail and never rescans the document.

Each dial has at most one small CPU preview job, with superseded results rejected.
It applies the saved SDR mapping using full-document guide coordinates and
transports the resulting thumbnail to Cairo as SDR ARGB32. The visible circle
then crops its center. Reduced fine texture and UI transport quantization are
preview limitations; canvas/export continue to process full-resolution Float32
pixels from the half-float master. A thumbnail failure does not disable the
actual local SDR transform. Document/device replacement invalidates its cache.

## Validation

Evidence and screenshots: `artifacts/color-m4/proof-dial/`. The shared control
tests sweep 20,301 Strength/Balance combinations, validate the saved recipe,
round-trip its inverse and serialization, and check that unrelated delivery
settings stay unchanged. Geometry tests cover 128–400 logical-pixel layouts,
120° arcs, distinct hit areas and all circle extremes.

Recorded results on Fedora 44 / NVIDIA RTX PRO 6000 Blackwell Max-Q, driver
610.57.04, isolated Mutter 1600×1000 at 120 Hz and bundled GTK 4.22.4:

- All 477 `layer-ui` tests pass; workspace check passes with the existing Apple
  dead-code warning. No algorithm or file-format migration was introduced.
- Five HDR photographs pass open, circle and arc adjustment, actual GTK arc hit
  routing, one-step undo, Escape cancellation and source/guide preservation.
  Thirty scripted pad updates, including 5 ms event pumping, took about
  201–214 ms in the final isolated run. This is not input-to-present latency.
- Paint/Photo default layouts fit without scrolling. Print settings, Off/SDR/
  Print selection, floating panel tab dragging and undo pass.
  Screenshot review found that GTK's default scale padding hid arcs in the
  shortest panel; the arcs now share the HDR color arc's inset-free style.
  The native check verifies equal circle/arc coordinates and compact arc hits.
- HDR editing, saved appearance compatibility, save/reopen, SDR/HDR PNG delivery,
  HDR JPEG and transparent AVIF preview/export/reopen, and GPU failure/recovery
  checks pass. The linear thumbnail test retains extended and negative values,
  verifies alpha-aware reduction, and rejects invalid preview sizes.
- On the retained 8192×7324 ProPhoto F16 document with 20 effects, 60 scripted
  control updates plus 16 ms event pumping and a final 100 ms drain took
  1,172 ms. The largest 10 ms heartbeat gap was **25.10 ms**. Full-image analysis
  was reused, one Undo restored the recipe, and master layers stayed unchanged.
  Peak sampled process-tree RSS was **1,298,612 KiB** and graphics residency
  **1,893 MiB**, with no codec staging. This is a single warm-control run; 0.5 s
  sampling can miss brief peaks. It is not a final-file export measurement.

Use `photos-published.log`, `layout-published.log`, `hdr-final.log`, `gainmap.log`,
`recovery.log`, `linear-preview.log`, `ui-tests.log`, and
`60mp-controls-published.{log,memory.json}`. Earlier failed harness runs are retained:
the arc test initially omitted the panel's allocation offset, and the large-
document timer initially included synchronous test-only file serialization.
The final control run removes those harness errors and runs without another
qualification test alongside it. Build hashes are in the review manifest.

Physical touch/pen, mixed-monitor behavior and emitted HDR calibration remain
unqualified. Browser host integration is unchanged; its previously unresolved
workflow checks are not claimed as passes by this GTK change.
