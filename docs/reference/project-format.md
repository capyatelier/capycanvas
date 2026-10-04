# Editable artwork and saving

[Technical documentation](../README.md)

`.capy` saves typed authored artwork in the lossless
[Capy package](capy-package.md). `ArtworkCapture` and `PreparedPackage` are the
shared capture and writer interfaces. Every host uses the same record adapters;
there is no direct `Document` serialization or separate Web artwork schema.
Photo export is a separate operation that renders a captured output.

The shared writer compresses manifest metadata with lossless DEFLATE when smaller.
Raster/channel resources retain their independently compressed bytes and direct
pack offsets. Encoding borrows authored/resource records instead of duplicating
the manifest JSON tree. Metadata compression and decoding run in the existing
file worker with bounded lengths and cancellation between input chunks.

## Authored and working state

An artwork contains compositions, ordered stacks, placed occurrences, paint and
coverage sources, effect applications and definitions, saved selections, guides
and outputs. Occurrences own placement, opacity, masks, clipping and names.
Sources own original images, sparse raster revisions and material state. See the
[authored model](authored-model.md) for field ownership and editable validation.
Hidden and unplaced content remains authored work and is validated and saved.

Working selection, saved-selection overlay visibility/color/opacity, editing targets, camera,
preferences, GPU handles, active
contacts and undo history stay outside the portable manifest. Saved selection
objects and initial mask coverage are authored data. Built-in effects save stable IDs, parameter-data versions and every keyed
value, and resolve the current app implementation on open. Custom effects are
development-only and retain embedded code in private sessions and worker transfers.
Portable `.capy` saves refuse custom effects. Editor presentation and render
optimizations stay in runtime packages; the [package contract](capy-package.md)
defines the authored data. Output contexts preserve captured integrated effect phases.

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
ICC, photo metadata and LUT resources use lossless LZ4 when smaller.

Unchanged immutable resources retain their IDs, encoding and compressed bytes
across snapshots and saves. GPU caches follow the
[resource identity contract](authored-model.md#source-and-material-resources);
ordinary installation does not decode pixels to compute a hash.
The package reader checks transport integrity, bounded decoding and semantic
validity before editable adoption. Unknown required semantics remain preserved
with the original package; they never become empty artwork. Preserved and
recovered views can copy the original or export a verified preview as exact PNG
bytes to a different destination. Neither action adopts an editable document.

The writer streams indexed resources through bounded I/O. Resource members are
STORED because heavy data already carries its own lossless encoding; only the
manifest may use ZIP DEFLATE. The
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
saves are valid. Normal saves attempt a bounded sRGB preview on the file worker
using the same checkpoint and captured output context. Rendering, device or
preview encoding failures omit the preview without discarding editable content.

Hosts perform picker and storage operations. Local atomic writers publish through
a temporary file and replacement; provider transports retain their actual
platform guarantees. Non-seekable input is spooled into private bounded storage.
Web workers use the shared resource transfer and stream to private browser
storage before publication, preserving the original Blob for unsupported files.

Recovery publication is separate from manual-save acknowledgement.
[`Editor::capture_session`](../../crates/layer-core/src/package/session.rs)
freezes working state, bounded Undo/Redo and edit/stroke identities alongside the
artwork capture. Workers encode private complete checkpoint metadata with shared
record versions and an immutable resource inventory. Sparse raster-index chunks
and original-image descriptors are shared across changed paint records; small
strokes do not repeat the complete canvas/photo index per Undo entry.
Ordinary `.capy` saves
continue to contain authored artwork only. The private reader validates resource
bytes, working targets, history transitions and budgets before returning an
editor. Invalid or unsupported state remains a restoration failure, preserving
the stored copy for diagnosis rather than substituting empty artwork or dropping
history.

Session metadata holds camera, drawing names and manual-save state separately.
Private sessions retain all ancillary records, including non-copy-safe data and
subjects temporarily absent after an edit. Native private storage publishes
resources before metadata and retains complete generations and live readers
through cleanup. Trusted worker session transfer is separate from the disk codec:
its verification receipts never bypass validation of persisted bytes. Exhaustive
field and edit-variant boundaries plus the shared restart/Undo/Redo fixture force
new editor features to classify their session persistence behavior.

## Photo metadata

Opening a photo retains admitted Exif, XMP and IPTC blocks as immutable resources.
Orientation and print density are applied during import; stale dimensions and
thumbnails are excluded from the descriptive Exif block. The three blocks share
a 64 MiB import allowance. Imports, pastes and new drawings do not replace the
current drawing's descriptive metadata. The package preserves the exact admitted
bytes, while photo export applies its separate metadata delivery policy.
