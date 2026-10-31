"""Host-wide CPU runner reservations retained by QEMU."""

from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import json
import os
from pathlib import Path
import tempfile
from typing import NamedTuple


LOCKS = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "capycanvas/vm-runners"


class Slot(NamedTuple):
    index: int
    cpus: int
    memory: str


class Unavailable(RuntimeError):
    pass


def capacity(settings):
    return min(settings["vcpu_budget"] // settings["runner_cpus"],
               settings["memory_budget_gib"] // settings["runner_memory_gib"])


def configuration(state):
    memory = os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE") // (1024 ** 3)
    settings = {"vcpu_budget": max(1, (os.cpu_count() or 1) // 2),
                "memory_budget_gib": max(1, memory // 2),
                "runner_cpus": 4, "runner_memory_gib": 8}
    settings["per_owner_limit"] = 6
    path = state / "cpu-pool.json"
    value = json.loads(path.read_text()) if path.exists() else {}
    if not isinstance(value, dict) or value.keys() - settings.keys():
        raise ValueError("CPU pool must be an object containing only runner sizes and budget limits")
    settings.update(value)
    if any(type(value) is not int or value < 1 for value in settings.values()):
        raise ValueError("CPU pool limits must be positive integers and fit at least one runner")
    if "per_owner_limit" not in value:
        settings["per_owner_limit"] = min(6, capacity(settings))
    if not capacity(settings):
        raise ValueError("CPU pool limits must be positive integers and fit at least one runner")
    return settings


def lock_file(path, blocking=False):
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    lock = os.fdopen(descriptor, "r+")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
    except BlockingIOError:
        lock.close()
        return None
    return lock


@contextmanager
def guard():
    LOCKS.mkdir(mode=0o700, parents=True, exist_ok=True)
    with lock_file(LOCKS / "pool-lock", blocking=True):
        yield


def owner(path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def slot_paths(settings):
    indexes = set(range(capacity(settings)))
    indexes.update(int(path.name[5:]) for path in LOCKS.glob("slot-*") if path.name[5:].isdigit())
    return [(index, LOCKS / f"slot-{index}") for index in sorted(indexes)]


@contextmanager
def reserve(state, name, worktree):
    lock = None
    try:
        with guard():
            settings = configuration(state)
            held = []
            chosen = None
            for index, path in slot_paths(settings):
                candidate = lock_file(path)
                if candidate is None:
                    held.append(owner(path))
                elif chosen is None and index < capacity(settings):
                    chosen, lock = index, candidate
                else:
                    candidate.close()
            if any(not isinstance(row, dict) or row.get("pool") != settings
                   or not isinstance(row.get("worktree"), str) for row in held):
                raise RuntimeError("CPU pool configuration differs from active reservations; stop their runners before changing it")
            if any(row.get("vm") == name and row.get("state") == str(state.resolve()) for row in held):
                raise RuntimeError(f"The {name} VM already holds a CPU runner reservation")
            worktree = str(Path(worktree).resolve())
            if sum(row["worktree"] == worktree for row in held) >= settings["per_owner_limit"]:
                raise Unavailable(f"CPU runner limit reached for {worktree}: {settings['per_owner_limit']}")
            if lock is None:
                raise Unavailable("No CPU runner slot is free; use `runners` to inspect reservations")
            state.mkdir(mode=0o700, parents=True, exist_ok=True)
            if not (state / "cpu-pool.json").exists():
                with tempfile.NamedTemporaryFile(mode="w", dir=state, delete=False) as output:
                    temporary = Path(output.name)
                    json.dump(settings, output, indent=2)
                try:
                    temporary.replace(state / "cpu-pool.json")
                finally:
                    temporary.unlink(missing_ok=True)
            lock.seek(0)
            lock.truncate()
            json.dump({"vm": name, "state": str(state.resolve()), "worktree": worktree,
                       "started": datetime.now(timezone.utc).isoformat(), "pool": settings}, lock)
            lock.flush()
            slot = Slot(chosen, settings["runner_cpus"], f"{settings['runner_memory_gib']}G")
        yield slot, lock
    finally:
        if lock is not None:
            lock.close()


def describe(state):
    with guard():
        settings = configuration(state)
        rows = []
        for index, path in slot_paths(settings):
            lock = lock_file(path)
            if lock is None:
                rows.append({"slot": index, "status": "reserved", "owner": owner(path)})
            else:
                lock.close()
                if index < capacity(settings):
                    rows.append({"slot": index, "status": "free", "owner": None})
        return settings, rows
