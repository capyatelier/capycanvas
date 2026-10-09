# Shared shader readiness

[Editor internals](README.md)

Every host uses the same demand-driven dependency tracking and input admission
policy in `layer-render-wgpu`.

Compiler failures retain the first reported cause, including a native worker's
panic payload. Dry-material labels identify the target, shader entry, operation
and contact flags, so a driver rejection can be traced to its actual recipe.
Recipes for one dry target share its two coverage-layout pipeline layouts.

## Order

Startup prepares paper/presentation, the actual document, then the selected
brush and physical eraser. Native document dependencies include the display
hierarchy's reduction kernels. Transform, warp, region tools and other brushes
prepare on first use. Unused filter programs retain recipes. Shared pipeline
recipes deduplicate work and prepared handles survive tool changes; a first
selection can wait for compilation, while later selections reuse those handles.
Startup cannot fail because of shaders needed only by an unused brush.
Native paint requires tile decoding before document readiness, including drawings
without imported images: restoration and Undo consume that same decoder.

The native compiler thread and browser task runner share `shader_admission.rs`.
Required canvas/brush dependencies and explicit package validation retain
priority and progress while input arrives. Visible optional previews wait for
200 ms without input; pipeline-cache finalization waits for one second and no
continuous canvas work. Required compilation renews the quiet interval on
completion. Strokes, held gestures, queued input, pending edits and settings
keep optional work parked. Native workers sleep on their existing condition
variable; browsers use a host timer. An in-flight driver call cannot be
interrupted; admission happens between jobs.

The browser admits one optional pipeline per task; required pipeline batches
remain bounded to four. Requested brush masks publish when their generation jobs
finish. Brush readiness includes their upload. Startup completion describes
current dependencies and cache finalization, not the entire tool catalog.
Retouch shader handles survive tool changes; unused source pixels and healing
buffers are still released.

Application controls such as Settings remain usable while required shaders and
raster work prepare. Shared command classification keeps these controls outside
the ordered drawing-edit queue; file commands still wait for preceding edits.

The initial backdrop frame includes only constant fills. Raster reconciliation
and pending bake dependencies follow that scene scope, so excluded paint stays
in its immutable backing until the full document and its decoder are ready.

Raster backing admits frames by pending captures and their byte budget. The
native worker creates its staging spares before processing queued captures;
spare preparation does not make an empty queue full or block a source-backed
drawing's first frame.

Readiness continues after initial startup. Shared `UiSession` wakes the host
when a UI-only brush change needs preparation. Native readiness snapshots use
the current dependencies, so a previous tool's ready flag cannot authorize the
new tool. A contact begun before readiness is held whole and delivered once its
shaders are ready; every host, the browser included, admits paint samples through
`layer_engine::DeferredContacts`, which discards only a contact held longer than
5 s or beyond 4096 samples. Hosts report whether samples are held with
`UiSession::set_input_held`; until they are delivered or the contact is cancelled,
document snapshots such as Save wait, so a save never misses a stroke or drag
already drawn. Other commands are not held back.
`ShaderDocument` memoizes the dependency-relevant document state: ordinary
raster/parameter edits do not invalidate readiness or trigger effect preparation.
Effect-chain inspection now borrows layers; only compiler jobs clone their
owned inputs.

Android and GTK no longer reload their already embedded filter package during
startup. Android also stops packaging that redundant asset copy. GTK still
accepts explicit `CAPY_FILTERS_DIR` / `CAPY_FILTERS_MODE` overrides; runtime
package installation and atomic validation remain available. Identical library
refreshes are no-ops on all three hosts. Existing native pipeline caches remain;
there is no additional platform cache or compiler worker. A renderer created
while another still holds the startup cache, such as a restored or opened
document's, starts from the saved cache read-only; only the holder cleans or
replaces it. The native cache closes after required startup work; later first-use
variants are not persisted in that cache.
