import unittest
import numpy as np
from flicker_metrics import (reversal_amplitude, occupancy, valid_intervals,
                            interpolate)
from detail_metrics import detail_metrics, excess_correction

class TemporalTests(unittest.TestCase):


    def test_duration_is_union_not_double_counted(self):
        ids=np.ones(6);dt=valid_intervals(ids,np.arange(6)*10000)
        result=occupancy(np.array([0,0,1,1,1,1],bool),dt,ids,True)
        self.assertAlmostEqual(result['seconds'],.05)
        self.assertEqual(result['episodes'],1)

    def test_contact_boundaries_and_missing_frames_break_episodes(self):
        self.assertEqual(valid_intervals(np.array([1,1,2,2]),np.array([0,8333,0,100000])).tolist(),[0.,.008333,0.,0.])
        np.testing.assert_array_equal(reversal_amplitude(np.array([[1.,0.],[1.,0.]]),np.array([[2.,0.],[-2.,0.]])),[0.,1.])

    def frames(self, endpoints, offsets=None):
        # 1 px/ms constant motion with a changing forecast horizon.
        actual=np.array([[t,t/1000,0.] for t in range(0,101000,1000)])
        frames=[]
        for i,end in enumerate(endpoints):
            latest=i*10000
            t=np.linspace(latest,end,17)
            curve=np.column_stack([t,t/1000,np.zeros(17)])
            if offsets is not None:curve[:,2]=offsets[i]*np.linspace(0,1,17)
            frames.append(dict(contact=1,query=i,frame_us=latest,latest_us=latest,target_us=end,
                               transform=[1,0,0,1,0,0],curve=curve,
                               actual_recent=actual[actual[:,0]<=latest]))
        return frames,{1:{'samples':actual}}


    def test_truth_never_extrapolates_or_bridges_missing_input(self):
        s=np.array([[0.,0,0],[100000.,10,0]])
        self.assertTrue(np.isnan(interpolate(s,np.array([-1,50000,100001]))).all())
        np.testing.assert_equal(interpolate(s[:1],np.array([0,1])),[[0,0],[np.nan,np.nan]])


    def test_two_frame_detour_credits_convergence_and_charges_overshoot(self):
        truth=np.zeros((4,2));old=np.array([[4.,0.]]*4)
        new=np.array([[2.,0.],[6.,0.],[-2.,0.],[0.,4.]])
        values=excess_correction(old,new,truth)
        np.testing.assert_allclose(values[:3],[0.,2.,2.])
        self.assertGreater(values[3],2.)

    def test_single_frame_wrong_ink_flash_needs_no_three_frame_overlap(self):
        frames,truth=self.frames([20000,30000],[4,0])
        signals=detail_metrics(frames,truth)
        self.assertAlmostEqual(signals['flash_tip'][0],4.)
        self.assertGreater(signals['flash_body'][0],1.)
        # The initial ghost is bad; fixing it is useful, not another error.
        self.assertAlmostEqual(signals['unproductive_tip'][1],0.)
        self.assertAlmostEqual(signals['unproductive_body'][1],0.)

    def test_detail_metric_detects_body_flash_with_perfect_tip(self):
        frames,truth=self.frames([20000,30000])
        frames[0]['curve'][:,2]=4*np.sin(np.linspace(0,np.pi,17))
        signals=detail_metrics(frames,truth)
        self.assertAlmostEqual(signals['flash_tip'][0],0.)
        self.assertGreater(signals['flash_body'][0],2.)

    def test_good_ink_extension_is_not_a_flash_with_shorter_lead(self):
        _,truth=self.frames([40000,50000,60000])
        shorter,_=self.frames([5000,15000,25000])
        signals=detail_metrics(shorter,truth)
        np.testing.assert_allclose(signals['flash_body'][:-1],0.)
        np.testing.assert_allclose(signals['flash_tip'][:-1],0.)
        # No triple overlap is required: committed input supplies the answer.
        self.assertTrue(np.isfinite(signals['unproductive_tip'][1:]).all())

if __name__=='__main__':unittest.main()
