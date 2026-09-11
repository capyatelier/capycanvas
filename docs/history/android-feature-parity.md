# Android feature parity

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

The native Android host projects the same Rust tool, color and workspace models
as GTK. The shared engine owns drawing, selection, snapping and transforms;
Compose supplies widgets and Android owns input, surfaces and file transport.

## Ported features

- Partial Zen renders the shared edge-toolbar clusters with their stable tile
  identities and configured sizes, preserving the saved dock layout. All four
  edges are covered; normal docking gestures are disabled in this projection.
- Tool Set follows the current tool's groups and subtools, including non-brush
  tools. Existing brush previews remain cached assets.
- Tool Settings renders the shared numeric fields, groups and enabled/checked
  actions, including Fill, Gradient, Figure, Ruler and Operation controls.
- Color exposes the shared HSV square/HLS triangle, foreground/background/
  transparent slots, swap and numeric components. Wheel hit-testing and color
  conversion remain in Rust; Android draws the supplied geometry and gradients.
- Native snapshots include panel views required by transient Zen and drawer
  projections, even when those panels are absent from ordinary dock groups.

- Content drawers use the shared placement/connection geometry, native scrolling,
  animation, Back handling and outside-contact dismissal without painting.
- Collapsed columns expose expand, panel icons, context menus and column dragging.
  Tabbed column drawers support nested tool drawers with live clipped tile anchors.
- Navigator samples the live composition in the existing Vulkan canvas surface,
  using shared camera geometry/actions. Native layout supplies visible bounds;
  Compose clears only the image rectangle. Painting and camera gestures update
  the overview in the same presentation pass, with no bitmap readback or polling.
  The renderer facade forwards color-sample requests for Eyedropper.
- Toolbar divider slots render as separators without action buttons.
- All eight application menus use shared models, including File, Edit, Layer,
  Select, Filter, View, Window and Help. Models are published before GPU setup,
  so opening a menu never waits for shader compilation. Closing a menu releases
  its input capture before the next canvas contact.
- New, Open, Save, Save As, Close and PNG Export use Android's document picker.
  Rust owns document validation, dirty checkpoints and unsaved-change decisions.
  File reads, project encoding and candidate GPU preparation run off the render
  owner. A replacement is adopted only if its approved document epoch/revision
  still matches; the existing drawing survives a failed or cancelled open.
  PNG export submits a GPU snapshot on the owner, then waits, packs rows and
  encodes sRGB PNG data on the file worker using the shared exporter.
- Document replacement invalidates thumbnail and filter-preview caches; the
  Navigator samples the replacement composition. Workspace persistence uses only the shared committed layout model.
  Image-import failures are visible to the user.
- Fresh Android workspaces use the shared full editor preset, with separate Tools
  and Commands ribbons and docked Tool Settings, Color and Navigator. Saved
  workspaces and customized/deleted toolbars retain their existing configuration.
- Fill and Auto Select expose shared Close gaps, Expansion and Edge smoothing
  controls and use the new GPU region-refinement implementation.
- Expanded toolbars use the shared expansion tile geometry; toolbar drawers use
  the shared full content height. Compact Color and Navigator docks fit their
  content to the available height. The header remains opaque over a zoomed canvas.

## Validation

Device coverage runs on the Wacom MovinkPad 14 (Android 15, ARM64,
landscape 2880×1800). `AndroidFeatureParityTest` checks Zen on four edges, six
tool families, displayed HSV/HLS color accuracy, nested collapsed-column
drawers, outside contact without paint, actual GPU Eyedropper color sampling,
Navigator pixels and camera gestures. It also verifies painted pixels for filled
figures, Move, Undo, flood fill, gradients and rulers, and saves/reopens/exports a
512×384 drawing through the real Android file picker. The exported PNG is decoded
and checked for the expected dimensions and painted pixels. Cancelling New must
preserve unsaved paint.

Additional device regressions cover pre-GPU menus/gray UI, stable workspace size
across system-bar transitions, touch navigation/surface recreation, palm rejection,
physical eraser input and camera-only updates. Shared host/UI tests exercise
document adoption/checkpoints and drawer geometry. Build, lint, instrumentation
logs and screenshots are recorded under `artifacts/android/feature-parity/`.

The integrated milestone passes 14 tablet tests (`worker-final-device.txt`),
226 shared host/UI tests (`merged-shared-tests.txt`), four renderer/PNG checks
(`png-encoder-tests.txt`), ARM64 app/test builds and Android lint
(`worker-final-build.txt`).


The follow-up refresh adds device checks for the complete editor preset, live
Navigator pixels while the pen remains down, and the shared region edge controls.
A deliberately gapped outline leaks with gap closing disabled, then contains
both Fill and Auto Select with gap closing enabled. Compact numeric controls are
also checked within their correct panel when the full preset shows multiple
brush-size fields. Refresh artifacts are under
`artifacts/android/parity-refresh/`. The integrated refresh passes all 17 tablet
tests (`integrated-device.txt`, 70.915 seconds), 229 shared host/UI tests
(`shared-tests.txt`), ARM64 application/instrumentation builds and Android lint
(`integrated-build.txt`). The tablet run also repeats pre-GPU UI, system-bar
sizing, navigation/surface recovery, camera publication, palm/eraser and compact
numeric/vertical-toolbar regressions.


Tab contact geometry includes the stable panel identity as well as group/index
and bounds. Previously Android omitted that identity, so a real contact on a tab
sent an empty `contact_tab` to Rust and reported “Unknown panel identity.” Layout
reset made this visible when users dragged the newly docked panels. The device
regression resets both a saved workspace and the editor preset, drags Tool
Settings out by its tab, docks it with Layers, and selects both tabs using actual
touch input. This regression reproduces the exact error before the fix; artifacts
are under `artifacts/android/panel-drag-fix/`.

The integrated drag fix passes all 18 tablet regressions in 82.845 seconds
(`panel-drag-fix/integrated-device.txt`), with ARM64 app/test builds and Android
lint passing (`panel-drag-fix/integrated-build.txt`). The regression waits for the
worker to apply Reset and makes its test tab visible independently of saved tab
preferences. The startup pixel check samples an uncovered placeholder location
before opening a menu, so floating panels cannot masquerade as a background-color
failure.


Missing built-in Tools/Commands bars can now be restored explicitly from the
Window menu. The shared action installs preset tiles with fresh identities,
chooses an unused title, and keeps existing toolbars and panel positions. Ordinary
Reset still preserves customized/deleted toolbar choices. The Android Commands
recovery test passes on the physical tablet, alongside all 220 shared UI tests
and ARM64 build/lint; evidence is in `artifacts/web-parity/commands-android-*` and
`artifacts/android/commands-restore-shared.txt`.
