# HDR picker spacing and paint previews

Follow-up to [the arc review](color-management-hdr-picker-arc.md), in
`~/code/capycanvas3`. Nothing is pushed pending user review.

## User-visible changes

The gap outside the hue ring now equals its gap from the inner circular field.
All three selector circles use the same radius. Foreground, background and
transparency keep their individual SDR edge clearances from the new outer arc;
the footer follows those positions instead of a fixed 44px shift. The EV arc is
slightly shorter from end to end, with unchanged thickness.

The paint bubbles and compact foreground/background indicators now use the
picker's HDR display presentation. Their colors already contain the chosen EV;
the previews do not multiply it a second time. HDR Edit Color shows **Base** and
**Adjusted** side by side in a single rectangle above the fields. EV changes
leave the base preview stable and update the adjusted half. Direct component
edits update both from the current draft. SDR dialogs keep a single preview.

Paint and dialog previews use tagged half-float Rec.2100-linear textures on a
capable display. Shared Rust performs Float32 color/EV and HDR mapping; alpha
composites the mapped artwork over the neutral checker in linear display light.
Alpha does not change the HDR shoulder. The saved SDR rendition remains the
fallback for an SDR display. Live dialog previews follow capability changes
without changing their draft or publishing paint. Closed dialogs leave only
weak registrations, pruned on the next refresh.

## Validation and review

Current logs and captures use `artifacts/color-m4/hdr-color-preview-*`.
The review launcher, executable hashes, source commit and previous build are
recorded in `artifacts/color-m4/review/build-manifest.json`.

- `shared.log`: 44 color tests pass, including base/adjusted RGB, alpha, signed
  values and draft preservation. `final-geometry.log` covers the shortened arc,
  equal gaps, selector radii and swatch clearances from 128 to 1024 logical pixels.
- Workspace/all-target and wasm32 checks pass. Native release builds pass.
- `native-final.log`, `2x-final.log`: native 1×/2× checks exercise mouse/touch/keyboard, double-click reset and editing,
  both paint slots, Apply/Cancel, all shapes, SDR picking and narrow/light/dark
  layouts. The comparison halves meet without a gap within native pixel rounding.
- HDR checker samples at alpha 0, 0.25, 0.5 and 1 match the display shoulder and
  linear checker composition. The adjusted dialog preview matches the foreground
  bubble. EV changes preserve the base half and change the adjusted half.
- `desktop/test.log`: the HDR desktop checks above-white GSK output, both paint
  bubbles, live dialog fallback/restoration and unchanged paint. GSK peaks are
  2.5515 for the edited dialog draft and 1.5594 for the unchanged foreground
  bubble behind it. Screenshots alone cannot prove HDR output.
- The same desktop run loads the 8192×7324 ProPhoto F16 document with 20 effects.
  Peak process RSS is 1,548,956 KiB (about 1.48 GiB), including dialogs and captures;
  see `desktop/process-memory.log`. The 60-update loop takes 744.03 ms including
  event pumping and coalesced rendering, not an input-to-present qualification.
  Host: Fedora 44, RTX PRO 6000 Blackwell Max-Q, NVIDIA 610.57.04, Vulkan.

The initial comparison-width assertion used exact equality on transformed GTK
floating-point bounds; it failed by 0.00006 pixels. The retained
`native-fractional-bounds.log` records it. The assertion now permits native pixel
rounding and still checks that the two halves meet. Earlier spacing-only captures
remain under `artifacts/color-m4/hdr-spacing-*`.

The earlier numerical, save/reopen, delivery and 60 MP evidence remains in the
preceding review manifest. Physical luminance/calibration, mixed-monitor moves,
physical touch/pen and other device classes remain unqualified. Browser HDR is
unsupported. The known older SDR hue-guide tolerance failure is unchanged.
