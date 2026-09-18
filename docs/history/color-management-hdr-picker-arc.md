# HDR picker arc review

This revision follows the horizontal picker milestone in
[color-management-hdr-picker.md](color-management-hdr-picker.md). It implements
user feedback in `~/code/capycanvas3` (implementation
`586b6f639283ee18f95507957c78a15b3356dfba`); nothing is pushed pending review.

## Behavior

- HDR documents have a current-color intensity arc below the hue circle, using
  the hue ring's thickness and a circular selector. The shapes still show the
  chosen EV; SDR shape rendering and picking remain unchanged.
- Drag follows the arc. Double-click anywhere on the track resets to **1× (0 EV)**.
  Keyboard range actions remain available. There is no visible HDR heading.
- The EV readout follows the lower arc, left of transparency. It is read-only;
  clicking it does not edit a number or pick through into the wheel. The two
  paint circles, swap button and transparency control sit below the arc.
- The pencil sits opposite the OKLCH label, at the same size as Swap. It is
  available in SDR and HDR. Double-clicking either paint circle opens that
  circle's editor. Existing single-click selection and context menus remain.
- HDR Edit Color puts editable **Intensity (EV)** directly below Model. Changing
  EV multiplies linear RGB and leaves alpha unchanged. Numeric RGB describes
  the final color; editing it retains the chosen EV. Cancel leaves the paint
  untouched. Accepting an untouched form retains the exact definition and
  remembered intensity, including dark colors and black.

Shared Rust owns arc geometry, HDR draft math and validation. GTK owns native
click timing, angular pointer capture, keyboard range actions and presentation.
The slider uses immediate value-control input under the
[drag convention](../ui/drag-and-reorder.md). It retains managed Float32-derived
HDR textures and an explicit SDR display derivative. No document precision or
export transforms were changed.

## Validation

Artifacts use the `artifacts/color-m4/hdr-arc-*` prefix. The runnable package and
its exact executable hashes are in `artifacts/color-m4/review/`.

- `shared.log`: 460 shared UI tests pass. New checks cover angular hits, separation
  from the wheel and footer buttons, exact numeric drafts across all RGB spaces,
  EV multiplication, alpha, black, signed colors and rejected invalid/range edits.
- `final-color.log`: all 44 color tests pass after the final selection fix.
- `workspace-check.log`, `web-check.log`: all-target workspace and wasm32 checks.
- `native-final.log`, `2x-final.log`: 1× and 2× isolated Mutter checks with real
  injected mouse/touch events and keyboard actions. They cover dragging, reset, the pencil, read-only
  caption, both swatch editors, Apply/Cancel, SDR picking, all three shapes,
  transparency, and narrow/light/dark layouts. Captures are retained alongside
  the logs. Touch injection is not physical touch or pen qualification.
- `60mp-final/test.log`, `60mp-final/process-memory.log`: the 8192×7324 ProPhoto
  F16 document with 20 effects passes on the HDR desktop. Rec.2100-linear field
  textures and GSK arc output retain above-white values; capability loss/restore
  preserves paint. Peak process RSS is 1,536,296 KiB (about 1.47 GiB), including
  load, dialogs and screenshots. The 60-update smoke loop took 570.84 ms, versus
  229.03 ms in the 1× virtual run and 1434.69 ms at 2×. These include event pumping
  and coalesced rendering; they are not input-to-present or 120Hz qualification.
  The earlier run before reordering the EV row remains in `60mp/`.
  Host: Fedora 44, NVIDIA RTX PRO 6000 Blackwell Max-Q, driver 610.57.04, Vulkan.
  The compositor's 49.261× headroom / 10000 cd/m² hint is not measured luminance.
- `journey-final.log`: the native HDR journey verifies open, exact color entry,
  exposure/curves,
  painting, undo/redo, SDR appearance, save/reopen and HDR/SDR delivery.

The first native attempt used a button label instead of a dialog response ID in
its test helper; its log is retained as `native-input-response-fixture.log`.
The corrected runs pass. The first full HDR journey (`journey.log`) still
looked for the removed horizontal control; its assertion was updated to query
the arc, retaining the check that a 65504 sample extends the drag interval.
At 2× on a 500-logical-pixel-high desktop, the native
editor scrolls and GTK emits minimum-width measurement warnings; EV remains
near the top and the dialog actions stay reachable.

The earlier SDR hue-guide comparison still has its documented baseline failure
(3/255 versus 2/255 tolerance); its assertion was not weakened. This revision's
native SDR picking checks pass. Physical luminance/calibration, mixed-monitor
moves, physical pen/touch and other device classes remain unqualified. Browser
HDR remains unsupported. The earlier phase-4 hardware/performance gates are not
claimed complete by this UI revision.
