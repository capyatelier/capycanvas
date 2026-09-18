#!/usr/bin/env python3
"""Fetch pinned CC0 HDR originals and build native review documents/comparisons.

Run from the repository root after:
  cargo build --offline --release -p layer-color --example local_tone_review
No production Radiance importer is installed by this tool.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

SOURCES = {
    "abandoned_hall_01": "3b3525a4caea1acb18200565acea70d3fd774bcc73b887b4eade0f65a54cd4e6",
    "venice_sunset": "cbfac020ee17ab36016ccc7fbe9020b828587b9864af6ecda0a93089914889a6",
    "neon_photostudio": "e6baa610a8e3518177465440777897def354015bf89aac82f1a23ee84b75e116",
    "kiara_1_dawn": "7261c613b35a9b760ed5c7846ec8182532b90d89f6a8385c42bb743883159a66",
    "studio_small_09": "36724313c0fc66dab126ff90f081b85a0b1ef47be65eda1417bd20b033f0573f",
}
root = Path("artifacts/color-m4/local-tone/images")
(root / "originals").mkdir(parents=True, exist_ok=True)
manifest = []
measurements = []
for name, digest in SOURCES.items():
    filename = f"{name}_2k.hdr"
    path = root / "originals" / filename
    url = f"https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/{filename}"
    if not path.exists():
        request = urllib.request.Request(url, headers={"User-Agent": "CapyCanvas HDR review fixtures"})
        with urllib.request.urlopen(request, timeout=120) as response:
            data = response.read(32 * 1024 * 1024 + 1)
        if len(data) > 32 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"Unexpected HDR asset: {name}")
        staging = path.with_suffix(".download")
        staging.write_bytes(data)
        staging.replace(path)
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        raise ValueError(f"HDR checksum mismatch: {path}")
    manifest.append(dict(name=name, source=f"https://polyhaven.com/a/{name}", url=url,
                         license="CC0", sha256=digest, bytes=path.stat().st_size))
    result = subprocess.check_output(["target/release/examples/local_tone_review", str(path),
                                      str(root / "review")], text=True)
    measurements.append(result)
    print(result, end="")
(root / "sources.json").write_text(json.dumps(manifest, indent=2) + "\n")
(root / "measurements.txt").write_text("".join(measurements))
