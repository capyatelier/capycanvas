import unittest
import numpy as np
from history_metrics import validate_pair, paired_oscillations, braking_exposure
from detail_metrics import detail_metrics


class HistoryMetricTests(unittest.TestCase):
    def fixture(self, offsets, horizon=48000):
        ts=np.arange(0,160001,1000)
        truth=np.column_stack([ts,ts*.001,ts*0.])
        frames=[]
        for i,(time,offset) in enumerate(zip([40000,48000,56000],offsets)):
            times=np.r_[time,np.linspace(time+1000,time+horizon,32)]
            xy=np.column_stack([times*.001,times*0.])+np.array(offset)
            xy[0]=[time*.001,0.]
            frames.append(dict(contact=1,query=i,frame_us=time,latest_us=time,target_us=time+horizon,
                source="Engine",requested_us=time+64000,transform=[1,0,0,1,0,0],curve=np.column_stack([times,xy]),
                actual_recent=truth[ts<=time]))
        return frames,{1:dict(samples=truth)}

    def test_equal_error_back_and_forth_is_oscillation(self):
        f,c=self.fixture([(0,2),(0,-2),(0,2)])
        r,s=paired_oscillations(f,f,c)
        self.assertAlmostEqual(s[0]['tip'][2],4.)
        self.assertGreater(r[0]['body']['thresholds']['1.0']['seconds'],0.)
        self.assertEqual(r[0],r[1])

    def test_strictly_convergent_revisions_are_credited(self):
        f,c=self.fixture([(2,1),(1,.5),(.5,.25)])
        _,s=paired_oscillations(f,f,c)
        self.assertAlmostEqual(s[0]['tip'][2],0.)

    def test_zigzag_convergence_receives_partial_not_all_or_nothing_credit(self):
        f,c=self.fixture([(2,0),(0,1),(.5,0)])
        _,s=paired_oscillations(f,f,c)
        self.assertGreater(s[0]['tip'][2],0.)
        self.assertLess(s[0]['tip'][2],.6)

    def test_nearly_equal_error_has_no_discontinuous_oscillation_penalty(self):
        scores=[]
        for end in [2.-1e-6,2.+1e-6]:
            f,c=self.fixture([(3,0),(0,2),(end,0)])
            _,s=paired_oscillations(f,f,c)
            scores.append(s[0]['tip'][2])
        self.assertGreater(scores[0],1.)
        self.assertAlmostEqual(scores[0],scores[1],places=5)

    def test_longer_previews_use_the_same_comparison_times(self):
        a,c=self.fixture([(0,2),(0,-2),(0,2)],32000)
        b,_=self.fixture([(0,2),(0,-2),(0,2)],48000)
        r,s=paired_oscillations(a,b,c)
        self.assertEqual(r[0],r[1])

    def test_contact_boundaries_and_long_gaps_never_form_oscillations(self):
        a,c=self.fixture([(0,2),(0,-2),(0,2)])
        a[-1]['contact']=2;c[2]=c[1]
        _,s=paired_oscillations(a,a,c)
        self.assertTrue(np.isnan(s[0]['tip']).all())
        a[-1]['contact']=1;a[-1]['frame_us']+=100000
        _,s=paired_oscillations(a,a,c)
        self.assertTrue(np.isnan(s[0]['tip']).all())

    def test_pairing_rejects_different_input_clocks(self):
        a,c=self.fixture([(0,0)]*3);b,_=self.fixture([(0,0)]*3)
        b[-1]['requested_us']+=1
        with self.assertRaises(ValueError):validate_pair(a,b)

    def test_constant_speed_is_not_braking(self):
        f,c=self.fixture([(0,0)]*3)
        d=detail_metrics(f,c)
        self.assertEqual(braking_exposure(f,c,d)['queries'],0)


class CorrectionSmoothnessTests(unittest.TestCase):
    def test_uniform_correction_is_smooth_even_away_from_truth(self):
        from history_metrics import correction_shock
        a=np.array([[0.,0.],[4.,4.]])
        np.testing.assert_allclose(correction_shock(a,a+1,a+2,1/120,1/120),0.,atol=1e-10)

    def test_irregular_cadence_does_not_create_artificial_shock(self):
        from history_metrics import correction_shock
        a=np.array([[0.,0.]])
        np.testing.assert_allclose(correction_shock(a,a+.5,a+2.,.005,.015),0.,atol=1e-10)

    def test_reversal_and_instantaneous_correction_are_abrupt(self):
        from history_metrics import correction_shock
        a=np.array([[0.,0.]])
        np.testing.assert_allclose(correction_shock(a,a+1,a,1/120,1/120),-2.)
        np.testing.assert_allclose(correction_shock(a,a,a+1,1/120,1/120),1.)

    def test_stop_and_corner_are_separate_from_ordinary_correction(self):
        from history_metrics import sudden_motion
        self.assertTrue(sudden_motion(np.array([[0.,0.],[16.,0.],[16.,0.]])))
        self.assertTrue(sudden_motion(np.array([[0.,0.],[16.,0.],[16.,16.]])))
        self.assertFalse(sudden_motion(np.array([[0.,0.],[16.,0.],[32.,.5]])))

    def test_missing_motion_truth_is_not_classified_as_ordinary(self):
        from history_metrics import paired_correction_smoothness
        f,c=HistoryMetricTests().fixture([(0,2),(0,-2),(0,2)])
        # Score a held frame, but omit the truth needed to classify its motion.
        f.append({**f[-1], 'query':3, 'frame_us':64000})
        c[1]['samples']=c[1]['samples'][c[1]['samples'][:,0]<=60000]
        r,_=paired_correction_smoothness(f,f,c)
        self.assertEqual(r[0]['ordinary']['tip']['eligible_seconds'],0.)
        self.assertGreater(r[0]['boundary']['tip']['eligible_seconds'],0.)


if __name__=='__main__':unittest.main()
