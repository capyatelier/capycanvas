#!/usr/bin/env python3
import argparse
from contextlib import contextmanager
import getpass
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from threading import Thread


ROOT = Path(__file__).resolve().parents[2]
CRATES = ("layer-workspace", "layer-core", "layer-ui", "layer-host")
MODES = ("rust", "upgrade", "downgrade", "web", "app-data", "windows")


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass


def saved_data_result(log, strict):
    report = json.loads(log.read_text().strip().splitlines()[-1])
    intact = (report["startup"] == "stored" and not report["notices"] and not report["editing_dropped"]
              and all(store["loaded"] for store in report["stores"].values())
              and all(session["loaded"] for session in report["sessions"]))
    return ("saved data was reset or could not be loaded" if strict and not intact else "", {"report": report})


def assemble_release(root, release, destination):
    version = release.removeprefix("v")
    fixtures = {crate: root / "crates" / crate / "tests/compat" / version for crate in CRATES}
    for folder in ["config", "data/workspaces", "data/color-profiles", "state/sessions"]:
        (destination / folder).mkdir(parents=True)
    for source, target in [("settings.json", "config/settings.json"), ("export-presets", "config/export-presets"),
                           ("workspaces.sqlite3", "data/workspaces/workspaces.sqlite3")]:
        shutil.copy2(fixtures["layer-workspace"] / source, destination / target)
    shutil.copytree(fixtures["layer-host"] / "papers", destination / "data/papers")
    ui = fixtures["layer-ui"]
    for path in ui.glob("*.icc"):
        shutil.copy2(path, destination / "data/color-profiles" / path.name)
    for identity in json.loads((ui / "hidden-profiles.json").read_text()):
        (destination / "data/color-profiles" / f"{identity}.hidden").touch()
    for manifest in ui.glob("session*.json"):
        shutil.copy2(manifest, destination / "state/sessions" / manifest.name)
        for drawing in json.loads(manifest.read_text())["drawings"]:
            target = destination / "state/sessions" / drawing["key"]
            if not target.exists():
                shutil.copytree(ui / drawing["key"], target)


def cargo(crate, test, ignored=False):
    command = ["cargo", "test", "--locked", "-p", crate]
    if crate == "layer-workspace":
        command += ["--features", "native"]
    command.append(test)
    if ignored:
        command += ["--", "--ignored"]
    return command


class Runner:
    def __init__(self, args, root=ROOT):
        self.args = args
        self.root = root
        self.work = args.work.resolve()
        self.work.mkdir(parents=True, exist_ok=True)
        self.scratch = self.work / "scratch"
        self.scratch.mkdir()
        self.owned = [self.scratch]
        self.logs = self.work / "logs"
        self.logs.mkdir(exist_ok=True)
        self.checks = []
        self.releases = []
        self.env = dict(os.environ)
        for key in ("CAPY_BLESS", "CAPY_COMPAT_IN", "CAPY_COMPAT_OUT", "CAPY_RELEASE_VERSION"):
            self.env.pop(key, None)
        temporary = self.scratch / "tmp"
        temporary.mkdir()
        self.env.update(CARGO_TARGET_DIR=str(self.scratch / "target"),
                        CAPY_STORAGE_DIR=str(self.scratch / "storage"), CARGO_TERM_COLOR="never",
                        TMPDIR=str(temporary), TMP=str(temporary), TEMP=str(temporary))

    def record(self, status, name, reason="", **details):
        self.checks.append(dict(status=status, name=name, reason=reason, **details))
        print(f"{status} {name}" + (f" {reason}" if reason else ""), flush=True)

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env,
                              check=True, capture_output=True, text=True).stdout.strip()

    def select_releases(self):
        if self.args.releases == "all":
            versions = []
            for tag in self.git("tag", "--list").splitlines():
                match = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", tag)
                if match and (version := tuple(map(int, match.groups()))) >= (1, 0, 15):
                    versions.append((version, tag))
            self.releases = [tag for _, tag in sorted(versions)]
        else:
            self.releases = list(dict.fromkeys(tag.strip() for tag in self.args.releases.split(",")))
            for tag in self.releases:
                self.git("show-ref", "--verify", f"refs/tags/{tag}")

    def contains(self, release, function):
        result = subprocess.run(["git", "grep", "-q", "-F", f"fn {function}",
                                 f"refs/tags/{release}", "--", "crates"],
                                cwd=self.root, env=self.env, capture_output=True, text=True)
        if result.returncode not in (0, 1):
            raise RuntimeError(result.stderr)
        return result.returncode == 0

    def check(self, name, command, tree, extra=None, output=None, inspect=None):
        log = self.logs / f"{len(self.checks) + 1:03d}.log"
        env = dict(self.env, **(extra or {}))
        details = {}
        try:
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=tree, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT)
            reason = f"exit {result.returncode}" if result.returncode else ""
            if not reason and output is not None and not any(output.iterdir()):
                reason = "generator wrote no inputs"
            if not reason and inspect is not None:
                reason, details = inspect(log)
        except (OSError, ValueError, KeyError, IndexError) as error:
            reason = str(error)
        self.record("FAIL" if reason else "PASS", name, reason,
                    command=command, log=str(log.relative_to(self.work)), **details)
        return not reason

    @contextmanager
    def release_tree(self, release):
        tree = self.scratch / "trees" / release
        tree.parent.mkdir(parents=True, exist_ok=True)
        self.git("worktree", "add", "--detach", str(tree), f"refs/tags/{release}")
        try:
            yield tree
        finally:
            self.git("worktree", "remove", "--force", str(tree))

    def output_root(self, mode):
        output = self.work / mode
        output.mkdir()
        self.owned.append(output)
        return output

    def generate(self, tree, output, name, release=None):
        okay = True
        for crate in CRATES:
            directory = output / crate
            directory.mkdir(parents=True)
            extra = dict(CAPY_COMPAT_OUT=str(directory))
            if release:
                extra["CAPY_RELEASE_VERSION"] = release.removeprefix("v")
            okay = self.check(f"{name}.generate.{crate}", cargo(crate, "write_compat_fixtures", True),
                              tree, extra, directory) and okay
        return okay

    def load(self, tree, inputs, name):
        for crate in CRATES:
            self.check(f"{name}.load.{crate}", cargo(crate, "external_compat_inputs_load"),
                       tree, dict(CAPY_COMPAT_IN=str(inputs / crate)))

    def rust(self):
        self.check("rust.compat", ["cargo", "test", "--locked", "-p", "layer-core", "-p", "layer-ui",
                   "-p", "layer-workspace", "--features", "layer-workspace/native", "compat"], self.root)
        self.check("rust.paper-compat", cargo("layer-host", "papers::tests::compat"), self.root)
        self.check("rust.tolerance", ["cargo", "test", "--locked", "-p", "layer-core", "-p", "layer-ui",
                   "-p", "layer-workspace", "-p", "layer-host", "--features", "layer-workspace/native",
                   "saved_documents_tolerate_missing_and_unknown_fields"], self.root)

    def skip_preview(self, mode):
        self.record("SKIP", f"{mode}.gtk-preview", "GTK native_write_compat_fixtures needs a GPU; "
                    "committed app-rendered preview fixtures are covered in rust")

    def upgrade(self):
        self.skip_preview("upgrade")
        output = self.output_root("upgrade")
        for release in self.releases:
            if not self.contains(release, "write_compat_fixtures"):
                self.record("SKIP", f"upgrade.{release}", "committed fixtures cover this release in `rust`")
                continue
            with self.release_tree(release) as tree:
                if self.generate(tree, output / release, f"upgrade.{release}", release):
                    self.load(self.root, output / release, f"upgrade.{release}")

    def downgrade(self):
        self.skip_preview("downgrade")
        output = self.output_root("downgrade") / "current"
        if not self.generate(self.root, output, "downgrade.current"):
            return
        for release in self.releases:
            if not self.contains(release, "external_compat_inputs_load"):
                self.record("SKIP", f"downgrade.{release}",
                            "first eligible release is the one after this test lands")
                continue
            with self.release_tree(release) as tree:
                self.load(tree, output, f"downgrade.{release}")

    def web(self):
        browser = self.env.get("CHROME") or next((path for name in ("google-chrome", "chromium", "chromium-browser")
                                                if (path := shutil.which(name))), None)
        if browser is None:
            self.record("FAIL", "web.browser", "Chrome or Chromium was not found")
            return
        output = self.output_root("web")
        if not self.check("web.build", ["bash", "apps/layer-web/build.sh", str(output / "pkg"), "dev"], self.root):
            return
        shutil.copy2(self.root / "tools/compat/web/index.html", output / "index.html")
        shutil.copy2(self.root / "apps/layer-web/workspace-store.js", output / "workspace-store.js")
        contract = output / "contract.json"
        if not self.check("web.contract-fixture", cargo("layer-workspace", "browser_transactions_match_sqlite_contract"),
                          self.root, dict(CAPY_STORE_CONTRACT_FIXTURE=str(contract))):
            return
        server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, directory=str(output)))
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            self.check("web.indexeddb", ["node", "tools/compat/web/check.mjs", f"http://127.0.0.1:{server.server_port}/"],
                       self.root, dict(CAPY_STORE_CONTRACT_FIXTURE=str(contract), CHROME=browser))
        finally:
            server.shutdown()
            thread.join()
            server.server_close()

    def app_data(self):
        if not self.check("app-data.build", ["cargo", "build", "--locked", "-p", "layer-workspace", "--features", "native",
                          "--example", "check-saved-data"], self.root):
            return
        output = self.output_root("app-data")
        executable = Path(self.env["CARGO_TARGET_DIR"]) / "debug/examples" / ("check-saved-data.exe" if os.name == "nt" else "check-saved-data")
        for release in self.releases:
            directory = output / release
            assemble_release(self.root, release, directory)
            self.check(f"app-data.{release}", [str(executable), str(directory)], self.root,
                       inspect=lambda log: saved_data_result(log, True))
        for index, directory in enumerate(self.args.app_data):
            self.check(f"app-data.user-{index}", [str(executable), str(directory.resolve())], self.root,
                       inspect=lambda log: saved_data_result(log, False))

    def windows(self):
        state = Path(self.env.get("CAPYCANVAS_VM_DIR") or
                     Path(self.env.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "capycanvas/windows-vm")
        if sys.platform != "linux" or not os.access("/dev/kvm", os.R_OK | os.W_OK) or not (state / "base/disk.qcow2").is_file():
            self.record("SKIP", "windows.saved-data", "local Windows CPU pool needs Linux/KVM and a configured base; "
                        "Actions uses windows-readiness.yml")
            return
        name = self.env.get("CAPYCANVAS_VM") or re.sub(r"[^a-z0-9]+", "-", self.root.name.lower()).strip("-")[:32] or "default"
        helper = [sys.executable, "tools/windows-vm/windows-vm.py", "--vm", name]
        listing = subprocess.run([*helper, "list"], cwd=self.root, env=self.env,
                                 check=True, capture_output=True, text=True).stdout
        running = any(line.startswith(name + "\trunning") for line in listing.splitlines())
        output = self.output_root("windows")
        try:
            self.check("windows.saved-data", [*helper, "fixtures", "--runners", "1", "saved-data"], self.root,
                       dict(CAPYCANVAS_VM_ARTIFACTS=str(output)))
        finally:
            if not running:
                self.check("windows.stop", [*helper, "stop"], self.root)

    def finish(self):
        if not self.args.keep:
            for path in reversed(self.owned):
                try:
                    shutil.rmtree(path)
                except OSError as error:
                    self.record("FAIL", "cleanup", str(error))
        report = dict(mode=self.args.mode, releases=self.releases, checks=self.checks)
        (self.work / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        return int(any(check["status"] == "FAIL" for check in self.checks))


def main(argv=None):
    parser = argparse.ArgumentParser(description="Run saved-data compatibility checks without a GPU.")
    parser.add_argument("mode", choices=(*MODES, "all"))
    parser.add_argument("--releases", default="all", help="Comma-separated tags; default all v1.0.15+ tags")
    default = (Path(os.environ["RUNNER_TEMP"]) / "capy-compat" if "RUNNER_TEMP" in os.environ
               else Path("/mnt/localssd") / f"capy-compat-{os.environ.get('USER', getpass.getuser())}")
    parser.add_argument("--work", type=Path, default=default)
    parser.add_argument("--keep", action="store_true", help="Retain generated inputs and build scratch too")
    parser.add_argument("--app-data", type=Path, action="append", default=[], help="Additional data folders to report on copies")
    args = parser.parse_args(argv)
    try:
        runner = Runner(args)
    except OSError as error:
        print(f"FAIL work {error}", flush=True)
        return 1
    try:
        runner.select_releases()
        for mode in MODES if args.mode == "all" else (args.mode,):
            getattr(runner, mode.replace("-", "_"))()
    except (OSError, RuntimeError, subprocess.CalledProcessError, KeyboardInterrupt) as error:
        runner.record("FAIL", "runner", str(error))
    return runner.finish()


if __name__ == "__main__":
    sys.exit(main())
