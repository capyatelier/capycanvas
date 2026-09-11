# Android feature parity

The native Android host projects the same Rust tool, color and workspace models
as GTK. The shared engine owns drawing, selection, snapping and transforms;
Compose supplies widgets and Android owns input, surfaces and file transport.

## Completed port milestone

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

## Validation

The Wacom MovinkPad 14 (Android 15, ARM64, landscape 2880×1800) passes all six
`AndroidFeatureParityTest` tests in `artifacts/android/feature-parity/drawer-device-final.txt`.
Coverage includes Zen on four edges, six tool families, displayed HSV/HLS color
accuracy, nested collapsed-column drawers, outside contact without paint, actual
GPU Eyedropper color sampling, Navigator pixels and camera gestures. Native-host
checks cover shared drawer queries and clipped/scrolled source tiles. ARM64
application/test builds and Android lint pass. Screenshots are collected under
`artifacts/android/feature-parity/`.

Android document workflows and pixel/gesture validation of the remaining newly
exposed drawing tools are the next milestone; they are not claimed complete here.
