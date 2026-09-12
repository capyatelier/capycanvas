# Shared visual comparison tools

Build the web app, then capture at the native capture's logical dimensions and
pixel scale using a fresh, temporary Chrome profile and a localhost-only server:

```sh
bash apps/layer-web/build.sh
node tools/visual/chrome-capture.mjs 1376 1032 2 artifacts/ui/parity light
```

Arguments are logical width, height, pixel scale, output directory and theme.
An optional final `layer-added` argument captures one new empty layer selected
above the original ink/paper layers, matching the iPad layer workflow's final
capture. The default scenario is `initial`.
`canvas-under-header` applies four shared zoom-in steps after fitting the default
document. Paper then extends behind the header, exposing opaque-header mistakes
that the initial gray surround cannot reveal. The native
`EditorLaunchTests/testEditorControlLayout` workflow attaches this scenario and
`initial` on both Apple targets, using the actual Navigator buttons.
`filter-properties` reproduces the final state of the shared Apple
`testFilterSearchPreviewAndProperties` workflow: Gaussian Blur radius 5, identity
Curves and default Gradient Map, with Properties open. The test attaches the full
native window/screen and logical viewport dimensions. Use those dimensions and
light theme for each platform's separate Chrome reference and full-image diff.
`panel-configuration` opens the Brush size configuration with every control
visible. On an isolated Apple debug launch, set `CAPY_INITIAL_ACTIONS` to
`[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"show_all_controls","panel":"sizes"}}]`.
Capture after the expansion settles, at the same viewport and scale. Native
control heights can differ; keep those differences in the full-image report.
`partial-zen` toggles Zen on the default workspace. The Apple debug actions are
`[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"zen_mode"}]`.
Both hosts now project shared edge toolbar sections. Older comparisons from
before the web implementation do not establish parity against the current preset.
`CAPY_CHROME` overrides the default macOS Chrome executable path. WebGPU must use
a hardware adapter. Captures wait for staged GPU startup, fonts/images, visible
layer thumbnail pixels and layout. GPU attachment alone can precede the actual
preview readbacks; a fixed delay is insufficient for a settled reference.
The native scenario must use the same theme, document, workspace and camera.

For Windows, use the native `capture-editor.ps1` fixture documented in
[the Windows host notes](../../apps/layer-windows/README.md#matched-editor-captures),
then pass its manifest to the `windows-editor` scenario:

~~~powershell
node tools/visual/chrome-capture.mjs 960 660 1.5 artifacts/windows/parity/web light windows-editor artifacts/windows/parity/native/fixtures.json
~~~

The manifest supplies each viewport, scale, theme, workspace and caption inset.
Chrome reserves the measured native caption area but measures its own controls and
runs the real shared camera commands. Reports retain camera/layout facts and full
screenshots; Tool Set position, size and edge comparisons have a one-physical-pixel
rounding bound. That component geometry check does not waive full-image differences.
The native manifest also records the complete XAML capture boundary and retains
the raw Windows client image, including the OS frame outside the app surface.

## Numeric editor controls

Capture the production SwiftUI number controls using current Rust catalog/tool
models and formatting, then compare the production browser controls at the same
dimensions. The fixture covers both Apple presets and themes, three panel widths,
slider endpoints/intermediate values, spin fields with units and disabled states.
It also drives the mounted AppKit text-field delegates through actual editor
actions: valid/invalid expressions, unit display, stepping, cancellation and an
external value update while a draft is unfinished.

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

Repeat Chrome/comparison for preset `1` and theme `dark`, matching the fixture
name and theme argument. Each pair contains 30 complete controls and 198 measured
rectangles. Geometry reports retain every position/size error, missing/extra
rectangles and a separate one-point bound. Measurements are disabled in ordinary
Apple editors. Browser text/fills come from the fixture's Rust numeric results;
this fixture does not duplicate numeric policy or test browser numeric actions.

After the numeric-control correction, all 792 compared rectangles fall within
one logical point; non-label control geometry is exact except fractional value
text widths. Full-image exact comparison still fails: light/dark differing-pixel
fractions are 4.862%/5.052%, with mean absolute channel errors 3.604/3.746 levels
and maxima 204/200. The previous captures differed at 14.762%/14.880% of pixels.
Retain text rasterization/baseline and disabled-text differences without masking
or tolerance waivers. These invisible AppKit captures and delegate checks cover
shared Apple components; physical UIKit widgets, pointer delivery, full-editor
pixels and sustained performance require separate evidence.

## Complete header components

This fast fixture renders the actual shared Apple header in an invisible AppKit
host, with a temporary workspace library and the three default task workspaces.
It captures both Apple presets at 744 and 1200 logical points, both themes,
clock/battery shown and hidden, and paper/surround backgrounds: 96 images.
The clock and battery use deterministic inputs through the real status view.
Optional geometry readers are disabled in ordinary editor views.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/WorkspaceMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_HEADER_CAPTURES="$PWD/artifacts/apple-headers" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/header-controls.swift
node tools/visual/chrome-capture.mjs 1200 870 2 artifacts/apple-headers light header-controls \
  artifacts/apple-headers/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-headers/web-0-painter-1200-light-clock-paper.png \
  artifacts/apple-headers/native-0-painter-1200-light-clock-paper.png \
  --output artifacts/apple-headers/diff-0-painter-1200-light-clock-paper
```

Compare every corresponding pair in the manifest with the same comparator.
Chrome switches the real workspace identities and captures the full 48-point header
at the native scale. Geometry reports retain position, size and edge errors
separately; an edge bound is not a waiver for a larger size error. Captures use
the actual menu, title, workspace, clock, battery, settings and Zen components.
The Mac reference explicitly reserves window-control space and removes in-app
menus. The browser fullscreen button is excluded to reflect Apple's different
capabilities; the iPad fullscreen action remains an open feature gate.

The deterministic background isolates header compositing, without Metal or
UIKit rasterization. This is component evidence, not whole-editor or physical
iPad pixel acceptance. Keep the complete raw differences, including fonts,
truncation and blending, without masks or resampling. The web's focused
`test.mjs --headless --header-controls` workflow separately checks narrow menu
reachability, real Zoom In, focus, resizing and workspace-pill geometry against
a running local web server.

## Workspace tabs

The direct Apple check runs the real shared SwiftUI headers and Rust owner in
an invisible AppKit host, once for each Apple preset, theme and docked/drawer
presentation. It checks natural clipped widths, frozen input rectangles,
halfway reversal, release between move events, Undo/Redo, cancellation followed
by a new gesture, and icon-only sizing. Every store injects disabled persistence;
the check does not depend on Debug-only environment overrides. Optional captures
use the built app's actual compiled vector assets in a temporary bundle:

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/PerformanceMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_TAB_CAPTURE_DIRECTORY="$PWD/artifacts/apple-tabs" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-tabs.swift
node tools/visual/chrome-capture.mjs 1200 870 2 artifacts/apple-tabs light workspace-tabs \
  artifacts/apple-tabs/native-0-docked-light-before.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-tabs/web-0-docked-light-drag.png \
  artifacts/apple-tabs/native-0-docked-light-drag.png --output artifacts/apple-tabs/diff-0-docked-light-drag
```

Use preset `1` for Mac, `drawer` for collapsed-column tabs, and `dark` for the
second theme. Pair both `before` and `drag` images. The fixture restores exactly
the same workspace in Chrome and records each host's full natural tab rectangles,
visible strip, scale and preview. It deliberately resizes the column through an
editor action so the last tab clips in both hosts. This tests manually sized
strips; Apple automatic panel measurement/fitting remains a separate parity gap.
Chrome uses real pointer events and checks the committed layout. The native
fixture calls the shared gesture adapter directly; it does not establish UIKit
touch routing or hardware rendering. Use the focused `testDrawerDragAndDock`
workflow for native gesture coverage.

Each image contains the complete measured tab strip, including clipping,
backgrounds and the docking indicator. Differences are retained without masks or
resampling. An AppKit rendering of the iPad preset is shared-component evidence,
not an iPad hardware pixel baseline. All background, geometry and platform
font/rasterization differences remain visible; exact comparison failure must not
be described as passed editor parity.

## Full editor captures

For routine Mac visual work, open the built app and capture its frontmost editor
window directly. This captures the composited Metal canvas and native controls,
without clicking menus or running an XCTest session:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcrun swift tools/visual/mac-capture.swift artifacts/ui/parity/mac
```

Screen capture permission is required. Keep the editor frontmost and unobscured,
with native tooltips dismissed. The script identifies the frontmost editor by
bundle identifier and process, independent of its displayed application name.
The command captures the screen inside its exact window bounds, preserving system
corner pixels rather than producing transparent corners, and writes the PNG and
its measured logical/pixel dimensions. Use those dimensions and the app's theme for the Chrome
capture. Mac top-level menus intentionally live in the OS menu bar; keep the full
image diff and document this platform adaptation. Use direct editor action/state
and canvas-output checks for routine behavioral coverage. Reserve UI automation
for targeted app input/lifecycle regressions; trust macOS menu mechanics.
Inspect each pair before comparison: a macOS permission dialog or another window
covering the editor makes the fixture invalid and must not be counted as parity
evidence. Captures can include system corner backgrounds; keep artifacts local.

The focused Apple `EditorLaunchTests/testCompleteEditorCapture` test attaches
`complete-editor-initial` and `complete-editor-geometry-initial` on either target. It uses
a fresh light-theme workspace and waits for the canvas and live Navigator.
Compare its exported native screenshot with a matching Chrome `initial` capture:

```sh
python3 -m venv artifacts/ui/parity/python-env
artifacts/ui/parity/python-env/bin/pip install -r tools/visual/requirements.txt
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/ui/parity/web-light-1376x1032@2x.png \
  artifacts/ui/parity/native-ipad-light.png --output artifacts/ui/parity/comparison
```

Use the same commands with a native Mac content capture and its matching logical
dimensions/scale. Keep each platform's native/reference pair and results separate;
an iPad comparison does not establish Mac parity. The initial launch fixture is
only the first scenario in [the Apple acceptance matrix](../../docs/history/apple-acceptance.md).

The comparator honors declared image orientation without resampling, normalizes
embedded ICC profiles to sRGB, checks matching pixel dimensions, compares every
pixel, and saves the raw difference, amplified red
heatmap, 50% overlay and JSON statistics. It never rescales, crops or masks.
Untagged captures are explicitly reported as assumed sRGB. Both inputs must be
opaque. A failing comparison exits with status 1 while retaining all artifacts.

The default acceptance is exact equality. Optional tolerance arguments make
exploratory reports reproducible; broad thresholds do not establish editor parity.
Document any narrowly justified text/shadow/system-control accommodation and
retain full-image error reports. Keep screenshots and reports in ignored artifacts,
especially captures or test bundles that may contain personal device metadata.

Raw `simctl io screenshot` images may retain the device's portrait raster while
the editor is landscape, without an EXIF orientation tag. Inspect the original
capture and use `--candidate-rotation 90` (counterclockwise), `180` or `270` only
to correct that known orientation. The original PNG stays untouched; the report
records the explicit rotation and every pixel is compared without interpolation,
cropping or resizing. Do not use rotation to compensate for mismatched layouts.

Verify the comparator with:

```sh
artifacts/ui/parity/python-env/bin/python -m unittest discover -s tools/visual
```

The native `testColorControls` fixture attaches each full HSV/HLS capture and a
JSON file containing its logical viewport, wheel bounds and selected color.
Check sampled interior colors against the shared Rust picker with:

```sh
cargo build -p layer-ui --example color_wheel_reference
artifacts/ui/parity/python-env/bin/python tools/visual/check_color_wheel.py \
  artifacts/color-hsv.png artifacts/color-hsv-geometry.json \
  --output artifacts/color-hsv-report.json
```

This color check preserves the source, honors EXIF/ICC data and rejects mismatched
viewport geometry. Rust classifies samples and computes their expected colors;
the Python tool does not duplicate color conversion or wheel hit policy. It
samples every 11 physical pixels, excluding antialiased boundaries (a two-pixel
neighborhood must remain in the same region) and markers (seven logical points).
The default limit is two levels per 8-bit channel, including opacity. Both the
ring and field must have at least 20 samples. The report retains every count and
the worst failures. This checks sampled color correctness only; it does not
replace full-image Chrome comparisons or establish editor visual parity. The
Chrome `initial` fixture includes the default HSV wheel; a matching dedicated
HLS interaction scenario is not currently provided.

A fixture may include `hue` alongside `rgba` to preserve the remembered hue of
black, white or gray paint. The oracle applies that hue through shared color
policy; it is not inferred from the screenshot. Existing fixtures may omit it.
On Windows, pass the built `color_wheel_reference.exe` with `--oracle`.
The comparator's test doubles use `--oracle-interpreter` so their tests run
without relying on POSIX executable-script behavior.

## Toolbar components

For fast Apple/browser component comparisons, generate actual Rust panel views
and render the shared SwiftUI buttons directly. The temporary capture bundle
uses `Assets.car` from a built Mac app; it opens no editor window and does not
run XCTest. Use a freshly built app when shared icons change.

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
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/ui/toolbar-parity/web-light-toolbar-tiles.png \
  artifacts/ui/toolbar-parity/native-light-toolbar-tiles.png \
  --output artifacts/ui/toolbar-parity/light-comparison
```

Repeat with `dark` and matching filenames. The generator also accepts `ios` and
`web` to inspect each host's projected fields. Rows are small, medium, large,
medium labeled and large labeled; each includes selected/disabled commands,
a brush preset, dynamic color, opacity, size and a wrapping shortcut label.
The browser uses its real toolbar factory, CSS and SVG assets and checks tile,
icon and label-column geometry. It needs no Wasm build or GPU drawing session
in this mode. The native renderer uses the same SwiftUI button/content types as
the editor, including disabled/selected rendering. All fixture pixels are compared;
there is no cropping, masking or global tolerance waiver.

These fixtures omit surrounding editor chrome, panel shadows, tooltips and
interaction. Mac component output does not prove physical iPad rasterization,
whole-window parity, drawer transitions, hit testing or performance. Keep exact
diff failures and validate native editor actions separately with
`EditorLaunchTests/testToolbarStylesAndActions` on each Apple target.

## Editor control colors

The following fixture captures actual `IconTile` and `ToolbarTileButton` views
for all enabled/selected combinations, including selected+disabled. Each row
repeats under the system, red and green accent environments while retaining the
editor's ordinary tint. This changes no system preference and opens no window.
Chrome uses its real Navigator-button and toolbar factories, CSS and SVG assets
with the same state combinations and placement. The fixture does not attach a
Navigator GPU surface or require a Wasm build.

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
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/ui/control-colors/web-light-control-colors.png \
  artifacts/ui/control-colors/native-light.png \
  --output artifacts/ui/control-colors/light-comparison
```

Repeat with `dark` and matching filenames. The focused checker requires identical
complete rows across native accent variants and compares flat fill samples with
Chrome. It permits one RGB level only for those samples, accounting for 8-bit
alpha composition rounding; it does not accept glyphs, rounded edges or the full
image. Keep the separate full-pixel comparison at its default zero tolerance.

The corrected light fixture has a one-level red-channel fill difference from
Chrome; the dark flat fills match exactly. Full raw comparisons still fail:
35.517% of light pixels and 3.571% of dark pixels differ, with mean absolute
channel errors of 0.354 and 0.236 levels respectively. The large light fraction
includes the one-level fill difference. Glyph/edge rasterization differences
remain. These AppKit component captures do not establish UIKit rendering,
pointer/keyboard interaction, full-editor parity or hardware performance.

## Tool-action buttons

The tool-action fixture compares the production SwiftUI `ToolActionControl` and
browser `toolSettings` factory. It contains all six shipped actions in four
enabled/selected combinations, at 120- and 226-point column widths. Generate
the action list from a complete `apple-inventory --gpu` result; the generator
requires matching action labels and checkability on both Apple presets. Theme
colors and font size come from the shared `toolbar-fixture` example.

```sh
mkdir -p artifacts/ui/tool-actions
cargo run -p layer-host --example apple-inventory -- --gpu \
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
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/ui/tool-actions/web-light-120.png \
  artifacts/ui/tool-actions/native-light-120.png \
  --output artifacts/ui/tool-actions/light-120-comparison
```

Repeat for `dark`, and for column width 226 with total capture width 940.
Height is 420 and scale is 2 in every case. Native and browser sidecar JSON
files expose each control's measured `frames` keyed by column/action index;
compare their `x`, `y`, `width` and `height` without rounding. The browser also
asserts labels, selected/disabled states, full column width and disabled opacity.
The native host never displays a window and closes its surface after capture.
It uses an explicitly active control environment without changing OS settings.

All 96 measured native bounds match Chrome exactly. Greedy wrapping retains
the browser's line breaks, including “Show rulers” on one line and “Snap to /
rulers” on two lines in the narrow case. Full raw sRGB comparisons still fail
at zero tolerance; no pixels are cropped, masked or excluded:

| Theme / column width | Mean absolute channel error | Exact differing pixels | Maximum channel error |
| --- | ---: | ---: | ---: |
| Light / 120 | 0.947 | 12.810% | 152 |
| Light / 226 | 0.365 | 8.202% | 106 |
| Dark / 120 | 0.994 | 13.028% | 173 |
| Dark / 226 | 0.409 | 8.349% | 128 |

Before the control change, the corresponding mean errors were 13.581, 7.679,
13.599 and 7.571. Residual glyph, edge and fill differences remain visible in
the full comparison reports; this is not a full pixel acceptance pass. These
AppKit component captures do not establish native UIKit appearance, activation,
accessibility interaction, complete editor parity or hardware performance.
The existing Apple ABI tool-panel and transform tests separately exercise
ruler settings, transform Apply/Cancel and exact pixel Undo/Redo through the
shared editor. They do not simulate a SwiftUI button click.
