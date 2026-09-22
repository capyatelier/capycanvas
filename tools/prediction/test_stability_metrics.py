import copy
import unittest
from stability_metrics import check_snapshot


class SnapshotTests(unittest.TestCase):
    def test_regression_and_lost_eligibility_are_detected_separately(self):
        expected = dict(version=2, metrics={'ordinary correction tip': dict(mean=2., eligible_seconds=50., ge2_seconds=10.)})
        self.assertEqual(check_snapshot(expected, expected), [])
        for key, value, message in [('mean', 2.2, 'mean'), ('ge2_seconds', 11., 'ge2_seconds'), ('eligible_seconds', 40., 'eligibility')]:
            actual = copy.deepcopy(expected)
            actual['metrics']['ordinary correction tip'][key] = value
            self.assertTrue(any(message in f for f in check_snapshot(expected, actual)))

    def test_schema_changes_require_review(self):
        self.assertTrue(check_snapshot(dict(version=0), dict(version=2)))
        self.assertTrue(check_snapshot(dict(version=2, metrics={}), dict(version=2, metrics={'new': {}})))


if __name__ == '__main__':
    unittest.main()
