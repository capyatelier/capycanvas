import unittest
import numpy as np
from balance_metrics import classify_motion, balance_metrics
from detail_metrics import detail_metrics


class BalanceTests(unittest.TestCase):
    def frames(self, horizon, sideways=0., count=3, requested=24000):
        samples=np.array([[t,t*.0008,0.] for t in range(0,151000,1000)])
        frames=[]
        for i,latest in enumerate(range(40000,40000+count*8000,8000)):
            now=latest+4000;target=latest+horizon
            times=np.linspace(latest,target,17)
            curve=np.column_stack([times,times*.0008,np.linspace(0,sideways,17)])
            frames.append(dict(contact=1,query=i,latest_us=latest,frame_us=now,target_us=target,
                               requested_us=now+requested,curve=curve,actual_recent=samples[samples[:,0]<=latest],
                               transform=[1,0,0,1,0,0]))
        contacts={1:{'samples':samples}}
        flashes=detail_metrics(frames,contacts)
        return balance_metrics(frames,contacts,flashes)

    def test_speed_and_direction_are_independent_axes(self):
        times=np.arange(-24,25,8)/1000
        for speed,label in [(300,'slow'),(900,'medium'),(2400,'fast')]:
            points=np.column_stack([times*speed,np.zeros(7)])
            measured,category=classify_motion(points)
            self.assertAlmostEqual(measured,speed)
            self.assertEqual(category,label+' / steady straight')

    def test_smooth_curve_is_distinct_from_tight_turn_and_reversal(self):
        t=np.arange(-24,25,8)/1000
        points=50*np.column_stack([np.sin(8*t),1-np.cos(8*t)])
        self.assertEqual(classify_motion(points)[1],'slow / smooth curve')
        points=5*np.column_stack([np.sin(80*t),1-np.cos(80*t)])
        self.assertEqual(classify_motion(points)[1],'slow / changing direction')
        points=np.column_stack([400*np.abs(t),np.zeros(7)])
        self.assertEqual(classify_motion(points)[1],'slow / changing direction')

    def test_sensor_noise_at_rest_does_not_masquerade_as_direction_changes(self):
        points=np.column_stack([np.array([0,.05,-.05,.05,-.05,.05,0]),np.zeros(7)])
        self.assertEqual(classify_motion(points)[1],'stationary / micro motion')
        self.assertIn('boundary',classify_motion(np.full((7,2),np.nan))[1])

    def test_perfect_but_short_prediction_pays_for_visible_lag(self):
        short,s=self.frames(2000);full,f=self.frames(28000)
        self.assertAlmostEqual(short['slow/medium steady']['off_path_tip']['mean'],0.)
        self.assertAlmostEqual(full['slow/medium steady']['off_path_tip']['mean'],0.)
        np.testing.assert_allclose(s['gap_24'],20.8)
        np.testing.assert_allclose(s['behind_ms_24'],26.)
        np.testing.assert_allclose(f['gap_24'],0.,atol=1e-10)
        np.testing.assert_allclose(f['gap_0'],0.,atol=1e-10)

    def test_sideways_ghost_cannot_earn_tracking_credit_from_forward_projection(self):
        _,s=self.frames(28000,5.)
        np.testing.assert_allclose(s['behind_24'],0.,atol=1e-10)
        np.testing.assert_allclose(s['sideways_24'],5.)
        self.assertTrue((s['gap_24']>4.).all())

    def test_missing_future_truth_is_excluded_not_extrapolated(self):
        # An extended forecast has no answer after the fixture ends.
        _,s=self.frames(160000, requested=160000)
        self.assertTrue(np.isnan(s['off_path_tip']).all())

    def test_straight_acceleration_is_not_a_direction_change(self):
        t=np.arange(-24,25,8)/1000
        x=500*t+2000*t*t
        # Rotation must not affect direction or speed categories.
        for angle in [0., .7, 2.4]:
            points=np.column_stack([x*np.cos(angle),x*np.sin(angle)])
            self.assertEqual(classify_motion(points)[1], 'slow / steady straight')

    def test_excess_forward_reach_has_zero_gap_but_pays_endpoint_overshoot(self):
        _,s=self.frames(44000)
        np.testing.assert_allclose(s['gap_24'],0.,atol=1e-10)
        np.testing.assert_allclose(s['ahead_24'],12.8)
        np.testing.assert_allclose(s['endpoint_error_24'],12.8)


    def test_severity_buckets_partition_eligible_time(self):
        result,_=self.frames(2000)
        for name in ['gap_24','behind_ms_24','off_path_tip']:
            r=result['slow/medium steady'][name]
            self.assertAlmostEqual(sum(r['severity_seconds']),r['eligible_seconds'])
            self.assertEqual(sum(r['severity_windows']),3)


if __name__=='__main__':unittest.main()
