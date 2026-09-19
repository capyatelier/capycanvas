#!/usr/bin/env python3
"""Independent FFmpeg/Python PQ interoperability fixture and numerical oracle.

generate DIR creates a 512x384 RGB16 PQ PNG using FFmpeg's PNG encoder.
verify JOURNEY_DIR decodes GTK's exports with FFmpeg, then compares every pixel
with the captured Float32 linear sRGB composite using independent double math.
No Capy libraries are used by the oracle. Python 3 and FFmpeg are required.
"""
import argparse
import hashlib
import json
import math
import pathlib
import struct
import subprocess


def run(*args):
    return subprocess.check_output(args, stderr=subprocess.PIPE)


def generate(root):
    root.mkdir(parents=True, exist_ok=True)
    raw = root / "input-pq.rgb48le"
    with raw.open("wb") as f:
        for y in range(384):
            for x in range(512):
                f.write(struct.pack("<3H", 30000 + x * 24000 // 511,
                                    25000 + y * 18000 // 383,
                                    23000 + (x + y) * 18000 // 894))
    run("ffmpeg", "-y", "-f", "rawvideo", "-pixel_format", "rgb48le",
        "-video_size", "512x384", "-i", str(raw), "-vf",
        "setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=gbr:range=full",
        "-frames:v", "1", "-pix_fmt", "rgb48be", "-update", "1",
        str(root / "ffmpeg-pq.png"))
    print(json.dumps({"ffmpeg": run("ffmpeg", "-version").decode().splitlines()[0],
                      "sha256": hashlib.sha256((root / "ffmpeg-pq.png").read_bytes()).hexdigest()}, indent=2))


def pq(value):
    # ST 2084, absolute cd/m² normalized to 10,000, independently evaluated.
    power = (value * 203.0 / 10000.0) ** (2610.0 / 16384.0)
    return ((3424.0 / 4096.0 + 2413.0 / 128.0 * power) /
            (1.0 + 2392.0 / 128.0 * power)) ** (2523.0 / 32.0)


def verify(root):
    hdr = root / "Edited HDR.png"
    info = json.loads(run("ffprobe", "-v", "error", "-show_frames", "-show_entries",
                         "frame=color_space,color_range,color_transfer,color_primaries",
                         "-of", "json", str(hdr)))
    frame = info["frames"][0]
    assert frame == dict(color_space="gbr", color_range="pc", color_transfer="smpte2084",
                         color_primaries="bt2020"), frame
    codes = run("ffmpeg", "-v", "error", "-i", str(hdr), "-f", "rawvideo",
                "-pix_fmt", "rgba64le", "-")
    sdr = run("ffmpeg", "-v", "error", "-i", str(root / "SDR rendition.png"),
              "-f", "rawvideo", "-pix_fmt", "rgba", "-")
    raw = (root / "edited-linear-srgb-rgba32le.bin").read_bytes()
    assert len(raw) == 512 * 384 * 16 and len(codes) == 512 * 384 * 8
    # D65 sRGB -> D65 BT.2020 matrix derived from published chromaticities.
    matrix = ((0.627403895934699, 0.329283038377883, 0.043313065687418),
              (0.069097289358232, 0.919540395075459, 0.011362315566309),
              (0.016391438875150, 0.088013307877226, 0.895595253247624))
    max_pq, max_sdr = 0, 0
    for i, (p, out) in enumerate(zip(struct.iter_unpack("<4f", raw), struct.iter_unpack("<4H", codes))):
        assert p[3] == 1.0 and out[3] == 65535
        for c in range(3):
            linear = sum(matrix[c][j] * p[j] for j in range(3))
            expected = round(pq(linear) * 65535.0)
            max_pq = max(max_pq, abs(expected - out[c]))
        rgb = [max(0.0, v) for v in p[:3]]
        peak = max(rgb)
        x = peak * 2.0 ** -0.5  # Authored journey exposure; contrast 1, knee .75.
        mapped = x if x <= .75 else 1.0 - .0625 / (x - .5)
        for c in range(3):
            linear = rgb[c] / peak * mapped if peak else 0.0
            encoded = 12.92 * linear if linear <= .0031308 else 1.055 * linear ** (1 / 2.4) - .055
            max_sdr = max(max_sdr, abs(round(encoded * 255) - sdr[i * 4 + c]))
    assert max_pq <= 1, max_pq
    assert max_sdr <= 1, max_sdr
    print(json.dumps({"pixels": 512 * 384, "metadata": frame,
                      "max_pq16_code_error": max_pq, "max_sdr8_code_error": max_sdr}, indent=2))


def verify_delivery(root):
    """Cross-check independent decodes of EXR and explicitly clipped PQ delivery.

    Declared tolerance: one 16-bit PQ code per channel. This checks the host's
    complete output route; it does not treat matching Capy reimports as an oracle.
    The authored local SDR mapper is covered separately by shared reference tests.
    """
    paths = [root / name for name in ("exr-delivery.exr", "hdr-delivery.png", "sdr-delivery.png")]
    def stream(path):
        return json.loads(run("ffprobe", "-v", "error", "-show_streams", "-of", "json", str(path)))["streams"][0]
    exr, hdr, sdr = map(stream, paths)
    assert exr["pix_fmt"] == "gbrapf32le" and exr["color_transfer"] == "linear", exr
    metadata = {k: hdr[k] for k in ("color_space", "color_range", "color_transfer", "color_primaries")}
    assert metadata == dict(color_space="gbr", color_range="pc", color_transfer="smpte2084", color_primaries="bt2020"), metadata
    assert {(s["width"], s["height"]) for s in (exr, hdr, sdr)} == {(512, 384)}
    count = 512 * 384
    floats = run("ffmpeg", "-v", "error", "-i", str(paths[0]), "-f", "rawvideo", "-pix_fmt", "gbrapf32le", "-")
    planes = struct.unpack(f"<{count * 4}f", floats)
    codes = run("ffmpeg", "-v", "error", "-i", str(paths[1]), "-f", "rawvideo", "-pix_fmt", "rgba64le", "-")
    matrix = ((0.627403895934699, 0.329283038377883, 0.043313065687418),
              (0.069097289358232, 0.919540395075459, 0.011362315566309),
              (0.016391438875150, 0.088013307877226, 0.895595253247624))
    maximum = 0
    negative = above = clipped = 0
    for i, out in enumerate(struct.iter_unpack("<4H", codes)):
        p = [planes[2 * count + i], planes[i], planes[count + i]]
        assert all(map(math.isfinite, p))
        assert planes[3 * count + i] == 1 and out[3] == 65535
        negative += any(v < 0 for v in p)
        above += any(v > 1 for v in p)
        for c in range(3):
            linear = sum(matrix[c][j] * p[j] for j in range(3))
            clipped += linear < 0 or linear > 10000 / 203
            expected = round(pq(max(0, min(10000 / 203, linear))) * 65535)
            maximum = max(maximum, abs(expected - out[c]))
    assert negative > 0 and above > 0 and clipped > 0
    assert maximum <= 1, maximum
    print(json.dumps({"pixels": count, "metadata": metadata, "negative_pixels": negative,
                      "above_white_pixels": above, "clipped_channels": clipped,
                      "max_pq16_code_error": maximum, "tolerance_pq16_codes": 1,
                      "ffmpeg": run("ffmpeg", "-version").decode().splitlines()[0],
                      "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["generate", "verify", "verify-delivery"])
    parser.add_argument("directory", type=pathlib.Path)
    args = parser.parse_args()
    {"generate": generate, "verify": verify, "verify-delivery": verify_delivery}[args.operation](args.directory)
