#!/usr/bin/env python3
"""Full-image, color-managed PNG comparison. Never resize, crop, or hide pixels."""
import argparse
import io
import json
from pathlib import Path
from PIL import Image, ImageChops, ImageCms, ImageOps, ImageStat

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("reference", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--channel-tolerance", type=int, default=0)
parser.add_argument("--allowed-different-fraction", type=float, default=0)
parser.add_argument("--candidate-rotation", type=int, choices=[0, 90, 180, 270], default=0,
    help="Counterclockwise orientation correction for raw device captures, after EXIF; never resamples")
args = parser.parse_args()
if not 0 <= args.channel_tolerance <= 255 or not 0 <= args.allowed_different_fraction <= 1:
    parser.error("Tolerance must be 0..255 and allowed fraction 0..1")

srgb = ImageCms.createProfile("sRGB")

def read(path):
    image = Image.open(path)
    image.load()
    # XCTest PNGs can store a rotated raster with EXIF orientation. Normalize
    # that declared orientation losslessly before checking capture dimensions.
    image = ImageOps.exif_transpose(image)
    if image.convert("RGBA").getchannel("A").getextrema() != (255, 255):
        raise ValueError(f"Expected an opaque editor screenshot: {path.name}")
    profile = image.info.get("icc_profile")
    if profile:
        source = ImageCms.ImageCmsProfile(io.BytesIO(profile))
        description = ImageCms.getProfileDescription(source).strip()
        rgb = ImageCms.profileToProfile(image.convert("RGB"), source, srgb,
            renderingIntent=ImageCms.Intent.RELATIVE_COLORIMETRIC, outputMode="RGB")
    else:
        # Chrome's forced-sRGB captures may omit the optional PNG profile chunk.
        description = "sRGB chunk" if "srgb" in image.info else "untagged; assumed sRGB"
        rgb = image.convert("RGB")
    return rgb, description

reference, reference_profile = read(args.reference)
candidate, candidate_profile = read(args.candidate)
if args.candidate_rotation:
    candidate = candidate.transpose({90: Image.Transpose.ROTATE_90,
        180: Image.Transpose.ROTATE_180, 270: Image.Transpose.ROTATE_270}[args.candidate_rotation])
if reference.size != candidate.size:
    raise SystemExit(f"Capture dimensions differ: {reference.size} vs {candidate.size}; capture again at matching dimensions")

diff = ImageChops.difference(reference, candidate)
r, g, b = diff.split()
maximum = ImageChops.lighter(ImageChops.lighter(r, g), b)
histogram = maximum.histogram()
pixels = reference.width * reference.height
changed = sum(histogram[args.channel_tolerance + 1:])
exact_changed = pixels - histogram[0]
stats = ImageStat.Stat(diff)
passed = changed / pixels <= args.allowed_different_fraction
args.output.mkdir(parents=True, exist_ok=True)
profile_bytes = ImageCms.ImageCmsProfile(srgb).tobytes()
diff.save(args.output / "difference.png", icc_profile=profile_bytes)
Image.blend(reference, candidate, 0.5).save(args.output / "overlay.png", icc_profile=profile_bytes)
# Red intensity is the largest channel error, amplified 8x for inspection only.
amplified = maximum.point(lambda value: min(255, value * 8))
zero = Image.new("L", reference.size, 0)
Image.merge("RGB", (amplified, zero, zero)).save(args.output / "heatmap.png", icc_profile=profile_bytes)
report = {
    "reference": args.reference.name, "candidate": args.candidate.name,
    "dimensions": reference.size, "comparison_space": "sRGB",
    "input_profiles": [reference_profile, candidate_profile],
    "candidate_rotation_counterclockwise": args.candidate_rotation,
    "pixels": pixels, "exact_different_pixels": exact_changed,
    "exact_different_fraction": exact_changed / pixels,
    "channel_tolerance": args.channel_tolerance,
    "different_pixels_above_tolerance": changed,
    "different_fraction_above_tolerance": changed / pixels,
    "mean_absolute_channel_error": stats.mean,
    "maximum_channel_error": maximum.getextrema()[1],
    "difference_bounds_pixels": maximum.getbbox(),
    "allowed_different_fraction": args.allowed_different_fraction, "passed": passed,
}
(args.output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
raise SystemExit(0 if passed else 1)
