#!/usr/bin/env python3
"""Sample canonical fixed fills, foreground outlines and overlap in icon grids.

This checks six flat interior points per case, independently of edge rasterization.
Keep the complete compare.py results too; these samples do not prove pixel parity.
"""
import argparse
import io
import json
import math
from pathlib import Path
import xml.etree.ElementTree as ET
from PIL import Image, ImageCms, ImageOps

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path, help="Icon fixture manifest and native/web PNG grids")
parser.add_argument("--sources", type=Path, default=Path("apps/layer-web/icons"))
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
fixtures = json.loads((args.directory / "fixtures.json").read_text())["fixtures"]
if not fixtures:
    raise SystemExit("No icon fixtures")
color = ET.parse(args.sources / "layer-color-symbolic.svg").getroot()
colors = ET.parse(args.sources / "layer-colors-symbolic.svg").getroot()
# Coordinates are in the canonical 16-unit viewbox; paints come from the SVG.
checks = [("color", [8, 8], color[0].get("fill")), ("color", [8, 2.5], None),
          ("colors", [5, 5], colors[2].get("fill")), ("colors", [13, 13], colors[0].get("fill")),
          ("colors", [8, 8], colors[2].get("fill")), ("colors", [.75, 5], None)]


def rgb(value):
    value = value.removeprefix("#")
    if len(value) == 3:
        value = "".join(c * 2 for c in value)
    if len(value) != 6:
        raise ValueError("Expected an RGB hex paint")
    return [int(value[i:i + 2], 16) for i in [0, 2, 4]]


samples = []
for fixture in fixtures:
    for host in ["native", "web"]:
        image = ImageOps.exif_transpose(Image.open(args.directory / f"{host}-{fixture['name']}.png"))
        scale = fixture["scale"]
        if image.size != (fixture["width"] * scale, fixture["height"] * scale):
            raise ValueError("Capture dimensions differ from the manifest")
        if image.convert("RGBA").getchannel("A").getextrema() != (255, 255):
            raise ValueError("Expected an opaque grid capture")
        if profile := image.info.get("icc_profile"):
            image = ImageCms.profileToProfile(image.convert("RGB"), ImageCms.ImageCmsProfile(io.BytesIO(profile)),
                ImageCms.createProfile("sRGB"), renderingIntent=ImageCms.Intent.RELATIVE_COLORIMETRIC, outputMode="RGB")
        else:
            image = image.convert("RGB")
        for icon, point, fill in checks:
            index = fixture["icons"].index(f"layer-{icon}-symbolic")
            size = fixture["size"]
            position = [math.floor((index % 12 * 48 + 24 - size / 2 + point[0] * size / 16) * scale),
                        math.floor((index // 12 * 48 + 24 - size / 2 + point[1] * size / 16) * scale)]
            expected = [round(c * fixture["opacity"] + b * (1 - fixture["opacity"]))
                        for c, b in zip(rgb(fill or fixture["foreground"]), rgb(fixture["background"]))]
            actual = list(image.getpixel(position))
            error = max(abs(a - b) for a, b in zip(actual, expected))
            samples.append({"case": fixture["name"], "host": host, "icon": icon, "point": point,
                            "actual": actual, "expected": expected, "error": error})
report = {"samples": len(samples), "maximum_error": max(s["error"] for s in samples),
          "failed": sum(s["error"] > 2 for s in samples), "channel_tolerance": 2,
          "scope": "Six flat paint samples per host/case; complete pixel comparison remains separate",
          "worst": sorted(samples, key=lambda s: s["error"], reverse=True)[:8]}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
raise SystemExit(1 if report["failed"] else 0)
