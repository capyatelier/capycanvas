#!/usr/bin/env python3
"""Capture one GTK store scene in a fresh private home and Wayland session."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
NATIVE = "workspace::tests::store_capture::native_store_capture"
CAPABILITIES = "workspace::tests::store_capture::store_capture_capabilities"


def scene_recipe(path, selected=None):
    raw = path.read_bytes()
    recipe = json.loads(raw)
    if not isinstance(recipe, dict) or set(recipe) != {"scenes"}:
        raise ValueError("Recipe must contain only a scenes array")
    scenes = recipe["scenes"]
    if not isinstance(scenes, list) or not scenes:
        raise ValueError("Recipe needs at least one scene")
    seen = set()
    resolved = []
    for scene in scenes:
        if not isinstance(scene, dict) or set(scene) - {"id", "source", "workspace", "steps"}:
            raise ValueError("Unknown scene fields")
        name = scene.get("id")
        if not isinstance(name, str) or not re.fullmatch(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*", name) or name in seen:
            raise ValueError("Scene IDs must be unique lowercase slugs")
        seen.add(name)
        if scene.get("workspace") not in {"painter", "illustrator", "photographer"}:
            raise ValueError("Unknown workspace")
        if not isinstance(scene.get("source"), str) or not scene["source"]:
            raise ValueError("Scene source must be a file path")
        source = (path.parent / scene["source"]).resolve(strict=True)
        if not source.is_file() or not isinstance(scene.get("steps", []), list):
            raise ValueError("Scene needs an existing source file and a steps array")
        resolved.append(dict(scene, source=str(source), steps=scene.get("steps", [])))
    choices = [scene for scene in resolved if selected is None or scene["id"] == selected]
    if len(choices) != 1:
        raise ValueError("Select exactly one scene with --scene")
    return choices[0], hashlib.sha256(raw).hexdigest()


def validate_size(width, height, scale):
    if not 320 <= width <= 4096 or not 240 <= height <= 4096 or not 1 <= scale <= 4:
        raise ValueError("Width must be 320–4096, height 240–4096, and integer scale 1–4")


def prepare_output(path):
    if path.is_symlink() or (path.exists() and (not path.is_dir() or any(path.iterdir()))):
        raise ValueError("Output must be a new or empty directory, without a symlink")
    path = path.resolve()
    worktrees = subprocess.check_output(["git", "worktree", "list", "--porcelain"], cwd=ROOT, text=True)
    for line in worktrees.splitlines():
        if line.startswith("worktree "):
            other = Path(line[9:]).resolve()
            if other != ROOT and path.is_relative_to(other):
                raise ValueError("Output cannot be inside another worktree")
    path.mkdir(parents=True, exist_ok=True)
    session = path / "session"
    session.mkdir()
    for name in ["home", "config", "data", "cache", "state", "tmp"]:
        (session / name).mkdir(mode=0o700)
    return path


def artifact_executable(messages):
    found = set()
    for line in messages.splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (item.get("reason") == "compiler-artifact" and item.get("profile", {}).get("test")
                and item.get("target", {}).get("name") in {"layer-linux", "layer_linux"} and item.get("executable")):
            found.add(item["executable"])
    if len(found) != 1:
        raise ValueError("Cargo did not report exactly one GTK test executable")
    return Path(found.pop()).resolve(strict=True)


def namespace(output, executable, sources, command):
    session = output / "session"
    home = Path.home().resolve()
    args = ["bwrap", "--ro-bind", "/", "/", "--bind", str(session / "home"), str(home),
            "--ro-bind", str(ROOT), str(ROOT), "--dev-bind", "/dev", "/dev", "--proc", "/proc",
            "--unshare-pid", "--bind", str(session / "tmp"), "/tmp", "--tmpfs", "/run/user",
            "--dir", f"/run/user/{os.getuid()}", "--bind", str(output), str(output), "--chdir", str(ROOT)]
    for path in dict.fromkeys([executable, *sources]):
        args += ["--ro-bind", str(path), str(path)]
    for key, value in {"PATH": "/usr/bin:/bin", "GSETTINGS_BACKEND": "memory",
                       **{f"XDG_{name.upper()}_HOME": session / name for name in ["config", "data", "cache", "state"]},
                       "XDG_RUNTIME_DIR": f"/run/user/{os.getuid()}"}.items():
        args += ["--setenv", key, str(value)]
    return args + command


def check_manifest(output, job):
    manifest = json.loads((output / "capture.json").read_text())
    for key in ["source_revision", "source_dirty", "executable_sha256", "executable_override", "recipe_sha256", "width", "height", "scale", "languages", "themes"]:
        if manifest.get(key) != job[key]:
            raise ValueError(f"Capture metadata mismatch: {key}")
    supported = manifest.get("supported_languages", [])
    if not isinstance(supported, list) or not supported or job["languages"][0] not in supported:
        raise ValueError("Capture language is not registered")
    captures = manifest.get("captures", [])
    if not isinstance(captures, list) or len(captures) != 1:
        raise ValueError("Expected exactly one capture")
    capture = captures[0]
    for key, expected in {"scene": job["scenes"][0]["id"], "language": job["languages"][0], "theme": job["themes"][0]}.items():
        if capture.get(key) != expected:
            raise ValueError(f"Capture variant mismatch: {key}")
    files = []
    for key in ["image", "sidecar"]:
        relative = Path(capture[key])
        path = (output / relative).resolve(strict=True)
        if relative.is_absolute() or not path.is_relative_to(output / "images") or not path.is_file():
            raise ValueError("Capture files must remain inside output/images")
        files.append(path)
    header = files[0].read_bytes()[:33]
    if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR" or len(header) != 33:
        raise ValueError("Capture is not a PNG")
    dimensions = list(struct.unpack(">II", header[16:24]))
    sidecar = json.loads(files[1].read_text())
    alpha = sidecar.get("alpha", {})
    counts = [alpha.get(key, -1) for key in ["transparent", "partial", "opaque"]]
    if (sidecar.get("dimensions") != dimensions or min(dimensions) <= 0 or min(counts) < 0
            or sum(counts) != dimensions[0] * dimensions[1] or not counts[0] or not counts[2]
            or alpha.get("corners") != [0, 0, 0, 0]):
        raise ValueError("Capture dimensions or transparent window metadata is invalid")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--recipe", type=Path)
    parser.add_argument("--scene", help="Select one scene from a multi-scene recipe")
    parser.add_argument("--output", type=Path, help="New or empty directory; failure evidence is retained")
    parser.add_argument("--language", default="en", help="Registered BCP47 language tag")
    parser.add_argument("--theme", choices=["light", "dark"], default="light")
    parser.add_argument("--width", type=int, default=1200)
    parser.add_argument("--height", type=int, default=800)
    parser.add_argument("--scale", type=int, default=2)
    parser.add_argument("--executable", type=Path, help="Use an already built GTK test executable")
    parser.add_argument("--list-languages", action="store_true", help="Print native language/theme capabilities as JSON")
    args = parser.parse_args()
    try:
        validate_size(args.width, args.height, args.scale)
        if not re.fullmatch(r"[A-Za-z]{2,8}(?:-[A-Za-z0-9]{1,8})*", args.language):
            raise ValueError("Language must be a BCP47 tag")
        if not args.list_languages and (args.recipe is None or args.output is None):
            raise ValueError("Capture requires --recipe and --output")
        scene, digest = scene_recipe(args.recipe.resolve(strict=True), args.scene) if not args.list_languages else (None, None)
        output = prepare_output(args.output or Path(tempfile.mkdtemp(prefix="capy-store-capabilities-")))
        executable = args.executable.resolve(strict=True) if args.executable else None
        if executable is None:
            build = subprocess.run(["cargo", "test", "--locked", "--release", "-p", "layer-linux", "--no-run", "--message-format=json"], cwd=ROOT, text=True, capture_output=True)
            (output / "session/build.log").write_text(build.stdout + build.stderr)
            build.check_returncode()
            executable = artifact_executable(build.stdout)
        if not executable.is_file() or not os.access(executable, os.X_OK):
            raise ValueError("Test executable must be an executable file")
        env = {key: value for key, value in os.environ.items() if key not in {"DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE", "LD_LIBRARY_PATH", "CAPY_STORAGE_DIR", "LAYER_UI_CAPTURE", "GDK_DEBUG"}}
        if args.list_languages:
            target = output / "session/capabilities.json"
            env["CAPY_GTK_STORE_CAPABILITIES"] = str(target)
            command = [str(executable), CAPABILITIES, "--exact", "--ignored", "--test-threads=1"]
            sources = []
        else:
            with executable.open("rb") as binary:
                executable_sha256 = hashlib.file_digest(binary, "sha256").hexdigest()
            job = dict(scenes=[scene], languages=[args.language], themes=[args.theme], width=args.width, height=args.height, scale=args.scale,
                       output=str(output), source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), recipe_sha256=digest,
                       source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, text=True).strip()),
                       executable_sha256=executable_sha256, executable_override=args.executable is not None)
            target = output / "session/job.json"
            target.write_text(json.dumps(job, indent=2) + "\n")
            (output / "images").mkdir()
            env.update(CAPY_GTK_STORE_JOB=str(target), LAYER_NATIVE_CAPTURE_DIR=str(output / "images"), LAYER_NATIVE_TEST_EXECUTABLE=str(executable),
                       LAYER_MOTION_VIEWPORT=f"{(args.width + 400) * args.scale}x{(args.height + 300) * args.scale}", LAYER_MOTION_SCALE=str(args.scale))
            command = ["/bin/bash", "tools/performance/workspace-motion.sh", "gtk", "--native-test=native_store_capture", "--native-storage"]
            sources = [Path(scene["source"])]
        with (output / "session/run.log").open("w") as log:
            subprocess.run(namespace(output, executable, sources, command), cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        print(json.dumps(json.loads(target.read_text()) if args.list_languages else check_manifest(output, job), indent=2))
    except (ValueError, OSError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
