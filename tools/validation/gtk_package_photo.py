#!/usr/bin/env python3
"""Launch a relocated GTK package with photos on a private Wayland display.

Requires dbus-run-session, Mutter and hardware Vulkan. Uses the application's
existing LAYER_UI_CAPTURE diagnostic, never the user's display or settings.
Checks that the packaged GTK is loaded and records the packaged executable's
digest. GTK and its platform dependencies are separate.
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
    parser.add_argument("--renderer", choices=["auto", "vulkan"], default="auto")
    args = parser.parse_args()
    binary, output = args.binary.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("Use an empty evidence directory")
    expected_gtk = str((binary.parent.parent / "lib/capycanvas/gtk/libgtk-4.so.1").resolve(strict=True))
    records = []
    with tempfile.TemporaryDirectory(prefix="capy-package-photo-") as temporary:
        root = Path(temporary)
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY="layer-bench-package",
                   GDK_BACKEND="wayland", GTK_A11Y="none",
                   GDK_DEBUG="no-portals:color-mgmt")
        for name in ["DISPLAY", "LD_LIBRARY_PATH"]:
            env.pop(name, None)
        env.pop("GSK_RENDERER", None)
        if args.renderer != "auto":
            env["GSK_RENDERER"] = args.renderer
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
                    loaded_gtk, high_water_kib = set(), 0
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
                                    loaded_gtk.update(line.split(maxsplit=5)[-1] for line in maps.splitlines() if "libgtk-4.so" in line)
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
                    if loaded_gtk != {expected_gtk}:
                        raise RuntimeError(f"{photo.name}: packaged GTK not used: {sorted(loaded_gtk)}")
                    record = {"photo": str(photo), "photo_sha256": hashlib.sha256(photo.read_bytes()).hexdigest(),
                        "capture": capture.name, "loaded_gtk": sorted(loaded_gtk), "sampled_vm_hwm_kib": high_water_kib,
                        "workflow_seconds": time.monotonic() - started, "exit_code": process.returncode}
                    records.append(record)
                    print(f"{photo.name}: relocated package opened and captured", flush=True)
            finally:
                compositor.terminate()
                try:
                    compositor.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    compositor.kill()
                    compositor.wait()
    (output / "report.json").write_text(json.dumps({"binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "results": records,
        "renderer": args.renderer,
        "executable_sha256": hashlib.sha256((binary.parent / "capycanvas-bin").read_bytes()).hexdigest(),
        "scope": "Photo application launch and capture; 4-second capture delay is not decode latency."}, indent=2) + "\n")


if __name__ == "__main__":
    main()
