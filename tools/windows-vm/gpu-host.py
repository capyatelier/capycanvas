#!/usr/bin/env python3
"""Configure an explicitly selected Linux PCI GPU pool for Windows VMs."""

import argparse
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import subprocess
import sys

import gpu_pool


def run(*args, **kwargs):
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def device_nodes(device):
    nodes = [Path("/dev/dri") / p.name for p in (device.path / "drm").glob("*")]
    information = Path("/proc/driver/nvidia/gpus") / device.address / "information"
    if information.exists():
        minor = next(line.split(":", 1)[1].strip() for line in information.read_text().splitlines()
                     if line.startswith("Device Minor:"))
        nodes.append(Path(f"/dev/nvidia{minor}"))
    for function in device.functions[1:]:
        for card in (gpu_pool.PCI / function / "sound").glob("card*"):
            index = card.name[4:]
            nodes += list(Path("/dev/snd").glob(f"*C{index}[!0-9]*"))
            nodes.append(Path("/dev/snd") / f"controlC{index}")
    return sorted(set(p for p in nodes if p.exists()))


def holders(device):
    nodes = device_nodes(device)
    if not nodes:
        if (device.path / "driver").exists() and not device.bound():
            raise RuntimeError(f"No inspectable device nodes for the bound GPU: {device.address}")
        return []
    result = subprocess.run(["fuser", *map(str, nodes)], capture_output=True, text=True)
    if result.returncode not in (0, 1) or (result.returncode == 1 and result.stderr.strip()):
        raise RuntimeError(f"Cannot inspect GPU users: {result.stderr.strip()}")
    return sorted({int(token) for token in result.stdout.split() if token.isdigit()})


def resource_defaults(count):
    slots = count + 1
    memory = os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE") // (1024 ** 3)
    return min(32, max(2, (os.cpu_count() or 2) // slots)), f"{min(64, max(4, memory // slots))}G"


def permission_rules(devices, uid):
    return "".join(f'SUBSYSTEM=="vfio", KERNEL=="[0-9]*", '
                   f'TEST=="/sys/kernel/iommu_groups/%k/devices/{device.address}", '
                   f'RUN+="/usr/bin/setfacl -m u:{uid}:rw /dev/vfio/%k"\n' for device in devices)


def write_file(path, text, mode=0o644, owner=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, mode)
    with os.fdopen(descriptor, "w") as output:
        os.fchmod(output.fileno(), mode)
        if owner:
            os.fchown(output.fileno(), *owner)
        output.truncate(0)
        output.write(text)


def configure(args):
    if not shutil.which("fuser"):
        raise RuntimeError("Install psmisc (fuser) before configuring a GPU pool")
    if not shutil.which("setfacl"):
        raise RuntimeError("Install acl (setfacl) before configuring a GPU pool")
    user = pwd.getpwnam(args.user)
    state = args.state_dir.resolve()
    if not state.is_dir() or state.stat().st_uid != user.pw_uid:
        raise ValueError("Use the existing VM state directory owned by the selected user")
    if len(set(args.devices)) != len(args.devices):
        raise ValueError("GPU PCI addresses must be distinct")
    devices = [gpu_pool.Device(address) for address in args.devices]
    cpus, memory = resource_defaults(len(devices))
    if args.cpus is not None and args.cpus < 1:
        raise ValueError("VM CPU count must be a positive integer")
    if args.memory is not None and not re.fullmatch(r"[1-9][0-9]*[KMGT]", args.memory):
        raise ValueError("VM memory must use a positive integer and K, M, G or T")
    artifacts = args.artifacts.resolve() if args.artifacts else state / "artifacts"
    configuration = {"devices": args.devices, "cpus": args.cpus or cpus,
                     "memory": args.memory or memory, "artifacts": str(artifacts)}
    driver = getattr(args, "guest_driver", None)
    if driver:
        driver = driver.resolve()
        if not driver.is_dir() or not any(path.suffix.lower() == ".inf" for path in driver.rglob("*")):
            raise ValueError("Guest driver must be a directory containing signed Windows INF packages")
        configuration["guest_driver"] = str(driver)
    print(json.dumps({"user": user.pw_name, "state": str(state), **configuration,
                      "cards": [{"address": d.address, "group": d.group,
                                 "vfio": d.bound(), "hostUsers": [] if d.bound() else holders(d)}
                                for d in devices]}, indent=2), flush=True)
    if not args.apply:
        return
    if os.geteuid() != 0:
        raise RuntimeError("--apply needs root; run the reviewed command from a host administrator session")
    if not shutil.which("driverctl"):
        raise RuntimeError("Install the host distribution's driverctl package first")
    for device in devices:
        if not device.bound() and (device.path / "driver").resolve().name == "nvidia":
            run("nvidia-smi", "-i", device.address, "-pm", "0")
    busy = {device.address: holders(device) for device in devices if not device.bound()}
    busy = {address: pids for address, pids in busy.items() if pids}
    if busy:
        raise RuntimeError(f"GPUs still have users; no devices detached: {busy}")
    rules = Path("/etc/udev/rules.d") / f"89-capycanvas-vm-gpus-{user.pw_uid}.rules"
    write_file(rules, permission_rules(devices, user.pw_uid))
    run("udevadm", "control", "--reload-rules")
    run("modprobe", "vfio-pci")
    for device in devices:
        for address in reversed(device.functions):
            if (gpu_pool.PCI / address / "driver").resolve().name != "vfio-pci":
                run("driverctl", "set-override", address, "vfio-pci")
            else:
                write_file(Path("/etc/driverctl.d") / f"pci-{address}", "vfio-pci\n")
        if not device.bound():
            raise RuntimeError(f"GPU did not bind to VFIO: {device.address}")
    run("udevadm", "settle")
    for device in devices:
        run("setfacl", "-m", f"u:{user.pw_uid}:rw", device.node)
    dropin = Path("/etc/systemd/system") / f"user@{user.pw_uid}.service.d/90-capycanvas-vm.conf"
    write_file(dropin, "[Service]\nLimitMEMLOCK=infinity\n")
    run("systemctl", "daemon-reload")
    manager = run("systemctl", "show", f"user@{user.pw_uid}.service", "-p", "MainPID", "--value",
                  capture_output=True, text=True).stdout.strip()
    if manager.isdigit() and int(manager):
        run("prlimit", "--pid", manager, "--memlock=unlimited:unlimited")
    run("loginctl", "enable-linger", user.pw_name)
    artifacts.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chown(artifacts, user.pw_uid, user.pw_gid)
    write_file(state / "gpu-pool.json", json.dumps(configuration, indent=2) + "\n", 0o600,
               (user.pw_uid, user.pw_gid))
    print("GPU pool configured. Existing VFIO devices were not detached. Use windows-vm.py gpus.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("devices", nargs="+", help="PCI function-zero GPU addresses to dedicate to VMs")
    parser.add_argument("--user", required=True, help="Linux user that runs the VMs")
    parser.add_argument("--state-dir", type=Path, required=True, help="existing Windows VM state directory")
    parser.add_argument("--artifacts", type=Path, help="large test-output directory, preferably on local SSD")
    parser.add_argument("--cpus", type=int, help="vCPUs per VM (default: host-sized, up to 32)")
    parser.add_argument("--memory", help="RAM per VM, e.g. 32G (default: host-sized, up to 64G)")
    parser.add_argument("--guest-driver", type=Path, help="signed Windows INF bundle staged on the host SSD")
    parser.add_argument("--apply", action="store_true", help="bind idle devices and persist host configuration")
    args = parser.parse_args()
    try:
        configure(args)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))


if __name__ == "__main__":
    main()
