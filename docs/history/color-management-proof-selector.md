# Proof selector compositing and export cleanup

Follow-up to the [cached Proof guide](color-management-proof-lens.md).

The existing draw order was correct: the immutable circle texture was drawn
first and the selector above it. The texture was not regenerated or painted into
during dragging. Offscreen widget snapshots and a standalone control did not
reproduce the reported trail.

Capturing the actual Mutter-composited application window did reproduce it.
After moving around the circle, five dark pixels remained at an earlier marker
edge (x=137–141, y=907 in the 1600×1000 capture). The regression fails on frame 12
in `artifacts/color-m4/proof-selector/composited-1x.log`; its screenshots retain
the evidence. These pixels are outside both the initial and current selector.

The selector now has its own small retained Cairo/GSK node, with transparent
padding around its antialiased edge. Dragging translates that node over the same
shared circle texture; its bounds cover the previous edge when GTK computes
damage. Size and keyboard-focus appearance are the only marker cache keys.
Unchanged arcs and readouts still reuse their nodes. The empty input widget no
longer receives redundant drawing invalidation. No tone-mapping, document or
export pixel-processing algorithm changes are involved.

Proof controls no longer show tooltips; accessible names and keyboard guidance
remain. Export Image no longer contains a Proof entry or a return-to-export
workflow. It captures the saved document rendition and still waits for pending
print-profile preparation. The short output description beneath Quality is
removed. The clipping option's group follows its visibility, removing the shadow
of an empty boxed list while preserving explicit clipping for unsupported ranges.

## Validation

Evidence is under `artifacts/color-m4/proof-selector/`. The review manifest records
the source commit, build hashes and completed checks. The native input driver can
optionally capture the isolated compositor through PipeWire; it never records or
injects input into the user's desktop. Screenshots use the real incremental
window-damage path, not `WidgetPaintable.render_texture`.

The 60 MP pre-change run completed 60 updates, deliberate 16 ms event pumping and
a final 100 ms drain in 1,136.98 ms, with a maximum 10 ms heartbeat gap of 12.44 ms.
After the fix, the same workload took 1,134.96 ms with a 12.57 ms heartbeat gap.
Input-handler CPU time was 0.483 ms median, 0.844 ms p95 and 1.268 ms maximum.
The guide analysis and circle texture were reused; one Undo restored the recipe,
and the master layers were unchanged. These single runs do not establish a
general speedup or input-to-present latency.

- Actual application compositor captures passed at 1× and 2×: 24 frames each,
  zero stale pixels outside the initial/current marker footprints, with visible
  new selector pixels over the unchanged guide. The faster initial 2× capture
  read the previous presented state; the completed 2× check allows 80 ms between
  pointer delivery and capture. This is a damage regression, not a latency test
  (`fixed-1x.log`, `fixed-2x-settled.log`).
- Cache/hit-region checks passed at 128, 160, 226, 320 and 400 logical pixels.
  All 600 changed-value updates reused the marker node, one shared circle
  texture, both unchanged arc snapshots and unchanged arc readouts. The one
  background texture build took 222.86 ms.
- Real mouse and virtual-touch input passed, including boundary crossing,
  double-click reset and one-step Undo (`pointer.log`).
- Export navigation, HDR range preflight and explicit clipping passed. The
  HDR input → edit → saved Proof → save/reopen → HDR/SDR delivery journey passed.
  Native screenshots confirm no Proof row or empty bottom group shadow.
- HDR JPEG and transparent AVIF recommendations, HDR/SDR preview switching,
  flattening and export/reopen passed (`gainmap.log`). Screenshots in `gainmap/`
  show Quality without the output-description text beneath it.
- Release application and native-test builds passed. The 60 MP run reported
  maximum RSS 1,286,700 KiB; 500 ms sampling observed process-tree RSS up to
  1,256,312 KiB and graphics residency up to 1,893 MiB (8 samples). Sampling can
  miss short peaks; graphics residency includes driver allocations.

The measurements used GTK 4.22.4 with the bundled runtime guard, Vulkan on an
NVIDIA RTX PRO 6000 Blackwell Max-Q, and isolated Mutter at 120 Hz. Physical panel
response, touch/pen hardware, constrained devices and other hosts remain
unqualified. Broader phase-4 qualification is unchanged.
