# Web and Android photo placement handoff

## Prompt for the next agent

Implement the user-approved GTK photo Open/Import/Paste/Drop workflow on Web and
Android. Start from current `origin/main`: `8de027b7` adds the implementation and
`522db8dc` integrates newer upstream renderer changes. Reuse shared Rust placement,
source storage, history and rendering. Finish the host integrations, test real
browser/Android input and realistically large photos, then provide runnable builds,
measured limitations and a concise result for user review. Stay focused on this
workflow; general smudge optimization and other platforms are separate work.

## Behavior to preserve

- **Open:** create a document at the oriented source dimensions and fit the view.
  **Import/Paste:** add layers to the current document, centered and initially
  fitted using `min(1, canvas_width/source_width, canvas_height/source_height)`.
- **External canvas Drop:** add layers at the captured document-space drop point
  with placement handles active. Layers-panel drops use shared above/below/into
  validation, including group, lock and clipping rules.
- **Apply** stores the affine transform; it does not downsample the original.
  Retain source depth/profile/alpha and layer-local paint outside the canvas.
  **Original Size (100%)** restores native pixel scale, including after save/reopen.
- Prepare a multi-file batch before insertion; preserve order, use shared handles
  and one Apply undo step. Cancel removes the provisional batch without an artwork
  undo entry. A failed or stale request must not partially insert or retarget it.
- Keep Apply/Cancel accessible with panels hidden and on small screens. Follow
  [drag conventions](../ui/drag-and-reorder.md) for internal draggable targets;
  receiving an external file drop introduces no extra hold gesture.

## Starting points and remaining integration

| Area | Read / reuse | Work to do |
| --- | --- | --- |
| Shared policy | [art_layers.rs](../../crates/layer-ui/src/art_layers.rs), [placement transaction](../../crates/layer-ui/src/operation/placement.rs) | Call `place_layer_sources(sources, center, destination)` for interactive insertion. `import_layer_source` is immediate insertion. Preserve epoch/revision/target and GPU-generation checks. |
| GTK reference | [file preparation](../../apps/layer-linux/src/files/place.rs), [external drops](../../apps/layer-linux/src/files/drop.rs) | Reuse behavior and shared validation; keep transport and pointer capture in each host. |
| Web | [documents.js](../../apps/layer-web/documents.js), [documents.rs](../../apps/layer-web/src/documents.rs), [raster worker](../../apps/layer-web/src/raster_worker.rs) | Adoption still calls direct import; picker is single-file and lists JPEG/PNG/TIFF. Add batch placement, browser external-drop routing, capability-driven filters and visible transaction controls. Preserve worker decoding, cancellation and device-loss handling. |
| Android | [Documents.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt), [native documents](../../apps/layer-android/native/src/documents.rs), [CanvasSurfaceView](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasSurfaceView.kt), [Layers](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt) | Adoption still calls direct import. Handle multi-selection/ClipData and external drag URI permissions, integrate placement controls, and preserve provider access, lifecycle cancellation and surface recreation. |

## Format and rendering constraints

- [Shared decoding](../../crates/layer-color/src/photo.rs) supplies JPEG, PNG, TIFF,
  BMP/DIB, GIF and WebP. Derive filters/MIME preferences from actual decoder
  capabilities. Retain original encoded input through decoding; avoid routing
  originals through an 8-bit browser canvas or Android Bitmap conversion.
- HEIC/AVIF currently require the optional **Linux-only** native codec bridge.
  Equivalent Web/Android support needs target-specific work; do not advertise
  parity without a working decoder. Make unsupported variants and any remaining
  parity gaps explicit. Preserve first-frame/primary-image naming disclosures.
- Placement/storage are shared already, including version 5 archives when needed.
  Exact export bypasses disposable display previews. Keep source-local geometry
  in painting, masks, thumbnails and watercolor composition.
- [Placement previews](../../crates/layer-render-wgpu/src/scene/placement/mips.rs)
  previously hit a 128 MiB cap: a clipped 61 MP photo jumped to ~108 ms/frame at
  1.2× fit. The fix uses admitted GPU memory and retains finer Float32 previews.
  Web already installs a capacity-based allowance in `raster_worker::install`;
  Android uses native memory admission. Qualify both on their own devices,
  including memory pressure; copying desktop memory assumptions is insufficient.

## Acceptance and useful commands

Test actual 24 MP and 61 MP photos on a 2000×1500 canvas: Open; Import/Paste/Drop;
batch Apply/Cancel; clipped translation **and drawing at 1.1×, 1.2× and 2× fit**;
Undo/Redo; save/reopen; Original Size; full-photo thumbnails; two large layers.
Verify source samples and off-canvas paint, not just screenshots. Test malformed
second files, stale destinations, cancellation and recovery/lifecycle boundaries.
Measure loading separately from warm interaction, reporting memory and p50/p95/max
on named hardware. Desktop GTK timings do not establish tablet performance.

Start with [Web development](web.md), [Android development](android.md),
[shared source tests](../../crates/layer-ui/src/session_source_tests.rs),
[GPU placement tests](../../crates/layer-render-wgpu/src/placement_tests.rs) and
[GTK native journey](../../apps/layer-linux/src/photo_workflow_tests.rs).

```sh
cargo test --locked -p layer-ui --lib placement
bash apps/layer-web/run.sh
# In another terminal, after the build/server is ready:
node apps/layer-web/test.mjs --headless --photo-paint
CAPY_ANDROID_SERIAL=DEVICE_SERIAL bash apps/layer-android/run.sh
```

Extend the host tests beyond these existing smoke checks. Use a real Android
tablet and tablet Chrome for performance; preserve user data and isolate test
documents. [Approved GTK results](image-placement-gtk-review.md) record the
baseline and limits. Raw `artifacts/` and `/tmp` evidence is local-only; reproduce
it with the checked-in tests rather than assuming those files exist in a fresh clone.
