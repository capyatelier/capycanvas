"""Score tracking and flashes together, on clocks independent of the forecast.

Retrospective direction classes use only recorded truth, never a model's output.
The predictor itself must use causal input only. Delay sweeps approximate possible
presentation latency; these recordings do not contain actual GPU presents.
"""
import numpy as np
from flicker_metrics import interpolate, surface, norm, distance_to_path, valid_intervals, occupancy

DELAYS = (0, 8, 16, 24)


def classify_motion(points):
    """Seven truth positions, 8 ms apart, centered on the recorded query.

    Overlapping 16 ms chords attenuate report quantization. Distinguish turn
    magnitude from changing/sign-reversing curvature; speed is a separate axis.
    """
    if not np.isfinite(points).all():
        return np.nan, 'unclassified boundary / gap'
    step = np.diff(points, axis=0)
    travel = norm(step).sum()
    speed = float(travel / .048)
    if speed < 60:
        return speed, 'stationary / micro motion'
    v = points[2:] - points[:-2]
    lengths = norm(v)
    angles = np.arctan2(v[:-1, 0]*v[1:, 1]-v[:-1, 1]*v[1:, 0], np.sum(v[:-1]*v[1:], axis=1))
    angles[np.minimum(lengths[:-1], lengths[1:]) < .75] = 0.
    turning = np.abs(angles).sum()
    change = np.abs(np.diff(angles)).sum()
    reversing = turning - abs(angles.sum())
    efficiency = norm(points[-1]-points[0]) / max(travel, 1e-12)
    if np.max(np.abs(angles)) > .25 or change > .20 or reversing > .18 or efficiency < .90:
        shape = 'changing direction'
    elif turning < .20 and change < .12 and efficiency > .985:
        shape = 'steady straight'
    else:
        shape = 'smooth curve'
    speed_class = 'slow' if speed < 600 else 'medium' if speed < 1400 else 'fast'
    return speed, speed_class + ' / ' + shape


def balance_metrics(frames, contacts, flashes):
    n = len(frames)
    ids = np.array([f['contact'] for f in frames])
    times = np.array([f['frame_us'] for f in frames])
    dt = valid_intervals(ids, times)
    held = np.r_[dt[1:], 0.]
    s = {name: np.full(n, np.nan) for name in ['speed', 'horizon_ms']}
    for delay in DELAYS:
        for name in ['gap', 'behind', 'ahead', 'sideways', 'endpoint_error', 'behind_ms']:
            s[f'{name}_{delay}'] = np.full(n, np.nan)
    classes = []
    for i, f in enumerate(frames):
        truth = contacts[f['contact']]['samples']
        m = f['transform']
        now = f['frame_us']
        local = surface(interpolate(truth, now + np.arange(-24, 25, 8)*1000), m)
        s['speed'][i], category = classify_motion(local)
        classes.append(category)
        s['horizon_ms'][i] = (f['target_us'] - f['latest_us']) / 1000
        # A nearby old loop must not conceal a hole at the current pen tip.
        recent = f['actual_recent']
        recent = recent[recent[:,0] >= f['latest_us']-16000]
        visible = surface(np.concatenate([recent[:,1:3], f['curve'][1:,1:3]]), m)
        tip = surface(f['curve'][-1:,1:3], m)[0]
        for delay in DELAYS:
            t = now + delay*1000
            a, q, b = surface(interpolate(truth, np.array([t-4000, t, t+4000])), m)
            if not np.isfinite(q).all():
                continue
            s[f'gap_{delay}'][i] = distance_to_path(q[None,:], visible)[0]
            s[f'endpoint_error_{delay}'][i] = norm(tip-q)
            v = b-a
            length = norm(v)
            if not np.isfinite(v).all() or length < .48:
                continue
            tangent = v/length
            residual = tip-q
            along = np.dot(residual, tangent)
            s[f'behind_{delay}'][i] = max(0., -along)
            s[f'ahead_{delay}'][i] = max(0., along)
            s[f'sideways_{delay}'][i] = abs(residual[0]*tangent[1]-residual[1]*tangent[0])
            s[f'behind_ms_{delay}'][i] = max(0., -along)*8/length
    classes = np.array(classes)
    selections = {'all': np.ones(n, bool),
                  **{label+' motion': np.char.startswith(classes, label+' / ')
                     for label in ['slow', 'medium', 'fast']},
                  'slow/medium steady': np.isin(classes, ['slow / steady straight', 'medium / steady straight']),
                  'slow/medium changing': np.isin(classes, ['slow / changing direction', 'medium / changing direction']),
                  'fast predictable': np.isin(classes, ['fast / steady straight', 'fast / smooth curve']),
                  **{label: classes == label for label in sorted(set(classes))}}
    for key in ['flash_tip', 'flash_body', 'off_path_tip', 'off_path_body', 'unproductive_tip', 'unproductive_body', 'correct_withdrawal_tip']:
        s[key] = flashes[key]
    results = {}
    for label, selected in selections.items():
        group = {'observed_seconds': float(held[selected].sum()), 'queries': int(selected.sum())}
        for key, values in s.items():
            exposure = dt if key.startswith(('unproductive', 'correct_withdrawal')) else held
            valid = selected & np.isfinite(values)
            seconds = float(exposure[valid].sum())
            group[key] = {'eligible_seconds': seconds,
                          'mean': float(np.sum(values[valid]*exposure[valid])/max(seconds,1e-12)),
                          'p95': float(np.percentile(values[valid],95)) if valid.any() else None}
            thresholds = [4.,8.,16.,24.] if key.startswith('behind_ms') else [.5,1.,2.,4.,8.,16.,32.,64.]
            if key not in ['speed', 'horizon_ms']:
                group[key]['thresholds'] = {}
                for threshold in thresholds:
                    active = valid & (values >= threshold)
                    result = occupancy(active, exposure, ids)
                    result['windows'] = int(np.sum(active))
                    result['eligible_percent'] = 100*result['seconds']/max(seconds,1e-12)
                    group[key]['thresholds'][str(threshold)] = result
                bins = np.searchsorted(thresholds, values, side='right')
                group[key]['severity_bounds'] = thresholds
                group[key]['severity_windows'] = [int(np.sum(valid & (bins == b)))
                                                  for b in range(len(thresholds)+1)]
                group[key]['severity_seconds'] = [float(exposure[valid & (bins == b)].sum())
                                                  for b in range(len(thresholds)+1)]
        results[label] = group
    s.update(category=classes, ids=ids, held_dt=held)
    return results, s
