#!/usr/bin/env python3
"""JPEG codec correctness fixtures, not a performance workload.

Requires Pillow and ImageMagick. Prepare, run layer-color's external JPEG tests
with LAYER_TEST_JPEG_FIXTURES pointing here, then verify the emitted handoffs.
ICC profiles are supplied locally; this script does not download test assets.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import PIL
from PIL import Image, ImageOps, features


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(args):
    dest = args.directory
    dest.mkdir(parents=True, exist_ok=True)
    cmyk_profile = args.cmyk_profile.read_bytes()
    rgb_profile = args.rgb_profile.read_bytes()
    width, height = 257, 33
    ink = bytes(v for y in range(height) for x in range(width)
                for v in (x % 256, y * 7, 255 - x % 256, (x // 3 + y) % 256))
    Image.frombytes("CMYK", (width, height), ink).save(
        dest / "cmyk.jpg", quality=97, icc_profile=cmyk_profile)
    # ImageMagick explicitly emits YCCK; Pillow emits direct Adobe CMYK.
    (dest / "input.cmyk").write_bytes(ink)
    subprocess.run(["magick", "-size", f"{width}x{height}", "-depth", "8",
                    f"CMYK:{dest / 'input.cmyk'}", "-profile", str(args.cmyk_profile),
                    "-sampling-factor", "1x1", "-quality", "97", "-depth", "8",
                    str(dest / "ycck.jpg")], check=True)
    for name, transform in [("cmyk", 0), ("ycck", 2)]:
        with Image.open(dest / f"{name}.jpg") as image:
            assert image.mode == "CMYK"
            assert image.info["adobe_transform"] == transform
            assert image.info["icc_profile"] == cmyk_profile
        subprocess.run(["magick", "-define", "jpeg:dct-method=slow",
                        str(dest / f"{name}.jpg"), "-depth", "8",
                        f"CMYK:{dest / (name + '.raw')}"], check=True)

    width, height = 513, 259
    rgb = bytes(v for y in range(height) for x in range(width)
                for v in ((x // 2) % 256, y % 256, (x // 3 + y // 2) % 256))
    image = Image.frombytes("RGB", (width, height), rgb)
    image.save(dest / "progressive.jpg", quality=95, progressive=True,
               subsampling=2, icc_profile=rgb_profile)
    image.convert("L").save(dest / "progressive-gray.jpg", quality=95,
                            progressive=True)
    exif = Image.Exif()
    exif[274] = 6
    image.save(dest / "oriented.jpg", quality=95, subsampling=0,
               icc_profile=rgb_profile, exif=exif)
    for name in ["progressive", "progressive-gray", "oriented"]:
        with Image.open(dest / f"{name}.jpg") as decoded:
            normalized = ImageOps.exif_transpose(decoded)
            (dest / f"{name}.raw").write_bytes(normalized.tobytes())
    # A separate large fixture proves coefficient-budget rejection and explicit
    # supported decoding. Memory and latency measurement belong to qualification.
    Image.new("RGB", (8192, 7324), (40, 100, 170)).save(
        dest / "progressive-60mp.jpg", quality=100, progressive=True,
        subsampling=0, icc_profile=rgb_profile)
    provenance = {
        "pillow": PIL.__version__, "pillow_libjpeg_turbo": features.version("libjpeg_turbo"),
        "imagemagick": subprocess.check_output(["magick", "-version"], text=True),
        "profiles": {str(p.resolve()): digest(p) for p in [args.cmyk_profile, args.rgb_profile]},
        "fixtures": {p.name: digest(p) for p in sorted(dest.iterdir())
                     if p.suffix in [".jpg", ".raw", ".cmyk"] and not p.name.startswith("capy-")},
    }
    (dest / "fixtures.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(json.dumps(provenance, indent=2))


def verify(args):
    results = {}
    for name in ["cmyk", "ycck"]:
        path = args.directory / f"capy-{name}.jpg"
        expected = (args.directory / f"{name}.raw").read_bytes()
        with Image.open(path) as image, Image.open(args.directory / f"{name}.jpg") as original:
            assert image.mode == "CMYK"
            assert image.size == original.size
            assert image.info["adobe_transform"] == 0
            assert image.info["icc_profile"] == original.info["icc_profile"]
            actual = image.tobytes()
        assert len(actual) == len(expected)
        difference = max(abs(a - b) for a, b in zip(actual, expected))
        assert difference <= 4, (name, difference)
        results[name] = {"sha256": digest(path), "max_ink_code_difference": difference,
                         "profile_preserved": True, "adobe_transform": 0}
    (args.directory / "handoff.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare", "verify"])
    parser.add_argument("directory", type=Path)
    parser.add_argument("--cmyk-profile", type=Path)
    parser.add_argument("--rgb-profile", type=Path)
    args = parser.parse_args()
    if args.action == "prepare":
        if args.cmyk_profile is None or args.rgb_profile is None:
            parser.error("prepare requires --cmyk-profile and --rgb-profile")
        prepare(args)
    else:
        verify(args)
