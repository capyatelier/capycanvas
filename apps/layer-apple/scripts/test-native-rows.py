#!/usr/bin/env python3
"""Check UIKit row scrolling and shared contacts without building Rust."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--simulator", help="Booted iPad simulator UUID; required if several are booted")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    apple = root / "apps/layer-apple"
    token = uuid.uuid4().hex
    output = root / "artifacts/apple-native-rows" / token
    output.mkdir(parents=True)
    env = dict(os.environ)
    env.setdefault("DEVELOPER_DIR", "/Applications/Xcode.app/Contents/Developer")

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
        parser.error("Boot one iPad simulator in Xcode, or select a booted iPad with --simulator")
    destination = candidates[0]["udid"]
    sdk = run("sdk", ["xcrun", "--sdk", "iphonesimulator", "--show-sdk-path"]).strip()
    env["SDKROOT"] = sdk
    app = output / "RowChecks.app"
    app.mkdir()
    sources = ["Shared/Bridge/JSON.swift", "Shared/Bridge/ReorderContact.swift",
               "Shared/Bridge/NativeReorderModel.swift", "Shared/Bridge/AppleContextMenu.swift",
               "Shared/Bridge/AppleContextMenuRequest.swift",
               "Shared/Editor/WorkspaceRowInteraction.swift",
               "iOS/Platform/NativeReorderInput.swift",
               "tests/native-row-menus.swift"]
    run("compile", ["xcrun", "--sdk", "iphonesimulator", "swiftc", "-parse-as-library",
                    "-sdk", sdk, "-target", "arm64-apple-ios18.0-simulator",
                    *[str(apple / name) for name in sources], "-o", str(app / "RowChecks")])
    bundle = "art.capycanvas.tests.rowcallbacks.run" + token
    (app / "Info.plist").write_bytes(plistlib.dumps({
        "CFBundleIdentifier": bundle, "CFBundleExecutable": "RowChecks",
        "CFBundleName": "Row Callback Checks", "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1", "CFBundleShortVersionString": "1.0",
        "MinimumOSVersion": "18.0", "UIDeviceFamily": [2], "LSRequiresIPhoneOS": True,
    }))
    run("sign", ["codesign", "--force", "--sign", "-", str(app)])
    installed = False
    try:
        run("install", ["xcrun", "simctl", "install", destination, str(app)])
        installed = True
        result = run("run", ["xcrun", "simctl", "launch", "--console", destination, bundle], timeout=60)
        # simctl can return zero after the app aborts on a failed assertion.
        # Require the marker emitted only after every callback check completes.
        if "PASS: native row sessions" not in result or "Precondition failed" in result:
            raise RuntimeError(f"Callback checks failed; inspect {output / 'run.log'}")
        print("PASS: UIKit row contacts, two-way scrolling, cancellation and one commit per drop")
    finally:
        if installed:
            run("terminate", ["xcrun", "simctl", "terminate", destination, bundle], check=False)
            run("uninstall", ["xcrun", "simctl", "uninstall", destination, bundle])
    print(f"Local evidence: {output.relative_to(root)}")


if __name__ == "__main__":
    main()
