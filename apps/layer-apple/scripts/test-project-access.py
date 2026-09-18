#!/usr/bin/env python3
"""Exercise production project writes with a real, file-only App Sandbox grant."""
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[3]
apple = repo / "apps/layer-apple"
env = os.environ | {"DEVELOPER_DIR": "/Applications/Xcode.app/Contents/Developer",
                    "MACOSX_DEPLOYMENT_TARGET": "15.0"}
with tempfile.TemporaryDirectory(prefix="capy-project-access-") as temporary:
    root = Path(temporary)
    content = root / "Access.app/Contents"
    (content / "MacOS").mkdir(parents=True)
    (content / "Resources").mkdir()
    (root / "selected").mkdir()
    source = root / "selected/drawing.capy"
    binary = content / "MacOS/Access"
    (content / "Info.plist").write_bytes(plistlib.dumps({
        "CFBundleExecutable": "Access", "CFBundleIdentifier": "art.capycanvas.tests.project-access",
        "CFBundlePackageType": "APPL"}))
    entitlements = root / "entitlements.plist"
    entitlements.write_bytes(plistlib.dumps({"com.apple.security.app-sandbox": True,
                                           "com.apple.security.files.user-selected.read-write": True}))
    subprocess.run(["cargo", "build", "--offline", "-p", "layer-apple", "--target", "aarch64-apple-darwin"],
                   cwd=repo, env=env, check=True)
    target = Path(env.get("CARGO_TARGET_DIR", repo / "target"))
    subprocess.run(["xcrun", "swiftc", "-import-objc-header", str(apple / "native/include/CapyApple.h"),
                    str(apple / "Shared/Bridge/JSON.swift"), str(apple / "Shared/Bridge/ProjectFileIO.swift"),
                    str(apple / "tests/project-file-access.swift"), "-L", str(target / "aarch64-apple-darwin/debug"),
                    "-llayer_apple", "-lc++", "-framework", "Metal", "-framework", "QuartzCore",
                    "-framework", "Security", "-framework", "AppKit", "-o", str(binary)], env=env, check=True)
    subprocess.run([str(binary), str(source), str(content / "Resources/input.bookmark")], env=env, check=True)
    subprocess.run(["codesign", "--force", "--sign", "-", "--entitlements", str(entitlements), str(content.parent)],
                   env=env, check=True)
    subprocess.run([str(binary)], env=env, check=True)
    assert source.read_bytes() == b"saved revision 3"
    assert list(source.parent.iterdir()) == [source], "No temporary siblings or unrelated writes"
