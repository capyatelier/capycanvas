# Editable artwork and saving

[Technical documentation](../README.md)

`.capy` saves typed authored artwork in the lossless
[Capy package](capy-package.md). `ArtworkCapture` and `PreparedPackage` are the
shared capture and writer interfaces. Every host uses the same record adapters;
there is no direct `Document` serialization or separate Web artwork schema.
Photo export is a separate operation that renders a captured output.

## Authored and working state

An artwork contains compositions, ordered stacks, placed occurrences, paint and
coverage sources, effect applications and definitions, saved selections, guides
and outputs. Occurrences own placement, opacity, masks, clipping and names.
Sources own original images, sparse raster revisions and material state. See the
[authored model](authored-model.md) for field ownership and editable validation.
Hidden and unplaced content remains authored work and is validated and saved.

Working selection, saved-selection overlay visibility, editing targets, camera,
preferences, GPU handles, active
contacts and undo history stay outside the portable manifest. Saved selection
objects and initial mask coverage are authored data. Embedded effect definitions
retain their exact program and keyed values independently of the installed
catalog. Output contexts preserve captured integrated effect phases.

An original image retains its independent extent, channels, depth, profile and
resolution. Rasterization replaces that original with document-space samples
without discarding its extent, painted overrides or placement. Undo can retain
the original owner. Assign and Convert publish prepared backing, interpretation
and history together; failed admission leaves the current document unchanged.

Canvas size is a window over source-local domains. Cropping does not discard
retained pixels outside the canvas. Transforms retain geometry until explicitly
applied to pixels; masks retain their own coverage sources and placement.
Watercolor material planes and edge settings survive saving because they affect
composition and future painting.

## Lossless storage

Raster tiles use bounded raw LZ4 blocks. Multibyte channels use reversible byte
shuffling before compression. Integer samples, finite float bit patterns, hidden
RGB and scalar coverage remain exact; saving does not quantize image or channel
data. Selection coverage uses binary compressed chunks instead of JSON arrays.
ICC, photo metadata, shader code and LUT resources use lossless LZ4 when smaller.

Unchanged immutable resources retain their IDs, encoding and compressed bytes
across snapshots and saves. GPU caches follow the
[resource identity contract](authored-model.md#source-and-material-resources);
ordinary installation does not decode pixels to compute a hash.
The package reader checks transport integrity, bounded decoding and semantic
validity before editable adoption. Unknown required semantics remain preserved
with the original package; they never become empty artwork. Preserved and
recovered views can copy the original or export a verified preview as exact PNG
bytes to a different destination. Neither action adopts an editable document.

The writer streams indexed resources through bounded I/O. ZIP members are STORED
because heavy resources already carry their own lossless encoding. The
[package contract](capy-package.md) specifies ZIP64, strict references, byte
layouts, limits, ancillary preservation and unsupported-content outcomes.

## Capture, publication and recovery

The ordered editor owner captures immutable artwork roots, its checkpoint and
working generation. The renderer supplies the exact evaluation context. Pending
raster publications retain their owners while workers await backing; failed
backing remains a failure for all dependent snapshots. Accepted jobs, undo/redo
and parked tabs retain resources independently of the active renderer.

A manual save acknowledges only the captured checkpoint after successful host
publication. Painting can continue during writing, and newer work stays modified.
Undo/Redo compares exact checkpoint identity. Cancellation, failure and stale
completion cannot acknowledge a different document or newer edits. Source-only
saves are valid; an optional preview must match both the checkpoint and captured
output context and is never required to preserve editable content.

Hosts perform picker and storage operations. Local atomic writers publish through
a temporary file and replacement; provider transports retain their actual
platform guarantees. Non-seekable input is spooled into private bounded storage.
Web workers use the shared resource transfer and stream to private browser
storage before publication, preserving the original Blob for unsupported files.

Recovery publication is separate from manual-save acknowledgement. Current
recovery writes ordinary artwork packages and retains its existing policy.
Resource enumeration does not require archive assembly, so later session and
bounded-history capture can use the same owners and generations. The
[automatic recovery plan](../development/autorecovery.md) remains separate work.

## Photo metadata

Opening a photo retains admitted Exif, XMP and IPTC blocks as immutable resources.
Orientation and print density are applied during import; stale dimensions and
thumbnails are excluded from the descriptive Exif block. The three blocks share
a 64 MiB import allowance. Imports, pastes and new drawings do not replace the
current drawing's descriptive metadata. The package preserves the exact admitted
bytes, while photo export applies its separate metadata delivery policy.
