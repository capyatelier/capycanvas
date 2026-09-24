"""Speed-sensitive costs, alongside unweighted geometric error and flash rates.

Weight squared displacement by 600/max(speed, 60), using retrospective truth
speed in physical px/s. Thus the same error costs twice as much at 300 as at
600 px/s; the floor bounds stationary penalties. Divide cost by actual eligible
time, not weighted time, so slowing an otherwise identical trace raises cost.
These are engineering priorities, not calibrated perceptual detection limits.
"""
import numpy as np
from flicker_metrics import occupancy


def speed_weight(speed, floor=60.):
    return 600. / np.maximum(speed, floor)


def error_summary(values, speed, held, ids, selected):
    good = selected & np.isfinite(values) & np.isfinite(speed) & (held > 0)
    seconds = float(held[good].sum())
    weights = speed_weight(speed)
    weighted_seconds = float(np.sum(held[good] * weights[good]))
    row = dict(eligible_seconds=seconds, weighted_eligible_seconds=weighted_seconds,
               mean_squared_cost=float(np.sum(held[good] * weights[good] * values[good]**2)
                                       / max(seconds, 1e-12)), thresholds={})
    for threshold in [.5, 1., 2., 4., 8., 16., 32., 64.]:
        active = good & (values >= threshold)
        exposure = occupancy(active, held, ids)
        exposure['windows'] = int(active.sum())
        exposure['eligible_percent'] = 100 * exposure['seconds'] / max(seconds, 1e-12)
        exposure['weighted_seconds'] = float(np.sum(held[active] * weights[active]))
        exposure['weighted_percent'] = 100 * exposure['weighted_seconds'] / max(weighted_seconds, 1e-12)
        row['thresholds'][str(threshold)] = exposure
    return row


def speed_weighted_metrics(signals):
    speed, held, ids = (signals[k] for k in ['speed', 'held_dt', 'ids'])
    values = {k: signals[k] for k in ['off_path_tip', 'off_path_body', 'flash_tip', 'flash_body',
                                      'gap_0', 'gap_8', 'gap_16', 'gap_24']}
    for part in ['tip', 'body']:
        values['retreat_' + part] = signals['retreat_retreat_' + part]
        # Require both measurements; missing truth is never counted as zero.
        values['transient_' + part] = np.maximum(values['flash_' + part], values['retreat_' + part])
    # Any visible part can flash. For severity cost, give the sensitive tip
    # twice the weight of the full-body RMS. Count overlapping failures once.
    values['transient_peak'] = np.maximum(values['transient_tip'], values['transient_body'])
    values['transient_cost'] = np.sqrt((2 * values['transient_tip']**2 + values['transient_body']**2) / 3)
    category = signals['category']
    selections = {'all': np.ones(len(speed), bool),
                  'micro motion': speed < 60,
                  'slow motion': (speed >= 60) & (speed < 600),
                  'medium motion': (speed >= 600) & (speed < 1400),
                  'fast motion': speed >= 1400,
                  'steady motion': signals['retreat_continuing'] != 'other',
                  'changing direction': np.char.endswith(category, 'changing direction')}
    return {group: {name: error_summary(value, speed, held, ids, selected)
                    for name, value in values.items()}
            for group, selected in selections.items()}
