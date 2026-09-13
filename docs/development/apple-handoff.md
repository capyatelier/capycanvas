# Apple port handoff

## Goal

Ship complete, visually consistent native iPadOS and macOS apps with fast,
readable workflows, shared maintainable code, and validated drawing performance.

Full acceptance remains in [the Apple goal tracker](../history/apple-acceptance.md#goal):
all exposed features, menus and actions; shared main-editor geometry and canvas
behind the header; iPad 120 Hz and current Mac 90 Hz performance targets. Mac
120 Hz testing is deferred. The overall goal is **incomplete**.

## Native docking validation

- Milestone `d520d49` groups the docking fixes with their native validation.
  Main advanced before publication; the clean integration now includes
  `688fd76`, preserving both branches and every recovery stash.
- Both hosts pass the drawer-tab reorder, tear-off, redock and group tear-off
  workflow, collapsed/nested drawers, and panel configuration followed by a live
  group drag. The iPad also passes held toolbar tiles with exact Undo/Redo and
  attached-column width/split resizing with history.
- The first Mac drawer test failed because successive injected mouse-down
  events reused event number zero. The native adapter incorrectly retained the
  preceding collapsed icon's hold policy for the next drawer-tab contact.
  It now identifies the mouse-down event object shared by pan and press.
  An unreachable event-type branch is removed; tablet subtype still distinguishes
  pen from mouse. Shared Rust movement, docking and history are unchanged.
- Panel-control tests scope the duplicate Color swatch to its popup and locate
  Brush Size's containing group through its visible tab, replacing a stale
  preset group ID. No product UI workaround is added. Native input fixtures use
  the shared popup host and permit only sub-millionth-point event round-trip
  rounding while keeping device, button, window and event checks.
- Native AppKit workspace checks pass on both Apple presets with increasing and
  repeated zero event numbers. Mouse and pen drawer-tab drags include continuous
  movement and exact Undo/Redo. Earlier full-editor and real-canvas isolation
  checks also pass. These injected contacts do not establish physical Pencil
  coverage. Both signed iteration-28 builds pass; the iPad app executable is
  identical to the installed iteration-23 review app.
- Native layer and workspace-list regressions also pass on both presets,
  including pen holds, immediate mouse rows/grips, history, keyboard menus,
  scrolling, focus loss and source removal. The workspace-list fixture now mounts
  the shared popup host; its earlier keyboard failure remains recorded.
- The final signed Mac batch passes all four workflows with no failures or
  skips: drawer docking, panel configuration/live dragging, collapsed/nested
  drawers and workspace-switcher order/pin persistence across restart.
- Evidence, original failures and temporary diagnostic cleanup are recorded in
  `artifacts/apple-docking-workflows-v1/checkpoint.json`. The review namespace is
  restored and the artist app descriptor remains unchanged. After the user
  opened Files, the regenerated export test reached the visible picker without
  an authentication prompt. Its Cancel tap did not dismiss the picker; XCTest
  reported no usable hit point for that remote element. Cancellation remains
  unverified. See `files-unlocked-v28/` for the failure and restored review state;
  no further Files-unlock confirmation is pending.
- Incoming shared GPU uploads now return mapping failures to the host; Apple's
  blank presentation forwards those errors. Both signed iteration-29 builds
  pass. Mac mouse drawing/Undo/Redo/layers and iPad Metal launch/layers each
  pass with no failures or skips. The new review app and runner are installed,
  the review namespace is restored and the artist descriptor is unchanged.
- The integrated regression has 529 passes and 19 existing hardware/benchmark
  skips. Its one strict filter-reference failure remains: all decoded input,
  output and reference pixels, plus the per-case error table, exactly match the
  retained Metal baseline. No reference or tolerance is changed. Integration
  evidence is under `artifacts/apple-main-integration-688fd76/`; final native
  results and installed descriptors are under `integrated-v29/` in the docking
  evidence folder. Overall acceptance remains incomplete.

## Workspace recovery and editor workflow milestone

- The Color milestone is published as `6c36d0a`. The periodic pull now includes
  `19d6722`; all integrations preserve working paths and every recovery stash.
  Milestone `072c7b7` adds the lifecycle and feature fixes below. The integrated
  upstream code keeps command icons steady during strokes and adds shared CPU
  input retirement for renderer failure.
- Quick restart tests exposed a shared native ownership bug: a killed process
  left Illustrator claimed, so restart selected Painter and hid the expected
  Color/Layers panels. `SqliteStore` now holds a standard OS file lock for each
  connection's lifetime; the first opener after all clients exit clears abandoned
  claims transactionally. Existing clients retain normal leases/fencing. The
  lock sidecar is protected from export overwrite.
- Included workspace Layout History now permits restoring an earlier layout.
  Its names and deletion remain protected, as do competing owners and busy or
  current history entries. Both native hosts pass the history workflow.
- Curve point and gradient stop counts are included in their native accessibility
  labels. Separate value attributes were not exposed on either host and were
  removed; no custom accessibility wrapper or gesture change was needed.
  Both hosts pass filter search, previews, numeric edits, curve insertion/reset
  and gradient insertion/position/reset.
- All six broader feature workflows pass on both hosts across the retained
  runs. The initial iPad shortcut test typed “Zen” but the search field contained
  “Zn”. Shortcut, settings and filter searches now reuse the existing local-draft
  text field to avoid replacing newer input with delayed
  snapshots. The old workspace-specific helper is removed. Both hosts pass
  complete query entry, conflicting shortcut capture/replacement and editor
  activation; toolbar editing also passes through the same helper.
- Integrated shared regression passes 476 tests: 42 Apple, 25 host, 343 UI and
  66 workspace checks, with one existing hardware-only host check ignored.
  Workspace coverage includes a real child-process kill, live-owner exclusion,
  saved contents, successor fencing, built-in history and protected backup paths.
  Both signed iteration-21 builds pass. Each host passes all six integrated
  native workflows with no failures or skips: Color, filters, shortcuts, toolbar
  editing, restart and history. All six fresh live Color checks pass; guide error
  is at most one channel level and field error is zero. Normal-size inspection
  shows the accepted Color layout retained. The final command-presentation pull
  also passes the 410 Apple/host/UI regressions and both signed iteration-22
  builds. Mac mouse drawing/Undo/Redo/layers and iPad Metal launch/layers each
  pass their native follow-up with no failures or skips. The final integration
  through `19d6722` passes 411 Apple/host/UI checks and the focused input-retirement
  regression, both signed iteration-23 builds and the same two native follow-ups.
- Mac passes all four lifecycle workflows: settings/workspace restart, artwork
  recovery, independent windows and New/Export cancellation. The iPad passes
  the first three. The later unlocked Files follow-up reaches the picker but
  still fails cancellation, as recorded above. All initial failures remain
  recorded.
- Installed-device XCTest uses `UseDestinationArtifacts` without local
  `DependentProductPaths` or bundle paths. Original `app.launch()` works,
  including terminate/relaunch. The unsuccessful explicit-bundle experiment and
  obsolete Color attach branch are removed. See the Apple README for setup.
- The iteration-23 iPad review app and runner are installed, its saved review
  namespace is restored and the artist descriptor remains unchanged. Earlier
  post-test process-query timeouts are retained with their verified recovery.
  No component-app exchange was needed. Old iteration-17 picker scripts must be
  regenerated against current validated products and installed descriptors.
- Evidence: `artifacts/apple-lifecycle-workflows-v1/checkpoint.json`,
  `artifacts/apple-feature-workflows-v1/checkpoint.json`,
  `artifacts/apple-main-integration-b56bca3/` and
  `artifacts/apple-main-integration-65a9855/` and
  `artifacts/apple-main-integration-19d6722/`. The full feature, visual,
  physical-input, lifecycle/expiration and sustained-performance gates remain open.

## Compact Color panel milestone

- `main` includes the periodic integration through `b8ba188`. All 29 existing
  working paths were preserved, with only the expected shared UI export merge.
  The exact recovery stash remains under
  `artifacts/apple-main-integration-b8ba188/`; do not pop or drop it or earlier
  recovery stashes. Publish completed major milestones, not individual fixes.
- The user approved testing on the connected iPad, including the pending review
  update/restart. That approval is resolved; do not ask again for the same testing.
  The user also clarified that visual acceptance means perceptual parity at
  normal viewing size. Imperceptible pixel differences are acceptable. Fix
  visible mismatches and simple refinements; prioritize shared, simple code and
  removal of dead, deprecated or unnecessary paths over exact PNG identity.
- Both hosts use the shared compact Okhsv circle, HSV square and HLS triangle,
  overlapping paint swatches, transparent paint, Swap, shape icons and curved
  shape/RGB readouts. Rust owns geometry, picking, conversion and readout text.
  Field and guide caches retain separate shared RGBA8 images; native clips,
  markers and buttons remain at display resolution. The old HLS-only bridge and
  cache are removed. Header and curved-readout metrics share one helper.
- Wheel painting uses panel coordinates and Web's rounded destination edges.
  Swatch selection borders sit behind paint. Each styled swatch has an explicit
  circular hit region: the first physical iPad editor test showed foreground
  corners intercepting a tap on the visible background swatch. The one-modifier
  correction passes the unchanged workflow on both hosts.
- Both signed Debug builds pass as version 14 against current main. The shared
  regression passes 42 Apple bridge, 25 host and 340 UI tests (407 total), with
  one existing hardware-only host check ignored. The Metal-analysis checks pass
  all 22 tests. No production drawing-performance change is included.
- Final Mac and physical iPad editor workflows each pass with no skips. They
  cover all three shapes, readout switching without changing paint, paint slots,
  transparency, Swap, continuous contacts and empty-corner behavior. All six
  live captures pass the unchanged color oracle: guide error at most one channel
  level, field error zero. The earlier failing swatch attempt remains recorded.
- Both 216-case component matrices pass the unchanged two-level color tolerance
  (guide maximum one, field maximum two). UIKit and AppKit frames agree exactly;
  Chrome differs by at most 0.00521 points. Both themes, presets, all shapes,
  readouts, paint slots and 128/160/226-point widths are covered. The final hit
  shape changes input only; the validated drawing paths are unchanged.
- Eighteen native Mac/Web hover, press and cancellation captures retain identical
  geometry and pass cancellation checks. Keyboard focus coverage is not inferred
  from those captures. The standard Mac-only `drawingGroup()` before the two
  shape-icon rotations improves every full panel; applying it on UIKit regressed
  its results and was rejected. No custom icon renderer was added.
- Full comparisons retain text, edge and compositing differences. Normal-size
  inspection, including the highest-mean-error UIKit case, shows no material
  layout or color mismatch. Mean channel error across the matrices is 0.619 on
  Mac and 0.859 on iPad, on a 0–255 scale; no exact PNG matches are claimed.
  Generic primitive substitutions changed at most one channel level in 19 pixels
  per probe cell and were rejected. A further compositing probe was prepared but
  not run after the user's simplicity clarification. Full-editor visual acceptance
  remains separate from this Color milestone.
- The final review app and test runner are installed on the iPad; the review
  namespace is restored and the artist app descriptor is unchanged. Temporary
  component apps are removed. Use fresh isolated namespaces for future tests.
  Keep device, signing and storage details in ignored artifacts.
- Final integrated builds, results, live color checks and deployment state are
  under `artifacts/apple-color-milestone-v1/`. The swatch diagnosis and first
  successful device workflow are under `artifacts/apple-color-device-final-v1/`.
  Original component captures, the portable `color-review.html` and earlier
  failures remain under `artifacts/apple-compact-color-v1/`. See the milestone
  checkpoint before continuing; retain all underlying evidence.

## Latest editor milestone

- Branch `main`. The shared flat menus, attached column panels and icon integration
  are grouped into one major milestone. Preserve any later working-tree changes.
- The Edit trigger is fixed with a full label hit shape. `ViewThatFits` is retained.
  Popup content now inherits the root's current theme; copying the invoking
  control's complete environment had made dark-menu text unreadable.
- The embedded UIKit context source also transfers only appearance and enabled
  state. This restores Zen accessibility on the connected iPad. Existing UIKit
  hold/pan recognizers retain the original contact. `EditorActionMenu`,
  `EditorMenuButton` and `editorPopover` share vertical, opaque presentation.
  No native context-menu/drag-session handoff or extra popup window remains.
- Menus reuse native keyboard capture, with shared navigation/actions and UIKit
  responder restoration. macOS retains its OS application menu bar and native
  secondary-click adapter. Its Select All action forwards to the focused text
  editor before canvas dispatch. Toolbar grip labels include their toolbar names.

## Verified progress and limits

- Connected iPad: both-theme main menu/Undo; layer/mask/footer anchors/actions;
  upward layer dragging with exact Undo/Redo; submenus, arrow navigation and
  command shortcuts; keyboard routing after dismissal; workspace menu actions
  and held dragging in both directions. Five tests pass together. Zen passes
  separately after its accessibility fix, including hold/tap suppression and
  the resulting Total Zen action. No skips; the earlier failure stays recorded.
- Mac: an isolated editor passes both blend choices and Undo/Redo. Actual native
  events in a separate fixture verify submenu/back navigation, disabled rows,
  Return, Escape and shifted shortcuts. Neither test addresses system-menu coordinates.
- Both hosts pass workspace pin/order persistence, toolbar styles/Zen and full
  toolbar creation/rename/duplicate/delete. Final customization follow-ups verify
  Mac Command-A routing and accessible toolbar names; both tests use native
  Select All text replacement, avoiding caret-dependent backspace counts.
- UIKit callback checks pass touch/pen/mouse classification, edge scrolling,
  hold/lift retention, immediate grips and cancellation. These are not a new
  physical Pencil pass. Attached-column and icon evidence is preserved below.
- Both signed Debug hosts build; 41 Apple bridge, 25 host and 306 UI tests pass,
  with one existing hardware-only host test ignored.
- iPad Escape/Return remain unverified. A first-responder simulator probe received
  a printable key but no XCTest Escape press or key-command callback; the physical
  Escape check also failed, and simulator Return did not execute a menu action.
  Do not add a product workaround just to accept missing synthetic input.
- Exact results and deployment state: `artifacts/apple-flat-menus-v1/checkpoint.json`.
  Earlier column/icon evidence: `artifacts/apple-column-panels-v1/` and
  `artifacts/apple-icons-checkpoint.json`. Raw evidence stays out of Git.

## Continue toward full acceptance

Use the goal tracker's complete inventory and remaining gates. Current performance
still fails sustained presentation cadence: Mac has residual long intervals and
iPad retains drawable-acquisition stalls. The other drawing workloads, calibrated
CPU/GPU/input-latency measurements, complete feature/lifecycle workflows and the
full visual matrix remain open. See [performance evidence](../../apps/layer-apple/PERFORMANCE.md)
and [Apple testing instructions](../../apps/layer-apple/README.md).

The subsequent reserved-drawable-slot experiment is rejected: eight matched
short physical runs show worse cadence on both hosts despite improved iPad CPU
tails. Production code/tests are restored to `232cd2b`; evidence and the next
diagnostic hypothesis are in `artifacts/performance/drawable-reserve-232cd2b/checkpoint.json`.
That experiment restored the iPad review app. The subsequent native diagnostic
temporarily replaced only the isolated review bundle; its process is closed.
After reconnection, the validated review app was restored. The regular artist
apps/data are preserved. The user's new compact-color request then took priority.

The presentation-notification diagnostic does not justify a renderer change.
Corrected Mac compute runs present all 1080 admitted frames in both modes while
GPU completion crosses the CPU deadline. Direct presentation must wait for
command scheduling; the first shared-event fixture did not, and its iPad GPU
timeout is retained as an invalid diagnostic. The corrected iPad executable now
completes all four cases in a separate disposable test identity: each presents
1440/1440 measured frames, with no zero/missing callbacks or GPU errors. Direct
and queued compute modes have 1425 and 1440 qualifying GPU-deadline crossings;
all present successfully. Each run retains one skipped warm-up presentation
outside measurement. This is a native Metal contract check, not application
cadence or drawing-latency acceptance. Its first directory-listing failure was
recovered from the same live process without repeating the measurement. All
diagnostic processes are closed, its app is removed, and the version-9 owned
runner is restored. The artist and version-5 review editors remain unchanged.
All 160 skipped presentations in the three older application iPad display-link
traces had completed CPU owner service at least 3 ms before the deadline;
their actual Metal scheduling times were not recorded. See the new performance
section, `artifacts/performance/metal-notification-2447108-v3/checkpoint.json`, and
the completed iPad `artifacts/performance/metal-notification-ipad-isolated-v1/checkpoint.json`.
The attempted real-viewport observer through `CommandEncoder::as_hal_mut` was
rejected: the actual Mac editor cannot mix normal wgpu encoding with raw encoder
access. Standalone callback tests and successful builds missed this restriction.
Both failed diagnostics are retained; all nine changed source/test files were
restored exactly to the preceding Color milestone state. No iPad deployment or
valid performance measurement occurred. Do not deploy the rejected observer
builds or revive that path; see
`artifacts/performance/viewport-commands-v1/checkpoint.json`. Existing Instruments
captures can supply diagnostic GPU execution observations, but actual unprofiled
viewport scheduling/completion is still missing. Do not infer that a
presentation-backend replacement will fix the retained stalls.

Reanalysis of the earlier real-app Instruments captures now joins all 6 Mac and
122 iPad recorded presentation requests to native frames. Requests and the last
observed GPU completion precede their frame targets, but actual presentation is
one refresh later. The new optional request analysis in `metal_frames.py` rejects
ambiguous identities, multiple requests and invalid ordering; all 22 Metal
analysis checks pass. These are small profiled samples from the earlier
`4a0a808` run, with incomplete capture coverage. They neither explain the current
stalls nor establish input latency; see
`artifacts/performance/retained-command-timeline-v1/checkpoint.json`.

Current-source Release recordings now complete on both hosts in separate
diagnostic identities. All 9 Mac and 97 iPad associated frames fall inside the
measured ink phase. Every captured Mac request/GPU end precedes its target, but
presentation follows two or three refreshes later. On iPad, 95/97 are early;
two late requests coincide with 8.692 ms and 13.197 ms drawable-acquisition stalls.
The requested rolling window retained only 89.236 ms of target GPU work on Mac
and 916.941 ms on iPad, so these are narrow profiled observations, not cadence
or physical-input acceptance. No renderer change is adopted. Earlier reductions
to the drawable pool and admission limit both regressed cadence; do not repeat
them merely because these traces show delay. The current recording checkpoint is
`artifacts/performance/current-command-timeline-v1/checkpoint.json`.

The original current iPad run completed but used an identifier Instruments could
not resolve; the successful follow-up uses the same device's hardware UDID.
An unexpected new disposable-app process was observed after the original closed;
the launch guard stopped before starting another run, and the owned process was
recorded and closed. Its cause remains unknown. One optional export also required
a retry; all failures remain recorded. The diagnostic app is removed and the
version-9 owned runner restored, with both existing editor descriptors unchanged.
The version-5 review editor has still not been updated or restarted.

The earlier authorized SSH pull incorporated the shared color-picker and
GTK/Web refinements; the current revision and color checks are recorded above.
No production presentation-scheduler change is present. These acceptance
notes remain for the next major milestone; do not commit an experiment separately.

Read `AGENTS.md` and the shared drag convention. Keep vertical menus, opaque
shared colors, simple native adapters and regular artist app data. Do not ask
for another speculative physical retry. Keep automation focused and never
coordinate-test the Mac system menu bar; check editor effects directly.

Fetch/integrate other ports periodically and before publishing. Commit and push
only completed major milestones to `main`, using author and committer
**Zack Drach <zackdrach@gmail.com>**. Public HTTPS fetching works; the verified
repository SSH-agent socket is recorded only in the ignored local checkpoint
folder. Publish source/docs only, without device/account identifiers, signing
details, raw logs or captures. No overall-goal completion is claimed here.
