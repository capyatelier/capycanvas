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
`CAPY_CHROME` overrides the default macOS Chrome executable path. WebGPU must use
a hardware adapter. Captures wait for staged GPU startup, fonts/images, visible
layer thumbnail pixels and layout. GPU attachment alone can precede the actual
preview readbacks; a fixed delay is insufficient for a settled reference.
The native scenario must use the same theme, document, workspace and camera.

For routine Mac visual work, open the built app and capture its frontmost editor
window directly. This captures the composited Metal canvas and native controls,
without clicking menus or running an XCTest session:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcrun swift tools/visual/mac-capture.swift artifacts/ui/parity/mac
```

Screen capture permission is required. Keep the editor frontmost and unobscured.
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

Compare a native screenshot exported from the Apple launch test:

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
only the first scenario in [the Apple acceptance matrix](../../docs/apple-acceptance.md).

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

Verify the comparator with:

```sh
artifacts/ui/parity/python-env/bin/python -m unittest discover -s tools/visual
```
