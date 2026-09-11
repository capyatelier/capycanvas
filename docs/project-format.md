# Editable projects

The shared `layer-core::Project` codec is the foundation for native Save/Open.
It is not yet connected to GTK file dialogs. Save retains editable content;
Export produces a flattened image. This distinction follows familiar creative
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

GTK file dialogs, save cancellation and unsaved-work behavior are not covered by
these codec tests. Browser/Android/Apple project UI and GPU replay are not yet
device-tested. The independent historical filter-reference discrepancy remains
open as documented in [runtime-filters.md](runtime-filters.md).
