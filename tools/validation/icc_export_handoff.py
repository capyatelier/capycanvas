#!/usr/bin/env python3
"""Independently decode custom ICC GTK exports with ImageMagick.

Run native_document_files with LAYER_TEST_CMYK_PROFILE, then supply its output
directory and process ID. The GTK fixture retains expected .raw/.icc siblings.
No color conversion is requested here: compare the published codes and tags.
"""
import argparse
import array
import hashlib
import json
from pathlib import Path
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path)
parser.add_argument("process_id", type=int)
args = parser.parse_args()
report = {"imagemagick": subprocess.check_output(["magick", "-version"], text=True), "outputs": {}}
for model, mode, extension in [("rgb", "RGBA", "tif"), ("gray", "GRAYA", "png"), ("cmyk", "CMYK", "tif")]:
    path = args.directory / f"custom-{model}-{args.process_id}.{extension}"
    expected = path.with_suffix(".raw").read_bytes()
    actual = subprocess.check_output(["magick", str(path), "-depth", "16", "-endian", "LSB", f"{mode}:-"])
    assert len(actual) == len(expected), (model, len(actual), len(expected))
    a, b = array.array("H"), array.array("H")
    a.frombytes(actual)
    b.frombytes(expected)
    if sys.byteorder != "little":
        a.byteswap()
        b.byteswap()
    maximum = max(abs(x - y) for x, y in zip(a, b))
    assert maximum <= 1, (model, maximum)
    profile = subprocess.check_output(["magick", str(path), "ICC:-"])
    assert profile == path.with_suffix(".icc").read_bytes(), model
    report["outputs"][model] = {"path": str(path), "samples": len(a), "max_code_difference": maximum,
                                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                "profile_sha256": hashlib.sha256(profile).hexdigest()}
print(json.dumps(report, indent=2))
