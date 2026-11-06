#!/usr/bin/env python3
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import time

DEFAULT_ROOT = Path("/mnt/localssd/capy9-android")
PROFILES = {"phone412": (412, 891, 160), "phone393": (1080, 2340, 420)}
SIZES = {"tablet1280": "1280x800"}
TARGET = "android-35"
DESKTOP_WINDOWING = ("lse_desktop_experience", "com.android.window.flags.enable_desktop_windowing_mode")
CONFIG = {
    "PlayStore.enabled": "true", "abi.type": "x86_64", "avd.ini.encoding": "UTF-8", "disk.dataPartition.size": "6G",
    "hw.accelerometer": "yes", "hw.audioInput": "no", "hw.battery": "yes", "hw.camera.back": "none", "hw.camera.front": "none",
    "hw.cpu.arch": "x86_64", "hw.cpu.ncore": "4", "hw.gpu.enabled": "yes", "hw.gpu.mode": "host", "hw.keyboard": "yes",
    "hw.mainKeys": "no", "hw.ramSize": "4096", "hw.sdCard": "no",
    "image.sysdir.1": f"system-images/{TARGET}/google_apis_playstore_tablet/x86_64/", "showDeviceFrame": "no", "skin.dynamic": "yes",
    "tag.display": "Google APIs PlayStore", "tag.displaynames": "Google APIs PlayStore,Tablet",
    "tag.id": "google_apis_playstore", "tag.ids": "google_apis_playstore,tablet", "target": TARGET}


def run(command, check=True):
    return subprocess.run(command, check=check, capture_output=True, text=True, timeout=30).stdout.strip()


def identity(pid):
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return None if fields[0] == "Z" else fields[19]
    except (FileNotFoundError, ProcessLookupError):
        return None


def alive(record):
    return record.get("start_ticks") is not None and identity(record["pid"]) == record["start_ticks"]


def wait_for(predicate, timeout, label):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= deadline:
            raise RuntimeError(f"Timed out waiting for {label}")
        time.sleep(.25)


def free_port(port):
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False


class Harness:
    def __init__(self, root, sdk):
        self.root = Path(root).resolve()
        self.sdk = Path(sdk)
        self.path = self.root / "emulators.json"
        self.state = json.loads(self.path.read_text()) if self.path.exists() else {"emulators": [], "display": []}

    def save(self):
        temporary = self.path.with_suffix(".new")
        temporary.write_text(json.dumps(self.state, indent=2) + "\n")
        temporary.replace(self.path)

    def spawn(self, command, name, env):
        with (self.root / f"{name}.log").open("ab") as output:
            process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, env=env, start_new_session=True)
        return {"pid": process.pid, "start_ticks": identity(process.pid), "command": command}

    def environment(self):
        env = os.environ.copy()
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"):
            env.pop(name, None)
        runtime = self.root / "run"
        runtime.mkdir(mode=0o700, exist_ok=True)
        runtime.chmod(0o700)
        env.update(XDG_RUNTIME_DIR=str(runtime), ANDROID_HOME=str(self.sdk),
                   ANDROID_AVD_HOME=str(self.root / "avd"), ANDROID_EMULATOR_HOME=str(self.root / "emulator-home"),
                   ANDROID_USER_HOME=str(self.root / "android-user"), TMPDIR=str(self.root / "tmp"))
        for name in ("ANDROID_EMULATOR_HOME", "ANDROID_USER_HOME", "TMPDIR"):
            Path(env[name]).mkdir(exist_ok=True)
        return env

    def display(self):
        env = self.environment()
        if self.state["display"]:
            if not all(alive(record) for record in self.state["display"]):
                raise RuntimeError("Recorded display has stopped; run stop --all-recorded before restarting")
            return env | self.state["display_environment"]
        number = next((number for number in range(97, 113)
                       if not Path(f"/tmp/.X11-unix/X{number}").exists() and not Path(f"/tmp/.X{number}-lock").exists()), None)
        if number is None:
            raise RuntimeError("No unused private X display in :97–:112")
        wayland = "capy-emulators-" + str(os.getpid())
        mutter = self.spawn(["mutter", "--headless", "--wayland", "--no-x11", "--virtual-monitor", "1280x1024", "--wayland-display=" + wayland], "mutter", env)
        self.state["display"] = [mutter]
        self.state["display_environment"] = {"DISPLAY": f":{number}", "WAYLAND_DISPLAY": wayland}
        self.save()
        wait_for(lambda: self.ready_process(mutter, Path(env["XDG_RUNTIME_DIR"]) / wayland), 30, "private Wayland display")
        env["WAYLAND_DISPLAY"] = wayland
        xwayland = self.spawn(["Xwayland", f":{number}", "-noreset", "-nolisten", "tcp"], "xwayland", env)
        self.state["display"].append(xwayland)
        self.save()
        wait_for(lambda: self.ready_process(xwayland, Path(f"/tmp/.X11-unix/X{number}")), 30, "private X display")
        return env | self.state["display_environment"]

    def ready_process(self, record, path):
        if not alive(record):
            raise RuntimeError(f"Process exited: {record['command']}; inspect {self.root}")
        return path.exists()

    def avd(self, profile):
        directory = self.root / "avd" / f"{profile}.avd"
        width, height, density = PROFILES[profile]
        config = CONFIG | {"AvdId": profile, "avd.ini.displayname": profile,
                           "hw.lcd.width": str(width), "hw.lcd.height": str(height), "hw.lcd.density": str(density)}
        text = "".join(f"{key}={config[key]}\n" for key in sorted(config))
        path = directory / "config.ini"
        if path.exists() and path.read_text() != text:
            raise RuntimeError(f"{directory} was created for another configuration; remove it to recreate {profile}")
        directory.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        directory.with_suffix(".ini").write_text(f"avd.ini.encoding=UTF-8\npath={directory}\ntarget={TARGET}\n")
        return not (directory / "userdata-qemu.img").exists()

    def adb(self, serial, *arguments, check=True):
        record = next((record for record in self.state["emulators"] if record["serial"] == serial), None)
        if not re.fullmatch(r"emulator-55[6-8][02468]", serial) or record is None or not alive(record):
            raise ValueError(f"Not a live emulator started by this launcher: {serial}")
        return run([str(self.sdk / "platform-tools/adb"), "-s", serial, *arguments], check)

    def booted(self, record, previous=None):
        current = {}
        def ready():
            if not alive(record):
                raise RuntimeError(f"Emulator exited: {record['serial']}; inspect its log")
            try:
                completed, current["boot_id"] = self.adb(record["serial"], "shell", "getprop sys.boot_completed; cat /proc/sys/kernel/random/boot_id").split()
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired, ValueError):
                return False
            return completed == "1" and current["boot_id"] != previous
        wait_for(ready, 600, record["serial"] + " boot")
        return current["boot_id"]

    def boot(self, record):
        started = time.monotonic()
        boot_id = self.booted(record)
        if self.adb(record["serial"], "shell", "device_config", "get", *DESKTOP_WINDOWING) != "false":
            self.adb(record["serial"], "shell", "device_config", "put", *DESKTOP_WINDOWING, "false")
            self.adb(record["serial"], "shell", "svc", "power", "reboot", check=False)
            self.booted(record, boot_id)
            if self.adb(record["serial"], "shell", "device_config", "get", *DESKTOP_WINDOWING) != "false":
                raise RuntimeError(f"{record['serial']} kept desktop windowing after reboot")
        record["boot_seconds"] = round(time.monotonic() - started, 3)
        return record

    def start(self, count, profile, port_base):
        ports = list(range(port_base, port_base + 2 * count, 2))
        if count < 1 or port_base % 2 or any(port < 5560 or port > 5584 for port in ports):
            raise ValueError("Use even ports 5560–5584 and a positive count")
        used = {record["port"] for record in self.state["emulators"]}
        if any(port in used or not free_port(port) or not free_port(port + 1) for port in ports):
            raise ValueError("Requested emulator ports are already recorded or occupied")
        env = self.display()
        first = 1 if self.avd(profile) else count
        records = []
        for batch in (ports[:first], ports[first:]):
            launched = []
            for port in batch:
                command = [str(self.sdk / "emulator/emulator"), "-avd", profile, "-read-only", "-no-window", "-no-snapshot",
                           "-no-audio", "-no-boot-anim", "-no-metrics", "-port", str(port), "-gpu", "host"]
                record = self.spawn(command, f"emulator-{port}", env)
                record.update(serial=f"emulator-{port}", port=port, avd=profile, profile=profile)
                self.state["emulators"].append(record)
                launched.append(record)
                self.save()
            if launched:
                with ThreadPoolExecutor(max_workers=len(launched)) as workers:
                    list(workers.map(self.boot, launched))
                self.save()
            records += launched
        return records

    def configure(self, serial, size, theme, font_scale=1., rotation=0, cutout="none", navigation="gestural"):
        size = SIZES.get(size, size)
        if navigation not in ("threebutton", "gestural"):
            raise ValueError("Invalid navigation mode")
        if not re.fullmatch(r"[1-9][0-9]{1,3}x[1-9][0-9]{1,3}", size) or not 0.5 <= font_scale <= 3:
            raise ValueError("Invalid size or font scale")
        record = next((record for record in self.state["emulators"] if record["serial"] == serial), None)
        if record is None:
            raise ValueError(f"Unrecorded emulator: {serial}")
        density = PROFILES[record["profile"]][2]
        commands = [["wm", "size", size], ["wm", "density", str(density)], ["cmd", "uimode", "night", "yes" if theme == "dark" else "no"],
                    ["settings", "put", "system", "font_scale", str(font_scale)], ["settings", "put", "system", "accelerometer_rotation", "0"],
                    ["settings", "put", "system", "user_rotation", str(rotation)], ["settings", "put", "secure", "immersive_mode_confirmations", "confirmed"]]
        commands += [["settings", "put", "global", name, "0"] for name in ("window_animation_scale", "transition_animation_scale", "animator_duration_scale")]
        commands.append(["cmd", "overlay", "enable" if cutout == "corner" else "disable", "com.android.internal.display.cutout.emulation.corner"])
        commands.append(["cmd", "overlay", "disable", "com.android.internal.systemui.navbar." + ("gestural" if navigation == "threebutton" else "threebutton")])
        commands.append(["cmd", "overlay", "enable", "com.android.internal.systemui.navbar." + navigation])
        for command in commands:
            self.adb(serial, "shell", *command)
        actual_size = re.findall(r"(?:Physical|Override) size: (\d+x\d+)", self.adb(serial, "shell", "wm", "size"))
        actual_density = re.findall(r"(?:Physical|Override) density: (\d+)", self.adb(serial, "shell", "wm", "density"))
        actual_font = self.adb(serial, "shell", "settings", "get", "system", "font_scale")
        actual_navigation = self.adb(serial, "shell", "settings", "get", "secure", "navigation_mode").strip()
        if actual_navigation != str(0 if navigation == "threebutton" else 2):
            raise RuntimeError(f"Configure readback differs: navigation={actual_navigation}, expected={navigation}")
        if not actual_size or actual_size[-1] != size or not actual_density or int(actual_density[-1]) != density or abs(float(actual_font) - font_scale) > .0001:
            raise RuntimeError(f"Configure readback differs: size={actual_size}, density={actual_density}, font_scale={actual_font}")
        record["configuration"] = {"size": size, "density": density, "theme": theme, "font_scale": font_scale, "rotation": rotation, "cutout": cutout, "navigation": navigation}
        self.save()
        return record

    def terminate(self, record):
        if not alive(record):
            return
        os.kill(record["pid"], signal.SIGTERM)
        try:
            wait_for(lambda: not alive(record), 10, f"PID {record['pid']} exit")
        except RuntimeError:
            if alive(record):
                os.kill(record["pid"], signal.SIGKILL)
            wait_for(lambda: not alive(record), 10, f"PID {record['pid']} killed")

    def stop(self):
        for record in self.state["emulators"]:
            if alive(record):
                try:
                    self.adb(record["serial"], "emu", "kill")
                    wait_for(lambda: not alive(record), 15, record["serial"] + " exit")
                except (RuntimeError, subprocess.SubprocessError):
                    self.terminate(record)
        for record in reversed(self.state["display"]):
            self.terminate(record)
        self.state["stopped"] = self.state["emulators"]
        self.state["emulators"] = []
        self.state["display"] = []
        self.save()


@contextmanager
def locked(root):
    root.mkdir(parents=True, exist_ok=True)
    with (root / "emulators.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--sdk", type=Path, default=Path(os.environ.get("ANDROID_HOME", Path.home() / "Android/Sdk")))
    commands = parser.add_subparsers(dest="command", required=True)
    start = commands.add_parser("start")
    start.add_argument("--count", required=True, type=int)
    start.add_argument("--profile", required=True, choices=PROFILES)
    start.add_argument("--port-base", default=5560, type=int)
    configure = commands.add_parser("configure")
    configure.add_argument("--serial", required=True)
    configure.add_argument("--size", required=True)
    configure.add_argument("--theme", required=True, choices=("light", "dark"))
    configure.add_argument("--font-scale", type=float, default=1.)
    configure.add_argument("--rotation", type=int, choices=(0, 1), default=0)
    configure.add_argument("--cutout", choices=("corner", "none"), default="none")
    configure.add_argument("--navigation", choices=("threebutton", "gestural"), default="gestural")
    stop = commands.add_parser("stop")
    stop.add_argument("--all-recorded", action="store_true", required=True)
    commands.add_parser("status")
    args = parser.parse_args()
    try:
        with locked(args.root):
            harness = Harness(args.root, args.sdk)
            if args.command == "start":
                result = harness.start(args.count, args.profile, args.port_base)
            elif args.command == "configure":
                result = harness.configure(args.serial, args.size, args.theme, args.font_scale, args.rotation, args.cutout, args.navigation)
            elif args.command == "stop":
                harness.stop()
                result = harness.state
            else:
                result = harness.state
            print(json.dumps(result, indent=2))
    except (ValueError, OSError, RuntimeError, subprocess.SubprocessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
