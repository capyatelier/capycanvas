import json
from pathlib import Path
import select
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import cpu_pool as pool


def contender(path):
    return subprocess.run([sys.executable, "-c", """import fcntl,sys
with open(sys.argv[1],'r+') as lock:
    try: fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    except BlockingIOError: sys.exit(23)
""", str(path)], capture_output=True).returncode


class CpuPoolTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.state = self.root / "state"
        self.state.mkdir()
        self.settings = {"vcpu_budget": 8, "memory_budget_gib": 16, "runner_cpus": 4,
                         "runner_memory_gib": 8, "per_owner_limit": 1}
        (self.state / "cpu-pool.json").write_text(json.dumps(self.settings))
        patcher = patch.object(pool, "LOCKS", self.root / "locks")
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_distinct_owners_fill_budget_without_stealing_and_release_on_close(self):
        with pool.reserve(self.state, "first-r1", self.root / "first") as (first, _), \
                pool.reserve(self.state, "second-r1", self.root / "second") as (second, _):
            self.assertNotEqual(first.index, second.index)
            self.assertEqual((first.cpus, first.memory), (4, "8G"))
            self.assertEqual(contender(pool.LOCKS / f"slot-{first.index}"), 23)
            before = (pool.LOCKS / f"slot-{first.index}").read_text()
            with self.assertRaisesRegex(RuntimeError, "No CPU runner slot is free"):
                with pool.reserve(self.state, "third-r1", self.root / "third"):
                    self.fail("A full pool admitted another runner")
            self.assertEqual((pool.LOCKS / f"slot-{first.index}").read_text(), before)
        self.assertTrue(all(row["status"] == "free" for row in pool.describe(self.state)[1]))

    def test_owner_cap_counts_across_state_directories(self):
        other = self.root / "other-state"
        other.mkdir()
        (other / "cpu-pool.json").write_text(json.dumps(self.settings))
        with pool.reserve(self.state, "first-r1", self.root / "first"):
            with self.assertRaisesRegex(RuntimeError, "CPU runner limit reached"):
                with pool.reserve(other, "first-r2", self.root / "first"):
                    self.fail("One owner exceeded its cap through another state directory")
            with pool.reserve(other, "second-r1", self.root / "second"):
                self.assertEqual(sum(row["status"] == "reserved" for row in pool.describe(other)[1]), 2)

    def test_same_vm_cannot_start_twice_with_different_runtime_or_owner(self):
        with pool.reserve(self.state, "first-r1", self.root / "first"):
            with self.assertRaisesRegex(RuntimeError, "already holds a CPU runner reservation"):
                with pool.reserve(self.state, "first-r1", self.root / "second"):
                    self.fail("One VM received a second live reservation")

    def test_qemu_inherited_descriptor_holds_slot_after_launcher_closes(self):
        child = None
        try:
            with pool.reserve(self.state, "first-r1", self.root / "first") as (slot, lock):
                child = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.buffer.read()"],
                                         stdin=subprocess.PIPE, pass_fds=(lock.fileno(),))
            self.assertEqual(contender(pool.LOCKS / f"slot-{slot.index}"), 23)
            child.stdin.close()
            child.wait(timeout=5)
            self.assertEqual(contender(pool.LOCKS / f"slot-{slot.index}"), 0)
        finally:
            if child is not None and child.poll() is None:
                child.terminate()
                child.wait(timeout=5)

    def test_failed_launch_releases_slot_and_retains_other_owner(self):
        with pool.reserve(self.state, "first-r1", self.root / "first"):
            with self.assertRaisesRegex(RuntimeError, "launch failed"):
                with pool.reserve(self.state, "second-r1", self.root / "second"):
                    raise RuntimeError("launch failed")
            rows = pool.describe(self.state)[1]
            self.assertEqual([row["owner"]["vm"] for row in rows if row["owner"]], ["first-r1"])

    def test_configuration_change_cannot_bypass_active_pool_budget(self):
        with pool.reserve(self.state, "first-r1", self.root / "first"):
            (self.state / "cpu-pool.json").write_text(json.dumps({**self.settings, "vcpu_budget": 12}))
            with self.assertRaisesRegex(RuntimeError, "differs from active reservations"):
                with pool.reserve(self.state, "second-r1", self.root / "second"):
                    self.fail("An active pool silently changed its budget")

    def test_owner_limit_can_change_while_pool_geometry_is_reserved(self):
        path = self.state / "cpu-pool.json"
        path.write_text(json.dumps({**self.settings, "per_owner_limit": 6}))
        with pool.reserve(self.state, "first-r1", self.root / "first") as (first, _):
            path.write_text(json.dumps({**self.settings, "per_owner_limit": 12}))
            with pool.reserve(self.state, "first-r2", self.root / "first") as (second, _):
                self.assertNotEqual(first.index, second.index)
                self.assertEqual(sum(row["status"] == "reserved" for row in pool.describe(self.state)[1]), 2)

    def test_listing_shows_live_metadata_only_while_reserved(self):
        with pool.reserve(self.state, "first-r1", self.root / "first") as (slot, _):
            settings, rows = pool.describe(self.state)
            self.assertEqual(settings, self.settings)
            row = rows[slot.index]
            self.assertEqual(row["status"], "reserved")
            self.assertEqual(row["owner"]["worktree"], str((self.root / "first").resolve()))
            self.assertTrue(row["owner"]["started"])
        self.assertIsNone(pool.describe(self.state)[1][slot.index]["owner"])

    def test_default_budget_leaves_half_host_and_persists_first_reservation(self):
        (self.state / "cpu-pool.json").unlink()
        with patch.object(pool.os, "cpu_count", return_value=192), \
                patch.object(pool.os, "sysconf", side_effect=lambda name: {"SC_PHYS_PAGES": 128 * 1024 * 1024,
                                                                          "SC_PAGE_SIZE": 4096}[name]):
            settings = pool.configuration(self.state)
            self.assertEqual(settings["vcpu_budget"], 96)
            self.assertEqual(settings["memory_budget_gib"], 256)
            self.assertEqual(pool.capacity(settings), 24)
            self.assertEqual(settings["per_owner_limit"], 6)
            with pool.reserve(self.state, "first-r1", self.root / "first"):
                path = self.state / "cpu-pool.json"
                self.assertEqual(json.loads(path.read_text()), settings)
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_memory_budget_limits_slots_independently_of_cpus(self):
        settings = {**self.settings, "vcpu_budget": 100, "memory_budget_gib": 8}
        self.assertEqual(pool.capacity(settings), 1)

    def test_invalid_configuration_is_rejected_before_locking_a_slot(self):
        for value in ([], None, {"runner_cpus": True}, {"runner_cpus": 0}, {"per_owner_limit": -1},
                      {"memory_budget_gib": 1}, {"runner_memory_gib": "8G"}, {"typo": 4}):
            with self.subTest(value=value):
                (self.state / "cpu-pool.json").write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    pool.configuration(self.state)

    def test_slot_symlink_cannot_modify_an_unrelated_file(self):
        pool.LOCKS.mkdir()
        target = self.root / "unrelated"
        target.write_text("keep")
        (pool.LOCKS / "slot-0").symlink_to(target)
        with self.assertRaises(OSError):
            with pool.reserve(self.state, "first-r1", self.root / "first"):
                self.fail("Followed a slot symlink")
        self.assertEqual(target.read_text(), "keep")

    def test_independent_launchers_claim_distinct_slots_atomically(self):
        code = """import json,pathlib,sys
sys.path.insert(0,sys.argv[1])
import cpu_pool
cpu_pool.LOCKS=pathlib.Path(sys.argv[2])
with cpu_pool.reserve(pathlib.Path(sys.argv[3]),sys.argv[4],sys.argv[4]) as (slot,lock):
    print(json.dumps({'slot':slot.index}),flush=True)
    sys.stdin.buffer.read()
"""
        children = []
        try:
            for owner in ("first", "second"):
                children.append(subprocess.Popen([sys.executable, "-c", code, str(Path(pool.__file__).parent),
                                                  str(pool.LOCKS), str(self.state), owner], stdin=subprocess.PIPE,
                                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
            slots = []
            for child in children:
                self.assertTrue(select.select([child.stdout], [], [], 5)[0], "Runner reservation stalled")
                slots.append(json.loads(child.stdout.readline())["slot"])
            self.assertEqual(set(slots), {0, 1})
            with self.assertRaisesRegex(RuntimeError, "No CPU runner slot is free"):
                with pool.reserve(self.state, "third-r1", "third"):
                    self.fail("A third launcher stole a held slot")
        finally:
            for child in children:
                child.stdin.close()
            for child in children:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.terminate()
                    child.wait(timeout=5)
                child.stdout.close()
                child.stderr.close()
        self.assertTrue(all(row["status"] == "free" for row in pool.describe(self.state)[1]))


if __name__ == "__main__":
    unittest.main()
