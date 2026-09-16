#!/usr/bin/env python3
"""Generate independent ICC samples for layer-color's optional reference test.

Only this offline validation tool uses the system LittleCMS library. The app and
its Rust tests have no LittleCMS build/runtime dependency. Supply a local, licensed
CMYK output profile; generated samples and provenance belong under artifacts/.
"""
import argparse
import ctypes as c
import ctypes.util
import hashlib
import json
from pathlib import Path
import struct


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", type=Path)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    library = ctypes.util.find_library("lcms2")
    if not library:
        parser.error("reference generation requires system LittleCMS")
    cms = c.CDLL(library)
    for name, result, arguments in [
        ("cmsCreate_sRGBProfile", c.c_void_p, []),
        ("cmsOpenProfileFromFile", c.c_void_p, [c.c_char_p, c.c_char_p]),
        ("cmsCloseProfile", c.c_int, [c.c_void_p]),
        ("cmsCreateTransform", c.c_void_p, [c.c_void_p, c.c_uint, c.c_void_p, c.c_uint, c.c_uint, c.c_uint]),
        ("cmsDoTransform", None, [c.c_void_p, c.c_void_p, c.c_void_p, c.c_uint]),
        ("cmsDeleteTransform", None, [c.c_void_p]),
        ("cmsGetEncodedCMMversion", c.c_uint, []),
    ]:
        function = getattr(cms, name)
        function.restype, function.argtypes = result, arguments
    rgb = cms.cmsCreate_sRGBProfile()
    cmyk = cms.cmsOpenProfileFromFile(str(args.profile.resolve()).encode(), b"r")
    if not rgb or not cmyk:
        raise RuntimeError("Cannot open reference profiles")
    args.directory.mkdir(parents=True, exist_ok=True)
    files = {}
    # ICC floating point formatters: RGB in [0,1], CMYK in percent.
    rgb_float, cmyk_float = 4456476, 4587556
    try:
        for intent in range(4):
            for name, channels, out_channels, steps, source, target, infmt, outfmt, maximum in [
                ("rgb-to-cmyk", 3, 4, 9, rgb, cmyk, rgb_float, cmyk_float, 1.),
                ("cmyk-to-rgb", 4, 3, 5, cmyk, rgb, cmyk_float, rgb_float, 100.),
            ]:
                count = steps ** channels
                values = [((i // steps ** channel) % steps) / (steps - 1) * maximum
                          for i in range(count) for channel in range(channels)]
                input_values = (c.c_float * len(values))(*values)
                output_values = (c.c_float * (count * out_channels))()
                # No BPC, no optimizer, no cache: independent float reference.
                transform = cms.cmsCreateTransform(source, infmt, target, outfmt, intent, 0x100 | 0x40)
                if not transform:
                    raise RuntimeError(f"Cannot construct {name} intent {intent}")
                try:
                    cms.cmsDoTransform(transform, input_values, output_values, count)
                finally:
                    cms.cmsDeleteTransform(transform)
                path = args.directory / f"{name}-{intent}.f32le"
                path.write_bytes(struct.pack(f"<{len(output_values)}f", *output_values))
                files[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
    finally:
        cms.cmsCloseProfile(rgb)
        cms.cmsCloseProfile(cmyk)
    provenance = {"lcms_version": cms.cmsGetEncodedCMMversion(), "bpc": False,
                  "profile": str(args.profile.resolve()),
                  "profile_sha256": hashlib.sha256(args.profile.read_bytes()).hexdigest(),
                  "fixtures": files}
    (args.directory / "reference.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(json.dumps(provenance, indent=2))


if __name__ == "__main__":
    main()
