# Editable projects

[Technical documentation](../README.md)

The shared `layer-core::Project` codec stores editable `.capy` drawings. Saving
retains document content; exporting produces a flattened PNG. The container and
validation rules below are shared. The detailed native workflow in this reference
describes GTK; see [platform integration](../platforms/README.md) for other hosts.

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

`CanvasRenderer::source_asset` exposes shared immutable storage for uploaded
images and masks. `Project::snapshot_with` requests only reachable dependencies;
when the renderer supplies a built-in mask, its exact bytes are embedded too.
The wgpu backend retains packed sRGB imports (four bytes per pixel) and shares
its existing mask source storage. Row padding is excluded, and releasing an
asset releases the renderer's source reference. Archive snapshots keep their own
Arc references. Owned uploads share that same allocation through GTK's worker
queue and the wgpu source cache, rather than retaining a second pixel copy.
Borrowed host rows are packed once at import. Import/save memory peaks and storage scheduling still need
hardware measurement; no full-resolution generated canvas copy is retained.

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

Host integration must perform validation, compression and file I/O off the input
thread and retain the existing document until opening has passed GPU resource
and shader validation. GTK/local filesystem saves use atomic replacement; other
transports follow the guarantees of their storage API. Cancellation or failure
must not clear the unsaved state. GTK implements these policies above. Other hosts use their own transports and
validation; see the [platform guide](../platforms/README.md) for coverage.

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

## Verification

The shared project tests cover validation and replay after saving and reopening.
The [initial validation record](../history/project-validation.md) preserves the
GTK and codec checkpoint. Current native workflow coverage is documented in
[platform integration](../platforms/README.md) and the port acceptance records.
