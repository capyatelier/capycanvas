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

## Validation

The Wacom MovinkPad 14 (Android 15, ARM64, landscape 2880×1800) passes all three
`AndroidFeatureParityTest` tests in `artifacts/android/feature-parity/color-device-tests.txt`.
They check partial Zen on each edge without changing the saved layout, tool-set
and settings schema projection for six tools, and color selection against actual
rendered HSV/HLS pixels as well as slot selection/swap. Native-host tests pass
10/10; ARM64 application/test builds and Android lint pass. Screenshots are named
`parity-*` in the app's external files directory and are collected locally under
`artifacts/android/feature-parity/`.

These checks establish native controls and projection behavior. Pixel/gesture
validation of every newly exposed drawing tool, content and collapsed-column
drawers, Navigator/Eyedropper transport, and Android document workflows are the
next port milestones. They are not claimed complete by this milestone.
