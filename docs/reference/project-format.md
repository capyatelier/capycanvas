# Editable raster projects

[Technical documentation](../README.md)

`layer-core::Project` stores editable `.capy` drawings. GTK uses the raster
container described here. Other host integration and qualification remain
outstanding. Profiled PNG/JPEG/TIFF exports transform a copy of the composition;
they are separate from the editable project.

## Pixels and revisions

`DocumentColor` records one of sRGB, Display P3, Adobe RGB or ProPhoto RGB and
independent integer8/integer16 SDR depth. The GTK working renderer uses linear
Float32 math; editing and effect processing domains are independent of stored
precision. No FP16 working buffer is an implicit integer16 boundary.

GTK paint stores straight profile-encoded RGB and linear coverage at the declared
integer depth in all four spaces, including sRGB8. Working composition associates
RGB in linear light. Mask/wetness backing uses linear integer coverage at the
document depth. Descriptors identify each stored plane.

Hosts awaiting native integration still produce an explicit sRGB8 attachment
layout: `sRGB_encode(linear_RGB × alpha)` plus linear alpha. Shared storage accepts
that descriptor only for sRGB8 color planes; it is no longer inferred from the
document mode. The normalized-attachment renderer rejects native straight tiles
instead of uploading them with the wrong alpha semantics. Integrating other hosts
remains separate work requiring approval.

A tiled image can be an **Original** with its independent RGB/gray/CMYK samples,
integer depth, profile bytes or explicit assumption, or **Rasterized** RGBA pixels
in the document's builtin profile and depth. Rasterization converts the original
image without cropping its extent or replacing painted overrides, masks or layer
placement. The rasterized image is no longer offered as an original for profile
repair. Its original survives only through retained undo history; history is not
saved in the project. Rasterized-image interpretation must match the document.

GTK document color changes publish completed backing, mode and history atomically
after preparing the matching GPU configuration. Assign changes the interpretation
of committed RGB numbers; Convert transforms editable backing, including full
rasterized image extents. Retained originals preserve their independent samples
and profiles. Bit depth changes rescale scalar coverage without gamma or dithering;
optional 8-bit dithering applies only to RGB. Undo/Redo restores exact backing and
the previous mode. A flattened conversion creates a separate one-layer drawing;
the layered original stays open. These operations do not add serialized history.

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

The header is the twelve bytes `CAPYRASTER\x04\0`, followed by a little-endian
u64 metadata length, a 32-byte SHA-256 metadata digest, JSON metadata and payload.
The metadata indexes raster targets, tile coordinates/planes, unique compressed
blobs, image roles/interpretations and source assets. Payload offsets are relative to the payload start.
The manifest explicitly declares `tile_codec: "zstd"`. Each tile is an independent
lossless Zstandard frame (fast level -20); its content digest covers its explicit
pixel descriptor and exact decoded bytes. Immutable image tiles use level 1;
integer16 byte-plane shuffling is reversible and digests cover original bytes.
Image profiles are binary payloads with independent hashes; builtins are explicit
identifiers. Packed brush/source assets also have their own digests. There are no
paths to extract.

Identical tile blobs are deduplicated in a save. Repeated saves reuse immutable
compressed backing without readback, conversion or recompression. The writer
streams payload after indexing; it does not build another full archive in RAM.
Readers reject malformed/unsupported headers, descriptors, references, duplicate
keys, noncanonical offsets, truncated or trailing data, integrity failures and
unused blobs before adopting a candidate. The former `CAPYPROJECT` codec is gone;
old files, including earlier raster-container versions, produce an unsupported-version
error. There is no migration reader. Version 4 adds the required image role and
validates rasterized-image interpretation before reading payloads.

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
Changes to retained sources are admitted in both Undo and Redo directions before
publication. An oversized source edit fails without changing the document or
existing history. Import, source repair and rasterization also validate aggregate
retained-source ownership before publishing; provisional layer IDs are allocated
only after validation. Pending raster transactions still use the existing capture
reservations; combined source/raster/history accounting remains under qualification.

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
