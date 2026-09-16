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
that the initial gray surround cannot reveal.
`paint-expanded` switches to the current Paint default with its right stack
open; `paint-canvas-under-header` then applies the four zoom steps.
The native `EditorLaunchTests/testEditorControlLayout` workflow attaches the last
two scenarios on both Apple targets, using View → Fit canvas and the Navigator
buttons. Pass its geometry JSON as the final argument to reproduce the native
window-control clearance through the shared layout model:

```sh
node tools/visual/chrome-capture.mjs 1376 1032 2 artifacts/ui/paint light paint-expanded native-geometry.json
```

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
Full-editor capture also waits for workspace ownership before dispatching setup
actions, then checks stable layout/camera measurements and rejects application
error/status messages. An earlier reference dispatched while ownership was still
loading and displayed a recovery message; it is retained locally as an invalid
fixture, not a parity result.

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

## Compact layer opacity

Both Apple targets use the shared numeric control for layer opacity. Compact
mode reserves width from the formatted range, shows a readout until editing,
and uses the same expression parser, slider mapping, pending-edit handling and
error feedback as other numeric fields. Changing the document or layer retires
the old editor state; delayed callbacks check their original target before
submitting an edit. Range formatting runs on mount, rather than every slider
update. The ordinary slider/spin presentation remains unchanged.

Capture the production layer-opacity wrapper with actual Rust models/actions,
then the production browser `createNumberField` with `inline: true`:

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/ColorMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_INLINE_CAPTURES="$PWD/artifacts/apple-inline-numbers" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/inline-number-controls.swift
node tools/visual/chrome-capture.mjs 116 36 2 artifacts/apple-inline-numbers light inline-numbers \
  artifacts/apple-inline-numbers/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-inline-numbers/web-0-light-226-100-enabled.png \
  artifacts/apple-inline-numbers/native-0-light-226-100-enabled.png \
  --output artifacts/apple-inline-numbers/diff-0-light-226-100-enabled
```

Repeat complete comparison for every manifest name. The 72 cases cover both
Apple presets, both themes, three panel widths, values 0/50/100 and enabled or
disabled state. The native host is invisible AppKit; UIKit and physical device
coverage remains separate. `inline-number-geometry.json` retains every bound,
error and missing/extra control; per-case reports also record the browser's
measurement span and entry width. All 216 complete readout/track/root rectangles
are within one point, maximum 0.40625. Native entry geometry and other font sizes
are not covered by these idle-control captures.

Complete comparisons retain 1,251,072 pixels: 49,352 differ exactly, with
per-case fractions 0.873–7.338%, weighted mean channel error 0.986982 and maximum
191. The previous implementation differed at 92,542 pixels with mean error
4.201418. Exact pixel parity still fails; differences remain unmasked.

The existing standard numeric fixture also passes both sets of AppKit delegate
checks. Its four complete images and all 792 geometry measurements are unchanged.
`EditorLaunchTests/testInlineLayerOpacity` exercises expression entry, invalid
input/correction, independent Properties readback, Undo/Redo, keyboard dismissal,
layer switching and the slider through the actual editor. UIKit accessibility
reports the readout's glyph bounds, so those bounds are not a substitute for
the complete SwiftUI layout measurements above.
The final iPad Simulator workflow passes; the Mac test compiles but was not
executed. Its full initial editor comparison differs at 302,151 of 5,680,128
pixels (5.319440%), so full-editor pixel acceptance remains open.

## Property and layer choices

Both Apple targets share the same dropdown component for property choices,
the full-width Curves channel selector and compact layer blending. It measures
all option labels, shrinks property rows with the browser's flex policy, and
matches the select's padding, disclosure, disabled opacity and text clipping.
The native popover dispatches the existing shared editor actions.

The direct fixture uses actual Rust blend names and both Apple presets, two
themes, three widths, short/long selections and enabled/disabled states: 48
images containing three complete controls each. These are invisible AppKit
captures. Chrome uses standard HTML selects with the production property/layer
CSS; it does not test browser popup routing or duplicate the editor model.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/ColorMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_CHOICE_CAPTURES="$PWD/artifacts/apple-choices" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/choice-controls.swift
node tools/visual/chrome-capture.mjs 226 140 2 artifacts/apple-choices light choices \
  artifacts/apple-choices/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-choices/web-0-light-226-0-enabled.png \
  artifacts/apple-choices/native-0-light-226-0-enabled.png \
  --output artifacts/apple-choices/diff-0-light-226-0-enabled
```

Repeat full-image comparison for every manifest name. `choice-geometry.json`
summarizes all three measured control rectangles per case; each
`geometry-NAME.json` also retains bounds, font metrics and every geometry error.
Native text/arrow allocations are retained in the manifest for diagnosis.
Measurement readers are disabled in ordinary editors.

All 144 control rectangles are within one logical point, with maximum error
0.126. Complete comparisons retain 6,325,760 pixels: 196,638 differ exactly,
per-case fractions are 1.706–5.014%, weighted mean channel error is 0.439592
and maximum is 191. The preceding implementation differed at 1,526,184 pixels
with mean error 5.104690. Text rasterization and truncated-label differences
remain unmasked; this default-font fixture does not establish all font-size,
popup or physical-device parity.

`EditorLaunchTests/testBlendChoices` on both Apple targets tests the actual
property and compact popovers, synchronized blend values, Undo and Redo using
in-app controls. The iPad Simulator test passes; the Mac test target compiles
but has not run at this checkpoint. The attached complete initial editor
comparison differs at 302,519 of 5,680,128 pixels (5.325919%), so full-editor
pixel acceptance remains open.

## Shared icon paints

Apple generates ordered vector paints from the canonical browser SVGs. Fixed
fills remain original colors; `currentColor` paints follow the native foreground.
Ordinary symbolic icons remain one image. Mixed paints retain drawing order and
composite as a group before disabled opacity. The generator rejects unsupported
mixed groups/effects rather than silently changing their compositing semantics.

The direct fixture captures all canonical compiled icons at 16/24/32 points, two explicit
foreground/background palettes, and normal/accent/disabled states: 18 complete
grids (157 icons and 2,826 glyphs per host in the current bank). It checks that
bare model keys and SVG filename stems resolve to the same asset, including
hyphenated names such as `add-layer`. The Mac fixture uses AppKit. A separate
UIKit simulator runner below renders the same manifest with `SharedIcon` in a
real `UIHostingController`; neither fixture establishes physical-device pixels.

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
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-icons/web-light-32-normal.png artifacts/apple-icons/native-light-32-normal.png \
  --output artifacts/apple-icons/diff-light-32-normal
```

For UIKit, build the iPad target for Simulator and boot one iPad simulator.
The runner accepts only a simulator asset bundle, creates a disposable capture
app, copies out its own grids and uninstalls it. It uses no XCTest or editor
storage. Select `--simulator UUID` if several iPads are booted; `--output` must
be a new local directory.

```sh
python3 apps/layer-apple/scripts/capture-icons-ios.py \
  --assets-app PATH_TO_BUILT_SIMULATOR_APP \
  --fixtures artifacts/apple-icons/fixtures.json --output artifacts/apple-icons-uikit
node tools/visual/chrome-capture.mjs 576 672 2 artifacts/apple-icons-uikit light icons \
  artifacts/apple-icons-uikit/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/check_icon_paints.py \
  artifacts/apple-icons-uikit --output artifacts/apple-icons-uikit/paint-check.json
```

Repeat complete comparison for every manifest name on each host. The current
157-icon matrix retains 27,869,184 pixels per host. AppKit's 216 flat-paint
samples pass with maximum channel error one; UIKit's 216 samples match exactly.
Full AppKit comparisons have mean channel error 0.145/255 and exact differing
fractions of 0.97–12.81%. UIKit has mean error 0.133/255 and fractions of
0.92–2.41%. Both have maximum channel error 107/255 and fail exact pixel parity;
flat-paint checks do not waive the remaining rasterization differences.

The full native editor fixture now waits for visible layer thumbnails using
opt-in Debug metadata (`CAPY_CAPTURE_PROBE`); an early GPU/Navigator readiness
signal alone can capture empty thumbnail placeholders and disabled commands.
Release ignores the probe. The settled iPad Simulator initial capture differs
from the preceding native image only at the corrected Color tab icon. Its full
Chrome comparison still differs at 303,396 of 5,680,128 pixels (5.341359%).

## Complete Color panels

The `color-panel` fixture renders the production shared Apple panel and the
production browser controls. Rust supplies the model, layout, guide stops,
field pixels and formatted readout. It covers both Apple presets, light/dark,
Okhsv circle / HSV square / HLS triangle, shape/RGB readouts, three selected
paint slots and 128/160/226-point widths: 216 complete panels per host.
The AppKit entry point needs a built Mac asset bundle and uses temporary native
windows. The same source also has a UIKit application entry point for an
independently signed, isolated component-capture app. Neither entry point reads
artist storage. Captures wait for stable pixels and require all measured bounds.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/CompactColorMac/Build/Products/Debug/CapyCanvas-Mac.app" \
CAPY_COLOR_CAPTURES="$PWD/artifacts/apple-color-panel" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/color-panel-capture.swift
node tools/visual/chrome-capture.mjs 226 226 2 artifacts/apple-color-panel light color-panel \
  artifacts/apple-color-panel/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-color-panel/web-0-light-circle-shape-foreground-226.png \
  artifacts/apple-color-panel/native-0-light-circle-shape-foreground-226.png \
  --output artifacts/apple-color-panel/diff-0-light-circle-shape-foreground-226
```

Schema 2 controls every browser viewport, scale, theme and state. The browser
fixture uses the real compact controls; it has no substitute numeric widgets.
Each case measures 12 rectangles, including all paint interiors and shape buttons.
Native measurements are disabled in ordinary editors. Repeat the full-image
comparison for every manifest name and retain differences without masks.

The same browser renderer accepts Windows' schema-2 grid of six `items`, generated
by `cargo run -p layer-ui --example compact_color_fixture -- OUTPUT SCALE` and
measured by its native Color fixture. Grid frame names use `ITEM/color-CONTROL`;
the native manifest supplies `frames`, the catalog and optional hover/focus state.
Field files resolve beside the input manifest, independently of the screenshot
output directory. The Apple adapter retains its required 216/18-case matrices.

Each case also writes `oracle-NAME.json` with the accepted Rust paint, remembered
hue, shape, wheel bounds and marker radius. Check both native and browser images
against the same independent picking oracle:

```sh
cargo build -p layer-ui --example color_wheel_reference
artifacts/ui/parity/python-env/bin/python tools/visual/check_color_wheel.py \
  artifacts/apple-color-panel/native-0-light-circle-shape-foreground-160.png \
  artifacts/apple-color-panel/oracle-0-light-circle-shape-foreground-160.json \
  --output artifacts/apple-color-panel/native-oracle-0-light-circle-shape-foreground-160.json
```

Geometry and interior color checks do not establish exact PNG identity. The
full-image reports retain native text, antialiasing and compositing differences.
Apple visual acceptance uses perceptual parity at normal viewing size: exact
identity is unnecessary for imperceptible differences. Fix visible mismatches
and simple refinements without adding complexity solely to reduce pixel error.
Component captures also do not exercise native touch/Pencil delivery, a full
editor or sustained drawing performance.

The Mac `tests/color-panel-interactions.swift` fixture uses the same asset and
output environment variables. It captures 18 resting, hover and held-press states
across both themes, and verifies that dragging a pressed button outside before
release leaves paint unchanged. Its pointer movement stays inside its owned
window's content; clicks are posted only to that window. Pass its manifest to the
same browser command above to compare the actual Web hover/active states.
`interaction_states: true` requires exactly 18 cases; the resting matrix still
requires 216. This fixture does not establish keyboard focus or iPad input.

The separate `testColorControls` workflow exercises native contacts and button
actions. Debug-only `CAPY_COLOR_PROBE` exposes the accepted Rust state on the
native wheel's accessibility value. Release builds keep the human color
readout. The workflow launches its own fresh namespace and initial actions.
Use the [installed-device test configuration](../../apps/layer-apple/README.md#validation)
for physical iPad runs; no separate prelaunch or attach mode is needed.

Apple fields use physical-pixel RGBA8 samples from shared Rust. A field image
changes only with hue, shape or size; the separate guide cache changes only
with shape or size. The guide interpolates the same adaptive sRGB stops as Web,
avoiding a full perceptual conversion per pixel during resizing. Fractional
native font advances and unquantized CoreText glyph positions keep the curved
readout's digit cells aligned. Ordinary native font smoothing stays enabled;
disabling it regresses some readouts despite correct advances.

The two rotated shape icons use `drawingGroup()` on macOS, improving all 216
complete-panel comparisons and the 18 interaction captures. The same modifier
slightly regresses UIKit, so its drawing path is retained. Repeated physical
UIKit captures verify the restored frames and pixels. Keep these host-specific
results separate from the unchanged interior-color tolerance and the remaining
full-image text, edge and compositing differences.
Wheel painting uses panel coordinates: a fractional child Canvas allocation can
round before drawing, even when the input view's measured bounds are correct.
The painted destination follows Web Canvas's rounded logical edges, and native
field/guide rasters use that destination's physical pixel size. Keep both the
full-image comparison and picking oracle; control-frame checks alone cannot
detect this rendering drift. Swatch paint is above its inset selection border.

```sh
cargo test -p layer-ui color::
cargo test -p layer-apple compact_color
cargo test -p layer-apple hls_raster
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/color-field-cache.swift
cargo build --release -p layer-ui --example color_field_benchmark
# Run after compilers, UI automation and GPU profilers have finished.
target/release/examples/color_field_benchmark
```

The benchmark covers all three fields and their guides at the matrix's physical
sizes. It excludes host allocation, upload and presentation, and measures CPU
cost rather than editor frame rate or input latency.

## Complete header components

This fast fixture renders the actual shared Apple header in an invisible AppKit
host, with a temporary workspace library and the three default task workspaces.
It captures both Apple presets at 744 and 1200 logical points, both themes,
all three title-bar sizes and paper/surround backgrounds: 144 images.
The schema-2 manifest records the projected model and shared item allocations.
The clock and battery use deterministic inputs through the real status view.
Optional geometry readers are disabled in ordinary editor views.

```sh
CAPY_TEST_ASSETS_APP="$PWD/apps/layer-apple/DerivedData/WorkspaceMac/Build/Products/Release/CapyCanvas-Mac.app" \
CAPY_HEADER_CAPTURES="$PWD/artifacts/apple-headers" \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/header-controls.swift
node tools/visual/chrome-capture.mjs 1200 870 2 artifacts/apple-headers light header-controls \
  artifacts/apple-headers/fixtures.json
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/apple-headers/web-0-sketch-1200-light-medium-paper.png \
  artifacts/apple-headers/native-0-sketch-1200-light-medium-paper.png \
  --output artifacts/apple-headers/diff-0-sketch-1200-light-medium-paper
```

Compare every corresponding pair in the manifest with the same comparator.
Chrome switches the real workspace identities and captures the full 48-, 60- or 72-point header
at the native scale. Geometry reports retain position, size and edge errors
separately; an edge bound is not a waiver for a larger size error. Captures use
the actual menu, title, workspace, clock, battery, settings and Zen components.
The Mac reference explicitly reserves window-control space and removes in-app
menus. The projected Apple model excludes the browser fullscreen tile. Deterministic
status inputs model an observed fullscreen scene; actual fullscreen transitions
remain covered by native tests. Web styling is preserved, and unmatched controls
and geometry differences are reported for inspection rather than masked. Current
GTK title-bar contrast and Web styling can differ; this capture alone does not
establish perceptual acceptance.

Chrome reports the resolved platform font and each workspace label's computed
CSS font and canvas advance alongside its measured control rectangle. Use those
records to distinguish font-family/weight differences from layout allocation.
Apple's shared header measures and draws medium labels at the CSS weight of 500
through the public font variation axis where supported. Font/width caches are
bounded; the native font remains the fallback when that axis is unavailable.

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
a fresh isolated production workspace library, verifies all three workspace
segments with Illustrator selected, and waits for the canvas and live Navigator.
The same setup is used by `testEditorControlLayout` and `testNumericToolControls`.
Older persistence-disabled captures omitted the workspace switcher and do not
establish complete current-header parity.
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

The current 13-inch iPad simulator pair uses a 1376×1032-point viewport at 2×.
Full raw differences are 5.818% of pixels initially and 6.367% after four Zoom In
steps place the canvas behind the header; mean absolute channel errors are
1.372/1.451 levels, with maxima 255. Exact comparison fails. Color controls,
icons/text and header/fullscreen differences remain visible, without masks or
rescaling. The associated UIKit workflows pass numeric expression/correction,
inactive-tab readback, brush-setting retention, layer blend and Undo checks.
These simulator results do not establish physical iPad or Mac acceptance.

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
the action list from a complete `inventory --gpu` result; the generator
requires matching action labels, icons and checkability on both Apple presets. Theme
colors and font size come from the shared `toolbar-fixture` example.

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
artifacts/ui/parity/python-env/bin/python tools/visual/compare.py \
  artifacts/ui/tool-actions/web-light-120.png \
  artifacts/ui/tool-actions/native-light-120.png \
  --output artifacts/ui/tool-actions/light-120-comparison
```

Repeat for `dark`, and for column width 226 with total capture width 940.
Height is 420 and scale is 2 in every case. Native and browser sidecar JSON
files expose each control's measured `frames` keyed by column/action index;
compare their `x`, `y`, `width` and `height` without rounding. The browser also
asserts labels, selected/disabled states, full column width, disabled opacity,
canonical icon identity, 16-point glyph bounds and the six-point icon/text gap.
The native host never displays a window and closes its surface after capture.
It uses an explicitly active control environment without changing OS settings.

All 96 measured native bounds match Chrome exactly with the published icons.
The controls retain the browser's leading icon, intrinsic text width and greedy
wrapping. At 120 points all six actions occupy two lines; at 226 they occupy one.
Full raw sRGB comparisons still fail
at zero tolerance; no pixels are cropped, masked or excluded:

| Theme / column width | Mean absolute channel error | Exact differing pixels | Maximum channel error |
| --- | ---: | ---: | ---: |
| Light / 120 | 0.812 | 14.460% | 191 |
| Light / 226 | 0.408 | 8.461% | 148 |
| Dark / 120 | 0.884 | 14.701% | 180 |
| Dark / 226 | 0.446 | 8.607% | 159 |

The current-source recapture on 2026-09-16 reproduces all eight retained native
and browser images byte for byte after RGBA decoding. Current shared palettes,
font size and all six command identities/checkability also match the fixtures.
All four pairs pass normal-size perceptual review: wrapping, icon placement,
selected fills and disabled appearance agree. Residual glyph/edge rasterization
differences remain in the raw reports; the zero-tolerance failures above are
unchanged. Evidence is `artifacts/apple-visual-closure-v1/`.

These AppKit component captures do not establish native UIKit appearance, activation,
accessibility interaction, complete editor parity or hardware performance.
The existing Apple ABI tool-panel and transform tests separately exercise
ruler settings, transform Apply/Cancel and exact pixel Undo/Redo through the
shared editor. They do not simulate a SwiftUI button click.
