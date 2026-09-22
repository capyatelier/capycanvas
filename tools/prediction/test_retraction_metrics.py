import unittest
import numpy as np
from retraction_metrics import retraction_metrics, straightish


class RetractionTests(unittest.TestCase):
    def measure(self, horizons=(24000,24000,24000), path=None, error=None):
        if path is None:path=lambda t:np.column_stack([t*.001,np.zeros(len(t))])
        t=np.arange(0,160001,1000)
        truth=np.column_stack([t,path(t)])
        frames=[]
        for i,(now,h) in enumerate(zip([48000,56000,64000],horizons)):
            times=np.linspace(now,now+h,33)
            xy=path(times)
            if error is not None:xy=xy+error(i,np.linspace(0,1,33))
            frames.append(dict(contact=1,frame_us=now,latest_us=now,target_us=now+h,
                transform=[1,0,0,1,0,0],curve=np.column_stack([times,xy]),actual_recent=truth[t<=now]))
        return retraction_metrics(frames,{1:{'samples':truth}})

    def test_perfect_advancing_tail_is_stable(self):
        _,s=self.measure()
        np.testing.assert_allclose(s['retreat_tip'][1:],0.,atol=1e-9)
        np.testing.assert_allclose(s['retreat_body'][1:],0.,atol=1e-9)

    def test_shortening_is_visible_even_if_tip_keeps_advancing(self):
        _,s=self.measure((24000,20000,20000))
        self.assertEqual(s['target_backstep_ms'][1],0.)
        self.assertAlmostEqual(s['retreat_tip'][1],4.)
        self.assertGreater(s['retreat_body'][1],0.5)

    def test_removal_of_old_wrong_tail_is_a_correction(self):
        _,s=self.measure(error=lambda i,f: np.column_stack([10*f if i==0 else f*0,f*0]))
        self.assertAlmostEqual(s['retreat_tip'][1],0.)
        self.assertLess(s['retreat_body'][1],1e-8)

    def test_true_stop_allows_immediate_tail_collapse(self):
        path=lambda t:np.column_stack([np.minimum(t,56000)*.001,np.zeros(len(t))])
        _,s=self.measure((24000,0,0),path)
        self.assertLess(s['retreat_tip'][1],1e-8)
        self.assertLess(s['retreat_body'][1],1e-8)

    def test_correct_curving_preview_transports_without_jitter(self):
        path=lambda t:np.column_stack([100*np.sin(t/100000),100*(1-np.cos(t/100000))])
        _,s=self.measure(path=path)
        self.assertLess(np.nanmax(s['retreat_tip']),.002)
        self.assertLess(np.nanmax(s['retreat_body']),.002)

    def test_no_truth_does_not_invent_good_scores(self):
        _,s=self.measure((240000,240000,240000))
        self.assertTrue(np.isnan(s['retreat_tip']).all())
        self.assertTrue(np.isnan(s['retreat_body']).all())

    def test_direction_label_excludes_stops_and_sharp_turns(self):
        t=np.arange(-24,25,8.)
        self.assertTrue(straightish(np.column_stack([t,t*.01])))
        self.assertFalse(straightish(np.column_stack([np.abs(t),t*.01])))
        self.assertFalse(straightish(np.zeros((7,2))))
        self.assertFalse(straightish(np.full((7,2),np.nan)))


if __name__=='__main__':unittest.main()
