# Android feature parity

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
- Navigator shares one bounded GPU preview producer across projections and uses
  shared camera geometry/actions. The renderer facade now forwards canvas-preview
  and color-sample requests, fixing the previously inert Eyedropper path.
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
- Document replacement invalidates thumbnail, Navigator and filter-preview
  caches. Workspace persistence uses only the shared committed layout model.
  Image-import failures are visible to the user.

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
