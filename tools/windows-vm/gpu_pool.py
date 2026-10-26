"""Exclusive hardware GPU reservations for QEMU guests."""

from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess


PCI = Path("/sys/bus/pci/devices")
VFIO = Path("/dev/vfio")
LOCKS = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "capycanvas/vm-gpus"
BDF = re.compile(r"[0-9a-f]{4}:[0-9a-f]{2}:[0-9a-f]{2}\.0")


def configuration(state):
    path = state / "gpu-pool.json"
    value = json.loads(path.read_text()) if path.exists() else {}
    if not isinstance(value, dict):
        raise ValueError("GPU pool configuration must be a JSON object")
    if "cpus" in value and (not isinstance(value["cpus"], int) or value["cpus"] < 1):
        raise ValueError("VM CPU count must be a positive integer")
    if "memory" in value and (not isinstance(value["memory"], str) or not re.fullmatch(r"[1-9][0-9]*[KMGT]", value["memory"])):
        raise ValueError("VM memory must use a positive integer and K, M, G or T")
    return value


class Device:
    def __init__(self, address):
        if not isinstance(address, str) or not BDF.fullmatch(address):
            raise ValueError(f"GPU addresses must name PCI function zero: {address}")
        self.address = address
        self.path = PCI / address
        if not self.path.is_dir() or int((self.path / "class").read_text(), 16) >> 16 != 3:
            raise ValueError(f"Not a PCI display controller: {address}")
        if (self.path / "boot_vga").exists() and (self.path / "boot_vga").read_text().strip() == "1":
            raise ValueError(f"Refusing the host boot GPU: {address}")
        if any(p.read_text().strip() == "connected" for p in (self.path / "drm").glob("card*/card*-*/status")):
            raise ValueError(f"GPU has a connected host display: {address}")
        group = self.path / "iommu_group"
        if not group.exists():
            raise ValueError(f"GPU has no IOMMU group: {address}")
        self.group = group.resolve().name
        audio = address[:-1] + "1"
        if (PCI / audio).exists() and int((PCI / audio / "class").read_text(), 16) >> 8 not in (0x0401, 0x0403):
            raise ValueError(f"GPU function one is not an audio controller: {audio}")
        self.functions = [address] + ([audio] if (PCI / audio).exists() else [])
        if {p.name for p in (group / "devices").iterdir()} != set(self.functions):
            raise ValueError(f"GPU shares its IOMMU group with other devices: {address}")

    @property
    def node(self):
        return VFIO / self.group

    def bound(self):
        return all((PCI / address / "driver").resolve().name == "vfio-pci" for address in self.functions)

    def in_use(self):
        try:
            result = subprocess.run(["fuser", str(self.node)], capture_output=True, text=True)
        except FileNotFoundError as error:
            raise RuntimeError("Install psmisc (fuser) to inspect GPU reservations") from error
        if result.returncode not in (0, 1) or (result.returncode == 1 and result.stderr.strip()):
            raise RuntimeError(f"Cannot inspect VFIO users for {self.address}: {result.stderr.strip()}")
        return result.returncode == 0


def devices(state):
    addresses = configuration(state).get("devices", [])
    if not isinstance(addresses, list) or not all(isinstance(value, str) for value in addresses) or len(set(addresses)) != len(addresses):
        raise ValueError("GPU pool must contain distinct PCI addresses")
    return [Device(address) for address in addresses]


def lock_device(device):
    LOCKS.mkdir(mode=0o700, parents=True, exist_ok=True)
    descriptor = os.open(LOCKS / device.address, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    lock = os.fdopen(descriptor, "r+")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        lock.close()
        return None
    return lock


@contextmanager
def reserve(state, name, requested="auto"):
    candidates = devices(state)
    if requested != "auto":
        candidates = [device for device in candidates if device.address == requested]
    if not candidates:
        raise RuntimeError("No matching GPUs are configured; see the Windows VM GPU setup guide")
    for device in candidates:
        if not device.bound() or not os.access(device.node, os.R_OK | os.W_OK):
            continue
        lock = lock_device(device)
        if lock is None:
            continue
        try:
            if device.in_use():
                continue
            lock.seek(0)
            lock.truncate()
            json.dump({"vm": name, "state": str(state.resolve())}, lock)
            lock.flush()
            yield device, lock
            return
        finally:
            lock.close()
    raise RuntimeError("No GPU is free and accessible; use `gpus` to inspect reservations")


def describe(state):
    rows = []
    for device in devices(state):
        status = "unavailable"
        owner = None
        if device.bound() and os.access(device.node, os.R_OK | os.W_OK):
            lock = lock_device(device)
            if lock is None:
                status = "reserved"
                try:
                    owner = json.loads((LOCKS / device.address).read_text())
                except (OSError, ValueError):
                    pass
            else:
                try:
                    status = "external" if device.in_use() else "free"
                finally:
                    lock.close()
        rows.append({"address": device.address, "group": device.group, "status": status, "owner": owner})
    return rows
