#!/usr/bin/env python3
"""Prepare independent photo-codec references; requires the system libwebp.

The Google gallery credits these images as public domain. Outputs are local
qualification fixtures, not application assets. No Python imaging codec is used.
"""
import argparse
import ctypes
import ctypes.util
import hashlib
import json
from pathlib import Path
import struct
import urllib.request

SAMPLES = [
    ("1_webp_ll.webp", "Jon Sullivan", "49a902b9c35bd2031bf26234c59ff369dd3ae18a3b58247285104a23346ba2d4"),
    ("1_webp_a.webp", "Jon Sullivan", "31090d2cdaa455d4153829074f2c91228964a83f86503600360b86d5d57160c3"),
    ("2_webp_a.webp", "Fizyplankton", "6dcb51b9ef8f4932d21584674af09336401e386782ca2dd2b0a7e2cb8bfdb49c"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    root = parser.parse_args().output
    root.mkdir(parents=True, exist_ok=True)
    lib = ctypes.CDLL(ctypes.util.find_library("webp") or "libwebp.so.7")
    lib.WebPGetDecoderVersion.restype = ctypes.c_int
    lib.WebPGetInfo.argtypes = [ctypes.c_void_p, ctypes.c_size_t,
                               ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)]
    lib.WebPDecodeRGBAInto.argtypes = [ctypes.c_void_p, ctypes.c_size_t,
                                      ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
    lib.WebPDecodeRGBAInto.restype = ctypes.c_void_p
    records = []
    for name, author, expected in SAMPLES:
        url = "https://www.gstatic.com/webp/gallery3/" + name
        with urllib.request.urlopen(url, timeout=30) as response:
            data = response.read(2 * 1024 * 1024 + 1)
        digest = hashlib.sha256(data).hexdigest()
        if digest != expected:
            raise ValueError(f"{name}: gallery file changed; review it before updating the fixture")
        width, height = ctypes.c_int(), ctypes.c_int()
        encoded = ctypes.create_string_buffer(data)
        if not lib.WebPGetInfo(encoded, len(data), ctypes.byref(width), ctypes.byref(height)):
            raise ValueError(f"{name}: libwebp rejected the header")
        if not (0 < width.value <= 1024 and 0 < height.value <= 1024):
            raise ValueError(f"{name}: unexpected reference dimensions")
        rgba = ctypes.create_string_buffer(width.value * height.value * 4)
        if not lib.WebPDecodeRGBAInto(encoded, len(data), rgba, len(rgba), width.value * 4):
            raise ValueError(f"{name}: libwebp rejected the image")
        raw = rgba.raw
        (root / name).write_bytes(data)
        (root / (name + ".rgba")).write_bytes(raw)
        at, chunks = 12, []
        while at + 8 <= len(data):
            size = struct.unpack_from("<I", data, at + 4)[0]
            tag = data[at:at + 4].decode("ascii")
            chunk = {"tag": tag, "length": size}
            if tag == "ALPH":
                chunk["compression"] = data[at + 8] & 3
            chunks.append(chunk)
            at += 8 + size + size % 2
        records.append({
            "file": name, "url": url, "author": author,
            "license": "Public domain per Google gallery credits",
            "page": "https://developers.google.com/speed/webp/gallery2",
            "sha256": digest, "extent": [width.value, height.value], "chunks": chunks,
            "reference": "system libwebp WebPDecodeRGBAInto",
            "reference_version": hex(lib.WebPGetDecoderVersion()),
            "rgba_sha256": hashlib.sha256(raw).hexdigest(),
            "partial_alpha_pixels": sum(0 < alpha < 255 for alpha in raw[3::4]),
        })
    manifest = root / "manifest.json"
    manifest.write_text(json.dumps(records, indent=2) + "\n")
    print(manifest)


if __name__ == "__main__":
    main()
