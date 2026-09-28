# Open, import and drop

[Workspace and UI](README.md) · [Color-management journeys](color-management.md)

| User action | Result |
| --- | --- |
| **Open image** | Create a document at the image's oriented pixel dimensions and fit the view. Transparency and the photo's descriptive metadata are kept; the editable `.capy` master is saved separately from the input photo. |
| **Import image / paste another app's image** | Add a layer centered on the canvas, with placement handles active. Pastes of Capy Canvas copies, Paste in Place and Paste Into follow [copy and paste](clipboard.md). The document's metadata does not change. |
| **Drop image on canvas** | The same placement flow, centered at the drop point in document coordinates. |
| **Drop onto Layers** | Above, below and into feedback, using the shared group, lock and clipping rules. |
| **Apply placement** | Store position, scale and rotation; keep the full-resolution source and existing paint in layer-local coordinates. |
| **Original Size (100%)** | Restore native pixel scale, including after Apply and save/reopen, keeping center and rotation. |
| **Import or drop several images** | Prepare every file before inserting, keep the provider's order, fit each image and select the batch with shared handles. Apply is one undo step. |
| **Cancel initial placement** | Remove the whole provisional batch and restore the previous selection, with no artwork undo entry. |

Initial fit uses `min(1, canvas_width / source_width, canvas_height / source_height)`,
so small images keep their native size. Embedded print DPI is metadata and does
not change placement.

Applying a placement does not downsample the layer to the canvas. A 6000×4000
photo fitted into a 2000×1500 canvas keeps its 6000×4000 source at about 33%
layer scale, and Original Size reveals the retained detail. Canvas edges hide
content without deleting it. Display previews may be smaller; a flattened export
samples onto its output grid. The source keeps decoded samples, not the original
compressed bytes.

A batch is atomic: one layer per image, and nothing is inserted if any file
fails or the user cancels. The error names the failed file. A batch that mixes
projects and images is rejected. Hosts supply local files; providers without a
local path are rejected.

Shared Rust owns destination validation, placement and history; hosts own
external data acquisition and pointer capture. External drags arrive already
recognized and add no hold. Readers are chosen by the file's signature, not its
extension: `.capy` masters open as projects, and photos go through the decoders
in [`crates/layer-color/src/photo`](../../crates/layer-color/src/photo). The
[color-management journeys](color-management.md) cover profiles, depth and HDR.

An external file launch can arrive before a saved workspace finishes loading.
The shared Open command stays disabled while that workspace is read only. Hosts
keep the file request until the workspace is ready, then submit it on the
session owner; a failed submission must release the queued request so a later
launch can open normally.
