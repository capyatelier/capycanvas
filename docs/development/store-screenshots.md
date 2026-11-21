# Native store screenshots

[Developer guide](README.md) · [Linux](linux.md) · [Releasing](releasing.md)

`capycanvas-web` owns showcase artwork, scene recipes, the generation script and
published images. This repository supplies native capture helpers. The GTK helper runs
locally on a hardware Vulkan GPU when refreshing the images; release runners do
not need to render or download them.

Store metadata generation, Windows CSV imports, Play listing uploads and Apple
preparation live beside the Flatpak orchestration in
[`capycanvas-web/site/scripts/store`](https://github.com/capyatelier/capycanvas-web/tree/main/site/scripts/store).
AppStream remains the shared source of translated listing text. Store overrides
live in that repository's `store/locales.json`. Keep native capture code here;
keep artwork and scene values in the website's showcase recipe.

## Android captures

Stage the native assets with the website's `native.mjs prepare` command. Reserve
the device under [Test devices](devices.md), then build both APKs with that owner's
application ID. The optional init script adds the staged artwork to the test APK:

```bash
CAPY_CAPTURE_ID=$(python3 tools/devices/devices.py appid)
apps/layer-android/gradlew -p apps/layer-android \
  -I "$PWD/tools/visual/android-store.init.gradle" \
  -PcapyStoreAssets=/path/to/staged-assets -PcapyAbi=arm64-v8a \
  -PcapyApplicationId="$CAPY_CAPTURE_ID" \
  :app:assembleDebug :app:assembleDebugAndroidTest
python3 tools/visual/android-store-capture.py \
  --device RESERVED_DEVICE --application-id "$CAPY_CAPTURE_ID" \
  --build-tools "$ANDROID_HOME/build-tools/37.0.0" \
  --apk apps/layer-android/app/build/outputs/apk/debug/app-debug.apk \
  --test-apk apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk \
  --kind tablet --languages ja,fr --output artifacts/store/android
```

`-PcapyStoreBuild=/path/to/owned-build` moves the Gradle output tree. The runner
checks both APK identities before installation and requires instrumentation's
`OK (1 test)` result. `AndroidStoreShowcaseTest` consumes the staged scene actions,
checks native previews, exact scopes and font coverage, and captures the native
display in both themes with private application storage. `--kind phone` uses
portrait orientation and the native Layers or Properties drawer. For tall phone
displays, `--size 1080x2160` sets a viewport within Play's 2:1 image limit. The
runner restores the original display override even when a capture fails. This
qualifies that viewport, not another physical device. Remove only your own test installs and release the device when finished.

## Windows captures

Build with `apps/layer-windows/scripts/build.ps1`, stage the same website recipe
and artwork on Windows, then run:

```powershell
./tools/visual/windows-store-capture.ps1 `
  -Recipe C:/capture-inputs/store-recipe.json `
  -Executable ./artifacts/windows/Release/CapyCanvas.exe `
  -Output ./artifacts/store/windows -Locales ja,fr
```

The coordinator calls `scripts/test-desktop.ps1` for each variant. Its native
adapter imports through the file picker, uses UI Automation and acknowledged
pointer input, and takes composed GPU captures. It supports the three showcase
scenes and their native framing; extending the showcase's brush preset requires
extending that adapter. Use the [Windows VM helper](windows-vm.md) from Linux,
with an owned hardware GPU VM. Store captures do not measure performance.

## Apple captures

Use the same staged inputs on an Apple Silicon Mac. Build both Apple targets with
the bundle ID from `tools/devices/devices.py appid`, then run the native editor
fixture with the built app's resources and Rust archive:

```sh
python3 tools/visual/apple-store-capture.py \
  --assets-app /path/to/CapyCanvas-Mac.app --inputs /path/to/staged-assets \
  --device mac --languages en,ja --output artifacts/store/apple/mac-run
```

For iPad use a signed device app, `--device ipad --physical RESERVED_DEVICE`
and `--signing-identity DEVELOPMENT_IDENTITY`; reserve it through the device tool
first. For iPhone use a simulator app and `--device iphone --simulator OWNED_UUID`.
The iOS app must target families `1,2`. The runner compiles the production editor
and Metal canvas with `tests/store-showcase.swift`, uses private storage, verifies
the artwork hash, awaits visible previews and exact scopes, checks native font
coverage and captures the composed native display. It rejects unsupported native
dimensions rather than resizing. Phone captures use the Layers drawer, or its
Properties page for Photo, and declare that layout's scope visibility.
Native PNGs stay in diagnostics; ColorSync encodes wide-color captures as sRGB
for the website catalog without changing their dimensions.

Outputs are `<mac|ipad|iphone>/<ui-language>/<scene>-<theme>.png` with native
JSON receipts. `binary.json` identifies the capture fixture separately from its
resource app; simulator receipts explicitly say `simulator: true`. Generate a
separate website `native.mjs catalog --platform apple` catalog for each fixture
binary and device family. `--build-only` prepares a fixture; `--fixture-app` reuses
one only when its Swift sources, Rust archive and recipe still match. Review every variant.
The website's Apple handoff still reports `readyForUpload: false`; upload and
submission are separate. Its metadata command can independently prepare or save
selected draft localizations without submitting for review.

Mac App Store delivery also needs an App Store distribution/archive export path
in [`release.sh`](../../apps/layer-apple/scripts/release.sh). Its current `mac`
path produces a Developer ID DMG, and its `publish` action accepts only `ipad`.
Keep that packaging work in this repository and qualify it on a Mac before
selecting processed production builds in App Store Connect.

## GTK captures

The helper starts one scene in a fresh private home, application storage, D-Bus
session and headless Mutter display. It imports through the native file-launch
path and applies shared editor actions. It waits for document admission, shader
startup, visible thumbnails, canvas presentation and GTK frames. Missing glyphs,
an unexpected window size, application errors, clipped Histogram or Waveform
content and clipped shadows fail the run.
Mutter captures the actual window and GPU subsurface. PNG output preserves its
native corners and shadow with transparent padding; no replacement corners or
background are drawn.

## Run a capture

Use the [Linux build prerequisites](linux.md#prerequisites), plus `bwrap`,
`dbus-run-session`, Mutter, GJS, PipeWire, WirePlumber and GStreamer with its
PipeWire, video conversion and app-sink plugins. GJS needs the GstApp, GstVideo
and GdkPixbuf introspection libraries. Install system fonts covering the app's
languages, including CJK and Thai. User font and settings directories are hidden
from the capture session.

From this checkout, obtain the language tags from the compiled app:

```bash
python3 tools/visual/gtk-store-capture.py --list-languages \
  --output artifacts/store-capture/capabilities
```

The command prints JSON with `languages` and `themes`. The default capture is
English, light theme, a 1200 × 800 logical window and a real 2× Wayland monitor.
Choose a recipe and variant explicitly:

```bash
python3 tools/visual/gtk-store-capture.py \
  --recipe /path/to/recipe.json --scene painting \
  --language en --theme dark --width 1200 --height 800 --scale 2 \
  --output artifacts/store-capture/painting-en-dark
```

Every output directory must be new or empty. The helper builds the release test
executable automatically. `--executable /path/to/layer_linux-test-binary` reuses
an existing build; use it only while its sources remain unchanged. The manifest
records the checkout revision, tracked dirty state, exact executable SHA-256 and
whether the executable was supplied. The checkout revision alone does not prove
the provenance of an executable supplied by the caller.

The website's generator enumerates the app's reported languages, both themes
and each scene, invoking the helper with a separate output directory for every
combination. Fresh storage prevents saved brushes, layouts and localized default
names from leaking between captures. Do not change the desktop user's settings
or use their running application to generate images.

## Recipe format

Recipes are JSON with a `scenes` array. `--scene` selects one entry and is optional
when there is only one. IDs are unique lowercase slugs; source paths are absolute
or relative to the recipe file. The included workspace IDs are `painter`
(Sketch), `illustrator` (Paint) and `photographer` (Photo).

```json
{
  "scenes": [{
    "id": "painting",
    "source": "drawing.capy",
    "workspace": "illustrator",
    "steps": [
      {"type": "action", "action": {"type": "invoke", "command": "fit_canvas"}},
      {"type": "show_panel", "panel": "color"}
    ]
  }]
}
```

The native fixture validates the following steps:

| Step | Fields and behavior |
| --- | --- |
| `action` | `action`: a serialized [`UiAction`](../../crates/layer-ui/src/lib.rs), dispatched by the real workspace. |
| `frame` | `zoom`: logical pixels per document pixel; `focus`: `[x, y]` in document coordinates, centered in the canvas viewport. |
| `select_layer` | `name`: the exact, unique authored layer name. |
| `layer_visibility` | `name`, `visible`: change that authored layer's visibility. |
| `show_panel` | `panel`: a shared panel ID; show it and select its tab. |
| `effect` | `effect`: shared effect ID; `parameters`: parameter keys mapped to serialized [`EffectValue`](../../crates/layer-core/src/effects.rs) values. Inserts and selects an adjustment, then applies its values. |
| `wait_histogram` | Wait for exact composite histogram data when the scene exposes it. |

For example, an effect step can use
`{"type":"effect","effect":"vibrance","parameters":{"vibrance":{"kind":"number","value":35}}}`.
Use the current shared action and effect definitions; do not reproduce editor
rules in the website script. Keep the canonical artwork and scene values in the
website repository, with adapters for its Web and GTK captures as needed.

## Outputs and publication

`capture.json` records the selected scene, language, theme, source and executable
provenance, logical window dimensions, scale, camera, document and font checks.
`images/` contains the lossless PNG and a JSON sidecar with pixel dimensions,
alpha counts and the crop bounds. `session/` holds the job, build and runtime
logs for diagnosis. Failed runs retain their evidence. Keep these local files
under `artifacts/`; publish the reviewed image files from the website repository.

Use stable public HTTPS URLs and replace the images when needed. Git retains
their history; separate versioned URLs are optional. Software centers cache
images, so replacements may take time to appear. Preserve alpha if optimizing or
converting the PNGs, and inspect the result at its intended display size.

AppStream metadata references the published images by URL. Use an unlocalized
English fallback and localized image variants with `xml:lang`. GNOME Software
filters images using desktop locales: map website tags `pt-BR`, `zh-Hans` and
`zh-Hant` to `pt_BR`, `zh_CN` and `zh_TW` in the XML attributes, while keeping
the canonical website URLs unchanged. Generic `pt` and `zh` aliases cover the
app's Portuguese and Simplified Chinese fallbacks; `zh_HK` and `zh_MO` reuse
Traditional Chinese images alongside `zh_TW`. Use `gnome` for light screenshot
environments and `gnome:dark` for dark. GNOME Software can show both theme
variants, so each should stand on its own. Captions describe the
feature shown. Add and validate the final metadata after the image URLs are live.
See the [AppStream screenshot specification](https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html#tag-screenshots)
and [Flathub screenshot guidance](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines#screenshots).

## Checks

```bash
python3 -m unittest discover -s tools/visual -p 'test_*store_capture.py'
node apps/layer-linux/bench/window-capture.test.mjs
cargo test --locked -p layer-linux
```

Walk actual captures for every scene in both themes. Check every supported
language for missing glyphs, unexpected fallback text and clipped controls;
successful metadata validation cannot establish the composition's quality.
