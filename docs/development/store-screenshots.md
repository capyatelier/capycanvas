# GTK store screenshots

[Developer guide](README.md) · [Linux](linux.md) · [Releasing](releasing.md)

`capycanvas-web` owns showcase artwork, scene recipes, the generation script and
published images. This repository supplies the native GTK capture helper. Run it
locally on a hardware Vulkan GPU when refreshing the images; release runners do
not need to render or download them.

The helper starts one scene in a fresh private home, application storage, D-Bus
session and headless Mutter display. It imports through the native file-launch
path and applies shared editor actions. It waits for document admission, shader
startup, visible thumbnails, canvas presentation and GTK frames. Missing glyphs,
an unexpected window size, application errors and clipped shadows fail the run.
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
English fallback, localized image variants with `xml:lang`, and screenshot
environment hints such as `gnome:light` and `gnome:dark`. GNOME Software can show
both theme variants, so each should stand on its own. Captions describe the
feature shown. Add and validate the final metadata after the image URLs are live.
See the [AppStream screenshot specification](https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html#tag-screenshots)
and [Flathub screenshot guidance](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines#screenshots).

## Checks

```bash
python3 -m unittest discover -s tools/visual -p 'test_gtk_store_capture.py'
node apps/layer-linux/bench/window-capture.test.mjs
cargo test --locked -p layer-linux
```

Walk actual captures for every scene in both themes. Check every supported
language for missing glyphs, unexpected fallback text and clipped controls;
successful metadata validation cannot establish the composition's quality.
