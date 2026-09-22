"""Loss of useful preview reach, including a retreat while the pen advances.

Transport last frame's preview by the *true* movement at the same future offsets.
Measure missing coverage in the new frame, discounting the old forecast's error.
This is offline scoring only; future truth never enters the running predictor.
"""
import numpy as np
from flicker_metrics import interpolate, surface, norm, distance_to_path, valid_intervals, occupancy
from detail_metrics import truth_path


def straightish(points):
    if not np.isfinite(points).all():
        return False
    chords = points[2:] - points[:-2]
    lengths = norm(chords)
    if lengths.min() < 1.:
        return False
    angles = np.arctan2(chords[:-1,0]*chords[1:,1]-chords[:-1,1]*chords[1:,0],
                        np.sum(chords[:-1]*chords[1:], axis=1))
    travel = norm(np.diff(points, axis=0)).sum()
    return bool(np.max(np.abs(angles)) < .12 and np.abs(np.diff(angles)).sum() < .12
                and norm(points[-1]-points[0]) / max(travel, 1e-12) > .975)


def retraction_metrics(frames, contacts, geometry_horizon='target'):
    n = len(frames)
    ids = np.array([f['contact'] for f in frames])
    times = np.array([f['frame_us'] for f in frames])
    dt = valid_intervals(ids, times)
    held = np.r_[dt[1:], 0.]
    s = {key: np.full(n, np.nan) for key in
         ['retreat_tip', 'retreat_body', 'target_backstep_ms', 'display_lead_drop_ms']}
    eligible = np.zeros(n, bool)
    for i, cur in enumerate(frames):
        truth = contacts[cur['contact']]['samples']
        m = cur['transform']
        q = surface(interpolate(truth, cur['frame_us'] + np.arange(-24,25,8)*1000), m)
        eligible[i] = straightish(q)
        if not dt[i]:
            continue
        prev = frames[i-1]
        delta = cur['frame_us']-prev['frame_us']
        s['target_backstep_ms'][i] = max(0,prev['target_us']-cur['target_us'])/1000
        s['display_lead_drop_ms'][i] = max(0,prev['target_us']+delta-cur['target_us'])/1000
        if prev['target_us'] <= prev['latest_us']:
            continue
        t = np.linspace(prev['latest_us'],prev['target_us'],33)
        old = surface(interpolate(prev['curve'],t),m)
        answer = surface(interpolate(truth,t),m)
        advanced = surface(interpolate(truth,t+delta),m)
        # Ground-truth transport discounts actual braking/curvature. A new
        # accurate forecast covering the same future offsets scores zero even
        # when the old prediction was wrong and has been corrected.
        projected = old + advanced-answer
        recent = cur['actual_recent']
        recent = recent[recent[:,0] >= cur['latest_us'] - 16000]
        visible = surface(np.concatenate([recent[:,1:3],cur['curve'][1:,1:3]]),m)
        valid = np.isfinite(projected).all(axis=1) & np.isfinite(answer).all(axis=1)
        loss = np.full(33,np.nan)
        error=norm(old[valid]-answer[valid])
        if geometry_horizon=='requested':
            path=truth_path(prev,truth,m)
            if path is None:continue
            error=distance_to_path(old[valid],path)
        loss[valid] = np.maximum(0.,distance_to_path(projected[valid],visible)-error)
        s['retreat_tip'][i] = loss[-1]
        if valid.all():
            segments = norm(np.diff(old,axis=0))
            weights = np.r_[segments,0.]+np.r_[0.,segments]
            s['retreat_body'][i] = np.sqrt(np.sum(weights*loss**2)/max(weights.sum(),1e-12))
    results = {}
    for name, selected in [('all',np.ones(n,bool)),('straightish',eligible)]:
        group = dict(observed_seconds=float(held[selected].sum()),queries=int(selected.sum()))
        for key, values in s.items():
            valid = selected & np.isfinite(values)
            seconds = held[valid].sum()
            thresholds = [.5,1.,2.,4.,8.,16.]
            metric = dict(eligible_seconds=float(seconds),
                          mean=float(np.sum(values[valid]*held[valid])/max(seconds,1e-12)),
                          thresholds={})
            for threshold in thresholds:
                active=valid & (values>=threshold)
                row=occupancy(active,held,ids)
                row['windows']=int(np.sum(active))
                row['eligible_percent']=100*row['seconds']/max(seconds,1e-12)
                metric['thresholds'][str(threshold)]=row
            bins=np.searchsorted(thresholds,values,side='right')
            metric['severity_bounds']=thresholds
            metric['severity_windows']=[int(np.sum(valid&(bins==b))) for b in range(len(thresholds)+1)]
            metric['severity_seconds']=[float(held[valid&(bins==b)].sum()) for b in range(len(thresholds)+1)]
            group[key]=metric
        results[name]=group
    s.update(straightish=eligible,ids=ids,held_dt=held)
    return results,s
