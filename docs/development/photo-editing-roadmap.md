# Photo editing roadmap

[Developer guide](README.md) · [Photo editing research](../history/photo-editing-research.md) · [M2–M4 record](../history/photo-editing-m2-m4.md) · [Photo editing performance](photo-editing-performance.md)

Status: **open** (2026-09-28). Milestones M0 to M4 of the [research record's sequencing](../history/photo-editing-research.md#7-recommended-sequencing) are done: M1 on every host, and M2 to M4 on GTK, Web and Android. This plan lists what is left of the photo editing epic. Item IDs (GEO-1, RET-5 and so on) are specified in sections 5 and 6 of the research record, and journey numbers refer to its section 2. Performance and memory gates are in [photo editing performance](photo-editing-performance.md).

Each milestone gets its own implementation plan, as M2–M4 had: decisions first, then steps with tests and an exit test, then host journeys in light and dark. Delete the plan when its work lands.

## Milestones

| Milestone | Contents | Journeys |
| --- | --- | --- |
| **M5 Lossless transforms** | P-8, P-10 with lossless Distort and Warp on placed photos, the rest of XF-4 (groups and several layers, pivot, snapping, nudge, Transform Again), split lines, XF-5, XF-6. | Opens 27 for placed photos; improves 17, 25 |
| **M6 Tone and color** | P-6, P-7, ADJ-1 with ADJ-4 and the picker bar modes (BAR-5), ADJ-2, ADJ-3, ADJ-5, ADJ-6, ADJ-10, VIEW-2 with its bar mode, VIEW-3 with sampler bars, IO-3, T-3, T-7, T-14. | Improves 5–10, 13, 29 |
| **M7 Masking and compositing** | SEL-6 as an on-canvas session, SEL-7, SEL-8, LYR-3, LYR-4, ADJ-7, ADJ-8, T-10. | Opens 15; improves 11, 12, 14, 16, 17, 19 |
| **M8 Advanced** | RET-5 (Content-Aware on the selection bar), RET-6, RET-8, RET-9 history brush, LYR-6 to LYR-9, IO-4 to IO-6, ADJ-11 remainder, ADJ-12, SEL-9, VIEW-4, VIEW-5, T-12, T-19, and the BAR-8 decision. | Opens 9; completes 21, 28 |

Order: M5 to M8 as listed. Gradient interpolation in Oklab (T-14, in M6) is a per-gradient choice, as decision 4 of the M2–M4 record planned.

**Later, by decision:** per-layer linear blending for non-Normal layers in Perceptual documents; constant-colour pages, so a Dodge & Burn layer and other fill layers stop costing a full layer of GPU memory.

## Apple and Windows

M1 shipped on every host. M2 to M4 need porting through the [Apple](../APPLE_PORTING_GUIDE.md) and [Windows](../WINDOWS_PORTING_GUIDE.md) porting guides. Shared Rust already provides the behaviour; the hosts need to present it:

- **Shared UI state:**
  - present `UiState.notice` and answer `UiAction::Notice` (Apple also does not show errors raised during a gesture; the notice fixes that);
  - read `CommandState.disabled_reason`, then retire `canvas_bar_reason`;
  - open bar menu items (`CanvasBarItem.menu` and `icon`, through `canvas_bar_choice_menu`);
  - draw a `checkable: false` action unpressed even when its command is selected.
- **M2:**
  - mode and guide bar labels;
  - the Refine dialog;
  - the zoom readout control (the `zoom_menu` query and `UiCatalog.zoom`);
  - WebP in the export lists and file types (the edits to Apple's `ExportForm.swift` and `ProjectFiles.swift` and to Windows' `ExportForm.h` are untested);
  - the labelled Color row with a "use current colour" bucket (`color_action`).
- **M3:**
  - the Canvas Size and Image Size dialogs, with the anchor picker (`layer_tools.canvas_size`);
  - the Edit ▸ Image submenu;
  - the crop tool icon (the overlay is shared renderer code);
  - clipboard image writers;
  - the export metadata row.
- **M4:**
  - the retouch tool icons (the source disc and its bar are shared);
  - the grouped blend menu instead of the flat `layer_blends` picker, which lists Pass Through for every layer;
  - the New Document Blending field (Edit ▸ Blending and the Properties row are shared);
  - the Color mixing brush choice;
  - the Use Pass Through for new groups setting;
  - the Frequency Separation dialog (`frequency_separation`, shaped like Refine) and the Dodge & Burn and Frequency Separation icons.
- **Tests:**
  - Apple's `command-coverage.json` lacks the M2–M4 commands;
  - `CanvasToolChecks.swift` must expect the new preset count and Liquify labels (Push, Twirl Counterclockwise, Twirl Clockwise, Pinch, Expand, Crystals);
  - Apple's Selection Mask checks may wait for a Grow flow that the Refine panel replaced;
  - the Swift ruler fixtures now check the `CAPYRASTER` signature and have not run on a Mac.

## Open items from M2–M4

**Behaviour**
- **Copy to New Layer** drops the source's mask and clipping, and the copy is unlocked. Copying from a locked layer is allowed; cutting is not.
- **Undo after a canvas change** is briefly disabled and says "Nothing to undo".
- **Hidden pixels** can still be written by brush dabs past the canvas edge and by a fill through an inverted selection.
- **Reselect** keeps the selection's position on undo of a canvas change.
- **Straighten and Delete Cropped Pixels:** Delete Cropped Pixels trims masks by tile, so a band under 256 px of mask coverage can remain.
- **Image commands:** undo of a turn or resize does not re-centre the view, and the resampled-tile prediction can overcount by one row or column.
- **Eyedropper:** choosing another tool while the Eyedropper is active returns to the previous tool.
- **Leave Copy** is not remembered across sessions.
- **Disabled shortcuts:** a shortcut pressed while its command is disabled gives no notice (for example Ctrl+C while a selection is still being prepared).
- **Preference actions** sent while Settings is closed are refused with no visible error.
- **Ungroup of an isolated group** ignores adjustment children below its layers.
- **Contact brushes without a release limit** (the Eraser, for example) still taper the last span when pressure falls without motion.
- **Metadata:** writing IPTC-IIM, and Extended XMP for packets larger than one JPEG segment.
- **Android:** right after a stylus Wand selection is published, a layer edit can briefly be refused with "Finish the canvas interaction first".

**Blending**
- **Spot Healing** scores its candidates on linear values; only its tone match follows the document's Blending.
- **Export matte and resize** run in linear light in Perceptual documents.
- **Brush previews** are rendered in Linear light while new documents blend perceptually, and previews 10 (Smudge) and 24 (Natural Blender) still show linear mixing.

**Host presentation**
- **Properties panel** still offers a flat blend choice in code order, including modes hidden from the menu in float documents.
- **GTK Document Properties** builds its rows in the host instead of from `DocumentInfo::describe`, as Web and Android do.
- **GTK Tool Options** put every grouped checkbox option in one radio group, which would misbehave if two groups ever showed at once.
- **Clone source bar on tablets** sits at the bottom of the work area instead of beside the disc.
- **Android bar captions** clip their last glyph (for example "Apply", "Disable" and "Edit Content").
- **Android Tool Options numbers** that are not sliders need two taps: one shows the field, one focuses it.
- **Android Tool Set** lists Crop twice while the Crop tool is active.
- **Accessible names:** relabelled bar buttons are announced by their command's label ("Enable Layer Mask" for a button reading "Disable").
- **Web on a tablet:** twice, interior tiles drew white after a crop or straighten Apply in the Huion's Chrome; it did not recur in six later runs.

**Tests and tooling**
- **GTK tablet proxy:** `--tablet` runs lose their Wayland connection whenever Quick Mask or Selection Layer rows change, and at the first clipboard write, so those pen journeys run on Web and Android only.
- **Web tests on tablets:** after a run leaves an unsaved document, the next `device.test.mjs` load waits on "Recover drawing?".
- **Headless Web** screenshots leave out WebGPU pixels, so the Clone and Heal live-preview checks need a headed run or a tablet.
- **Flaky test:** `live_display::tests::moving_transforms_drawn_into_the_display_match_recomposition_and_release_exactly` fails intermittently under heavy parallel GPU load.
- **Stale Web filter copy:** `apps/layer-web/filters` is committed but unread (filters load from `assets/filters`) and out of date; delete it or regenerate it.
