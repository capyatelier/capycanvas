import copy
import unittest
from stability_metrics import check_snapshot, VERSION


class SnapshotTests(unittest.TestCase):
    def test_regression_and_lost_eligibility_are_detected_separately(self):
        expected = dict(version=VERSION, metrics={'ordinary correction tip': dict(mean=2., eligible_seconds=50., ge2_seconds=10.)})
        self.assertEqual(check_snapshot(expected, expected), [])
        for key, value, message in [('mean', 2.2, 'mean'), ('ge2_seconds', 11., 'ge2_seconds'), ('eligible_seconds', 40., 'eligibility')]:
            actual = copy.deepcopy(expected)
            actual['metrics']['ordinary correction tip'][key] = value
            self.assertTrue(any(message in f for f in check_snapshot(expected, actual)))

    def test_schema_changes_require_review(self):
        self.assertTrue(check_snapshot(dict(version=VERSION-1), dict(version=VERSION)))
        self.assertTrue(check_snapshot(dict(version=VERSION, metrics={}), dict(version=VERSION, metrics={'new': {}})))

    def test_speed_cost_cannot_improve_by_losing_slow_motion_eligibility(self):
        expected = dict(version=VERSION, metrics={'speed weighted flash_tip': dict(
            mean_squared_cost=2., eligible_seconds=50., weighted_eligible_seconds=80., ge2_weighted_seconds=10.)})
        actual = copy.deepcopy(expected)
        actual['metrics']['speed weighted flash_tip']['weighted_eligible_seconds'] = 70.
        self.assertTrue(any('eligibility' in f for f in check_snapshot(expected, actual)))


if __name__ == '__main__':
    unittest.main()
