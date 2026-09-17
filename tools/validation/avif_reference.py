#!/usr/bin/env python3
"""Build reproducible independent AVIF references using libavif 1.3.0 and AOM.

Requires cc and system libavif.so.16 with AOM encode/decode. Downloads are opt-in;
all upstream samples and the public C header are verified before use. Fixtures
stay in the supplied output directory, outside application assets.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

HERE = Path(__file__).resolve().parent
HEADER = {
    "file": "avif.h",
    "url": "https://raw.githubusercontent.com/AOMediaCodec/libavif/v1.3.0/include/avif/avif.h",
    "sha256": "ece1a0ab723ae006b72a191bc9bb3ae55e90d245e61356dac847185d85635d17",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(root, spec, fetch):
    path = root / spec["file"]
    if not path.exists():
        if not fetch:
            raise RuntimeError(f"Missing {path}; provide the file or run with --fetch")
        with urllib.request.urlopen(spec["url"], timeout=30) as response:
            data = response.read(2 * 1024 * 1024 + 1)
        if hashlib.sha256(data).hexdigest() != spec["sha256"]:
            raise RuntimeError(f"Source digest mismatch: {spec['file']}")
        path.write_bytes(data)
    if digest(path) != spec["sha256"]:
        raise RuntimeError(f"Source digest mismatch: {spec['file']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fetch", action="store_true")
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)
    samples = json.loads((HERE / "avif_reference.json").read_text())
    for spec in [HEADER, *samples]:
        prepare(root, spec, args.fetch)
    for name in ["avif_precision", "avif_reference"]:
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-I", str(root), str(HERE / (name + ".c")), "-l:libavif.so.16",
                        "-o", str(root / name)], check=True)
    subprocess.run([str(root / "avif_precision"), str(root)], check=True)
    # This pinned sequence has independently decodable third color/alpha
    # samples. Point only the primary items at those samples; tracks retain the
    # original first frame. An avif major brand also exercises AUTO's preference
    # for the poster. These fixture-only offsets are guarded by exact bytes.
    poster = bytearray((root / "colors-animated-12bpc-keyframes-0-2-3.avif").read_bytes())
    assert poster[110:114] == b"iloc" and poster[118:122] == bytes.fromhex("44000002")
    assert poster[128:136] == bytes.fromhex("000007e600000027")
    assert poster[142:150] == bytes.fromhex("0000073c0000001d")
    poster[8:12] = b"avif"
    poster[128:136] = (2022 + 39 + 36).to_bytes(4, "big") + (38).to_bytes(4, "big")
    poster[142:150] = (1852 + 29 + 20).to_bytes(4, "big") + (26).to_bytes(4, "big")
    (root / "sequence-different-poster.avif").write_bytes(poster)
    # Preserve property indices and encoded offsets while removing only the
    # known fixture's container color tag. The AV1 payload is unchanged.
    for name in ["p3-10bit-bitstream", "p3-12bit-bitstream", "cosmos1650_yuv444_10bpc_p3pq"]:
        data = bytearray((root / (name + ".avif")).read_bytes())
        if data.count(b"colrnclx") != 1:
            raise RuntimeError(f"{name}: unexpected container color layout")
        offset = data.index(b"colrnclx")
        data[offset:offset + 4] = b"free"
        (root / (name + "-no-colr.avif")).write_bytes(data)
    references = []
    for name in ["abc_color_irot_alpha_irot", "seine_sdr_gainmap_srgb",
                 "colors-animated-12bpc-keyframes-0-2-3", "colors-animated-8bpc-alpha-exif-xmp",
                 "sequence-different-poster",
                 "color_grid_alpha_nogrid", "sofa_grid1x5_420",
                 "p3-10bit-bitstream-no-colr", "p3-12bit-bitstream-no-colr",
                 "cosmos1650_yuv444_10bpc_p3pq-no-colr"]:
        subprocess.run([str(root / "avif_reference"), str(root / (name + ".avif")),
                        str(root / (name + ".reference"))], check=True)
        record = json.loads((root / (name + ".reference.json")).read_text())
        if name.startswith("p3-"):
            assert (record["primaries"], record["transfer"], record["matrix"]) == (12, 13, 0)
        elif name.startswith("cosmos"):
            assert (record["primaries"], record["transfer"]) == (12, 16)
        record.update(file=name + ".avif", sha256=digest(root / (name + ".avif")),
                      rgba_sha256=digest(root / (name + ".reference.rgba16")))
        references.append(record)
    subprocess.run([str(root / "avif_reference"), str(root / "sequence-different-poster.avif"),
                    str(root / "sequence-different-poster.poster"), "poster"], check=True)
    assert (root / "sequence-different-poster.reference.rgba16").read_bytes() != (root / "sequence-different-poster.poster.rgba16").read_bytes()
    suffixes = ["", "-rotated", "-bitstream", *[f"-crop-r{r}-m{m}" for r in range(4) for m in range(2)]]
    generated = [f"p3-{depth}bit{suffix}.{extension}"
                 for depth in (10, 12) for suffix in suffixes
                 for extension in ("avif", "rgba16")]
    generated.extend(f"p3-{depth}bit-bitstream-no-colr.avif" for depth in (10, 12))
    generated.extend(["sequence-different-poster.avif", "sequence-different-poster.poster.rgba16", "sequence-different-poster.poster.json"])
    record = {
        "upstream_samples": samples, "header": HEADER,
        "license_provenance": "https://github.com/AOMediaCodec/libavif/blob/d98b4bcb67e9b2e5056d1f9ae18a088779d7945f/tests/data/README.md",
        "tools": {name: digest(HERE / name) for name in
                  ["avif_precision.c", "avif_reference.c", "avif_reference.py"]},
        "references": references,
        "generated": {name: digest(root / name) for name in generated},
    }
    manifest = root / "reference-manifest.json"
    manifest.write_text(json.dumps(record, indent=2) + "\n")
    print(manifest)


if __name__ == "__main__":
    main()
