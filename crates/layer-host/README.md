# layer-host

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-host` shares session and renderer integration between the Android, Apple
and Windows clients. It combines [`layer-ui`](../layer-ui/README.md) with the
[`wgpu renderer`](../layer-render-wgpu/README.md), handling common input transport,
startup state and UI snapshots. GTK and web integrate the shared session directly.

## NativeHost and the attached renderer

`NativeHost` owns a `UiSession`, tracks canvas readiness and prepares state for the
native frontend. Pointer batches preserve the view revision from the moment their
samples were collected. UI snapshots are refreshed when relevant state changes,
with camera updates tracked separately.

`NativeHost::set_localization` adopts a prepared immutable language context on
the session owner and replaces retained UI publication baselines and catalog
copy. Its localization generation belongs to the window and survives drawing
switches. It does not wake the canvas or change document/request generations.
Hosts also call `DocumentWindow::set_localization` to refresh generated tab
captions. Parked sessions catch up when selected, and prepared Open/New sessions
adopt the current window language before publication without repeating GPU work.

`layer_render_wgpu::AttachedRenderer` wraps an optional `WgpuRasterizer`. This allows a host to create editor
state before attaching the GPU renderer and preparing its shaders. Pixel operations
require the attached GPU; the wrapper does not provide a CPU painting fallback.

The platform calls the host from one engine/render owner. Native callbacks queue
work for that owner. Thread creation, native widgets, surfaces, frame callbacks and
file access remain platform responsibilities.

File and preview workers retain immutable `ArtworkCapture` resources and typed
scene snapshots. Source-only package writing does not require a preview or GPU
readback. Export and comparison previews use the captured output context,
including its integrated effect phases; durable save completion remains a shared
session decision. `OpenEnvironment` callers retain `ImportedDocument` until
renderer admission completes. Unsupported preparation uses its retained backing,
verified preview and output inventory for the shared read-only package view;
cancelled or stale requests keep their existing refusal path.

`DocumentWindow::adopt_session` publishes a prepared session with its restored
camera, working state, history and save checkpoint, retaining its prepared tools.
Window preferences and display geometry follow the current window before publication. Ordinary restart
restoration preserves the drawing's destination and modified state.
`restore_sessions` replaces an untouched startup drawing with admitted candidates
in their saved order and selects the saved active drawing. A captured session
stamp fences startup edits and camera changes. Inactive renderers are returned
to the worker for destruction, and retained history counts against the window's
metadata budget. Tab views include session stamps so hosts can schedule
checkpoints after working-state and camera changes as well as artwork edits.
When startup edits prevent replacement, `append_restored_sessions` parks the
restored drawings beside the live drawing and returns their new identity mapping.
The live selection, editor and renderer keep their ownership.
Hosts that hydrate inactive drawings after presenting the active drawing use
`DocumentSessions::append_parked_with_id` to preserve their saved identities and
active selection. Refused membership returns the incoming owner intact.
Pending identities can be reserved while later drawings are decoded. The native
`hydrate_restored` helper retains active input and maps collisions to unused
identities. Batch workers check aggregate editor/history admission before GPU
preparation and park each inactive candidate before retaining it, keeping only
the active renderer and one preparation renderer alive.

Closing a drawing uses `prepare_close` before durable membership removal. It
completes target inheritance and source parking, then rejects shared actions
until the storage result arrives. Queued native input returns success without
changing the prepared source. `commit_close` applies the prepared
membership change and current window dimensions without GPU or fallible work
after publication. `cancel_close`
resumes the source when publication fails or is cancelled. Hosts retire source
storage only after commit, and destroy returned renderers and owners on workers.

## Where to start

- [lib.rs](src/lib.rs) defines `NativeHost`, pointer batches, action dispatch,
  startup coordination and snapshots.
- [`AttachedRenderer`](../layer-render-wgpu/src/attached.rs) forwards the rendering
  contract to the attached `WgpuRasterizer`.
- The [Android bridge](../../apps/layer-android/native),
  [Apple bridge](../../apps/layer-apple/native) and
  [Windows bridge](../../apps/layer-windows/native) show how hosts use this crate.

Read [platform integration](../../docs/platforms/README.md) for the shared/native
boundary. This is a Rust integration layer.
