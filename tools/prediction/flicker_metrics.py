"""Temporal preview diagnostics, independent of predictor implementation.

Units: physical pixels and seconds on the recorded query clock. A threshold is
an engineering sensitivity sweep, not a calibrated human detection threshold.
The primary oscillation detector requires opposing correction directions;
monotone convergence and constant bias score zero. Ghost error is separate.
"""
import json
from pathlib import Path
import numpy as np


def norm(v):
    return np.linalg.norm(v, axis=-1)


def reversal_amplitude(a, b):
    """Amount retraced by the smaller update; zero for non-opposing steps."""
    return np.maximum(0., -np.sum(a*b, axis=-1)) / np.maximum(np.maximum(norm(a), norm(b)), 1e-12)


def valid_intervals(ids, times):
    dt = np.r_[0., np.diff(times)] / 1e6
    valid = np.r_[False, ids[1:] == ids[:-1]] & (dt > 0) & (dt <= .05)
    return np.where(valid, dt, 0.)


def occupancy(active, dt, ids, both_intervals=False):
    active = active.copy() & (dt > 0)
    if both_intervals:
        active[:-1] |= active[1:] & (ids[1:] == ids[:-1])
    active &= dt > 0
    durations, current = [], 0.
    for yes, duration in zip(active, dt):
        if yes:
            current += duration
        elif current:
            durations.append(current); current = 0.
    if current:
        durations.append(current)
    return dict(seconds=float(dt[active].sum()), percent=float(100*dt[active].sum()/max(dt.sum(),1e-12)),
                episodes=len(durations), longest_episode_ms=max(durations,default=0.)*1000,
                p95_episode_ms=float(np.percentile(durations,95)*1000) if durations else 0.)


def surface(p,m):
    return p @ np.array([[m[0],m[1]],[m[2],m[3]]]) + m[4:6]


def interpolate(samples,times):
    samples=np.asarray(samples)
    # Exact samples and <=32 ms interpolation only; never extend truth.
    t=samples[:,0];p=samples[:,1:3]
    if len(t)==1:
        return np.where((np.asarray(times)==t[0])[:,None],p[0],np.nan)
    result=np.column_stack([np.interp(times,t,p[:,axis],left=np.nan,right=np.nan) for axis in [0,1]])
    i=np.searchsorted(t,times,side='right')
    j=np.clip(i,1,len(t)-1)
    gaps=t[j]-t[j-1]
    exact=(times==t[j-1])|(times==t[j])
    result[(gaps>32000)&~exact]=np.nan
    return result


def load_trace(path, contacts):
    frames=[];actual=[];cid=None
    for line in Path(path).open():
        f=json.loads(line)
        if 'format' in f:continue
        if f['contact']!=cid:cid=f['contact'];actual=[]
        actual[f['real_start']:]=f['real']
        f['actual_recent']=np.array(actual[max(0,len(actual)-64):])
        curve=np.array(f['preview']);f['curve']=curve
        frames.append(f)
    return frames


def distance_to_path(points, path):
    """Exact Euclidean distance to line segments, including round end caps."""
    if len(path)==1:
        return norm(points-path[0])
    a=path[:-1];v=np.diff(path,axis=0)
    t=np.sum((points[:,None,:]-a)*v,axis=2)/np.maximum(np.sum(v*v,axis=1),1e-20)
    return np.min(norm(points[:,None,:]-(a+np.clip(t,0,1)[:,:,None]*v)),axis=1)


def sample_path(path, spacing=1.):
    """Arc-length quadrature: long segments receive proportionally more weight."""
    if len(path)==1:return path.copy(),np.zeros(1)
    length=norm(np.diff(path,axis=0));total=length.sum()
    if total<1e-9:return path[-1:].copy(),np.zeros(1)
    edges=np.linspace(0,total,max(2,int(np.ceil(total/spacing))+1))
    times=(edges[:-1]+edges[1:])/2
    arc=np.r_[0,np.cumsum(length)]
    return np.column_stack([np.interp(times,arc,path[:,axis]) for axis in [0,1]]),np.diff(edges)
