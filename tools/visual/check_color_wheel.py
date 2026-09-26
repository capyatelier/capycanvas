#!/usr/bin/env python3
"""Check native wheel interior colors against shared Rust picker semantics.

This samples color correctness, not whole-editor visual parity. The original
capture remains untouched. Geometry comes from the focused native UI fixture.
"""
import argparse
import io
import json
import math
from pathlib import Path
import subprocess
from PIL import Image, ImageCms, ImageOps

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("image", type=Path)
parser.add_argument("fixture", type=Path)
parser.add_argument("--oracle", type=Path, default=Path("target/debug/examples/color_wheel_reference"))
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--oracle-interpreter", type=Path, help="Interpreter for a script oracle, including test doubles")
parser.add_argument("--channel-tolerance", type=int, default=2)
args = parser.parse_args()
if not 0 <= args.channel_tolerance <= 255:
    parser.error("Channel tolerance must be 0..255")
fixture = json.loads(args.fixture.read_text())
image = ImageOps.exif_transpose(Image.open(args.image))
alpha = image.convert("RGBA").getchannel("A")
if profile := image.info.get("icc_profile"):
    image = ImageCms.profileToProfile(image.convert("RGB"), ImageCms.ImageCmsProfile(io.BytesIO(profile)),
        ImageCms.createProfile("sRGB"), renderingIntent=ImageCms.Intent.RELATIVE_COLORIMETRIC, outputMode="RGB")
else:
    image = image.convert("RGB")
width, height = fixture["viewport"]
scale = image.width / width
if not math.isclose(scale, image.height / height, abs_tol=1e-6):
    raise SystemExit("Image orientation or viewport dimensions do not match the fixture")
left, top, wheel_width, wheel_height = fixture["wheel"]
if not math.isclose(wheel_width, wheel_height, abs_tol=0.5):
    raise SystemExit("Expected a square wheel allocation")

positions, points = [], []
for y in range(math.ceil(top * scale), math.floor((top + wheel_height) * scale), 11):
    for x in range(math.ceil(left * scale), math.floor((left + wheel_width) * scale), 11):
        if not (0 <= x < image.width and 0 <= y < image.height):
            raise SystemExit("The wheel is clipped by the capture")
        positions.append((x, y))
        # Ask Rust to classify neighboring samples too, excluding only the
        # antialiased boundary. No color conversion or hit policy is duplicated.
        for dx, dy in [(0,0),(-2,0),(2,0),(0,-2),(0,2)]:
            points.append([((x+0.5+dx)/scale-left)/wheel_width, ((y+0.5+dy)/scale-top)/wheel_height])
request = {"space": fixture["space"], "rgba": fixture["rgba"], "points": points}
if "shape" in fixture:
    request["shape"] = fixture["shape"]
if "hue" in fixture:
    request["hue"] = fixture["hue"]
command = [str(args.oracle.resolve())]
if args.oracle_interpreter:
    command.insert(0, str(args.oracle_interpreter.resolve()))
oracle = subprocess.run(command, input=json.dumps(request), text=True, capture_output=True, check=True)
reference = json.loads(oracle.stdout)
markers = [reference["model"][key] for key in ["wheel_hue_marker", "wheel_marker"]]
# Okhsv has a sharp blue gamut cusp. Native conic gradients antialias that
# internal boundary, just like the ring silhouette. Locate discontinuities from
# the shared stops, and exclude only their subpixel fringe (never missing alpha).
model = reference["model"]
stops = model.get("wheel_hue_stops", [])
discontinuities = [(a["offset"] + b["offset"]) * 0.5 for a, b in zip(stops, stops[1:])
    if b["offset"] - a["offset"] < 1e-6
    and max(abs(x-y) for x, y in zip(a["color"], b["color"])) > 8/255]
excluded_cusp_pixels = 0
checked = {"hue": 0, "field": 0}
maximum = {"hue": 0, "field": 0}
failures = []
for index, (x, y) in enumerate(positions):
    neighbors = reference["samples"][index*5:index*5+5]
    sample = neighbors[0]
    part = sample["part"]
    if part is None or any(n["part"] != part for n in neighbors):
        continue
    point = points[index*5]
    # Square picking covers its full rectangle; hosts round the painted corners.
    # Test the host's actual silhouette, including the same AA neighbors used above.
    corner = fixture.get("field_corner_radius", 0)
    if part == "field" and corner > 0:
        sx, sy, side = [v * wheel_width for v in reference["model"]["geometry"]["square"]]
        def inside_rounded_field(p):
            px, py = p[0] * wheel_width - sx, p[1] * wheel_width - sy
            dx = px - min(max(px, corner), side - corner)
            dy = py - min(max(py, corner), side - corner)
            return math.hypot(dx, dy) <= corner
        if not all(inside_rounded_field(p) for p in points[index*5:index*5+5]):
            continue
    # Hosts may use larger preview markers; exclude their outline and AA fringe.
    marker_exclusion = fixture.get("marker_radius", 3.5) + 3.5
    if any(math.hypot(point[0]-m[0], point[1]-m[1])*wheel_width < marker_exclusion for m in markers):
        continue
    if part == "hue" and discontinuities and alpha.getpixel((x,y)) == 255:
        cx, cy = model["geometry"]["center"]
        dx, dy = point[0]-cx, point[1]-cy
        angle = math.atan2(dy, dx) - math.radians(model["wheel_hue_start_degrees"])
        radius = math.hypot(dx, dy) * wheel_width * scale
        if any(abs(math.remainder(angle - offset*2*math.pi, 2*math.pi))*radius <= 0.75
                for offset in discontinuities):
            excluded_cusp_pixels += 1
            continue
    expected = [round(channel*255) for channel in sample["rgba"][:3]] + [255]
    actual = list(image.getpixel((x,y))) + [alpha.getpixel((x,y))]
    error = max(abs(a-b) for a,b in zip(actual,expected))
    checked[part] += 1
    maximum[part] = max(maximum[part],error)
    if error > args.channel_tolerance:
        failures.append({"pixel":[x,y],"part":part,"expected":expected,"actual":actual,"error":error})
if min(checked.values()) < 20:
    raise SystemExit("Insufficient interior samples for both the hue ring and color field")
report = {"image":args.image.name,"space":fixture["space"],"sampled_pixels":checked,
    "excluded_cusp_pixels":excluded_cusp_pixels,
    "maximum_channel_error":maximum,"channel_tolerance":args.channel_tolerance,
    "pixels_above_tolerance":len(failures),"worst_samples":sorted(failures,key=lambda f:f["error"],reverse=True)[:12],
    "scope":"Interior color correctness only; excludes boundaries, subpixel gamut cusps and markers; not whole-editor parity",
    "passed":not failures}
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps(report,indent=2))
raise SystemExit(0 if report["passed"] else 1)
