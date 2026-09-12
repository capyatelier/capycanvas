#!/usr/bin/env python3
"""Exercise native iPad list scrolling with disposable, coordinator-created data."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import sqlite3
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--simulator", help="Available iPad simulator UUID; otherwise prefer a booted iPad")
    parser.add_argument("--output", help="New directory under the repository's ignored artifacts/")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    token = uuid.uuid4().hex
    output = (root / (args.output or f"artifacts/apple-workspace-scrolling/{token}")).resolve()
    if not output.is_relative_to(root / "artifacts"):
        parser.error("Keep raw environment details under ignored artifacts/")
    output.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ)
    env.setdefault("DEVELOPER_DIR", "/Applications/Xcode.app/Contents/Developer")
    commands = []

    def run(name, command, *, extra=None, check=True):
        print(name, flush=True)
        commands.append(command)
        (output / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
        with (output / f"{name}.log").open("w") as log:
            result = subprocess.run(command, cwd=root, env=env | (extra or {}), stdout=log,
                                    stderr=subprocess.STDOUT)
        if check and result.returncode:
            raise RuntimeError(f"{name} failed; inspect {output / (name + '.log')}")
        return (output / f"{name}.log").read_text()

    def backup(source, destination):
        destination.parent.mkdir(parents=True, exist_ok=True)
        with sqlite3.connect(source.as_uri() + "?mode=ro", uri=True) as src:
            with sqlite3.connect(destination) as dst:
                src.backup(dst)

    devices = json.loads(run("destinations", ["xcrun", "simctl", "list", "devices", "available", "--json"]))
    candidates = [d for group in devices["devices"].values() for d in group
                  if d.get("isAvailable") and "iPad" in d["name"]
                  and (args.simulator is None or d["udid"] == args.simulator)]
    if not candidates:
        raise RuntimeError("No matching available iPad simulator; install a runtime in Xcode")
    device = sorted(candidates, key=lambda d: d["state"] != "Booted")[0]
    destination = device["udid"]
    bundle = "art.capycanvas.tests.scrolling.run" + token
    namespace = str(uuid.uuid4()).upper()
    derived = output / "DerivedData"
    seed = output / "seed"
    installed = booted = False
    try:
        run("seed", ["bash", "apps/layer-apple/scripts/test-project-files.sh",
                     "apps/layer-apple/tests/workspace-switcher-seed.swift"],
            extra={"CAPY_SWITCHER_SEED_DIRECTORY": str(seed)})
        run("build", ["xcodebuild", "-project", "apps/layer-apple/CapyCanvas.xcodeproj",
                      "-scheme", "CapyCanvas-iPad", "-destination", "generic/platform=iOS Simulator",
                      "-derivedDataPath", str(derived), "-parallel-testing-enabled", "NO",
                      "CODE_SIGNING_ALLOWED=NO", f"CAPY_APPLE_BUNDLE_ID={bundle}", "build-for-testing"])
        if device["state"] != "Booted":
            run("boot", ["xcrun", "simctl", "boot", destination])
            booted = True
        run("boot-ready", ["xcrun", "simctl", "bootstatus", destination, "-b"])
        products = derived / "Build/Products"
        app = products / "Debug-iphonesimulator/CapyCanvas-iPad.app"
        run("install", ["xcrun", "simctl", "install", destination, str(app)])
        installed = True
        container = Path(run("container", ["xcrun", "simctl", "get_app_container", destination, bundle, "data"]).strip())
        database = container / "Library/Application Support" / bundle / f"test-{namespace}/workspaces.sqlite3"
        backup(seed / "workspaces.sqlite3", database)

        manifests = list(products.glob("*.xctestrun"))
        if len(manifests) != 1:
            raise RuntimeError("Expected one generated test manifest")
        manifest = plistlib.loads(manifests[0].read_bytes())
        if "TestConfigurations" in manifest:
            targets = [t for c in manifest["TestConfigurations"] for t in c["TestTargets"]]
        else:
            targets = [v for k, v in manifest.items() if not k.startswith("__")]
        for target in targets:
            target.setdefault("EnvironmentVariables", {})["CAPY_SWITCHER_SEED_NAMESPACE"] = namespace

        def relocate(value):
            if isinstance(value, str):
                return value.replace("__TESTROOT__", str(products))
            if isinstance(value, list):
                return [relocate(v) for v in value]
            if isinstance(value, dict):
                return {k: relocate(v) for k, v in value.items()}
            return value

        custom = output / "scrolling.xctestrun"
        custom.write_bytes(plistlib.dumps(relocate(manifest)))
        result = output / "scrolling.xcresult"
        run("test", ["xcodebuild", "test-without-building", "-xctestrun", str(custom),
                     "-destination", f"platform=iOS Simulator,id={destination}",
                     "-parallel-testing-enabled", "NO", "-resultBundlePath", str(result),
                     "-only-testing:CapyCanvas-iPadTests/EditorLaunchTests/testWorkspaceSwitcherScrolling"])
        summary = json.loads(run("summary", ["xcrun", "xcresulttool", "get", "test-results", "summary",
                                              "--path", str(result)]))
        if summary.get("passedTests") != 1 or summary.get("skippedTests", 0) or summary.get("failedTests", 0):
            raise RuntimeError("The scrolling selector must pass once without skips")
        # XCTest may reinstall the app and move its data container. Rediscover
        # the current sandbox instead of retaining the pre-test absolute path.
        container = Path(run("final-container", ["xcrun", "simctl", "get_app_container", destination, bundle, "data"]).strip())
        database = container / "Library/Application Support" / bundle / f"test-{namespace}/workspaces.sqlite3"
        backup(database, output / "final-workspaces.sqlite3")
        with sqlite3.connect(output / "final-workspaces.sqlite3") as db:
            order = json.loads(db.execute("SELECT workspace_ids FROM workspace_order WHERE id=1").fetchone()[0])
            pins = json.loads(db.execute("SELECT workspace_ids FROM workspace_switcher WHERE id=1").fetchone()[0])
        with sqlite3.connect(seed / "workspaces.sqlite3") as db:
            original_pins = json.loads(db.execute("SELECT workspace_ids FROM workspace_switcher WHERE id=1").fetchone()[0])
        # Before the first explicit reorder, shared policy supplies the list's
        # default order without writing a workspace_order preference row.
        original = [row["id"] for row in json.loads((seed / "fixture-rows.json").read_text())]
        painter = "builtin:workspace:painter"
        if (len(order) != 27 or order.index(painter) < 3
                or [i for i in order if i != painter] != [i for i in original if i != painter]
                or pins != original_pins):
            raise RuntimeError("The native drop must move only Painter, preserving other rows and all pins")
        print(f"PASS: native iPad scrolling fixture; results in {output.relative_to(root)}", flush=True)
    finally:
        if installed:
            run("terminate", ["xcrun", "simctl", "terminate", destination, bundle], check=False)
            if not (output / "final-workspaces.sqlite3").exists():
                try:
                    container = Path(run("failure-container", ["xcrun", "simctl", "get_app_container",
                                                               destination, bundle, "data"]).strip())
                    database = container / "Library/Application Support" / bundle / f"test-{namespace}/workspaces.sqlite3"
                    backup(database, output / "failure-workspaces.sqlite3")
                except Exception as error:
                    (output / "database-capture-error.log").write_text(str(error) + "\n")
            run("uninstall-runner", ["xcrun", "simctl", "uninstall", destination, bundle + ".tests.xctrunner"], check=False)
            run("uninstall", ["xcrun", "simctl", "uninstall", destination, bundle], check=False)
        if booted:
            run("shutdown", ["xcrun", "simctl", "shutdown", destination], check=False)


if __name__ == "__main__":
    main()
