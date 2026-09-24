# GTK drawing tabs

Status: implemented and reviewed, 2026-09-19. Native qualification is recorded below.

## User experience

A GTK window owns an ordered set of drawings, one selected drawing, and one
canvas. The existing Document Title title-bar component becomes the drawing tab
strip. Tabs divide its available width equally and expand into unused title-bar
space. Each shows its name, unsaved marker, full location in a tooltip, and a
close button. Below 140 logical pixels per tab, replace the strip with a compact
selector showing the current drawing and a scrollable list of all drawings.
Shared Rust packs neighboring items toward the window edges and gives the title
item the remaining interval, including when customization places it in a side
zone. Native controls and 12px window-drag gutters remain available. Customize Title Bar retains its existing geometry
and intercepts inner-tab input. Resizing changes presentation only, never selection or tab order. A selector
remains reachable through the title-bar overflow and keyboard when the title
component is hidden by a customized workspace. With one drawing, show the original
plain title and dimensions at the normal title width, with native window dragging;
tab styling, the close button, and the dropdown appear only for multiple drawings.

Tabs keep GNOME Web's full-height [AdwTabBar shape](https://gitlab.gnome.org/GNOME/libadwaita/-/blob/main/src/stylesheet/widgets/_tab-view.scss)
without an accent underline. Tabs abut in one full-height strip on the title
bar's translucent canvas surround; the selected tab uses an opaque grey
matching GNOME's window close button, with full-tab hover/pressed overlays, separators only between adjacent idle tabs, and 24px
circular close controls. Keyboard focus outlines the whole tab; high contrast
adds the native inset border. Titles stay centered across the full tab width.
The application CSS provider follows Adwaita's high-contrast preference, including
changes while a window is open, so its media query uses the same mode as the theme.

New, Open, desktop/command-line file activation, native drawing drops, and
recovery append and select a tab in the initiating window. External activation
uses the active drawing window, creating one if necessary. File preparation is
serial and belongs to that window; closing it cancels late results. Explicit
New Window remains a way to create a separate window. Place/Paste and image drops
onto a drawing/layer list keep their existing insert-as-layer semantics. Dropping
image files on the drawing tab strip opens them as drawings. Failed or cancelled
opens leave current drawings unchanged. Reopening a path intentionally creates an
independent drawing, consistent with existing opening semantics.

Ctrl+Tab / Ctrl+Shift+Tab cycle drawings, Ctrl+PageDown / Ctrl+PageUp do the same,
Ctrl+W closes the current drawing, Ctrl+Shift+W closes the window, and
Ctrl+Shift+A opens the drawing selector even when Document Title is removed or
chrome is hidden in fullscreen/Zen mode. Ctrl+N
continues to open the New Drawing dialog. The selector supports keyboard focus,
full names, unsaved indicators, and individual close controls. Tab bodies reorder
by native press/movement slop without a hold for mouse, touch, and pen. Reordering
stays inside the window; no tear-off window is implied. Drop validation, order,
cancellation, and undo/redo of tab order are owned by a small Rust tab model;
layout undo is separate from drawing undo. Compact list rows have explicit grab
handles if reorder is exposed there; otherwise they only select/close.

Closing a tab uses the existing shared Save/Discard/Cancel decision. Cancelling
save or a failed write leaves that drawing open. Closing a window visits tabs
sequentially, stopping at cancellation/failure; already approved closes stay
closed. Closing the last tab closes the window. Background close selects
the drawing so the user sees what is being saved/discarded, even when its canvas
is unavailable. Select the right neighbor after close, or the left neighbor
when closing the last tab in order. Duplicate names show locations in the selector;
new untitled drawings have stable numbered labels. Workspace ownership is released
only when the final tab/window closes.

## Ownership and switching

The window keeps a CPU-backed Wayland surround below the GPU canvas for its whole
mapped lifetime. Startup, parking, failed rendering, and renderer replacement
therefore show the theme background instead of exposing other windows. GTK starts
opaque and becomes transparent only after that background has been installed;
synchronized subsurfaces publish its pixels and geometry with GTK's parent commit.
Nine slices retain the 12px window corners while stretching a tiny immutable
shared-memory sample (2,500 bytes at scale 1, bounded below 150 KiB), independent of
window area and tab count. Resizing reuses the sample; palette, scale, and corner
changes replace it. No old drawing image or inactive GPU worker is kept for this.
This requires the stable Wayland viewporter interface in addition to the existing
subcompositor/shm support. If background initialization fails, keep GTK opaque
and report the startup failure. A later update failure uses the same opaque
fallback; successful updates or canvas restart restore the live drawing view.

Keep one Workspace/native window and its retained controls. Store the active
GpuCanvas in its existing slot and inactive GpuCanvas sessions in a tab owner.
An inactive renderer is stopped and joined, retaining no native GPU surface,
renderer worker, device/snapshot handles, thumbnails, or pending image replies.
Reactivation reuses the existing renderer-replacement API, which preserves
editor history, save checkpoints, exact raster roots, camera, selection, tools,
color interpretation, and document-owned proof settings. Recovery ownership is
per tab and switches with its session. Recovery snapshots and their recovery owner are captured synchronously before
parking. Async writers never reacquire a document through the active slot. A
window serializes these writes; unsaved inactive drawings retain independent
recovery and no writer can accidentally capture its successor.

Window settings, workspace layout/history/manager ownership, fullscreen, and
native chrome remain window-owned. Copy that window state to the incoming
session through a shared Rust method; do not recreate native workspace managers
or replay drawing commands. Document tools/camera/history remain per tab.
Invalidate retained document-dependent UI caches on selection changes.

Switches are serialized and disabled while a document request, modal dialog,
canvas contact, transform/placement, layout gesture, or workspace transition is
in progress. Existing async operations therefore cannot complete against another
document that happens to reuse an epoch/request number. Open requests queue their
prepared result until their initiating request has completed. Pause/cancel proof,
local tone, and histogram jobs and retire them before releasing the old GPU.
Wait for submitted input/restoration and exact raster captures before parking;
ordinary parking never uses the GPU-failure API to discard unsubmitted input.
If activation fails, select the incoming session with the existing unavailable
canvas/Restart Canvas presentation, retaining Save/Discard/Cancel and the ability
to switch away. Do not make successful rendering a prerequisite for closing.

## Memory and disk

Use three storage levels, without serializing the entire session or discarding
undo to make a tab small:

* Active: one GPU renderer per window, existing renderer working-set bounds.
* Inactive RAM: CPU session metadata, history and compressed immutable source/
  raster tiles. Share existing Arc identities between current state and undo.
* Inactive disk: spill compressed TileBlob payloads to a private backing file;
  the same shared tile handles in current state, undo, redo, and recovery now
  reference file offsets. Decode/read only the requested tile, with the existing
  content digest validation. No full-image decompression on activation.

Implement spillable storage behind TileBlob, since both source photos and painted
raster histories already use it. The host creates private temporary storage and
schedules writes on a worker. Write and flush successfully before replacing a
RAM reference. A spill error leaves the original bytes intact. Use immutable spill chunks of at most 8 MiB, reference-counted and removed
after each chunk's last tile disappears, without a free list or compaction. Use
the user cache directory rather than /tmp (which can be tmpfs). Each file is a cache,
not the recovery copy or the user's saved file. Recovery still uses atomic .capy
publication and is never reported as a manual save.

Bound inactive compressed RAM across tabs (64 MiB initial policy), evicting older
inactive payloads first, and serialize spill work. Count unique tile identities
across document and both history directions. Retained non-tile assets, history
metadata, and session overhead are separately admitted against a conservative
window admission watermark (256 MiB initially); recount after edits and reject additional opens with an actionable
error if that unspillable floor is exceeded. Existing documents are never closed
or silently truncated to satisfy admission. Post-edit overage remains usable:
switch/save/close stay available, but further opens are refused. This is an
admission watermark, not a hard bound on all CPU session metadata. Disk-full failures keep drawings in
RAM and block further admission until the user saves/closes drawings or frees
space. Each file read remains governed by existing project/import limits. No
thumbnail for every inactive drawing and no per-tab polling/render timers.

This bounds large inactive pixel payloads independently of tab count. It does
not promise a process RSS ceiling: active renderer/driver memory, allocator
retention, one bounded import, and OS file cache are separate. Tests must measure
owned GPU workers and retained tile bytes, not infer release from widget hiding.

## Validation

The implementation adds shared tests for tab membership/order/close/undo, compact
thresholds, flexible title geometry in all three zones, exact disk-backed history
and saves, float32 samples, failed writes, corrupted reads, and final-owner file
release. The core, engine, UI and GTK unit suites pass; Web also compiles with the
shared spillable tile API.

Native GTK tests run against an isolated Mutter compositor and real GPU:

* `native_canvas_background_during_startup_and_tab_switch`: actual compositor
  captures over a contrasting window, with GPU initialization deliberately paused;
  covers startup, New, returning to an inactive tab, resize, light/dark themes,
  failed rendering, restart, and window destruction. Center pixels must stay
  opaque while waiting, reveal the drawing when ready, and uncover the other
  window only when the drawing window closes. Edge joins and rounded corners are
  checked at scales 1 and 2. Requires `LAYER_NATIVE_CAPTURE_DIR`.
* `native_document_tabs_history_storage_and_close`: production New dialog creates
  a tab; independent undo/redo, dirty state, camera and brush; forced disk spill
  with zero retained inactive tile bytes and joined workers; exact save/reopen;
  separate recovery snapshots; original single-title presentation before/after
  multiple tabs; equal widths and wide/narrow/wide presentation;
  cancelled close/save, background close, and final-window close.
* `native_document_tabs_immediate_stroke_and_undo`: switch with unsubmitted input,
  then switch immediately after Undo while the new raster belongs only to Redo;
  preserve its exact capture, spill, return and redo successfully.
* `native_document_tabs_multiple_recovery_offers`: sequential recovery into tabs,
  independent leases, originals retained until explicit discard.
* `native_document_tabs_failed_renderer_remains_navigable`: select away from a
  failed canvas, restore it, and close it without losing its dirty neighbor.
* `native_document_tabs_disk_failure_keeps_data`: an unwritable cache keeps RAM
  data and Save/Close usable, releases inactive GPU resources, and refuses further
  opens while storage remains unavailable.
* `native_document_tab_input`: Mutter-delivered mouse and touch reorder after
  native slop, Escape cancellation, one-step undo, ordinary click/tap selection,
  Ctrl+Tab, single-title native caption actions, and selector access after removing
  the title component. Light/dark and high-contrast captures cover idle, hover, selected, pressed,
  close-hover, and keyboard focus states against native AdwTabBar styling.
* Updated `native_application_file_launch`: cold/warm multi-file activation,
  per-photo import policy, independent duplicate opens, failed-file continuation,
  explicit New Window and target routing, and closing during import.

Existing New/photo master, proof cancellation, GPU recovery, open cancellation,
and native header customization/window actions are regression checks. Run each
native case in a separate process: GTK requires one initialization thread.
Use `tools/performance/gtk-raster.sh` for lifecycle cases and
`tools/performance/workspace-motion.sh gtk --native-test=NAME --native-storage`
for native input. The disk-failure case uses
`CAPY_TAB_CACHE_DIR=/dev/null/capy-tabs`; normal spill tests use an isolated cache.

Native pen hardware is not available in this environment. The common title-tab
contact path has no device-specific hold gate; physical pen acceptance remains
outstanding. Memory assertions cover owned workers and retained payloads, not a
process RSS cap or driver allocation reporting. A disk error can temporarily
exceed the inactive RAM policy; metadata growth in existing drawings remains
subject to the admission-watermark behavior described above.

## Fresh-context review

Review performed by a fresh-context agent against the draft and current code.
All six findings were accepted:

1. Fixed title metrics/center-zone cap: explicitly expand shared geometry and
   decide compact presentation from final allocation, including side zones.
2. Existing-session growth: make limits admission watermarks, recount on parking,
   preserve editing state and allow switch/save/close during overage.
3. Recovery attribution/concurrency: capture immutable project and owner before
   awaiting; serialize window writes and never read through a successor session.
4. Failed activation/close: allow selecting a session with an unavailable canvas
   so failed rendering cannot strand the drawing or block Discard.
5. Pending engine frame: require a completed canvas boundary before normal
   parking; reserve failure suspension for actual renderer failure.
6. Dead spill ranges/tmpfs: bounded 8 MiB immutable chunks in the user cache
   directory, released by final tile ownership; no compactor or free-list manager.

Added journeys: immediate post-stroke/undo switching, rapid switching, failed
renderer navigation and close, initiating-window close during import/recovery,
removed title/fullscreen/Zen selector access, deterministic close neighbor,
duplicate names with locations, and title customization input arbitration.
No tear-off, MRU policy, inactive thumbnails, or session serialization is needed.

A fresh-context follow-up review of the persistent background found no ownership
or synchronization blockers. Its recovery finding was addressed: successful
updates and Restart Canvas must remove the opaque GTK fallback so they reveal
the drawing again. Compositor tests cover that recovery and the requested
slice-join/corner checks at scales 1 and 2; the viewporter prerequisite is explicit.
