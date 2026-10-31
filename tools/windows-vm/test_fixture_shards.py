import unittest

from fixture_shards import balance, merge


class ShardTests(unittest.TestCase):
    def test_longest_first_balances_recorded_durations_and_preserves_runner_plan_order(self):
        selectors = ["a", "b", "c", "d"]
        durations = dict(zip(selectors, [90, 80, 30, 20]))
        buckets = balance(selectors, 2, durations)
        self.assertEqual(buckets, [["a", "d"], ["b", "c"]])
        self.assertEqual([sum(durations[name] for name in bucket) for bucket in buckets], [110, 110])

    def test_unknown_durations_round_robin_and_do_not_start_empty_runners(self):
        self.assertEqual(balance(["a", "b", "c", "d", "e"], 2, {}), [["a", "c", "e"], ["b", "d"]])
        self.assertEqual(balance(["a", "b"], 6, {}), [["a"], ["b"]])

    def test_missing_duration_uses_median_instead_of_a_zero_cost(self):
        self.assertEqual(balance(["a", "b", "c"], 2, {"a": 100, "b": 20}), [["a"], ["b", "c"]])

    def test_invalid_durations_are_ignored_and_invalid_plans_are_rejected(self):
        self.assertEqual(balance(["a", "b"], 2, {"a": float("nan"), "b": -1}), [["a"], ["b"]])
        for names, count in (([], 1), (["a", "a"], 2), (["a"], 0), (["a"], True)):
            with self.subTest(names=names, count=count), self.assertRaises(ValueError):
                balance(names, count, {})

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
