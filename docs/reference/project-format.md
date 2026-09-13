# Editable raster projects

[Technical documentation](../README.md)

`layer-core::Project` stores editable `.capy` drawings. GTK uses the raster
container described here. Other hosts have not yet been qualified for this
replacement. Export remains a flattened, straight sRGB RGBA8 PNG.

## Pixels and revisions

The document mode is `Srgb8V1`: sRGB primaries, D65, SDR and eight-bit channels.
Paint tiles store `sRGB_encode(linear_RGB × alpha)` with unencoded coverage in
alpha. Hardware decodes RGB before sampling and encodes after blending; shader
math remains Float32. This differs from multiplying already encoded RGB by alpha.
Source images are straight sRGB RGBA8. Masks and persistent wetness are linear R8.
Effect processing domains are independent of the storage transfer function.

Each layer or mask owns an immutable sparse raster revision. Tile size is 256².
Changed physical pages are captured at a completed contact or raster-operation
boundary. Unchanged tile backing is shared across revisions, history and saves.
Undo/redo restores changed pages directly. Fills, gradients, figures, Apply mask
and transforms are transient submission commands; their recipes and historical
brush contacts are not stored. Embedded live effects retain their exact WGSL,
parameters and metadata and remain editable after reopening.

The project also retains dimensions, layer/group order and properties, source
images, masks, selection, rulers, references, allocators and the edit target.
Per-contact reservoirs, prediction, accumulation coverage, UI preferences, GPU
handles, source filenames and undo history are excluded. Watercolor wetness and
live edge settings are committed because they affect composition and later paint.

## Container and validation

The header is the twelve bytes `CAPYRASTER\x01\0`, followed by a little-endian
u64 metadata length, a 32-byte SHA-256 metadata digest, JSON metadata and payload.
The metadata indexes raster targets, tile coordinates/planes, unique compressed
blobs and source assets. Payload offsets are relative to the payload start.
The manifest explicitly declares `tile_codec: "zstd"`. Each tile is an independent
lossless Zstandard frame (fast level -20); its content digest covers its explicit
pixel descriptor and exact decoded bytes. Sources use indexed packed bytes with
their own digest. There are no paths to extract.

Identical tile blobs are deduplicated in a save. Repeated saves reuse immutable
compressed backing without readback, conversion or recompression. The writer
streams payload after indexing; it does not build another full archive in RAM.
Readers reject malformed/unsupported headers, descriptors, references, duplicate
keys, noncanonical offsets, truncated or trailing data, integrity failures and
unused blobs before adopting a candidate. The former `CAPYPROJECT` codec is gone;
old files produce an unsupported-version error. There is no migration reader.

Default decoded limits are 64 MiB metadata, 512 MiB sources, 1 GiB raster data,
16384 tile instances, 32768 pixels per axis and 4096 layers. Repeated references
to one compressed blob still count as separate physical tile instances. Device
limits and shader/resource preparation remain separate checks during opening.

## Submission, recovery and durability

These are distinct boundaries:

- A frame submission orders drawing and changed-tile copies on the GPU queue.
- A raster revision becomes host-backed when its readbacks and lossless
  compression finish. Failed/abandoned backing stays an error for every owner.
- A manual save becomes durable only after successful atomic publication by the
  host. Capture or autosave never acknowledges a manual save checkpoint.

GTK transfers immutable roots to its GPU owner. Readback mapping/compression runs
on a separate worker, with 256 MiB staging per frame and a 512 MiB pending-staging
ceiling. At most 16 small capture jobs can share that budget; admission always
reserves room for the largest next frame. A worker-prepared 64 MiB spare pool
reuses unmapped buffers of at most 16 MiB.
Compression copies at most four chunks into cached CPU memory (64 MiB scratch)
and runs at most four compression jobs per capture, including within smaller
chunks. Capture pressure defers pen-up/correction/operation boundaries; ordinary
move frames continue. The separate native frame mailbox stays bounded to two.
History retains at most 256 edits within a conservative 512 MiB backing/metadata budget,
excluding current document ownership. No precision is reduced to fit a budget.

Contact reconstruction is limited to the active contact and the most recently
completed contact's two-second correction window. Starting a new contact,
editing document metadata, or navigating undo/redo closes the late-correction
window. Accepted corrections replace the current raster root without adding an
undo step; earlier save snapshots stay immutable. Live input is limited to
131072 points and 32 predictions. Exceeding the contact budget cancels the
uncommitted contact with an error.

## GTK file workflow

New/Open create another window, preserving the current drawing if loading or GPU
startup fails. Save/Save As snapshots the last committed boundary even while a
stroke is active; that active stroke keeps the document modified. Workers await
pending backing and perform validation and file I/O. Cancelled/failed saves do
not acknowledge checkpoints. Undoing to a successfully saved checkpoint clears
modified state unless active input or recovered unsaved work remains.

Close waits for interactions to finish and rechecks the saved checkpoint after
an asynchronous save. Its Save/Discard/Cancel decisions remain shared Rust policy.
Local saves write a sibling temporary file, sync its contents, rename it over the
destination and sync the containing directory. Failure before replacement retains
the previous destination; a publication/sync failure never reports a clean save.

GTK autosave attempts a private checkpoint every 15 seconds when modified, with
one file worker per window. It uses the same immutable revision model and atomic
writer. A failed capture/write retains the previous copy. Startup offers copies
from terminated processes for recovery; recovery opens a new, modified document
requiring an explicit Save. Active GPU-only samples are not promised recoverable.
Native failure and performance qualification is recorded in the
[GTK validation report](../history/color-management-gtk-m1-validation.md).
