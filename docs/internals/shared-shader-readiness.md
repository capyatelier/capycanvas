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
brush and physical eraser. Only after that brush is ready does it queue idle
preparation of the built-in brushes for the document's blend space. Common dry
brushes lead, followed by Smudge, Wet Round, Liquify and retouching, then
transform and warp shaders, and finally the remaining specialty brushes. Shared
pipeline recipes deduplicate the work. Visible previews and requested document
dependencies take priority over brush warmup. Region tools and unused filter
programs retain recipes.
Custom brush settings still request any variants that have not been prepared.

The existing native compiler thread and browser task runner share
`shader_admission.rs`: visible optional previews wait for 200 ms without input;
speculative brush and transform preparation waits for one second. Both require
an idle session. Required compilation renews the quiet interval on completion,
giving the newly ready tool time to receive input before another speculative
job starts. Optional completions do not renew it. Short pauses between strokes
do not restart the catalogue.
Strokes, held gestures, queued input, pending document edits and
settings hold that gate closed. Native workers sleep on their existing
condition variable; browsers use a host timer. Window input observers forward
activity without changing gesture routing or drag conventions. Required
canvas/brush dependencies and explicit package validation retain priority and
can progress while input is arriving. An in-flight driver call cannot be
interrupted; this is admission between jobs, not preemption.
Speculative jobs also wait while the canvas engine requests continuous frames,
including visible animation and unfinished refinement. Visible previews keep
their separate eligibility, so animation does not block requested preview
shaders or create a dependency on completing the speculative queue.
The browser admits one optional pipeline per task; required pipeline batches
remain bounded to four. Optional mask publication also respects the input gate
and uploads at most one mask per poll. Selecting a brush promotes its shaders
and masks ahead of the remaining queue. Readiness of the current brush does not
wait for full background completion.

Retouch shader handles belong to the renderer and survive tool changes. Its
source pixels and healing buffers are still released when unused; warming these
shaders does not allocate retouch source pages.

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
