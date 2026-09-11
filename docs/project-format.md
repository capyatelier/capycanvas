# Editable projects

The shared `layer-core::Project` codec backs GTK's `.capy` Save/Open workflow.
Save retains editable content; Export produces a flattened PNG image. This distinction follows familiar creative
applications such as [Krita](https://docs.krita.org/en/reference_manual/main_menu/file_menu.html).

## Stored content

- Document dimensions, layer/group order, properties, clipping and edit target.
- Source images and custom brush masks as packed immutable pixel assets.
- Brush snapshots and samples, selection coverage, rulers and references.
- Live masks and immutable histories retained by Apply mask, fills, gradients,
  figures and transforms, including their original operation order.
- Each effect's exact WGSL program, metadata and values. Reopening must not
  silently substitute a subsequently changed filter catalog definition.

The codec prunes unreachable strokes and unused image assets. It does not save
the undo stack, GPU handles, UI preferences, source filenames or host information.
Known versioned built-in brush assets may be resolved from the app; custom
textures and imported images must be embedded. Source pixels are retained for
replay, not rendered on the CPU. Saving needs no canvas readback.

## Container and validation

Version one is `CAPYPROJECT` followed by byte `1`, then a gzip stream containing
an unsigned little-endian 64-bit JSON length, the JSON manifest, and packed asset
blocks in manifest order. Image bytes are not JSON arrays or base64, and there
are no archive paths to extract. The gzip checksum/trailer and end of stream
must validate before loading succeeds.

The manifest serializes the actual document types; there is no parallel document
schema to maintain. Incompatible model changes require a container-version
change or an explicit migration. This initial format makes no compatibility
promise for unreleased development builds.

Default decoded limits: 64 MiB metadata, 512 MiB image assets, 32768-pixel image
dimensions, 4096 layers, 500000 strokes and eight million samples. Hosts can
choose stricter load limits. GPU device limits and shader compilation are a
separate pre-publication gate, not a guarantee provided by parsing the file.

## GTK document workflow

The File menu supplies New, Open, Save, Save As, Export PNG and Close. New/Open
create a separate document window, preserving the current drawing even if the
incoming project is corrupt or its GPU initialization fails. New offers width
and height in pixels; the initial 2048×1536 canvas can range from 1 to 8192 pixels
on either axis. File dialogs start in the current drawing's folder when known.

Shared Rust owns request IDs, single-flight operations, filenames, dirty state
and close authorization. The editor exposes an undo-state checkpoint rather
than treating its monotonically increasing revision as unsaved work. Undoing to
the saved state clears the indicator; branching history, references and rulers
change it. Target/selection navigation, camera, workspace and preferences do not.

Save captures immutable document/source state; pruning, validation, compression
and disk I/O run on a worker. A sibling temporary file is flushed and synced
before atomic replacement. Errors leave the existing file intact before that
replacement; a subsequent directory-sync failure is reported rather than claiming
durability. Temporary files are cleaned up. Native transport currently accepts
local filesystem destinations, not arbitrary remote GIO providers.

Edits may continue while writing. Only the captured checkpoint becomes saved;
later work remains modified. Closing offers Save, Discard Changes or Cancel, and
waits for an accepted save. If another stroke starts during the write, the close
decision waits for pen-up and checks again. Cancellation/failure never marks the
document clean. Export neither renames the project nor marks it saved.

GTK uses the asynchronous [FileDialog API](https://docs.gtk.org/gtk4/class.FileDialog.html)
and follows [GNOME's confirmation-dialog guidance](https://developer.gnome.org/hig/patterns/feedback/dialogs.html).
No native dialog loop or disk work is added to the input path. Export queues the
renderer’s existing explicit whole-document readback after pending image edits;
it is a cold operation, not a live drawing/presentation mechanism.

Startup catalog refresh validates new definitions without migrating an opened
document's embedded programs. Explicit runtime replacement retains its existing
migration behavior. Namespace conflicts still reject a candidate library.
Validation covers asset formats, references/ownership, history ordering,
allocators, mask/group identity and depth, numeric constraints, stroke bounds,
selection coverage and effect definitions. Metadata output is bounded while
encoding; malformed lengths never trigger an upfront allocation of that size.

Host integration must perform validation/compression/file I/O off the input
thread, use an atomic replacement for saves, and retain the existing document
until opening has passed GPU resource and shader validation. Cancellation or
failure must not clear the unsaved state. Those file workflows remain to build.

## Stateful brush replay

Watercolor transports pigment after a material update. The old replay combined
an entire stroke's updates into one, reducing bleed after replay even though the
pointer samples were unchanged. Stroke history now retains exclusive sample
ends for those updates. Render batches carry an update identity, so live drawing,
active recovery and reopened documents preserve the same transport boundaries.

This retains the current live algorithm rather than adding transport passes or
changing the artist's existing appearance. The extra history is one `u32` per
update (about 480 bytes per second at 120 updates/s, excluding vector/Arc
overhead); there is no additional image channel or GPU allocation. Idle and
prediction-only frames do not add boundaries. Other brush families need no
watercolor update history. Material-update lookup scans only its contiguous
batch group, not unrelated strokes in a full document replay.

## Evidence

- Core round trips cover all 40 filters and advanced brush/mask histories.
- Rejection tests cover corrupt/truncated streams, budgets, missing/duplicate or
  wrongly typed assets, invalid selections, IDs, history order and group depth.
- Engine tests compare live, active and committed replay with feedback on/off
  and coalesced input, including idle frames.
- `cargo test -p layer-render-wgpu --test project` compares incremental live
  drawing against save/reopen on fresh GPU renderers. Imported transparency,
  wet brushes, applied/live masks, groups, gradients, figures, transforms,
  clipped multipass filters, curves/gradient lookup and animation match exactly
  on the Vulkan test workstation, including subsequent wet painting.
- Optional `CAPY_PROJECT_CAPTURES` writes generated test PNGs for inspection;
  use a directory under ignored `artifacts/`, never commit those captures.

The native `workspace::tests::native_document_files` test exercises GTK document
dialogs, cancellation, malformed files, Save As, PNG export, fresh-window GPU
reopening and close-after-save. It uses the toolkit's file-chooser fallback on
an isolated Wayland display; desktop portal-provider interaction is not automated.
Shared tests cover failed/overlapping writes, undo checkpoints and edits during
save. Browser/Android/Apple project UI is a separate platform-validation gate.
The independent historical filter-reference discrepancy remains
open as documented in [runtime-filters.md](runtime-filters.md).
