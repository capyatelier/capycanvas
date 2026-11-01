import json
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from fixture_shards import Queue, merge


class ShardTests(unittest.TestCase):
    def queue(self, selectors, runners=("r1", "r2"), durations=None):
        directory = TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        return Queue(selectors, list(runners), durations or {}, Path(directory.name) / "plan.json")

    def request(self, queue, runner, token=None):
        request = Request(f"http://127.0.0.1:{queue.server.server_port}/{runner}", method="POST",
                          headers={"Authorization": "Bearer " + (token or queue.tokens[runner])})
        with urlopen(request, timeout=5) as response:
            return json.load(response)["selector"]

    def test_idle_runner_pulls_longest_remaining_without_waiting_for_busy_runner(self):
        with self.queue(["a", "b", "c", "d"], durations=dict(zip("abcd", [90, 80, 30, 20]))) as queue:
            self.assertEqual(self.request(queue, "r1"), "a")
            self.assertEqual([self.request(queue, "r2") for _ in range(3)], ["b", "c", "d"])
            self.assertIsNone(self.request(queue, "r2"))
            self.assertIsNone(self.request(queue, "r1"))
            plan = json.loads(queue.plan.read_text())
            self.assertEqual(plan, {"runs": list("abcd"), "shards": {"r1": ["a"], "r2": ["b", "c", "d"]}})
            self.assertNotIn(queue.tokens["r1"], queue.plan.read_text())
            self.assertEqual(queue.server.server_address[0], "127.0.0.1")
            self.assertTrue(queue.configuration("r1")["url"].startswith("http://10.0.2.2:"))

    def test_median_for_unknown_duration_and_stable_unknown_order(self):
        with self.queue(["a", "b", "c"], durations={"a": 100, "b": 20}) as queue:
            self.assertEqual([self.request(queue, "r1") for _ in range(3)], ["a", "c", "b"])
        with self.queue(["a", "b"], durations={"a": float("nan"), "b": -1}) as queue:
            self.assertEqual([self.request(queue, "r1") for _ in range(2)], ["a", "b"])
        for selectors, runners in (([], ["r1"]), (["a", "a"], ["r1"]), (["a"], []), (["a"], ["r1", "r1"])):
            with self.subTest(selectors=selectors, runners=runners), self.assertRaises(ValueError):
                self.queue(selectors, runners)

    def test_bad_authentication_and_wrong_runner_do_not_consume_selectors(self):
        with self.queue(["a"]) as queue:
            for runner in ("r1", "unknown"):
                with self.assertRaises(HTTPError) as failure:
                    self.request(queue, runner, "wrong")
                self.assertEqual(failure.exception.code, 403)
                failure.exception.close()
            self.assertEqual(self.request(queue, "r1"), "a")

    def test_concurrent_claims_cover_once_and_missing_claim_is_never_replayed(self):
        names = [f"case-{index}" for index in range(40)]
        with self.queue(names, runners=[f"r{index}" for index in range(6)]) as queue:
            def drain(runner):
                while self.request(queue, runner) is not None:
                    pass
            with ThreadPoolExecutor(max_workers=6) as executor:
                list(executor.map(drain, queue.assignments))
            self.assertCountEqual([name for selection in queue.assignments.values() for name in selection], names)
            shards = [self.shard(runner, selection) for runner, selection in queue.assignments.items()]
            claimed = next(shard for shard in shards if shard["selectors"])
            missing = claimed["results"].pop(0)["name"]
            claimed.update(complete=False, failure="claim response or runner was lost")
            rows, errors = merge(names, shards)
            self.assertEqual(next(row for row in rows if row["name"] == missing)["exit"], "missing")
            self.assertTrue(any("claim response or runner was lost" in error for error in errors))
            self.assertIsNone(self.request(queue, "r0"))

    def shard(self, name, selectors):
        return {"vm": name, "selectors": selectors, "complete": True,
                "results": [{"name": selector, "exit": 0, "seconds": 1, "message": ""} for selector in selectors]}

    def test_merge_restores_global_order_without_discarding_shard_results(self):
        rows, errors = merge(["a", "b", "c"], [self.shard("r2", ["b"]), self.shard("r1", ["a", "c"])])
        self.assertFalse(errors)
        self.assertEqual([row["name"] for row in rows], ["a", "b", "c"])

    def test_missing_shard_fails_even_when_every_returned_row_passed(self):
        rows, errors = merge(["a", "b"], [self.shard("r1", ["a"]),
                                            {"vm": "r2", "selectors": ["b"], "failure": "transport stopped"}])
        self.assertEqual(rows[1]["exit"], "missing")
        self.assertTrue(any("transport stopped" in error for error in errors))
        self.assertTrue(any("missing shard completion" in error for error in errors))

    def test_incomplete_shard_and_duplicate_rows_cannot_hide_behind_passing_rows(self):
        shard = self.shard("r1", ["a", "b"])
        shard["results"] = [shard["results"][0]] * 2
        rows, errors = merge(["a", "b"], [shard])
        self.assertEqual(rows[1]["exit"], "missing")
        self.assertTrue(any("duplicate" in error for error in errors))

    def test_failed_result_and_unassigned_or_duplicate_selector_fail_merge(self):
        first = self.shard("r1", ["a"])
        first["results"][0].update(exit=1, message="assertion failed")
        second = self.shard("r2", ["a"])
        rows, errors = merge(["a", "b"], [first, second])
        self.assertEqual(rows[0]["exit"], 1)
        self.assertTrue(any("exactly once" in error for error in errors))
        self.assertTrue(any("assertion failed" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
