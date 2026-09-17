#!/usr/bin/env python3
"""Launch a relocated GTK package with photos on a private Wayland display.

Requires dbus-run-session, Mutter and hardware Vulkan. Uses the application's
existing LAYER_UI_CAPTURE diagnostic, never the user's display or settings.
Records actual loaded codec paths and the packaged executable's digest.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    if "--session" not in sys.argv:
        result = subprocess.run(["dbus-run-session", "--", sys.executable,
                                 str(Path(__file__).resolve()), "--session", *sys.argv[1:]])
        raise SystemExit(result.returncode)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session", action="store_true")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--photo", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary, output = args.binary.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("Use an empty evidence directory")
    library = binary.parent.parent / "lib/capycanvas/photo"
    expected = [library / name for name in
                ["libcapy_photo.so.1", "libheif.so.1", "libde265.so.0", "libdav1d.so.7", "libavif.so.16"]]
    expected = {str(path.resolve(strict=True)) for path in expected}
    records = []
    with tempfile.TemporaryDirectory(prefix="capy-package-photo-") as temporary:
        root = Path(temporary)
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY="layer-bench-package",
                   GDK_BACKEND="wayland", GSK_RENDERER="vulkan", GTK_A11Y="none",
                   GDK_DEBUG="no-portals:color-mgmt")
        for name in ["DISPLAY", "CAPY_PHOTO_CODEC_DIR", "CAPY_PHOTO_CODEC_PREFIX", "LD_LIBRARY_PATH"]:
            env.pop(name, None)
        with (output / "mutter.log").open("w") as log:
            compositor = subprocess.Popen(["mutter", "--headless", "--wayland", "--no-x11",
                "--virtual-monitor=1600x1000@120", "--wayland-display=layer-bench-package"],
                env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 10
                while not (runtime / "layer-bench-package").exists():
                    if compositor.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("Private compositor did not start; inspect mutter.log")
                    time.sleep(.05)
                for index, input_path in enumerate(args.photo):
                    photo = input_path.resolve(strict=True)
                    capture = output / f"{index}-{photo.stem}.png"
                    settings = root / f"job-{index}"
                    settings.mkdir()
                    photo_env = dict(env, LAYER_UI_CAPTURE=str(capture),
                        LAYER_SETTINGS_FILE=str(settings / "settings.json"),
                        CAPY_WORKSPACE_DIR=str(settings / "workspaces"),
                        CAPY_RECOVERY_DIR=str(settings / "recovery"))
                    loaded, high_water_kib = set(), 0
                    started = time.monotonic()
                    with (output / f"{index}-{photo.stem}.log").open("w") as app_log:
                        process = subprocess.Popen([str(binary), str(photo)], cwd=root,
                            env=photo_env, stdout=app_log, stderr=subprocess.STDOUT)
                        try:
                            while process.poll() is None:
                                if time.monotonic() - started > 45:
                                    raise RuntimeError(f"{photo.name}: package photo capture timed out")
                                try:
                                    maps = Path(f"/proc/{process.pid}/maps").read_text()
                                    loaded.update(line.split()[-1] for line in maps.splitlines()
                                                  if any(name in line for name in
                                                      ["libcapy_photo", "libheif", "libde265", "libdav1d", "libavif"]))
                                    status = Path(f"/proc/{process.pid}/status").read_text()
                                    for line in status.splitlines():
                                        if line.startswith("VmHWM:"):
                                            high_water_kib = max(high_water_kib, int(line.split()[1]))
                                except FileNotFoundError:
                                    pass
                                time.sleep(.025)
                            if process.returncode or not capture.is_file():
                                raise RuntimeError(f"{photo.name}: package launch failed ({process.returncode})")
                        finally:
                            if process.poll() is None:
                                process.terminate()
                                try:
                                    process.wait(timeout=5)
                                except subprocess.TimeoutExpired:
                                    process.kill()
                                    process.wait()
                    # The optional native libraries load only for HEIF/AVIF.
                    # JPEG/PNG/TIFF and the Rust raster readers do not need them.
                    with photo.open("rb") as encoded:
                        requires_native_codecs = encoded.read(12)[4:8] == b"ftyp"
                    if (requires_native_codecs or loaded) and loaded != expected:
                        raise RuntimeError(f"{photo.name}: unexpected codec libraries: {sorted(loaded)}")
                    record = {"photo": str(photo), "photo_sha256": hashlib.sha256(photo.read_bytes()).hexdigest(),
                        "capture": capture.name, "loaded_codecs": sorted(loaded), "sampled_vm_hwm_kib": high_water_kib,
                        "requires_native_codecs": requires_native_codecs,
                        "workflow_seconds": time.monotonic() - started, "exit_code": process.returncode}
                    records.append(record)
                    print(f"{photo.name}: relocated package opened and captured; "
                          f"{'bundled codecs confirmed' if requires_native_codecs else 'native codecs not required'}", flush=True)
            finally:
                compositor.terminate()
                try:
                    compositor.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    compositor.kill()
                    compositor.wait()
    (output / "report.json").write_text(json.dumps({"binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "results": records,
        "scope": "Photo application launch and capture; 4-second capture delay is not decode latency."}, indent=2) + "\n")


if __name__ == "__main__":
    main()
