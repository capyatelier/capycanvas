#!/usr/bin/env python3
"""Check flat control colors and accent independence; retain full image diffs separately."""
import argparse
import io
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageCms

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("fixture", type=Path)
parser.add_argument("reference", type=Path)
parser.add_argument("native", type=Path)
args = parser.parse_args()
fixture = json.loads(args.fixture.read_text())
assert fixture["schema"] == 1
assert {row["kind"] for row in fixture["rows"]} == {"icon", "toolbar"}
for kind in ["icon", "toolbar"]:
    rows = [row for row in fixture["rows"] if row["kind"] == kind]
    assert len(rows) == 3 and {row["accent"] for row in rows} == {"system", "red", "green"}
    for row in rows:
        assert [(cell["selected"], cell["enabled"]) for cell in row["cells"]] == [
            (False, True), (True, True), (False, False), (True, False)]
def load_srgb(path):
    image = Image.open(path)
    assert image.convert("RGBA").getchannel("A").getextrema() == (255, 255), "Expected an opaque control fixture"
    rgb = image.convert("RGB")
    if image.info.get("icc_profile"):
        rgb = ImageCms.profileToProfile(rgb, ImageCms.ImageCmsProfile(io.BytesIO(image.info["icc_profile"])),
                                       ImageCms.createProfile("sRGB"),
                                       renderingIntent=ImageCms.Intent.RELATIVE_COLORIMETRIC, outputMode="RGB")
    return rgb


reference = load_srgb(args.reference)
native = load_srgb(args.native)
scale = fixture["scale"]
assert native.size == reference.size == (fixture["width"] * scale, fixture["height"] * scale)
colors = []
repeated = {}
for row in fixture["rows"]:
    cells = []
    for cell in row["cells"]:
        bounds = cell["bounds"]
        # Interior selection fill, away from the rounded edge and center glyph.
        # Browser/Core Graphics 8-bit alpha composition can differ by one byte.
        x = round((bounds["x"] + 4) * scale)
        y = round((bounds["y"] + bounds["height"] / 2) * scale)
        expected, actual = reference.getpixel((x, y)), native.getpixel((x, y))
        error = max(abs(a - b) for a, b in zip(expected, actual))
        cells.append({"selected": cell["selected"], "enabled": cell["enabled"],
                      "reference": expected, "native": actual, "maximum_channel_error": error})
    first, last = row["cells"][0]["bounds"], row["cells"][-1]["bounds"]
    region = tuple(round(v * scale) for v in [first["x"], first["y"],
                   last["x"] + last["width"], first["y"] + first["height"]])
    repeated.setdefault(row["kind"], []).append(native.crop(region))
    colors.append({"kind": row["kind"], "accent": row["accent"], "cells": cells})
maximum = max(cell["maximum_channel_error"] for row in colors for cell in row["cells"])
invariant = all(ImageChops.difference(images[0], image).getbbox() is None
                for images in repeated.values() for image in images[1:])
passed = maximum <= 1 and invariant
print(json.dumps({"scope": "Flat color samples and complete repeated rows only; use compare.py for full raw pixel differences",
                  "maximum_flat_color_error": maximum, "system_accent_independent": invariant,
                  "passed": passed, "rows": colors}, indent=2))
raise SystemExit(0 if passed else 1)
