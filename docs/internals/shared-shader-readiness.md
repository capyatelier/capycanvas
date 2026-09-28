# Shared shader readiness

[Editor internals](README.md)

Every host uses the same demand-driven dependency tracking and input admission
policy in `layer-render-wgpu`.

## Order

Startup prepares paper/presentation, the actual document, then the selected
brush and physical eraser. Unused brushes, procedural masks, region tools and
filter programs retain recipes; adding one no longer adds a startup compile.
Visible previews request their own variants later. Already compiled variants
are reused for subsequent selections and document effects.

The existing native compiler thread and browser task runner share
`shader_admission.rs`: optional work waits for 200 ms without input and an idle
session. Strokes, held gestures, queued input, pending document edits and
settings hold that gate closed. Native workers sleep on their existing
condition variable; browsers use a host timer. Window input observers forward
activity without changing gesture routing or drag conventions. Required
canvas/brush dependencies and explicit package validation retain priority and
can progress while input is arriving. An in-flight driver call cannot be
interrupted; this is admission between jobs, not preemption.

Readiness continues after initial startup. Shared `UiSession` wakes the host
when a UI-only brush change needs preparation. Native readiness snapshots use
the current dependencies, so a previous tool's ready flag cannot authorize the
new tool. Contacts begun before readiness remain suppressed through release.
`ShaderDocument` memoizes the dependency-relevant document state: ordinary
raster/parameter edits do not invalidate readiness or trigger effect preparation.
Effect-chain inspection now borrows layers; only compiler jobs clone their
owned inputs.

Android and GTK no longer reload their already embedded filter package during
startup. Android also stops packaging that redundant asset copy. GTK still
accepts explicit `CAPY_FILTERS_DIR` / `CAPY_FILTERS_MODE` overrides; runtime
package installation and atomic validation remain available. Identical library
refreshes are no-ops on all three hosts. Existing native pipeline caches remain;
there is no additional platform cache or compiler worker. Current native cache
saving still closes the initial cache after required startup work; this change
does not add persistence for later first-use variants.
