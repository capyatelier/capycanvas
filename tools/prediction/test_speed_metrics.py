import unittest
import numpy as np
from speed_metrics import error_summary, speed_weight, speed_weighted_metrics


class SpeedCostTests(unittest.TestCase):
    def summary(self, values, speed, held=None):
        n = len(values)
        return error_summary(np.array(values), np.array(speed), np.ones(n) if held is None else np.array(held),
                             np.ones(n), np.ones(n, bool))

    def test_identical_error_cost_grows_at_lower_speed_and_is_bounded_at_rest(self):
        for speed, weight in [(0, 10), (60, 10), (300, 2), (600, 1), (1200, .5)]:
            self.assertEqual(self.summary([3.], [speed])['mean_squared_cost'], 9 * weight)
        self.assertTrue(np.isnan(speed_weight(np.nan)))

    def test_cost_is_duration_weighted_and_quadratic_in_error(self):
        self.assertEqual(self.summary([2., 4.], [600., 600.], [.75, .25])['mean_squared_cost'], 7.)
        self.assertEqual(self.summary([4.], [600.])['mean_squared_cost'], 4 * self.summary([2.], [600.])['mean_squared_cost'])

    def test_slow_flash_receives_more_weight_without_changing_physical_threshold(self):
        row = self.summary([3., 0.], [300., 1200.])['thresholds']['2.0']
        self.assertEqual(row['windows'], 1)
        self.assertEqual(row['eligible_percent'], 50.)
        self.assertEqual(row['weighted_percent'], 80.)

    def test_unknown_speed_or_geometry_and_zero_duration_do_not_earn_good_scores(self):
        row = self.summary([3., np.nan, 3., 3.], [600., 600., np.nan, 600.], [1., 1., 1., 0.])
        self.assertEqual(row['eligible_seconds'], 1.)
        self.assertEqual(row['mean_squared_cost'], 9.)

    def test_body_only_flash_and_tip_only_cutback_both_count_without_double_counting(self):
        s = dict(speed=np.full(3, 600.), held_dt=np.full(3, .01), ids=np.ones(3),
                 category=np.full(3, 'medium / steady straight'), retreat_continuing=np.full(3, 'steady line'))
        for key in ['off_path_tip', 'off_path_body', 'flash_tip', 'flash_body',
                    'retreat_retreat_tip', 'retreat_retreat_body', 'gap_0', 'gap_8', 'gap_16', 'gap_24']:
            s[key] = np.zeros(3)
        s['flash_body'][0] = 3.
        s['retreat_retreat_tip'][1:] = 3.
        s['flash_tip'][2] = 3.
        rows = speed_weighted_metrics(s)['all']
        self.assertEqual(rows['transient_peak']['thresholds']['2.0']['windows'], 3)
        self.assertAlmostEqual(rows['transient_cost']['mean_squared_cost'], 5.)


if __name__ == '__main__':
    unittest.main()
