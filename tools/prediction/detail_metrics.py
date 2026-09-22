"""Two-frame whole-preview error signals in physical pixels.

Motion classification, timing and duration aggregation live in balance_metrics.
The thresholds are engineering probes, not calibrated perceptual thresholds.
"""
import numpy as np
from flicker_metrics import (surface, interpolate, norm, valid_intervals,
                            distance_to_path, sample_path)


def excess_correction(old,new,truth):
    """Motion not paid for by getting closer; straight convergence costs zero.

    Half the triangle-inequality excess: full cost for moving away, partial cost
    for a sideways detour/overshoot, zero for monotone motion toward truth.
    """
    return np.maximum(0.,(norm(new-old)+norm(new-truth)-norm(old-truth))*.5)


def truth_path(frame, samples, transform, horizon='requested'):
    """Common geometric opportunity; never extend truth across missing input.

    Geometry can be right despite a small phase error. Limit the answer to the
    recorded request (not the whole stroke) and score phase/overshoot separately
    at fixed display times. 'target' retains the historical coupled diagnostic.
    """
    lo=frame['latest_us'];hi=frame.get('requested_us',frame['target_us']) if horizon=='requested' else frame['target_us']
    ends=interpolate(samples,np.array([lo,hi]))
    if not np.isfinite(ends).all():return None
    return surface(np.concatenate([ends[:1],samples[(samples[:,0]>lo)&(samples[:,0]<hi),1:3],ends[1:]]),transform)


def detail_metrics(frames,contacts,geometry_horizon='requested'):
    if geometry_horizon not in ('target','requested'):raise ValueError('Unknown geometry horizon')
    n=len(frames);ids=np.array([f['contact'] for f in frames]);times=np.array([f['frame_us'] for f in frames])
    dt=valid_intervals(ids,times);held=np.r_[dt[1:],0.]
    names=['unproductive_tip','unproductive_body','off_path_tip','off_path_body','flash_tip','flash_body','correct_withdrawal_tip']
    s={name:np.full(n,np.nan) for name in names}
    for i,cur in enumerate(frames):
        m=cur['transform'];lo=cur['latest_us'];hi=cur['target_us'];truth=contacts[cur['contact']]['samples']
        path=truth_path(cur,truth,m,geometry_horizon)
        if path is not None:
            preview=surface(cur['curve'][:,1:3],m);p,w=sample_path(preview)
            d=distance_to_path(p,path)
            s['off_path_tip'][i]=distance_to_path(preview[-1:],path)[0]
            s['off_path_body'][i]=np.sqrt(np.sum(w*d*d)/max(w.sum(),1e-12))
            if held[i]>0:
                following=frames[i+1]
                next_path=surface(np.concatenate([following['actual_recent'][:,1:3],following['curve'][1:,1:3]]),m)
                # Initial exposure of wrong ink that vanishes on the next query.
                # The correction itself is not counted as unproductive motion.
                gone=np.minimum(d,distance_to_path(p,next_path))
                s['flash_body'][i]=np.sqrt(np.sum(w*gone*gone)/max(w.sum(),1e-12))
                s['flash_tip'][i]=min(s['off_path_tip'][i],distance_to_path(preview[-1:],next_path)[0])
        if not dt[i]:continue
        prev=frames[i-1];low=prev['latest_us'];high=min(prev['target_us'],hi)
        if high>low:
            t=np.linspace(low,high,33)
            a=surface(interpolate(prev['curve'],t),m)
            path=np.concatenate([cur['actual_recent'],cur['curve'][1:]])
            _,rev=np.unique(path[::-1,0],return_index=True);path=path[np.sort(len(path)-1-rev)]
            b=surface(interpolate(path,t),m);answer=surface(interpolate(truth,t),m)
            valid=np.isfinite(a).all(1)&np.isfinite(b).all(1)&np.isfinite(answer).all(1)
            if valid.any():
                excess=excess_correction(a[valid],b[valid],answer[valid])
                weights=(1+3*np.linspace(0,1,33)**2)[valid]
                s['unproductive_body'][i]=np.sqrt(np.average(excess**2,weights=weights))
                s['unproductive_tip'][i]=excess[-1]
        # A shorter forecast is allowed, but withdrawing already-correct ink
        # must not silently disappear from the shared-time revision metric.
        end=surface(prev['curve'][-1:,1:3],m)
        answer=surface(interpolate(truth,np.array([prev['target_us']])),m)
        current=surface(np.concatenate([cur['actual_recent'][:,1:3],cur['curve'][1:,1:3]]),m)
        if np.isfinite(answer).all():
            s['correct_withdrawal_tip'][i]=max(0.,distance_to_path(end,current)[0]-norm(end-answer)[0])
    return s
