# Unified drawing tabs: review and acceptance

The [behavior inventory](document-tabs-web-android-assessment.md) enumerates the
opening, presentation, input, session, close/recovery and storage contract. The
[implementation log](document-tabs-progress.md) records commits, upstream merges,
failed attempts and successful checks. This document is the review guide; final
deployment details will be filled after the final upstream synchronization.

## Shared ownership

`layer-ui::DocumentSessions` owns membership, stable IDs, order history, inactive
CPU editors, labels, admission and least-recently-used spill selection. Every host
keeps one active canvas slot. `UiSession::park_document` refuses active operations
and waits for exact current, undo and redo tile backing before GPU retirement.
`inherit_window_state` carries window settings/layout and current display size
without replacing drawing history, camera orientation or tools.

`layer-core::raster_storage` owns immutable compressed tile references. GTK and
Android use private, unlinked file chunks; Web supplies asynchronous OPFS chunks.
Successful writes publish backing references, failed writes retain RAM, reads
check identity, and final references release chunks. The default inactive pixel
budget is 64 MiB and non-spillable admission watermark is 256 MiB. These bound
owned inactive data, not total process RSS or driver allocations.

Hosts own pointer timing/capture, surfaces, provider/file handles and scheduling.
A window retains its GPU device; inactive editors retain no renderer. One bounded
activation/import job may overlap retirement. Proof, tone and inspection jobs
finish/cancel before exchange. Failure keeps the selected CPU editor available
for saving, closing, navigation or renderer restart.

Recovery uses one lease/policy per stable drawing ID and one serialized writer
per window. Accepted captures are immutable before they wait for storage. Opening
or recovering appends; saving or closing retires only that drawing's record.
Original recovery files survive until the replacement checkpoint is durable.

## Review journeys

1. Make ink, undo it, create another drawing, then return and redo. Repeat with
   different layers, zoom/rotation, tools and color depths in each drawing.
2. Open several files including duplicate names, the same URI/file twice, and a
   corrupt file between valid files. Valid results append in order; rejected
   results leave existing drawings intact. Cancel stops subsequent admissions.
3. Drag an inactive tab immediately with mouse, touch or pen. Selection stays
   unchanged. Escape, native cancellation, leaving the strip, resize or rotation
   cancels without a history entry. Reorder undo changes order only.
4. Narrow the title bar until it becomes a selector. Select/close by stable ID,
   use keyboard reorder or immediate handles. Touch/pen row bodies scroll before
   a hold; after a hold they can drag. Mouse holds do not open context menus.
5. Customize the title bar. The title component moves as one item; inner tabs
   remain inactive. Hide it and open Drawings from the application menu or
   Ctrl+Alt+D. Alt+PageUp/PageDown cycles drawings; Ctrl+Alt+W closes the selected
   drawing. Focused tab/list keyboard actions also support order undo and moves.
6. Close a background dirty drawing. It becomes selected before Save/Discard/
   Cancel. Cancel and failed Save leave it open. Successful close selects its
   right neighbor, or its left neighbor at the end. Final Android close flushes
   its workspace and finishes the Activity; final Web close leaves a fresh blank.
7. Recreate/rotate the Android Activity while multiple drawings are open. They
   retain their histories in the ViewModel and resume at the current viewport.
   Process termination instead uses independent recovery offers; recovery restores
   content as unsaved, without promising the pre-termination undo histories.
8. Leave two drawings dirty, restart, and recover both. Each appends independently,
   including when an earlier recovered drawing remains dirty. Keep for Later and
   Discard concern only their offered record. On Web, an inactive-only dirty tab
   still protects unload.
9. Exercise low cache budget, failed writes and retry. Tabs and exact undo/redo
   data remain usable after failure; further admission is refused with an error.
   Freeing space/retrying or closing drawings can allow admission again.
10. Drop photos on the title to open drawings, and on the canvas/layer list to
    place layers. Native drawing files open drawings. On Android, mixed canvas
    batches place photos into the captured original target first, then open native
    drawings after successful placement; cancellation prevents subsequent opens.

## Validation boundaries

Automated Huion input uses typed native mouse, stylus and touch events through
real native views. It validates device-specific UI arbitration and Vulkan/WebGPU
execution, not physical pen pressure calibration or thermal endurance. GTK uses
an isolated Mutter compositor and the actual desktop GPU. Desktop Chrome's
explicit offscreen test mode qualifies drawing data and lifecycle because of an
existing headless Dawn presentation warning; Huion browser presentation is
checked separately. External application file associations remain dependent on
OS/browser capabilities, while the shared file-open path is tested directly.
