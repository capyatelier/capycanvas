#!/usr/bin/env python3
"""Independent LittleCMS writer for matrix-v2 and original-grid XYZ LUT fixtures.

No application dependency or third-party profile redistribution. Feed the output
to proof_reference.py alongside the downloaded lab profiles.
"""
import ctypes as c
import itertools
from pathlib import Path
import sys
from proof_reference import Reference, XYZ, RGB_FLOAT, XYZ_FLOAT


def main(directory):
    directory.mkdir(parents=True, exist_ok=True)
    reference = Reference()
    cms = reference.cms
    p, u, b = c.c_void_p, c.c_uint32, c.c_int
    for name, result, arguments in [
        ("cmsCreate_sRGBProfile", p, []),
        ("cmsCreateProfilePlaceholder", p, [p]),
        ("cmsSetProfileVersion", None, [p, c.c_double]),
        ("cmsSetColorSpace", None, [p, u]), ("cmsSetPCS", None, [p, u]),
        ("cmsSetDeviceClass", None, [p, u]),
        ("cmsWriteTag", b, [p, u, p]),
        ("cmsSaveProfileToFile", b, [p, c.c_char_p]),
        ("cmsPipelineAlloc", p, [p, u, u]), ("cmsPipelineFree", None, [p]),
        ("cmsPipelineInsertStage", b, [p, b, p]),
        ("cmsPipelineSetSaveAs8bitsFlag", b, [p, b]),
        ("cmsStageAllocToneCurves", p, [p, u, p]),
        ("cmsStageAllocCLut16bit", p, [p, u, u, u, p]),
    ]:
        fn = getattr(cms, name)
        fn.restype, fn.argtypes = result, arguments
    sig = lambda value: int.from_bytes(value.encode("ascii"), "big")
    source = cms.cmsCreate_sRGBProfile()
    cms.cmsSetProfileVersion(source, 2.1)
    assert cms.cmsSaveProfileToFile(source, str(directory / "matrix-v2.icc").encode())
    edge = 17
    grid = [v / (edge - 1) for point in itertools.product(range(edge), repeat=3) for v in point]
    forward = reference.apply([source, reference.xyz], [1, 1], [False, False], grid,
                              infmt=RGB_FLOAT, outfmt=XYZ_FLOAT)
    reverse = reference.apply([reference.xyz, source], [1, 1], [False, False],
                              [v * 65535 / 32768 for v in grid], infmt=XYZ_FLOAT, outfmt=RGB_FLOAT)
    forward = [v * 32768 / 65535 for v in forward]
    for version, eight, name in [(2.1, True, "mft1-xyz"), (2.1, False, "mft2-xyz"),
                                  (4.3, False, "mab-xyz")]:
        profile = cms.cmsCreateProfilePlaceholder(None)
        cms.cmsSetProfileVersion(profile, version)
        cms.cmsSetColorSpace(profile, sig("RGB "))
        cms.cmsSetPCS(profile, sig("XYZ "))
        cms.cmsSetDeviceClass(profile, sig("prtr"))
        white = XYZ(.9642, 1., .8249)
        assert cms.cmsWriteTag(profile, sig("wtpt"), c.byref(white))
        for values, tag in [(forward, "A2B0"), (reverse, "B2A0")]:
            table = (c.c_uint16 * len(values))(*[round(min(1., max(0., v)) * 65535) for v in values])
            pipeline = cms.cmsPipelineAlloc(None, 3, 3)
            assert cms.cmsPipelineInsertStage(pipeline, 1, cms.cmsStageAllocToneCurves(None, 3, None))
            assert cms.cmsPipelineInsertStage(pipeline, 1, cms.cmsStageAllocCLut16bit(None, edge, 3, 3, table))
            assert cms.cmsPipelineInsertStage(pipeline, 1, cms.cmsStageAllocToneCurves(None, 3, None))
            cms.cmsPipelineSetSaveAs8bitsFlag(pipeline, eight)
            for intent in range(3):
                assert cms.cmsWriteTag(profile, sig(tag[:3] + str(intent)), pipeline)
            cms.cmsPipelineFree(pipeline)
        assert cms.cmsSaveProfileToFile(profile, str(directory / f"{name}.icc").encode())
        cms.cmsCloseProfile(profile)
    cms.cmsCloseProfile(source)
    reference.close()


if __name__ == "__main__":
    main(Path(sys.argv[1]))
