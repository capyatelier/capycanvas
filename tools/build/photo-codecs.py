#!/usr/bin/env python3
"""Build the pinned GTK HEIF/AVIF shared libraries without system installation.

Requires a C/C++ compiler, CMake, Ninja, Meson, pkg-config, and NASM on x86.
Network access is opt-in (--fetch); normal application builds stay offline.
The output includes corresponding upstream source archives and this build recipe.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import sys
import subprocess
import tarfile
import urllib.request

HERE = Path(__file__).resolve().parent
LOCK = HERE / "photo-codecs.json"


def run(args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), check=True, **kwargs)


def source(name, spec, cache, work, fetch):
    archive = cache / spec["archive"]
    if not archive.exists():
        if not fetch:
            raise RuntimeError(f"Missing {archive}; supply the archive or run with --fetch")
        with urllib.request.urlopen(spec["url"], timeout=60) as response:
            data = response.read(64 * 1024 * 1024 + 1)
        if len(data) > 64 * 1024 * 1024:
            raise RuntimeError(f"{name}: download exceeds the source archive limit")
        if hashlib.sha256(data).hexdigest() != spec["sha256"]:
            raise RuntimeError(f"{name}: source digest mismatch")
        temporary = archive.with_suffix(archive.suffix + ".part")
        temporary.write_bytes(data)
        temporary.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != spec["sha256"]:
        raise RuntimeError(f"{archive}: source digest mismatch")
    directory = work / f"{name}-{spec['version']}"
    # Re-extract verified source on every build. Generated build trees are separate.
    with tarfile.open(archive) as package:
        for entry in package.getmembers():
            if Path(entry.name).parts[0] != directory.name:
                raise RuntimeError(f"{name}: unexpected archive root")
        package.extractall(work, filter="data")
    return directory, archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=HERE.parents[1] / "target/photo-codecs")
    parser.add_argument("--cache", type=Path)
    parser.add_argument("--fetch", action="store_true")
    parser.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    parser.add_argument("--cmake", default="cmake")
    args = parser.parse_args()
    if platform.system() != "Linux" or sys.byteorder != "little" or args.jobs < 1:
        parser.error("This bundle targets Linux and requires a positive job count")
    for tool in [args.cmake, "ninja", "meson", "pkg-config", "c++", "patch"]:
        if not shutil.which(tool):
            parser.error(f"Missing build tool: {tool}")
    if platform.machine() in ("x86_64", "i386", "i686") and not shutil.which("nasm"):
        parser.error("NASM is required for dav1d's optimized x86 decoder")
    output = args.output.resolve()
    marker = output / ".capy-photo-codecs"
    if output.exists() and any(output.iterdir()) and not marker.exists():
        parser.error("Refusing to modify an unmarked nonempty codec output directory")
    output.mkdir(parents=True, exist_ok=True)
    marker.write_text("Generated Capy Canvas photo codec bundle\n")
    work, prefix = output / "build", output / "prefix"
    cache = (args.cache or output / "downloads").resolve()
    for directory in (work, prefix, cache):
        directory.mkdir(parents=True, exist_ok=True)
    dependencies = json.loads(LOCK.read_text())
    sources = {name: source(name, spec, cache, work, args.fetch)
               for name, spec in dependencies.items()}
    patch = HERE / "libheif-source-profile.patch"
    if not patch.exists():
        patch = HERE.parents[1] / "vendor/libheif-source-profile.patch"
    run(["patch", "--batch", "--fuzz=0", "-p1", "-i", patch], cwd=sources["libheif"][0])
    env = {**os.environ, "PKG_CONFIG_PATH": str(prefix / "lib/pkgconfig")}
    common = [args.cmake, "-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release",
              f"-DCMAKE_INSTALL_PREFIX={prefix}", "-DCMAKE_INSTALL_LIBDIR=lib",
              "-DCMAKE_INSTALL_RPATH=$ORIGIN", "-DCMAKE_BUILD_WITH_INSTALL_RPATH=OFF",
              "-DBUILD_SHARED_LIBS=ON"]

    def cmake(name, options):
        directory = work / (name + "-build")
        run([*common, "-S", sources[name][0], "-B", directory, *options], env=env)
        run([args.cmake, "--build", directory, "--parallel", args.jobs], env=env)
        run([args.cmake, "--install", directory, "--strip"], env=env)

    cmake("libde265", ["-DENABLE_DECODER=OFF", "-DENABLE_ENCODER=OFF", "-DENABLE_SDL=OFF"])
    dav1d = work / "dav1d-build"
    run(["meson", "setup", *( ["--reconfigure"] if (dav1d / "meson-private/coredata.dat").exists() else []),
         dav1d, sources["dav1d"][0], "--prefix", prefix, "--libdir=lib",
         "--buildtype=release", "--default-library=shared", "-Denable_asm=true",
         "-Denable_tools=false", "-Denable_tests=false"], env=env)
    run(["meson", "compile", "-C", dav1d, "-j", args.jobs], env=env)
    run(["meson", "install", "-C", dav1d, "--no-rebuild", "--strip"], env=env)
    cmake("libaom", ["-DCONFIG_PIC=1", "-DENABLE_DOCS=OFF", "-DENABLE_EXAMPLES=OFF",
                     "-DENABLE_TESTDATA=OFF", "-DENABLE_TESTS=OFF", "-DENABLE_TOOLS=OFF"])
    cmake("libjpeg-turbo", ["-DENABLE_STATIC=OFF", "-DWITH_TURBOJPEG=OFF", "-DWITH_TOOLS=OFF",
                           "-DCMAKE_POSITION_INDEPENDENT_CODE=ON"])
    cmake("libultrahdr", [f"-DCMAKE_PREFIX_PATH={prefix}", "-DUHDR_BUILD_DEPS=OFF",
                         "-DUHDR_BUILD_EXAMPLES=OFF", "-DUHDR_BUILD_TESTS=OFF",
                         "-DUHDR_ENABLE_HEIF=OFF", "-DUHDR_ENABLE_INSTALL=ON",
                         "-DUHDR_ENABLE_GLES=OFF", "-DUHDR_WRITE_XMP=ON", "-DUHDR_WRITE_ISO=ON",
                         "-DUHDR_MAX_DIMENSION=32768"])
    cmake("libavif", [f"-DCMAKE_PREFIX_PATH={prefix}", "-DAVIF_CODEC_DAV1D=SYSTEM",
                      "-DAVIF_CODEC_AOM=SYSTEM", "-DAVIF_LIBYUV=OFF", "-DAVIF_LIBSHARPYUV=OFF",
                      "-DAVIF_LIBXML2=OFF", "-DAVIF_BUILD_APPS=OFF",
                      "-DAVIF_BUILD_TESTS=OFF", "-DAVIF_BUILD_EXAMPLES=OFF",
                      *[f"-DAVIF_CODEC_{name}=OFF" for name in ("LIBGAV1", "RAV1E", "SVT", "AVM")]])
    disabled = ["X265", "KVAZAAR", "UVG266", "VVDEC", "VVENC", "X264",
                "OpenH264_DECODER", "AOM_DECODER", "AOM_ENCODER", "SvtEnc", "RAV1E",
                "JPEG_DECODER", "JPEG_ENCODER", "OpenJPEG_ENCODER", "OpenJPEG_DECODER",
                "FFMPEG_DECODER", "OPENJPH_ENCODER", "UNCOMPRESSED_CODEC", "WEBCODECS",
                "LIBSHARPYUV", "HEADER_COMPRESSION", "EXAMPLES", "GDK_PIXBUF"]
    cmake("libheif", [f"-DCMAKE_PREFIX_PATH={prefix}", "-DENABLE_PLUGIN_LOADING=OFF",
                      "-DWITH_LIBDE265=ON", "-DWITH_LIBDE265_PLUGIN=OFF",
                      "-DWITH_DAV1D=OFF", "-DWITH_DAV1D_PLUGIN=OFF",
                      "-DBUILD_TESTING=OFF", "-DBUILD_DOCUMENTATION=OFF",
                      *[f"-DWITH_{name}=OFF" for name in disabled]])
    # Probe the installed HEIF library. AVIF uses libavif/dav1d below.
    lib = ctypes.CDLL(str(prefix / "lib/libheif.so.1"))
    lib.heif_get_version.restype = ctypes.c_char_p
    lib.heif_have_decoder_for_format.argtypes = [ctypes.c_int]
    version = lib.heif_get_version().decode()
    if version != dependencies["libheif"]["version"]:
        raise RuntimeError(f"Unexpected libheif version: {version}")
    if not lib.heif_have_decoder_for_format(1):
        raise RuntimeError("The bundle has no HEVC decoder")
    bridge = HERE / "heif_bridge.c"
    if not bridge.exists():
        bridge = HERE.parents[1] / "crates/layer-color/src/photo/heif_bridge.c"
    run(["cc", "-std=c11", "-O2", "-fPIC", "-shared", "-Wall", "-Wextra", "-Werror",
         "-Wl,-soname,libcapy_photo.so.1", "-Wl,-rpath,$ORIGIN", "-Wl,-z,defs",
         "-I", prefix / "include", bridge, "-L", prefix / "lib", "-lheif", "-lavif",
         "-o", prefix / "lib/libcapy_photo.so.1"])
    bridge_lib = ctypes.CDLL(str(prefix / "lib/libcapy_photo.so.1"))
    bridge_lib.capy_photo_avif_version.restype = ctypes.c_char_p
    if bridge_lib.capy_photo_abi() != 3 or bridge_lib.capy_photo_avif_version().decode() != dependencies["libavif"]["version"]:
        raise RuntimeError("Unexpected photo bridge ABI or libavif version")
    if not all(bridge_lib.capy_photo_decoder(format) for format in (1, 4)):
        raise RuntimeError("The bridge requires both HEVC and AV1 decoders")
    hdr_worker = HERE / "hdr_codec.cpp"
    if not hdr_worker.exists():
        hdr_worker = HERE.parents[1] / "crates/layer-color/src/photo/hdr_codec.cpp"
    run(["c++", "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror",
         "-I", prefix / "include", hdr_worker, "-L", prefix / "lib",
         "-Wl,-rpath,$ORIGIN", "-luhdr", "-lavif", "-ljpeg",
         "-o", prefix / "lib/capy-hdr-codec"])
    if subprocess.check_output([prefix / "lib/capy-hdr-codec", "--version"]).strip() != b"capy-hdr-codec 1":
        raise RuntimeError("Unexpected HDR codec protocol")
    (prefix / "lib/hdr-codec-abi").write_text("1\n")
    docs = prefix / "share/doc/capycanvas-photo-codecs"
    docs.mkdir(parents=True, exist_ok=True)
    (docs / "sources").mkdir(exist_ok=True)
    for name, (directory, archive) in sources.items():
        dest = docs / name
        dest.mkdir(exist_ok=True)
        shutil.copy2(archive, docs / "sources")
        for license in dependencies[name]["licenses"]:
            shutil.copy2(directory / license, dest)
    for path in (LOCK, Path(__file__), bridge, patch, hdr_worker):
        shutil.copy2(path, docs)
    libraries = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in (prefix / "lib").glob("*.so.*") if not p.is_symlink()}
    manifest = {"dependencies": dependencies, "libraries": libraries,
                "machine": platform.machine(), "decoders": ["HEIF/libheif/libde265", "AVIF/libavif/dav1d"],
                "hdr_codecs": ["JPEG/libultrahdr", "AVIF/libavif/libaom"],
                "hdr_worker_sha256": hashlib.sha256((prefix / "lib/capy-hdr-codec").read_bytes()).hexdigest(),
                "dynamic_plugins": False, "recipe_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "bridge_sha256": hashlib.sha256(bridge.read_bytes()).hexdigest(),
                "patch_sha256": hashlib.sha256(patch.read_bytes()).hexdigest()}
    (prefix / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Verified photo codec bundle: {prefix}")


if __name__ == "__main__":
    main()
