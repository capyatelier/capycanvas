# Shared workflow centralization: Web and Android handoff

2026-09-17. Requested after auditing Windows parity against Web and Android at
`90adbb6d`; the evidence below remains applicable after `6da20d05`. This is a work
list and proposed end state, not a completion or platform-acceptance claim.

## Objective and scope

Centralize the portable business and rendering decisions below in shared Rust,
then migrate Web and Android to those implementations before the Windows port
adds these workflows. Significant native UI changes are acceptable. The concern
is duplicating product rules, transaction semantics or rendering decisions in
each frontend. Rust under `apps/` is still frontend-owned code.

The end goal is that Windows can implement native UI, input, file/storage and GPU
lifecycle adapters while calling the same workflow services as Web and Android.
An API existing in a shared crate is insufficient if each host must reconstruct
the operation's correctness rules around it. Migrate both existing consumers and
remove their duplicate rules as part of each extraction.

Coordinate with the [photo placement handoff](image-placement-web-android-handoff.md)
and preserve its retained-source, batch-placement and Apply/Cancel behavior. The
[Windows color handoff](color-management-m2-windows-handoff.md) remains the later
host-integration task; this work should replace the need to copy Android's jobs.
Existing GTK and Apple integrations must remain compatible. Audit other consumers
of each extracted rule and migrate affected callers or record explicit follow-up
work; Web/Android validation does not establish native acceptance elsewhere.

## Intended ownership

These are responsibility boundaries, not prescribed new type or crate names.
Choose locations that preserve the current dependency direction.

| Owner | Responsibility |
| --- | --- |
| `layer-core` / `layer-engine` | Document data, exact samples, edit semantics and history. |
| `layer-color` | Profile interpretation, conversion, source repair preparation, codecs and reusable output processing. |
| `layer-ui` / shared host services | Operation choices, draft actions, candidate validity, transaction state, recovery/library policy and publication decisions. |
| `layer-render-wgpu` | GPU candidate preparation, shared preview/output plans and rendering decisions. |
| Platform adapters | Native controls, pointer/input collection, picker and clipboard transport, storage operations/locks, worker scheduling, capability observations and surface/device lifetime. |

Expose portable operations or state transitions that hosts can drive with their
own executors. Browser workers and asynchronous GPU mapping need not use the
same transport or blocking model as native workers. Keep I/O and expensive GPU
preparation away from the live editor owner. Publish the approved result and its
history coherently, with cancellation and stale-result rejection owned once.

## Issues to centralize

### C1. Document color transactions and view/brush remapping

Evidence: [Android color jobs](../../apps/layer-android/native/src/color_edit.rs)
and [Web color jobs](../../apps/layer-web/src/color_edit.rs).

Both independently match Assign/Convert/Depth choices to requests, restrict
flattened copies, check document/request/GPU identity, and coordinate renderer
replacement with history commit and rollback. Both also calculate the transform
for the brush color, secondary color and view background when the working space
changes. That last part is rendering-related color semantics, not native UI.

**End goal:** one shared color-change operation owns valid choices, candidate
identity, copy-versus-edit behavior, view/brush remapping, comparison state and
commit/rollback rules. Reuse the existing
[color transitions](../../crates/layer-ui/src/document_color_edit.rs),
[conversion](../../crates/layer-color/src/document.rs) and
[GPU candidate preparation](../../crates/layer-render-wgpu/src/snapshot/color_candidate.rs).
Hosts provide a device-generation/lifetime observation and execute preparation;
they do not independently decide whether a candidate can commit. Preserve exact
Undo/Redo, the separate destination for a flattened copy, and off-owner retirement
of old GPU resources.

### C2. Retained-source repair and rasterization transactions

Evidence: [Android source jobs](../../apps/layer-android/native/src/source_edit.rs)
and [Web source jobs](../../apps/layer-web/src/source_edit.rs).

Each determines whether the layer contains baked edits, enforces source-profile
choices, prepares repaired interpretation, checks candidate validity, and requires
a complete comparison before Apply. The underlying rasterization and layer edits
are already shared, but the complete operation's rules are repeated.

**End goal:** one shared source-edit operation prepares the interpretation or
rasterization, reports whether Apply adds a corrected original, and owns preview
readiness and commit validation. Build on
[source edits](../../crates/layer-ui/src/source_edit.rs) and
[rasterization](../../crates/layer-color/src/rasterize.rs). Preserve original
samples/profile/depth until explicit conversion, existing paint/masks/position,
and one-step history. Reuse C1's common candidate-validity machinery where the
semantics match, without forcing the two operations into an overgeneralized API.

### C3. Photo Open/Place/Paste preparation and adoption policy

Evidence: [Android document jobs](../../apps/layer-android/native/src/documents.rs),
[Web document jobs](../../apps/layer-web/src/documents.rs), and
[Web worker project transport](../../apps/layer-web/src/raster_project.rs).

Hosts repeat decisions about project versus photo input, which inputs Place
accepts, when an assumed profile requires a user decision, how photo editing
depth is selected, and clearing a photo's save location on adoption. In particular,
requiring a separate editable master is a portable product rule.

**End goal:** shared preparation/adoption policy consumes source data, operation
intent and preferences, and returns either a prepared result or a required
interpretation decision. The prepared result carries its source kind and save
semantics, so a host cannot accidentally treat the original photo as the master.
Reuse [decoding](../../crates/layer-color/src/photo.rs),
[photo construction](../../crates/layer-color/src/photo_project.rs) and the shared
placement transaction. Native pickers, browser handles, Android URI permissions
and clipboard formats remain adapters. Derive file filters from actual shared
decoder capabilities instead of repeating format lists where possible. Preserve
all-or-nothing placement, approved targets and stale document/device rejection.

### C4. Export planning and dependent option changes

Evidence: [Web output](../../apps/layer-web/src/output.rs) repeats the conditions
for identity export already present in
[shared snapshot output](../../crates/layer-render-wgpu/src/snapshot/output.rs):
unchanged dimensions, default conversion, no matte, and a matching original
source. These decisions determine whether exact original samples, including
hidden RGB, survive delivery.

Also, [Android export controls](../../apps/layer-android/app/src/main/java/art/capycanvas/ExportDialog.kt)
and [Web export controls](../../apps/layer-web/export-controls.js) independently
force U8 depth and replace Preserve transparency with White when JPEG is chosen.
The [shared recipe](../../crates/layer-ui/src/export.rs) validates the final
combination, but does not own those draft transitions.

**End goal:** one shared output plan selects original-source versus composed
output and defines conversion, resizing, alpha, depth, resolution and preview
semantics. Native and browser executors may obtain rows differently while using
the same decision. A shared form action handles dependent option changes and
returns valid choices/control state. Reuse current recipes, presets, CMM,
resampler and codecs. Preserve bounded memory and exact identity exports; do not
replace the browser's worker streaming with a full-image allocation. The actual
destination selection and atomic publication mechanism remain host-owned.

### C5. ICC-profile library policy

Evidence: [Android ProfileStore](../../apps/layer-android/app/src/main/java/art/capycanvas/ProfileStore.kt)
and [Web profileLibrary](../../apps/layer-web/raster-worker.js).

Kotlin and JavaScript independently implement content-addressed profile identity,
integrity checks, deduplication, the 16 MiB per-profile limit, the 128-profile /
64 MiB library limits, and unavailable-entry handling. Only ICC inspection itself
is shared. Adding another equivalent Windows store would repeat business rules.

**End goal:** shared profile-library operations own identities, limits, validation,
listing results and import/remove decisions over a small storage interface. Hosts
provide bytes, metadata, atomic storage and serialization/locking. An imported
profile is an app-owned copy; removing it must never delete its source file.
Retain readable failure states for corrupt/missing entries and preserve existing
library data when changing implementations.

### C6. Artwork recovery state machine

Evidence: [Android RecoveryController](../../apps/layer-android/app/src/main/java/art/capycanvas/Recovery.kt)
and recovery logic in [Web documents](../../apps/layer-web/documents.js).

Both decide whether an epoch/revision/modified combination needs a snapshot,
whether a clean document should retire its copy, how replacement/discard affects
recovery, and that recovering a document must create its new durable copy before
deleting the abandoned source. Capturing committed artwork is already shared.

**End goal:** one shared recovery state machine decides capture, publish, retire,
offer, restore and discard transitions. Hosts report lifecycle/timer events,
storage completion and ownership-lock results. Reuse
[`capture_project_recovery`](../../crates/layer-ui/src/document_files.rs), which
excludes active contact pixels and does not acknowledge a manual save. Preserve
failure/retry behavior, pending-write ordering, multiple live owners, and a valid
previous recovery copy until replacement is durable. Native files/locks and
browser IndexedDB/Web Locks remain separate storage adapters.

### C7. Windows color-preview integration must consume existing shared rendering

Evidence: [Windows effect colors](../../apps/layer-windows/EffectView.cpp) still
read array colors, and [Windows gradients](../../apps/layer-windows/GradientView.cpp)
construct a native sRGB-interpolated ramp. Shared tagged color forms and gradient
samples already exist in [color UI](../../crates/layer-ui/src/color/form.rs).

**End goal:** Windows consumes shared tagged values, converted swatches and sampled
gradient output. It only draws those results and forwards user actions. No new
shared abstraction is needed just to replace this legacy path. The Web/Android
agent should preserve/document that reusable contract; changing WinUI remains the
Windows follow-up and does not block completion of the shared extractions.

## Smaller follow-ups and boundaries

- New-document defaults/preset mutations and export draft changes should use
  shared actions where hosts currently reconstruct the same rule. Creation,
  palette and export validation/models already exist; extend those rather than
  building another model alongside them.
- Histogram computation and sample averaging are already shared. Consider a
  reusable inspection state model for revision tracking, stale labels, cancellation
  and retry/refresh eligibility. Native chart drawing, UI layout and executor
  scheduling can remain frontend-owned. This is lower priority than C1-C6.
- Floating-panel sizing and toolbar-drawer switching already have shared policy.
  Windows needs measurements, event integration and feature enablement, not a
  second layout algorithm. Preserve the [drag convention](../ui/drag-and-reorder.md)
  if related surfaces are changed.
- Native GPU capabilities, swapchains, pointer capture, OS prediction, clipboard
  transport and packaging remain platform responsibilities. Keep shaders, color
  conversion, sampling and output math in their existing shared implementations.
- This centralization work does not add HDR, print proofing, a new renderer,
  unsupported codecs or the deferred dirty-rendering redesign.

## Suggested delivery order

1. Extract C1/C2 and their common operation-validity rules; migrate both hosts.
2. Centralize C3/C4 alongside the existing photo-placement work, preserving
   browser worker boundaries and current retained-source precision.
3. Extract C5/C6 into reusable services with storage adapters and shared tests.
   Fold in the smaller form-state issues where they touch the same code.
4. Document the actual APIs and remaining caller migrations for Windows and other
   hosts. Keep C7 explicit as later Windows integration.

## Completion criteria

- Web and Android use the same shared implementations for C1-C6; the duplicated
  semantic branches are removed, not merely wrapped in new APIs. Record remaining
  transport-specific code and other-platform follow-ups with reasons.
- Shared tests cover wrong operation choices, cancellation, stale document/request/
  device results, rollback, source preservation, exact color history, identity
  export eligibility, profile-library limits/integrity, and recovery write/retire
  ordering. Use fake transport/storage for portable decisions.
- Build/check the affected Rust crates and actual Wasm/Android targets. Run the
  existing host journeys for creation, profiled Open/Place/Paste, color/source
  edits, previews, export, palettes/profiles, restart recovery and device loss.
  Target additional tests at changed boundaries; compilation alone is not proof
  that a host still drives the shared state machine correctly.
- Preserve P3 U8 / ProPhoto U16 samples, profiles, masks/effects, Undo/Redo and
  save/reopen behavior. Re-run the existing large-photo drawing/recovery journey
  when changes affect its preparation, capture or publication paths. Record any
  unavailable hardware validation rather than implying a new performance pass.
- Update this handoff with completed items, API locations, migration notes and
  actual validation. The Windows follow-up should be able to enumerate its native
  adapters without copying a color/source/export/recovery business state machine.
