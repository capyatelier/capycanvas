#!/usr/bin/env python3
"""Reserve the shared Android test tablets and run commands on them one at a time.

See docs/development/devices.md.
"""

import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import zlib


ROOT = Path(__file__).resolve().parents[2]
STATE = Path(os.environ.get("CAPY_DEVICE_STATE_DIR") or "/tmp")
TABLETS = {
    "9465X": ("tcl", "low"),
    "DTHA116": ("movinkpad11", "mid"),
    "DTHA140": ("movinkpad14", "top"),
    "KP1202": ("huion", None),
    "MNP1095": ("xppen", None),
}
TIERS = ("low", "mid", "top")


class Device:
    def __init__(self, serial, model):
        self.serial = serial
        self.model = model
        self.name, self.tier = TABLETS.get(model, (model.lower() or serial, None))

    @property
    def lock_path(self):
        return STATE / f"capy-{self.serial}.lock"

    @property
    def reservation_path(self):
        return STATE / f"capy-{self.serial}.reservation.json"

    def reservation(self, now=None):
        try:
            record = json.loads(self.reservation_path.read_text())
        except (FileNotFoundError, ValueError):
            return None
        if record.get("expires", 0) <= (now or time.time()):
            return None
        return record


def adb():
    candidates = [os.environ.get("ADB"), shutil.which("adb")]
    home = os.environ.get("ANDROID_HOME") or Path.home() / "Android/Sdk"
    candidates.append(str(Path(home) / "platform-tools/adb"))
    for candidate in candidates:
        if candidate and os.access(candidate, os.X_OK):
            return candidate
    sys.exit("adb not found; set ADB or ANDROID_HOME (see docs/development/android.md).")


def attached():
    listing = subprocess.run([adb(), "devices", "-l"], check=True, capture_output=True, text=True).stdout
    devices = []
    for line in listing.splitlines()[1:]:
        fields = line.split()
        if len(fields) < 2 or fields[1] != "device":
            continue
        model = next((field[6:] for field in fields[2:] if field.startswith("model:")), "")
        devices.append(Device(fields[0], model))
    return devices


def default_owner():
    return os.environ.get("CAPY_DEVICE_OWNER") or ROOT.name


def application_id(owner):
    suffix = re.sub(r"[^a-z0-9_]+", "_", owner.lower()).strip("_") or "agent"
    if not suffix[0].isalpha():
        suffix = f"w{suffix}"
    return f"art.capycanvas.{suffix}"


def ports(owner):
    base = 20000 + zlib.crc32(owner.encode()) % 4000 * 10
    return base, base + 1


def find(devices, name):
    matches = [device for device in devices if name in (device.name, device.serial, device.model)]
    if not matches:
        known = ", ".join(device.name for device in devices) or "none"
        sys.exit(f"No attached tablet named {name}; attached: {known}.")
    return matches[0]


def held_by(device):
    with open(device.lock_path, "a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            lock.seek(0)
            return lock.read().strip() or "another process"
        fcntl.flock(lock, fcntl.LOCK_UN)
    return None


def describe(record):
    remaining = max(0, int(record["expires"] - time.time()))
    note = f", {record['note']}" if record.get("note") else ""
    return f"reserved by {record['owner']} for {remaining // 3600}h{remaining % 3600 // 60:02d}m{note}"


def list_devices(args):
    devices = attached()
    if not devices:
        print("No tablets attached.")
    for device in devices:
        record = device.reservation()
        holder = held_by(device)
        state = [describe(record) if record else "free"]
        if holder:
            state.append(f"busy: {holder}")
        tier = f"{device.tier} tier" if device.tier else "no tier"
        print(f"{device.name:12} {device.model:9} {tier:9} {device.serial:20} {'; '.join(state)}")


def claimable(device, owner):
    record = device.reservation()
    return record is None or record["owner"] == owner


def reserve(args):
    devices = attached()
    if args.device:
        device = find(devices, args.device)
        record = device.reservation()
        if record and record["owner"] != args.owner:
            sys.exit(f"{device.name} is {describe(record)}.")
    else:
        choices = [device for device in devices if device.tier == args.tier and claimable(device, args.owner)]
        if not choices:
            sys.exit(f"No free {args.tier} tier tablet.")
        device = choices[0]
    record = {"owner": args.owner, "note": args.note or "", "expires": time.time() + args.hours * 3600}
    device.reservation_path.write_text(json.dumps(record))
    print(f"{device.name} ({device.serial}) {describe(record)}.")


def release(args):
    for device in attached():
        if args.device and args.device not in (device.name, device.serial, device.model):
            continue
        record = device.reservation()
        if record and record["owner"] == args.owner:
            device.reservation_path.unlink(missing_ok=True)
            print(f"Released {device.name}.")


def run(args):
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        sys.exit("Give a command after --.")
    device = find(attached(), args.device)
    record = device.reservation()
    if record and record["owner"] != args.owner:
        sys.exit(f"{device.name} is {describe(record)}.")
    web_port, cdp_port = ports(args.owner)
    environment = dict(os.environ, ANDROID_SERIAL=device.serial, CAPY_ANDROID_SERIAL=device.serial,
                       CAPY_APPLICATION_ID=application_id(args.owner),
                       CAPY_WEB_PORT=str(web_port), CAPY_CDP_PORT=str(cdp_port))
    with open(device.lock_path, "a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            lock.seek(0)
            print(f"Waiting for {device.name}: {lock.read().strip() or 'another process'}", file=sys.stderr)
            fcntl.flock(lock, fcntl.LOCK_EX)
        lock.seek(0)
        lock.truncate()
        lock.write(f"{args.owner} pid {os.getpid()}: {' '.join(command)[:120]}")
        lock.flush()
        try:
            return subprocess.run(command, env=environment).returncode
        finally:
            lock.seek(0)
            lock.truncate()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--owner", default=default_owner(),
                        help="reservation owner (default: $CAPY_DEVICE_OWNER or the worktree directory name)")
    commands = parser.add_subparsers(required=True, metavar="command")

    def command(name, handler, summary):
        subcommand = commands.add_parser(name, help=summary, description=summary)
        subcommand.set_defaults(handler=handler)
        return subcommand

    command("list", list_devices, "show attached tablets with their tier, reservation and lock")
    reservation = command("reserve", reserve, "reserve a tablet for this owner")
    target = reservation.add_mutually_exclusive_group(required=True)
    target.add_argument("device", nargs="?", help="tablet name, model or serial")
    target.add_argument("--tier", choices=TIERS, help="reserve any free tablet of this tier")
    reservation.add_argument("--hours", type=float, default=4, help="reservation length (default: 4)")
    reservation.add_argument("--note", help="what the tablet is reserved for")
    command("release", release, "release this owner's reservations").add_argument(
        "device", nargs="?", help="release only this tablet")
    runner = command("run", run, "run a command while holding the tablet's lock")
    runner.add_argument("device", help="tablet name, model or serial")
    runner.add_argument("command", nargs=argparse.REMAINDER, help="-- command and arguments")
    args = parser.parse_args()
    sys.exit(args.handler(args))


if __name__ == "__main__":
    main()
