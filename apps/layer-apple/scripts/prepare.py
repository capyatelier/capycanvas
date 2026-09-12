#!/usr/bin/env python3
"""Generate Apple resources from the shared sources; never edit Generated."""
import json
import shutil
from pathlib import Path
from icon_assets import icon_layers

APP = Path(__file__).resolve().parents[1]
ROOT = APP.parents[1]
GENERATED = APP / "Generated"
CATALOG = GENERATED / "SharedAssets.xcassets"
CATALOG.mkdir(parents=True, exist_ok=True)

def write_json(path, value):
    text = json.dumps(value, indent=2) + "\n"
    if not path.exists() or path.read_text() != text:
        path.write_text(text)

write_json(CATALOG / "Contents.json", {"info": {"version": 1, "author": "xcode"}})
icon_manifest = {}
icon_sets = set()
for source in sorted((ROOT / "apps/layer-web/icons").glob("*.svg")):
    name = source.stem.removeprefix("layer-").removesuffix("-symbolic")
    try:
        layers = icon_layers(source.read_text())
    except ValueError as error:
        raise ValueError(f"{source.name}: {error}") from error
    entries = []
    for index, (mode, svg) in enumerate(layers):
        asset = f"icon-{name}" + (f"-{index}" if len(layers) > 1 else "")
        dest = CATALOG / f"{asset}.imageset"
        icon_sets.add(dest.name)
        dest.mkdir(exist_ok=True)
        if not (dest / source.name).exists() or (dest / source.name).read_text() != svg:
            (dest / source.name).write_text(svg)
        write_json(dest / "Contents.json", {
            "images": [{"filename": source.name, "idiom": "universal"}],
            "info": {"version": 1, "author": "xcode"},
            "properties": {"preserves-vector-representation": True, "template-rendering-intent": mode},
        })
        entries.append({"asset": asset, "template": mode == "template"})
    # Ordinary symbolic icons need no lookup entry or additional drawing layers.
    if len(layers) > 1 or layers[0][0] != "template":
        icon_manifest[name] = entries
for stale in CATALOG.glob("icon-*.imageset"):
    if stale.name not in icon_sets:
        shutil.rmtree(stale)
manifest = CATALOG / "shared-icon-paints.dataset"
manifest.mkdir(exist_ok=True)
write_json(manifest / "layers.json", icon_manifest)
write_json(manifest / "Contents.json", {
    "data": [{"filename": "layers.json", "idiom": "universal", "universal-type-identifier": "public.json"}],
    "info": {"version": 1, "author": "xcode"},
})
for source in sorted((ROOT / "apps/layer-web/brush-previews").glob("*.png")):
    dest = CATALOG / f"preview-{source.stem}.imageset"
    dest.mkdir(exist_ok=True)
    shutil.copy2(source, dest / source.name)
    write_json(dest / "Contents.json", {"images": [{"filename": source.name, "idiom": "universal"}],
        "info": {"version": 1, "author": "xcode"}})
shutil.copytree(ROOT / "assets/filters", GENERATED / "filters", dirs_exist_ok=True)
notices = GENERATED / "licenses"
notices.mkdir(exist_ok=True)
for name in ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "BRANDING.md", "THIRD_PARTY_NOTICES.md"]:
    if (ROOT / name).is_file():
        shutil.copy2(ROOT / name, notices / name)
