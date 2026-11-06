#!/usr/bin/env python3
"""Full-image, color-managed PNG comparison. Never resize, crop, or hide pixels."""
import argparse
import io
import json
import struct
from pathlib import Path
from PIL import Image, ImageChops, ImageCms, ImageOps, ImageStat

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("reference", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--channel-tolerance", type=int, default=0)
parser.add_argument("--allowed-different-fraction", type=float, default=0)
parser.add_argument("--sheet", action="store_true", help="Save reference, candidate and difference beside each other")
parser.add_argument("--candidate-rotation", type=int, choices=[0, 90, 180, 270], default=0,
    help="Counterclockwise orientation correction for raw device captures, after EXIF; never resamples")
args = parser.parse_args()
if not 0 <= args.channel_tolerance <= 255 or not 0 <= args.allowed_different_fraction <= 1:
    parser.error("Tolerance must be 0..255 and allowed fraction 0..1")

srgb = ImageCms.createProfile("sRGB")

def bt709_gamma22(profile):
    if not profile or len(profile) < 132 or profile[36:40] != b"acsp" or profile[16:24] != b"RGB XYZ ":
        return False
    try:
        length, = struct.unpack_from(">I", profile)
        count, = struct.unpack_from(">I", profile, 128)
        if length != len(profile) or count > (length - 132) // 12:
            return False
        tags = {}
        for index in range(count):
            name, offset, size = struct.unpack_from(">4sII", profile, 132 + index * 12)
            if name in tags or offset < 132 + count * 12 or offset % 4 or size < 8 or size > length - offset:
                return False
            tags[name] = profile[offset:offset + size]
        for name in (b"rTRC", b"gTRC", b"bTRC"):
            curve = tags[name]
            if curve[:4] == b"curv" and len(curve) in (14, 16) and struct.unpack_from(">I", curve, 8)[0] == 1:
                if struct.unpack_from(">H", curve, 12)[0] != round(2.2 * 256) or any(curve[14:]):
                    return False
            elif curve[:4] == b"para" and len(curve) == 16 and curve[8:12] == bytes(4):
                if struct.unpack_from(">i", curve, 12)[0] != round(2.2 * 65536):
                    return False
            else:
                return False
        for name, expected in ((b"rXYZ", (0x6FA2, 0x38F5, 0x0390)),
                (b"gXYZ", (0x6299, 0xB785, 0x18DA)), (b"bXYZ", (0x24A0, 0x0F84, 0xB6CF))):
            colorant = tags[name]
            if len(colorant) != 20 or colorant[:4] != b"XYZ " or any(
                    abs(value - target) > 1 for value, target in zip(struct.unpack_from(">3i", colorant, 8), expected)):
                return False
    except (KeyError, struct.error):
        return False
    return True


def embedded_profile(path):
    with Image.open(path) as image:
        return image.info.get("icc_profile")


def read(path, paired_profile):
    image = Image.open(path)
    image.load()
    # XCTest PNGs can store a rotated raster with EXIF orientation. Normalize
    # that declared orientation losslessly before checking capture dimensions.
    image = ImageOps.exif_transpose(image)
    if image.convert("RGBA").getchannel("A").getextrema() != (255, 255):
        raise ValueError(f"Expected an opaque editor screenshot: {path.name}")
    profile = image.info.get("icc_profile")
    declared = None
    encoding = None
    metadata = path.with_suffix(".json")
    if metadata.exists():
        try:
            values = json.loads(metadata.read_text())
        except (ValueError, OSError) as error:
            raise SystemExit(f"unsupported encoding: {path.name}: invalid capture metadata: {error}")
        if isinstance(values, dict):
            encoding = values.get("pixel_encoding")
            if encoding is not None and encoding != "gamma22-on-srgb":
                raise SystemExit(f"unsupported encoding: {path.name}: unknown pixel encoding")
            if encoding == "gamma22-on-srgb" and profile is not None:
                raise SystemExit(f"unsupported encoding: {path.name}: explicit pixel encoding conflicts with embedded profile")
        if isinstance(values, dict) and "colorimetry" in values:
            declared = values["colorimetry"]
            if (not isinstance(declared, dict)
                    or any(type(declared.get(key)) is not int for key in ("primaries", "transfer", "matrix", "range"))
                    or declared["primaries"] != 1 or declared["matrix"] != 1
                    or declared["range"] not in (0, 1)
                    or declared["transfer"] not in ((4, 7) if encoding == "gamma22-on-srgb" else (4,))):
                raise SystemExit(f"unsupported encoding: {path.name}: invalid capture colorimetry")
            if encoding is None and (not bt709_gamma22(paired_profile)
                    or profile is not None and profile != paired_profile):
                raise SystemExit(f"unsupported encoding: {path.name}: capture declaration and paired ICC profile do not match")
            if encoding is None:
                profile = paired_profile
    if encoding == "gamma22-on-srgb":
        inverse = [round(((value / 255 / 12.92 if value / 255 <= 0.04045
            else ((value / 255 + 0.055) / 1.055) ** 2.4) ** (1 / 2.2)) * 255)
            for value in range(256)]
        rgb = image.convert("RGB").point(inverse * 3)
        description = "declared gamma22-on-srgb; inverse SDR output mapping"
    elif profile:
        source = ImageCms.ImageCmsProfile(io.BytesIO(profile))
        description = ImageCms.getProfileDescription(source).strip()
        if declared is not None:
            description = f"declared BT.709 gamma 2.2; matched paired ICC: {description}"
        rgb = ImageCms.profileToProfile(image.convert("RGB"), source, srgb,
            renderingIntent=ImageCms.Intent.RELATIVE_COLORIMETRIC, outputMode="RGB")
    else:
        # Chrome's forced-sRGB captures may omit the optional PNG profile chunk.
        description = "sRGB chunk" if "srgb" in image.info else "untagged; undeclared; assumed sRGB"
        rgb = image.convert("RGB")
    return rgb, description

reference, reference_profile = read(args.reference, embedded_profile(args.candidate))
candidate, candidate_profile = read(args.candidate, embedded_profile(args.reference))
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
if args.sheet:
    sheet = Image.new("RGB", (reference.width * 3, reference.height))
    for index, image in enumerate((reference, candidate, diff)):
        sheet.paste(image, (index * reference.width, 0))
    sheet.save(args.output / "sheet.png", icc_profile=profile_bytes)
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
