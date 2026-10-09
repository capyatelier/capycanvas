# Shared visual comparison tools

These tools compare a native client's pixels and geometry with the browser
reference at the same viewport, scale, theme, document and workspace.

| Tool | Purpose |
| --- | --- |
| `chrome-capture.mjs` | Captures the Web editor or a component fixture in headless Chrome. |
| `compare.py` | Full-image comparison of two PNGs. |
| `check_color_wheel.py` | Checks sampled color-wheel pixels against the shared Rust picker. |
| `check_icon_paints.py`, `check_control_colors.py` | Flat-fill checks for icon and control-color fixtures. |
| `tool_action_fixture.py` | Builds the tool-action fixture from an inventory. |
| `mac-capture.swift` | Captures the frontmost Mac editor window. |

Set up the Python environment once, and check the comparator after changing it:

```sh
python3 -m venv artifacts/ui/parity/python-env
artifacts/ui/parity/python-env/bin/pip install -r tools/visual/requirements.txt
artifacts/ui/parity/python-env/bin/python -m unittest discover -s tools/visual
```

## Chrome captures

Build the web app, then capture at the native capture's logical size and scale:

```sh
bash apps/layer-web/build.sh
node tools/visual/chrome-capture.mjs 1376 1032 2 artifacts/ui/parity light
```

Arguments are logical width, height, pixel scale, output directory, theme
(`light` or `dark`), scenario (default `initial`) and, for some scenarios, a
fixture or manifest JSON. Chrome runs headless with a temporary profile and a
localhost-only server. `CAPY_CHROME` selects the executable; the default is the
macOS Chrome path. WebGPU must use a hardware adapter.

Full-editor scenarios:

| Scenario | State |
| --- | --- |
| `initial` | Default workspace, fitted document. |
| `sketch`, `photo`, `paint-expanded` | That default workspace; Paint opens its right stack. |
| `canvas-under-header`, `paint-canvas-under-header` | Four Zoom In steps after fitting, so paper extends behind the header and exposes an opaque header. |
| `layer-added` | One new empty layer selected above the original layers. |
| `filter-properties` | Gaussian Blur radius 5, identity Curves and default Gradient Map, with Properties open. |
| `panel-configuration` | Brush size configuration with every control shown. |
| `partial-zen` | Zen toggled on the default workspace. |
| `windows-editor` | Viewports, scales and caption insets from a Windows fixture manifest. |

A geometry JSON as the final argument of a full-editor scenario reserves the
native window-control clearance, for example the one attached by the Apple
`EditorLaunchTests/testEditorControlLayout` workflow. The component scenarios
below take a native manifest instead.

Captures wait for staged GPU startup, workspace ownership, fonts, images,
visible layer-thumbnail pixels and a stable layout and camera; a fixed delay is
not enough. They reject application error or status messages. The native side
must use the same theme, document, workspace and camera. On Apple debug builds,
`CAPY_INITIAL_ACTIONS` sets up the matching state, for example
`[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"zen_mode"}]`
for `partial-zen`.

## Comparing images

```sh
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/ui/parity/web-light-1376x1032@2x.png \
  artifacts/ui/parity/native-ipad-light.png --output artifacts/ui/parity/comparison
```

The comparator honors declared orientation, converts embedded ICC profiles to
sRGB (untagged images are reported as assumed sRGB), requires opaque inputs of
equal pixel size, and compares every pixel. It writes the raw difference, an
amplified heatmap, a 50% overlay and JSON statistics, and exits with status 1 on
failure. It never rescales, crops or masks.

- **Acceptance is exact equality.** Tolerance options only make exploratory
  reports reproducible. Geometry bounds and flat-fill checks never waive the
  full-image comparison, and a failing exact comparison is never reported as
  parity.
- **Apple visual acceptance is perceptual.** At normal viewing size,
  imperceptible differences are acceptable. Fix visible mismatches and simple
  refinements; do not add complexity only to reduce pixel error.
- **Rotation only for a known raster orientation.** `simctl io screenshot` can
  keep a portrait raster for a landscape editor without an orientation tag; pass
  `--candidate-rotation 90`, `180` or `270` for that case only, never to hide a
  layout difference.
- **Inspect each pair first.** A permission dialog, tooltip or other window over
  the editor makes the capture invalid. Keep captures and reports in ignored
  `artifacts/`; they can hold device metadata.
- Keep each platform's pair separate: an iPad comparison says nothing about Mac.

## Full editor captures

- **GTK store images.** The [native capture helper](../../docs/development/store-screenshots.md)
  reads website-owned scene recipes and captures a real window in an isolated
  Wayland session, including the GPU canvas, native shadow and transparency.
  It supports both themes and every registered app language.

- **Mac.** Open the built app and capture its frontmost editor window, including
  the composited Metal canvas. It needs Screen Recording permission. Use the
  printed logical size and the app's theme for the Chrome capture. Mac menus live
  in the OS menu bar; keep that difference in the report.

  ```sh
  DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
    xcrun swift tools/visual/mac-capture.swift artifacts/ui/parity/mac
  ```

- **iPad and Mac tests.** `EditorLaunchTests/testCompleteEditorCapture` attaches
  `complete-editor-initial` and its geometry from a fresh workspace library,
  after the canvas and Navigator are live. Compare it with a Chrome `initial`
  capture at the same size. Debug-only `CAPY_CAPTURE_PROBE` makes the fixture
  wait for visible layer thumbnails.
- **Windows.** Capture with the native `capture-editor.ps1` fixture from the
  [Windows host notes](../../docs/development/windows.md#matched-editor-captures),
  then pass its manifest to the `windows-editor` scenario:

  ~~~powershell
  node tools/visual/chrome-capture.mjs 960 660 1.5 artifacts/windows/parity/web light windows-editor artifacts/windows/parity/native/fixtures.json
  ~~~

  Chrome reserves the measured caption area and runs the real camera commands.
  The manifest records the native effective palette; Chrome supplies its accent
  through the shared `system_theme_changed` action and verifies the result.
  Tool Set bounds describe the visible control after viewport and ancestor
  clipping, with full DOM bounds retained as `unclipped_bounds`; wholly clipped
  controls are excluded from that geometry inventory. Layers identifies its
  current semantic blend button. Workspace buttons use their shared workspace
  IDs, with the options button identified separately. Tool Set position, size
  and edges retain the one-physical-pixel rounding bound. Preserve prior
  references and exact comparison failures: these geometry checks never waive
  the full-image comparison or qualify a pixel failure as parity.

Use direct editor actions and canvas-output checks for behavior; reserve UI
automation for targeted input and lifecycle regressions.

## Component fixtures

Each fixture renders production native controls in an invisible host from a
current Rust model and writes a manifest; Chrome renders the production browser
controls from the same manifest. Compare every manifest name, both presets and
both themes, with `compare.py`. The Apple fixtures run through
`apps/layer-apple/scripts/test-project-files.sh` with `CAPY_TEST_ASSETS_APP` set to
a built Mac app, whose compiled assets they load. Component captures do not
cover UIKit touch routing, full-editor pixels or performance; the named
`EditorLaunchTests` workflows cover the native actions.

## Numeric editor controls

Covers both Apple presets and themes, three panel widths, slider endpoints and
intermediate values, spin fields with units and disabled states, and drives the
AppKit text-field delegates through real editor actions.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/NumbersMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_NUMBER_CAPTURES="$PWD/artifacts/apple-numbers" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/number-controls.swift
node tools/visual/chrome-capture.mjs 734 652 2 artifacts/apple-numbers light number-controls \
  artifacts/apple-numbers/native-0-light.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-numbers/web-0-light.png artifacts/apple-numbers/native-0-light.png \
  --output artifacts/apple-numbers/diff-0-light
```

Repeat for preset `1` and theme `dark`. Geometry reports keep every position
and size error, missing or extra rectangles, and a one-point bound.

## Compact layer opacity

Compares the Apple layer-opacity wrapper with the browser `createNumberField`
in `inline: true` mode across presets, themes, three widths, values 0/50/100 and
enabled or disabled state. `EditorLaunchTests/testInlineLayerOpacity` covers
entry, correction, Undo/Redo and layer switching in the editor.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/ColorMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_INLINE_CAPTURES="$PWD/artifacts/apple-inline-numbers" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/inline-number-controls.swift
node tools/visual/chrome-capture.mjs 116 36 2 artifacts/apple-inline-numbers light inline-numbers \
  artifacts/apple-inline-numbers/fixtures.json
```

## Property and layer choices

Covers the shared dropdown for property choices, the Curves channel selector
and compact layer blending, with real Rust blend names, short and long
selections and disabled states. Chrome uses standard selects with the
production CSS. `EditorLaunchTests/testBlendChoices` covers the real popovers.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/ColorMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_CHOICE_CAPTURES="$PWD/artifacts/apple-choices" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/choice-controls.swift
node tools/visual/chrome-capture.mjs 226 140 2 artifacts/apple-choices light choices \
  artifacts/apple-choices/fixtures.json
```

`choice-geometry.json` summarizes the measured rectangles; each
`geometry-NAME.json` keeps bounds, font metrics and every error.

## Shared icon paints

Apple generates ordered vector paints from the canonical browser SVGs: fixed
fills keep their colors, `currentColor` follows the native foreground, and mixed
paints composite as a group before disabled opacity. The generator rejects
mixed groups or effects it cannot reproduce. The fixture draws every compiled
icon at 16/24/32 points, in two palettes and normal, accent and disabled states,
and checks that model keys and SVG file stems resolve to the same asset.

```sh
python3 apps/layer-apple/tests/test_icon_assets.py
python3 apps/layer-apple/scripts/prepare.py
# Build the Mac app after preparing assets, then:
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/ColorMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_ICON_SOURCES="$PWD/apps/layer-web/icons" \
CAPY_ICON_CAPTURES="$PWD/artifacts/apple-icons" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/icon-capture.swift
node tools/visual/chrome-capture.mjs 576 672 2 artifacts/apple-icons light icons \
  artifacts/apple-icons/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/check_icon_paints.py \
  artifacts/apple-icons --output artifacts/apple-icons/paint-check.json
```

For UIKit, build the iPad target for Simulator and boot one iPad simulator. The
runner creates a disposable capture app, copies out its grids and uninstalls it;
pass `--simulator UUID` when several are booted and a new `--output` directory.

```sh
python3 apps/layer-apple/scripts/capture-icons-ios.py \
  --assets-app PATH_TO_BUILT_SIMULATOR_APP \
  --fixtures artifacts/apple-icons/fixtures.json --output artifacts/apple-icons-uikit
```

Then run the same Chrome capture and paint check on `artifacts/apple-icons-uikit`.

## Complete color panels

Renders the shared Apple color panel and the browser's compact controls from the
same Rust model, layout, guide stops, field pixels and readout, for Okhsv circle,
HSV square and HLS triangle, shape and RGB readouts, three paint slots and three
widths. Each case measures 12 rectangles.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/CompactColorMac/Build/Products/Debug/CapyCanvas-Mac.app" \
CAPY_COLOR_CAPTURES="$PWD/artifacts/apple-color-panel" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/color-panel-capture.swift
node tools/visual/chrome-capture.mjs 226 226 2 artifacts/apple-color-panel light color-panel \
  artifacts/apple-color-panel/fixtures.json
```

`tests/color-panel-interactions.swift` captures resting, hover and held-press
states; pass its manifest to the same Chrome command. Windows' Color fixture
feeds the same renderer a grid generated by
`cargo run -p layer-ui --example compact_color_fixture -- OUTPUT SCALE`.

Each case also writes `oracle-NAME.json`. Check native and browser images against
the shared Rust picker, which classifies samples and computes expected colors:

```sh
cargo build -p layer-ui --example color_wheel_reference
artifacts/ui/parity/python-env/bin/python tools/visual/check_color_wheel.py \
  artifacts/apple-color-panel/native-0-light-circle-shape-foreground-160.png \
  artifacts/apple-color-panel/oracle-0-light-circle-shape-foreground-160.json \
  --output artifacts/apple-color-panel/native-oracle-0-light-circle-shape-foreground-160.json
```

The check samples every 11 physical pixels away from antialiased boundaries and
markers, allows two levels per 8-bit channel including alpha, and needs at least
20 ring and 20 field samples. A fixture's optional `hue` keeps the remembered hue
of neutral paint. On Windows pass the built `color_wheel_reference.exe` with
`--oracle`. Keep both this check and the full-image comparison: frame checks
alone miss wheel painting drift. The Apple host tests are
`cargo test -p layer-apple compact_color`, `cargo test -p layer-apple hls_raster`
and `tests/color-field-cache.swift`; the native `testColorControls` workflow
exposes accepted Rust state through Debug-only `CAPY_COLOR_PROBE`.

## Complete header components

Renders the shared Apple header in AppKit with a temporary workspace library and
the three default workspaces, at two widths, both themes, all three title-bar
sizes and paper or surround backgrounds, with deterministic clock and battery
inputs. Chrome switches the real workspaces and records the resolved fonts and
advances beside each measured control.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/WorkspaceMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_HEADER_CAPTURES="$PWD/artifacts/apple-headers" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/header-controls.swift
node tools/visual/chrome-capture.mjs 1200 870 2 artifacts/apple-headers light header-controls \
  artifacts/apple-headers/fixtures.json
```

The Mac reference reserves window-control space and has no in-app menus. The
Web `test.mjs --headless --header-controls` journey separately checks menu
reachability, focus and resizing.

## Workspace tabs

Runs the shared SwiftUI tab headers and Rust owner in AppKit for each preset,
theme and docked or drawer presentation: clipped widths, frozen input
rectangles, reversal, cancellation, Undo/Redo and icon-only sizing. Chrome
restores the same workspace and drags with real pointer events.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/PerformanceMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_TAB_CAPTURE_DIRECTORY="$PWD/artifacts/apple-tabs" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-tabs.swift
node tools/visual/chrome-capture.mjs 1200 870 2 artifacts/apple-tabs light workspace-tabs \
  artifacts/apple-tabs/native-0-docked-light-before.json
```

Use preset `1` for Mac, `drawer` for collapsed-column tabs and `dark` for the
second theme, and pair both `before` and `drag` images. `testDrawerDragAndDock`
covers native gestures.

## Toolbar components

Renders the shared SwiftUI toolbar buttons from real Rust panel views, without
an editor window; rebuild the app when shared icons change. Rows cover small,
medium, large and labeled tiles with selected and disabled commands, a brush
preset, color, opacity, size and a wrapping shortcut label.

```sh
mkdir -p artifacts/ui/toolbar-parity
cargo run -p layer-host --example toolbar-fixture -- mac light \
  > artifacts/ui/toolbar-parity/mac-light.json
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  bash apps/layer-apple/scripts/capture-toolbar.sh \
  artifacts/ui/toolbar-parity/mac-light.json \
  artifacts/ui/toolbar-parity/native-light-toolbar-tiles.png \
  PATH_TO_BUILT_MAC_APP
node tools/visual/chrome-capture.mjs 780 324 2 artifacts/ui/toolbar-parity \
  light toolbar-tiles artifacts/ui/toolbar-parity/mac-light.json
```

The generator also accepts `ios` and `web`. The browser side needs no Wasm build.
`EditorLaunchTests/testToolbarStylesAndActions` covers the editor actions.

## Editor control colors

Captures `IconTile` and `ToolbarTileButton` in every enabled and selected
combination under the system, red and green accents, without changing system
settings. Chrome uses the real Navigator-button and toolbar factories.

```sh
mkdir -p artifacts/ui/control-colors
cargo run -p layer-host --example toolbar-fixture -- mac light \
  > artifacts/ui/control-colors/light.json
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  CAPY_COMPONENT_CAPTURE_SOURCE=apps/layer-apple/tests/control-colors.swift \
  bash apps/layer-apple/scripts/capture-toolbar.sh \
  artifacts/ui/control-colors/light.json \
  artifacts/ui/control-colors/native-light.png PATH_TO_BUILT_MAC_APP
node tools/visual/chrome-capture.mjs 168 258 2 artifacts/ui/control-colors \
  light control-colors artifacts/ui/control-colors/native-light.json
artifacts/ui/parity/python-env/bin/python tools/visual/check_control_colors.py \
  artifacts/ui/control-colors/native-light.json \
  artifacts/ui/control-colors/web-light-control-colors.png \
  artifacts/ui/control-colors/native-light.png
```

The checker requires identical rows across accents and allows one RGB level on
flat fills only, for 8-bit alpha rounding; glyphs, edges and the full image get
no tolerance.

## Tool-action buttons

Compares the SwiftUI `ToolActionControl` with the browser `toolSettings` factory
for all shipped actions in four enabled and selected combinations. The generator
requires matching labels, icons and checkability on both Apple presets.

```sh
mkdir -p artifacts/ui/tool-actions
cargo run -p layer-host --example inventory -- --gpu \
  > artifacts/ui/tool-actions/inventory.json
cargo run -p layer-host --example toolbar-fixture -- mac light \
  > artifacts/ui/tool-actions/theme-light.json
python3 tools/visual/tool_action_fixture.py \
  artifacts/ui/tool-actions/inventory.json artifacts/ui/tool-actions/theme-light.json \
  --column-width 120 > artifacts/ui/tool-actions/light-120.json
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  CAPY_COMPONENT_CAPTURE_SOURCE=apps/layer-apple/tests/tool-action-capture.swift \
  bash apps/layer-apple/scripts/capture-toolbar.sh \
  artifacts/ui/tool-actions/light-120.json \
  artifacts/ui/tool-actions/native-light-120.png PATH_TO_BUILT_MAC_APP
node tools/visual/chrome-capture.mjs 516 420 2 artifacts/ui/tool-actions \
  light tool-actions artifacts/ui/tool-actions/light-120.json
```

Repeat for `dark`, and for column width 226 with capture width 940. Compare the
sidecar `frames` of both hosts without rounding.
