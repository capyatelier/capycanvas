#!/usr/bin/env python3
"""Capture production SharedIcon on a booted iPad simulator, without XCTest.

Uses only Assets.car from a simulator build. A disposable app captures a shared
manifest, copies its own images out, then uninstalls; editor data is untouched.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets-app", type=Path, required=True, help="Built iPad simulator .app")
    parser.add_argument("--fixtures", type=Path, required=True, help="icon-capture.swift manifest")
    parser.add_argument("--output", type=Path, required=True, help="New local evidence directory")
    parser.add_argument("--simulator", help="Booted iPad UUID; required if several are booted")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    apple = root / "apps/layer-apple"
    assets = args.assets_app.resolve()
    if plistlib.loads((assets / "Info.plist").read_bytes()).get("DTPlatformName") != "iphonesimulator":
        parser.error("--assets-app must be a simulator build, not a signed device app")
    manifest = json.loads(args.fixtures.read_text())
    if manifest.get("schema") != 1 or not manifest.get("fixtures"):
        parser.error("Expected a nonempty schema 1 icon manifest")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ)

    def run(name, command, *, check=True, timeout=None):
        with (output / (name + ".log")).open("w") as log:
            result = subprocess.run(command, cwd=root, env=env, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=timeout)
        if check and result.returncode:
            raise RuntimeError(f"{name} failed; inspect {output / (name + '.log')}")
        return (output / (name + ".log")).read_text()

    devices = json.loads(run("destinations", ["xcrun", "simctl", "list", "devices", "available", "--json"]))
    candidates = [d for group in devices["devices"].values() for d in group
                  if d.get("isAvailable") and d["state"] == "Booted" and "iPad" in d["name"]
                  and (args.simulator is None or d["udid"] == args.simulator)]
    if len(candidates) != 1:
        parser.error("Boot one iPad simulator, or select a booted iPad with --simulator")
    destination = candidates[0]["udid"]
    sdk = run("sdk", ["xcrun", "--sdk", "iphonesimulator", "--show-sdk-path"]).strip()
    app = output / "IconCapture.app"
    app.mkdir()
    shutil.copyfile(assets / "Assets.car", app / "Assets.car")
    shutil.copyfile(args.fixtures, app / "fixtures.json")
    sources = ["Shared/Bridge/JSON.swift", "Shared/Editor/EditorStyle.swift", "tests/icon-capture-ios.swift"]
    run("compile", ["xcrun", "--sdk", "iphonesimulator", "swiftc", "-parse-as-library",
                    "-sdk", sdk, "-module-cache-path", str(output / "modules"),
                    "-target", f"{platform.machine()}-apple-ios18.0-simulator",
                    *[str(apple / name) for name in sources], "-o", str(app / "IconCapture")])
    bundle = "art.capycanvas.tests.iconcapture.run" + uuid.uuid4().hex
    (app / "Info.plist").write_bytes(plistlib.dumps({
        "CFBundleIdentifier": bundle, "CFBundleExecutable": "IconCapture",
        "CFBundleName": "Icon Capture", "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1", "CFBundleShortVersionString": "1.0",
        "MinimumOSVersion": "18.0", "UIDeviceFamily": [2], "LSRequiresIPhoneOS": True,
        "UILaunchScreen": {},
    }))
    run("sign", ["codesign", "--force", "--sign", "-", str(app)])
    installed = False
    try:
        run("install", ["xcrun", "simctl", "install", destination, str(app)])
        installed = True
        result = run("run", ["xcrun", "simctl", "launch", "--console", destination, bundle], timeout=60)
        if "PASS: UIKit shared icon captures" not in result or "Precondition failed" in result:
            raise RuntimeError(f"Icon capture failed; inspect {output / 'run.log'}")
        container = Path(run("container", ["xcrun", "simctl", "get_app_container", destination, bundle, "data"]).strip())
        captures = container / "Documents/captures"
        for fixture in manifest["fixtures"]:
            name = "native-" + fixture["name"] + ".png"
            # Accept only files from this manifest, never other simulator data.
            if Path(name).name != name:
                raise ValueError("Invalid fixture name")
            shutil.copyfile(captures / name, output / name)
        shutil.copyfile(captures / "fixtures.json", output / "fixtures.json")
    finally:
        if installed:
            run("terminate", ["xcrun", "simctl", "terminate", destination, bundle], check=False)
            run("uninstall", ["xcrun", "simctl", "uninstall", destination, bundle])
    print(f"PASS: {len(manifest['fixtures'])} UIKit simulator grids; local evidence: {args.output}")


if __name__ == "__main__":
    main()
