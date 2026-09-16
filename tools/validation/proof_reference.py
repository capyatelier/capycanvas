#!/usr/bin/env python3
"""Generate independent print-proof references using system LittleCMS only.

First emit exact working profiles with layer-color's proof_profiles example.
Pass local licensed proof profiles with --target (repeatable). No system CMM is
linked into the app. Float XYZ references retain colors outside display gamut.
"""
import argparse
import ctypes as c
import ctypes.util
import hashlib
import json
import math
from pathlib import Path
import random
import struct


RGB_FLOAT = (1 << 22) | (4 << 16) | (3 << 3) | 4
XYZ_FLOAT = (1 << 22) | (9 << 16) | (3 << 3) | 4
LAB_FLOAT = (1 << 22) | (10 << 16) | (3 << 3) | 4
NO_OPTIMIZE = 0x100 | 0x40


class XYZ(c.Structure):
    _fields_ = [(axis, c.c_double) for axis in ("x", "y", "z")]


class Reference:
    def __init__(self):
        library = ctypes.util.find_library("lcms2")
        if not library:
            raise RuntimeError("System LittleCMS is required for independent validation")
        self.cms = c.CDLL(library)
        ptr, uint, boolean, double = c.c_void_p, c.c_uint32, c.c_int, c.c_double
        for name, result, arguments in [
            ("cmsOpenProfileFromFile", ptr, [c.c_char_p, c.c_char_p]),
            ("cmsCreateXYZProfile", ptr, []),
            ("cmsCreateLab4Profile", ptr, [ptr]),
            ("cmsCloseProfile", boolean, [ptr]),
            ("cmsGetColorSpace", uint, [ptr]),
            ("cmsChannelsOfColorSpace", c.c_int, [uint]),
            ("cmsFormatterForColorspaceOfProfile", uint, [ptr, uint, boolean]),
            ("cmsCreateExtendedTransform", ptr, [ptr, uint, c.POINTER(ptr),
                c.POINTER(boolean), c.POINTER(uint), c.POINTER(double), ptr,
                uint, uint, uint, uint]),
            ("cmsDoTransform", None, [ptr, ptr, ptr, uint]),
            ("cmsDeleteTransform", None, [ptr]),
            ("cmsDetectBlackPoint", boolean, [c.POINTER(XYZ), ptr, uint, uint]),
            ("cmsDetectDestinationBlackPoint", boolean, [c.POINTER(XYZ), ptr, uint, uint]),
            ("cmsGetEncodedCMMversion", uint, []),
        ]:
            fn = getattr(self.cms, name)
            fn.restype, fn.argtypes = result, arguments
        self.profiles = []
        self.xyz = self.cms.cmsCreateXYZProfile()
        self.lab = self.cms.cmsCreateLab4Profile(None)
        self.profiles.extend([self.xyz, self.lab])

    def open(self, path):
        handle = self.cms.cmsOpenProfileFromFile(str(path.resolve()).encode(), b"r")
        if not handle:
            raise RuntimeError(f"Cannot open {path}")
        self.profiles.append(handle)
        return handle

    def apply(self, profiles, intents, bpc, values, infmt=RGB_FLOAT, outfmt=XYZ_FLOAT,
              inch=3, outch=3):
        count = len(profiles)
        handle = self.cms.cmsCreateExtendedTransform(
            None, count, (c.c_void_p * count)(*profiles), (c.c_int * count)(*bpc),
            (c.c_uint32 * count)(*intents), (c.c_double * count)(*[1.] * count),
            None, 0, infmt, outfmt, NO_OPTIMIZE)
        if not handle:
            raise RuntimeError(f"Cannot construct reference chain: {intents}, {bpc}")
        try:
            output = (c.c_float * (len(values) // inch * outch))()
            self.cms.cmsDoTransform(handle, (c.c_float * len(values))(*values), output,
                                   len(values) // inch)
            return list(output)
        finally:
            self.cms.cmsDeleteTransform(handle)

    def black(self, profile, intent, destination):
        xyz = XYZ()
        fn = self.cms.cmsDetectDestinationBlackPoint if destination else self.cms.cmsDetectBlackPoint
        valid = bool(fn(c.byref(xyz), profile, intent, 0))
        return {"valid": valid, "xyz": [xyz.x, xyz.y, xyz.z]}

    def close(self):
        for profile in self.profiles:
            self.cms.cmsCloseProfile(profile)


def samples(depth):
    # Include corners, off-grid saturated colors, dark ramps and dark randoms.
    rng = random.Random(0x43415059)
    colors = [[r / 8, g / 8, b / 8] for r in range(9) for g in range(9) for b in range(9)]
    subsets = {"cube": [0, len(colors)]}
    start = len(colors)
    colors += [[v / 4096] * 3 for v in range(257)]
    subsets["dark_neutral"] = [start, len(colors)]
    start = len(colors)
    colors += [[rng.random() ** 3 for _ in range(3)] for _ in range(512)]
    subsets["dark_random"] = [start, len(colors)]
    start = len(colors)
    colors += [[rng.random() for _ in range(3)] for _ in range(512)]
    subsets["random"] = [start, len(colors)]
    maximum = (1 << depth) - 1
    return [round(v * maximum) / maximum for rgb in colors for v in rgb], subsets


def gamut_scores(reference, source, target, values):
    # Independent PCS round trips. The threshold and inverse-error correction
    # follow the CMM's gamut sampler, without its quantized alarm CLUT.
    lab = reference.lab
    original = reference.apply([source, lab], [1, 1], [False, False], values,
                               outfmt=LAB_FLOAT)
    once = device_roundtrip(reference, lab, target, lab, original, 1, False, 1, False,
                            infmt=LAB_FLOAT, outfmt=LAB_FLOAT)
    twice = device_roundtrip(reference, lab, target, lab, once, 1, False, 1, False,
                             infmt=LAB_FLOAT, outfmt=LAB_FLOAT)
    scores = []
    for i in range(0, len(values), 3):
        first = math.dist(original[i:i + 3], once[i:i + 3])
        second = math.dist(once[i:i + 3], twice[i:i + 3])
        scores.extend([first if second < 5 else first / second, first, second])
    return scores


def device_roundtrip(reference, source, target, output, values, intent, bpc,
                     viewing_intent, viewing_bpc, infmt=RGB_FLOAT, outfmt=XYZ_FLOAT):
    # A single optimized/multiprofile matrix round trip can preserve impossible
    # negative/above-one device RGB. Make the bounded physical-device boundary
    # explicit. Both legs, including white and black policies, remain independent
    # CMM calculations. LUT profiles already impose this bound internally.
    cms = reference.cms
    channels = cms.cmsChannelsOfColorSpace(cms.cmsGetColorSpace(target))
    fmt = cms.cmsFormatterForColorspaceOfProfile(target, 4, True)
    device = reference.apply([source, target], [intent, intent], [bpc, bpc],
                              values, infmt=infmt, outfmt=fmt, outch=channels)
    maximum = 100. if channels == 4 else 1.
    device = [max(0., min(maximum, v)) for v in device]
    return reference.apply([target, output], [1, viewing_intent], [False, viewing_bpc],
                            device, infmt=fmt, outfmt=outfmt, inch=channels)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("working_profiles", type=Path)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--target", action="append", type=Path, required=True)
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    reference = Reference()
    manifest = {"lcms_version": reference.cms.cmsGetEncodedCMMversion(),
                "flags": NO_OPTIMIZE, "adaptation": 1., "cases": [], "profiles": {},
                "device_boundary": "float device coordinates clamped between independent CMM legs",
                "gamut_components": ["score", "first_delta_e76", "repeat_delta_e76"],
                "simulation": {"adapted": "relative white, compensate viewing black",
                               "ink": "relative white, retain target black",
                               "paper": "absolute white, retain target black"}}

    def profile(path):
        name = path.name
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        previous = manifest["profiles"].get(name)
        if previous and previous["sha256"] != digest:
            raise RuntimeError(f"Duplicate profile basename: {name}")
        (args.directory / name).write_bytes(data)
        handle = reference.open(path)
        manifest["profiles"][name] = {"path": str(path.resolve()), "sha256": digest,
            "channels": data[16:20].decode("ascii"), "version_major": data[8],
            "black": [{"source": reference.black(handle, i, False),
                       "destination": reference.black(handle, i, True)} for i in range(3)]}
        return handle

    def save(name, values):
        if not all(math.isfinite(v) for v in values):
            raise RuntimeError(f"Nonfinite oracle output: {name}")
        data = struct.pack(f"<{len(values)}f", *values)
        (args.directory / name).write_bytes(data)
        return {"file": name, "sha256": hashlib.sha256(data).hexdigest()}

    try:
        sources = [(name, profile(args.working_profiles / f"{name}.icc"))
                   for name in ("srgb", "p3", "adobe", "prophoto")]
        targets = [(path.stem, path.name, profile(path)) for path in args.target]
        for depth in (8, 16):
            values, subsets = samples(depth)
            input_record = save(f"input-{depth}.f32le", values)
            for source_name, source in sources:
                for target_name, target_file, target in targets:
                    prefix = f"{source_name}-{target_name}-{depth}"
                    gamut = save(f"{prefix}-gamut.f32le", gamut_scores(reference, source, target, values))
                    for intent in range(4):
                        for bpc in (False, True) if intent != 3 else (False,):
                            for simulation in ("adapted", "ink", "paper"):
                                output = device_roundtrip(reference, source, target, reference.xyz,
                                    values, intent, bpc, 3 if simulation == "paper" else 1,
                                    simulation == "adapted")
                                record = {"source": f"{source_name}.icc", "target": target_file,
                                    "depth": depth, "intent": intent, "bpc": bpc,
                                    "simulation": simulation, "input": input_record,
                                    "subsets": subsets, "gamut": gamut,
                                    "xyz": save(f"{prefix}-{intent}-{int(bpc)}-{simulation}.f32le", output)}
                                manifest["cases"].append(record)
        (args.directory / "reference.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"LittleCMS {manifest['lcms_version']}: {len(manifest['cases'])} proof cases")
    finally:
        reference.close()


if __name__ == "__main__":
    main()
