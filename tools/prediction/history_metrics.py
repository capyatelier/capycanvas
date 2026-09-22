"""Paired revision scores on identical absolute points of two preview streams.

Longer previews otherwise get scored at harder, later targets. This paired score
complements (never replaces) full visible-path flashes, ghosts and tracking gaps.
"""
import numpy as np
from flicker_metrics import interpolate, surface, valid_intervals, occupancy


def validate_pair(before, after):
    keys=lambda fs:[(f['contact'],f['query'],f['frame_us'],f['latest_us'],f['requested_us'],f['transform']) for f in fs]
    if keys(before)!=keys(after):raise ValueError('Input/query schedules differ')
    for a,b in zip(before,after):
        if not np.array_equal(a['actual_recent'],b['actual_recent']):raise ValueError('Raw inputs differ')
        if b['source']!='Platform' and not b['latest_us']<=b['target_us']<=min(b['requested_us'],b['latest_us']+64000):raise ValueError('Dishonest target time')
        if a['source']=='Platform' and (b['source']!='Platform' or not np.array_equal(a['curve'],b['curve'])):raise ValueError('Native precedence changed')


def correction_shock(a,b,c,dt1,dt2):
    """Change in correction velocity, in px per nominal 120 Hz frame squared.

    Linear corrections are smooth even if they temporarily increase RMSE.
    Compare the same future point, not successively advancing pen positions.
    """
    return ((c-b)/dt2-(b-a)/dt1) / ((dt1+dt2)*.5) / 120.**2


def sudden_motion(points):
    """Independent truth classification using adjacent 16 ms travel chords."""
    from flicker_metrics import norm
    if not np.isfinite(points).all():return False
    v=np.diff(points,axis=0);length=norm(v)
    if length.max()<1.:return False
    if length.min()<length.max()*.6:return True
    turn=np.arctan2(v[0,0]*v[1,1]-v[0,1]*v[1,0],np.dot(v[0],v[1]))
    return abs(turn)>.35


def paired_correction_smoothness(before,after,contacts):
    """Whole-preview correction acceleration, with separate sudden-motion rows.

    No credit/penalty depends on whether the correction reduces RMSE. Timing,
    ghosts, oscillation and retreat remain independent metrics. Sudden motion
    may legitimately need a discontinuity; never use this score alone to tune
    stop response. Both algorithms are evaluated at identical future times.
    """
    from flicker_metrics import norm
    ids=np.array([f['contact'] for f in before]);times=np.array([f['frame_us'] for f in before])
    dt=valid_intervals(ids,times);held=np.r_[dt[1:],0.]
    truth_windows=[surface(interpolate(contacts[f['contact']]['samples'],
        f['frame_us']+np.array([-16000,0,16000])),f['transform']) for f in before]
    known=np.array([np.isfinite(p).all() for p in truth_windows])
    sudden=np.array([sudden_motion(p) for p in truth_windows])
    abrupt=sudden.copy()
    abrupt[1:] |= sudden[:-1] & (ids[1:]==ids[:-1])
    signals=[{k:np.full(len(before),np.nan) for k in ['tip','body']} for _ in range(2)]
    for i in range(2,len(before)):
        if not dt[i] or not dt[i-1]:continue
        low=before[i]['latest_us']
        high=min(fs[j]['target_us'] for fs in [before,after] for j in [i-2,i-1,i])
        if high<=low:continue
        t=np.linspace(low,high,33);m=before[i]['transform']
        paths=[];valid=True
        for fs in [before,after]:
            ps=[surface(interpolate(f['curve'],t),m) for f in fs[i-2:i+1]]
            valid &= np.isfinite(ps).all()
            paths.append(ps)
        if not valid:continue
        for s,(a,b,c) in zip(signals,paths):
            shock=norm(correction_shock(a,b,c,dt[i-1],dt[i]))
            s['tip'][i]=shock[-1]
            s['body'][i]=np.sqrt(np.average(shock**2,weights=1+3*np.linspace(0,1,33)**2))
    results=[]
    for s in signals:
        groups={}
        for label,selection in [('all',np.ones(len(before),bool)),('ordinary',known&~abrupt),('sudden',abrupt),('boundary',~known&~abrupt)]:
            group={}
            for k,v in s.items():
                good=np.isfinite(v)&selection;seconds=held[good].sum()
                group[k]=dict(eligible_seconds=float(seconds),mean=float(np.sum(v[good]*held[good])/max(seconds,1e-12)),
                    thresholds={str(x):occupancy(good&(v>=x),held,ids) for x in [.5,1.,2.,4.,8.,16.]})
                for x,row in group[k]['thresholds'].items():row['windows']=int(np.sum(good&(v>=float(x))))
            groups[label]=group
        results.append(groups)
        s['sudden']=abrupt
        s['motion_known']=known
    return results,signals


def braking_exposure(frames, contacts, flashes):
    """Fast loss of measured speed, distinguished from a geometric reversal.

    Classify using arclength over consecutive 16 ms truth windows; do not infer a
    stop from cancelling displacement around a corner. Scoring is retrospective.
    """
    from flicker_metrics import norm
    ids=np.array([f['contact'] for f in frames]);times=np.array([f['frame_us'] for f in frames])
    dt=valid_intervals(ids,times);held=np.r_[dt[1:],0.]
    braking=np.zeros(len(frames),bool)
    for i,f in enumerate(frames):
        xy=surface(interpolate(contacts[f['contact']]['samples'],f['frame_us']+np.arange(-16,17,4)*1000),f['transform'])
        if not np.isfinite(xy).all():continue
        speed=norm(np.diff(xy,axis=0))
        before=speed[:4].sum()/.016;after=speed[4:].sum()/.016
        braking[i]=before>180 and after<before*.6
    result=dict(observed_seconds=float(held[braking].sum()),queries=int(braking.sum()))
    for key in ['flash_tip','flash_body','off_path_tip','correct_withdrawal_tip']:
        v=flashes[key];exposure=dt if key=='correct_withdrawal_tip' else held
        valid=braking&np.isfinite(v);seconds=exposure[valid].sum()
        result[key]=dict(eligible_seconds=float(seconds),mean=float(np.sum(v[valid]*exposure[valid])/max(seconds,1e-12)),
            thresholds={str(t):occupancy(valid&(v>=t),exposure,ids) for t in [.5,1.,2.,4.,8.,16.]})
    return result


def paired_oscillations(before, after, contacts):
    """Opposing revisions on identical times; monotone convergence is credited.

    Credit net progress continuously instead of classifying an entire reversal
    from the sign of a tiny error change. Pure equal-error oscillation pays the
    full reversal amplitude. A convergent zigzag pays only its inefficient share;
    aligned corrections toward the answer have no reversal to begin with.
    Full-path flashes separately measure briefly displaying incorrect geometry.
    """
    from flicker_metrics import norm, reversal_amplitude
    ids=np.array([f['contact'] for f in before]);times=np.array([f['frame_us'] for f in before])
    dt=valid_intervals(ids,times)
    signals=[{k:np.full(len(before),np.nan) for k in ['tip','body']} for _ in range(2)]
    for i in range(2,len(before)):
        if not dt[i] or not dt[i-1]:continue
        low=before[i-1]['latest_us']
        high=min(fs[j]['target_us'] for fs in [before,after] for j in [i-2,i-1,i])
        if high<=low:continue
        t=np.linspace(low,high,33);m=before[i]['transform']
        truth=surface(interpolate(contacts[ids[i]]['samples'],t),m)
        paths=[]
        valid=np.isfinite(truth).all(1)
        for fs in [before,after]:
            ps=[]
            for f in fs[i-2:i+1]:
                path=np.concatenate([f['actual_recent'],f['curve'][1:]])
                _,rev=np.unique(path[::-1,0],return_index=True);path=path[np.sort(len(path)-1-rev)]
                ps.append(surface(interpolate(path,t),m))
                valid &= np.isfinite(ps[-1]).all(1)
            paths.append(ps)
        if not valid.all():continue
        for s,(a,b,c) in zip(signals,paths):
            amplitude=reversal_amplitude(b-a,c-b)
            ea,eb,ec=[norm(p-truth) for p in [a,b,c]]
            travel=norm(b-a)+norm(c-b)
            progress=np.maximum(0.,ea-ec)
            amplitude *= np.clip(1.-progress/np.maximum(travel,1e-12),0.,1.)
            s['tip'][i]=amplitude[-1]
            s['body'][i]=np.sqrt(np.average(amplitude**2,weights=1+3*np.linspace(0,1,33)**2))
    results=[]
    for s in signals:
        r={}
        for key,v in s.items():
            good=np.isfinite(v)
            r[key]=dict(eligible_seconds=float(dt[good].sum()),mean=float(np.sum(v[good]*dt[good])/max(dt[good].sum(),1e-12)),
                thresholds={str(x):occupancy(good&(v>=x),dt,ids,True) for x in [.5,1.,2.,4.,8.,16.]})
        results.append(r)
    return results,signals
