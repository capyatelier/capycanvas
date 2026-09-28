#!/usr/bin/env python3
"""Join delayed GPU samples to brush submissions, never their observation time.

The terminal renderer/GPU ID difference is derived per capture and every selected
submission must have all four GPU observations. Inspect the printed offsets and
sample coverage along with the raw counters; timestamps include queue gaps.
"""
import argparse
import csv
import io
import json
import statistics
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--processor', default='trace_processor')
    args = parser.parse_args()
    names = ['Capy GPU elapsed ns', 'Capy GPU paint ns', 'Capy GPU prediction ns', 'Capy GPU composition ns']
    query = """SELECT c.ts,t.name,c.value FROM counter c JOIN counter_track t ON t.id=c.track_id
WHERE t.name IN ('Capy renderer frames','Capy GPU observation','Capy GPU elapsed ns',
'Capy GPU phase observation','Capy GPU paint ns','Capy GPU prediction ns','Capy GPU composition ns')
ORDER BY c.ts,c.id"""
    for trace in sorted(args.directory.glob('*.perfetto-trace')):
        label = trace.name.removesuffix('.perfetto-trace')
        info = json.loads(trace.with_name(label + '-info.json').read_text())
        if info.get('mode') == 'pinch' or 'repeats' not in info:
            continue
        raw = subprocess.run([args.processor, 'query', str(trace), query],
                             capture_output=True, text=True, check=True).stdout
        trace.with_name(label + '-gpu-observations.csv').write_text(raw)
        gpu, rendered = {}, []
        elapsed_id = phase_id = None
        for row in csv.DictReader(io.StringIO(raw)):
            name, value = row['name'], float(row['value'])
            if name == 'Capy renderer frames':
                rendered.append(int(value))
            elif name == 'Capy GPU observation':
                elapsed_id = int(value)
            elif name == 'Capy GPU phase observation':
                phase_id = int(value)
            else:
                frame = elapsed_id if name == 'Capy GPU elapsed ns' else phase_id
                if frame is not None:
                    gpu.setdefault(frame, {})[name] = value / 1e6
        if not rendered or not gpu:
            raise RuntimeError(f'{label}: trace has no renderer/GPU counters')
        if set(gpu[max(gpu)]) != set(names):
            raise RuntimeError(f'{label}: final GPU observation has not fully drained')
        offset = max(rendered) - max(gpu)
        if offset < 0:
            raise RuntimeError(f'{label}: inconsistent terminal renderer/GPU counters')
        runs = []
        for index in range(info['repeats']):
            data = json.loads(trace.with_name(f'{label}-{index}.json').read_text())
            start, end = data['motion']['begin_ns'], data['motion']['end_ns']
            last = int(next(row['value'] for row in data['renderer_before']['rows'] if row['label'] == 'Frames'))
            samples = []
            for _, queued, completed, raster in data['completions']:
                changed = raster > last
                last = max(last, raster)
                if changed and start <= queued < end:
                    observed = gpu.get(raster - offset, {})
                    if set(observed) != set(names):
                        raise RuntimeError(f'{label}: incomplete GPU sample for renderer {raster}: {observed}')
                    samples.append(dict(renderer=raster, gpu_id=raster-offset,
                                        queued_ns=queued, completed_ns=completed, **observed))
            if not samples:
                raise RuntimeError(f'{label}: no selected nonempty submissions in run {index}')
            runs.append(dict(run=index, samples=samples))
        samples = [sample for run in runs for sample in run['samples']]
        means = {name: statistics.mean(s[name] for s in samples) for name in names}
        output = dict(label=label, renderer_minus_gpu_id=offset,
                      terminal_renderer=max(rendered), terminal_gpu=max(gpu), n=len(samples),
                      means_ms=means, runs=runs,
                      method='Terminal ID offset, validated by complete elapsed and phase coverage for every nonempty submission queued during input; includes observations after input stops.')
        trace.with_name(label + '-aligned-gpu.json').write_text(json.dumps(output, indent=2))
        print(label, 'offset', offset, 'samples', len(samples), {k: round(v, 2) for k, v in means.items()})


if __name__ == '__main__':
    main()
