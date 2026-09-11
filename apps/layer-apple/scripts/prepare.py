#!/usr/bin/env python3
"""Generate Apple resources from the shared sources; never edit Generated."""
import json
import shutil
import xml.etree.ElementTree as ET
from pathlib import Path

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
for source in sorted((ROOT / "apps/layer-web/icons").glob("*.svg")):
    name = source.stem.removeprefix("layer-").removesuffix("-symbolic")
    dest = CATALOG / f"icon-{name}.imageset"
    dest.mkdir(exist_ok=True)
    # Symbolic foreground follows the editor palette via template rendering.
    svg = source.read_text().replace("currentColor", "#ffffff")
    ET.fromstring(svg)  # Fail at the shared source before invoking asset compilers.
    if not (dest / source.name).exists() or (dest / source.name).read_text() != svg:
        (dest / source.name).write_text(svg)
    write_json(dest / "Contents.json", {
        "images": [{"filename": source.name, "idiom": "universal"}],
        "info": {"version": 1, "author": "xcode"},
        "properties": {"preserves-vector-representation": True, "template-rendering-intent": "template"},
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
