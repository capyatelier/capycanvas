"""Whole-preview evaluation on recorded clocks. Requires NumPy, not a GPU."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import numpy as np
from balance_metrics import balance_metrics
from detail_metrics import detail_metrics
from flicker_metrics import load_trace
from history_metrics import paired_oscillations, paired_correction_smoothness, braking_exposure, validate_pair
from retraction_metrics import retraction_metrics
from stability_metrics import regression_snapshot
from speed_metrics import speed_weighted_metrics


def read_recording(path):
    contacts, current = {}, None
    for line in path.open():
        record = json.loads(line)
        if 'Begin' in record:
            b = record['Begin']
            current = {'samples': [], 'queries': {}, 'policy': b['policy']}
            contacts[b['id']] = current
        if 'Predictor' in record and current is not None:
            event = record['Predictor']
            if 'sample' in event or 'stationary' in event:
                current['samples'].append(event.get('sample', event.get('stationary')))
            if 'replace' in event:
                index, value = event['replace']; current['samples'][index] = value
            if 'policy' in event:current['policy'] = event['policy']
            if 'query' in event:current['queries'][event['query'][0]] = current['policy']
        if 'End' in record:current = None
    for c in contacts.values():
        c['samples'] = np.array(sorted({s[0]: s for s in c['samples']}.values()))
    return contacts


def score(frames, contacts):
    # Always score all of this model's visible output, not an old model's shorter
    # horizon. Paired A/B temporal metrics remain available separately below.
    validate_pair(frames, frames)
    flashes = detail_metrics(frames, contacts, geometry_horizon='requested')
    balance, bs = balance_metrics(frames, contacts, flashes)
    retraction, rs = retraction_metrics(frames, contacts, geometry_horizon='requested')
    oscillation, os = paired_oscillations(frames, frames, contacts)
    correction, cs = paired_correction_smoothness(frames, frames, contacts)
    result = dict(balance=balance, retraction=retraction, oscillation=oscillation[0],
                  correction_smoothness=correction[0], braking=braking_exposure(frames, contacts, flashes))
    signals = {**bs, **{'retreat_'+k:v for k,v in rs.items()},
               **{'oscillation_'+k:v for k,v in os[0].items()}, **{'correction_'+k:v for k,v in cs[0].items()}}
    result['speed_weighted'] = speed_weighted_metrics(signals)
    return result, signals


def analyze(records, frames_path, output, before_path=None):
    output.mkdir(parents=True, exist_ok=True)
    contacts = read_recording(records); frames = load_trace(frames_path, contacts)
    result, signals = score(frames, contacts)
    (output/'metrics.json').write_text(json.dumps(result, indent=2)+'\n')
    np.savez_compressed(output/'signals.npz', **signals)
    snapshot = regression_snapshot(result)
    (output/'regression-snapshot.json').write_text(json.dumps(snapshot, indent=2)+'\n')
    with (output/'severity.csv').open('w') as f:
        writer=csv.writer(f);writer.writerow(['category','metric','unit','lower','upper','windows','seconds'])
        groups = {**result['balance'], **{'retreat / '+k:v for k,v in result['retraction'].items()}}
        for category, row in groups.items():
            for key, values in row.items():
                if not isinstance(values,dict) or 'severity_windows' not in values:continue
                edges=[0,*values['severity_bounds'],float('inf')]
                for lo,hi,n,seconds in zip(edges,edges[1:],values['severity_windows'],values['severity_seconds']):
                    writer.writerow([category,key,'ms' if key.startswith('behind_ms') or key.endswith('_ms') else 'px',lo,hi,n,seconds])
    if before_path:
        before=load_trace(before_path, contacts);validate_pair(before,frames)
        old,old_signals=score(before,contacts)
        np.savez_compressed(output/'before-signals.npz',**old_signals)
        (output/'before-metrics.json').write_text(json.dumps(old,indent=2)+'\n')
        # Both streams use exactly the same future points for temporal comparison.
        oscillation,_=paired_oscillations(before,frames,contacts)
        correction,_=paired_correction_smoothness(before,frames,contacts)
        (output/'comparison.json').write_text(json.dumps(dict(
            oscillation=oscillation,correction_smoothness=correction),indent=2)+'\n')
    sources=list(Path(__file__).parent.glob('*.py'))
    inputs=[records,frames_path]+([before_path] if before_path else [])
    (output/'analysis-manifest.json').write_text(json.dumps(dict(
        inputs={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs},
        metrics={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}),indent=2)+'\n')
    return snapshot


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('records',type=Path);p.add_argument('frames',type=Path)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--before',type=Path,help='Optional saved frame export from another revision')
    a=p.parse_args();analyze(a.records,a.frames,a.output,a.before)
