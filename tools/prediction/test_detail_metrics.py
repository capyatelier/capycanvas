import unittest
import numpy as np
from detail_metrics import detail_metrics
from balance_metrics import balance_metrics
from retraction_metrics import retraction_metrics


class GeometryClockTests(unittest.TestCase):
    def fixture(self, stop=False, lateral=0.):
        t=np.arange(0,160001,1000)
        x=np.minimum(t,64000)*.001 if stop else t*.001
        truth=np.column_stack([t,x,np.zeros(len(t))])
        frames=[]
        for i,now in enumerate([48000,56000,64000]):
            times=np.linspace(now,now+16000,33)
            # Two milliseconds early, but on the actual upcoming line.
            f=np.linspace(0,1,33)
            curve=np.column_stack([times,times*.001+2*f,lateral*f])
            frames.append(dict(contact=1,query=i,frame_us=now,latest_us=now,
                target_us=now+16000,requested_us=now+24000,
                transform=[1,0,0,1,0,0],curve=curve,actual_recent=truth[t<=now]))
        return frames,{1:dict(samples=truth)}

    def test_geometry_does_not_confuse_phase_with_ghosts(self):
        frames,contacts=self.fixture()
        old=detail_metrics(frames,contacts,geometry_horizon='target')
        new=detail_metrics(frames,contacts,geometry_horizon='requested')
        np.testing.assert_allclose(old['off_path_tip'],2.)
        np.testing.assert_allclose(new['off_path_tip'],0.,atol=1e-9)
        # The independent time-domain score still sees the two-pixel error.
        _,timing=balance_metrics(frames,contacts,new)
        np.testing.assert_allclose(timing['endpoint_error_16'],2.)

    def test_sideways_ghosts_and_stop_overshoot_are_still_errors(self):
        for stop,lateral in [(False,5.),(True,0.)]:
            frames,contacts=self.fixture(stop,lateral)
            new=detail_metrics(frames,contacts,geometry_horizon='requested')
            self.assertGreaterEqual(new['off_path_tip'][1],5.)

    def test_missing_requested_truth_is_not_assumed_correct(self):
        frames,contacts=self.fixture()
        frames[0]['requested_us']=200000
        new=detail_metrics(frames,contacts,geometry_horizon='requested')
        self.assertTrue(np.isnan(new['off_path_tip'][0]))

    def test_removal_of_correct_early_ink_has_no_phase_discount(self):
        frames,contacts=self.fixture()
        # Identical accepted time; remove the early but geometrically right tip.
        frames[1]['curve'][:,1]-=2*np.linspace(0,1,33)
        _,old=retraction_metrics(frames,contacts)
        _,new=retraction_metrics(frames,contacts,geometry_horizon='requested')
        self.assertAlmostEqual(old['retreat_tip'][1],0.)
        self.assertAlmostEqual(new['retreat_tip'][1],2.)


if __name__=='__main__':unittest.main()
