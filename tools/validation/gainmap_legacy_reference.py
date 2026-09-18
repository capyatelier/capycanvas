#!/usr/bin/env python3
"""Independent ICC-aware SDR JPEG references (system Pillow + LittleCMS).

Run after the gainmap_rendition_changes Rust test with LAYER_GAINMAP_OUTPUT set
and native_gainmap_export_choices_preview_flatten_and_reopen GTK journey.
The resulting control/references feed test.mjs --gainmap-interchange.
"""
import io
import json
from pathlib import Path
from PIL import Image, ImageCms

root = Path(__file__).resolve().parents[2] / "artifacts/color-m4"
output = root / "gainmap-interchange"

def sdr(path):
    image = Image.open(path)
    profile = ImageCms.ImageCmsProfile(io.BytesIO(image.info["icc_profile"]))
    return ImageCms.profileToProfile(image, profile, ImageCms.createProfile("sRGB"), outputMode="RGB")

sdr(output / "jpeg-neutral.jpg").save(output / "jpeg-control.jpg")
image = sdr(root / "gainmap-ui/Opaque edited HDR.jpg")
(output / "legacy-sdr-reference.json").write_text(json.dumps({
    "pixel": image.getpixel((32, 24)), "size": image.size,
    "decoder": "Pillow/LCMS, ICC-aware SDR JPEG",
}) + "\n")
