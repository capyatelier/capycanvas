#!/usr/bin/env python3
"""Action-tagged CPU wall/running time, GPU observations and canvas latches.

Markers come from AndroidPenMotion (CLOCK_BOOTTIME). GPU observations arrive
asynchronously; their bucket is observation time, not exact originating action.
Nested scopes overlap. Latches prove buffer consumption, not image correctness
or physical pen-to-photon latency. Failed traces must remain labelled failed.
"""
import argparse
import bisect
import csv
import io
import json
import math
import subprocess


def distribution(values):
    values = sorted(values)
    if not values:
        return {"n": 0}
    def p(f):
        return round(values[max(0, math.ceil(len(values) * f) - 1)], 3)
    return {"n": len(values), "sum": round(sum(values), 3),
            "p50": p(.5), "p95": p(.95), "p99": p(.99), "max": p(1)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace")
    parser.add_argument("markers")
    parser.add_argument("--processor", required=True)
    args = parser.parse_args()
    query = """
    SELECT 'bounds' kind, start_ts ts, end_ts dur, '' name FROM trace_bounds
    UNION ALL
    SELECT 'slice', s.ts, s.dur, s.name FROM slice s
      JOIN thread_track tt ON tt.id=s.track_id JOIN thread t USING(utid)
      JOIN process p USING(upid)
      WHERE p.name='art.capycanvas' AND t.name='capy-canvas' AND s.dur>=0
    UNION ALL
    SELECT 'sched', s.ts, s.dur, '' FROM sched s JOIN thread t USING(utid)
      JOIN process p USING(upid) WHERE p.name='art.capycanvas' AND t.name='capy-canvas'
    UNION ALL
    SELECT 'counter', c.ts, c.value, t.name FROM counter c
      JOIN counter_track t ON t.id=c.track_id WHERE t.name GLOB 'Capy*'
    UNION ALL
    SELECT 'latch', ts, dur, name FROM slice
      WHERE name GLOB 'latchBuffer SurfaceView*art.capycanvas/*'
    UNION ALL
    SELECT 'error', 0, value, name FROM stats WHERE severity='error' AND value>0
    ORDER BY ts
    """
    output = subprocess.run([args.processor, "query", args.trace, query],
                            check=True, capture_output=True, text=True)
    rows = list(csv.DictReader(io.StringIO(output.stdout)))
    for row in rows:
        row["ts"] = int(row["ts"])
        row["dur"] = int(float(row["dur"]))
    markers = {}
    with open(args.markers) as source:
        for line in source:
            parts = line.split()
            if len(parts) == 2 and parts[1].isdigit():
                markers[parts[0]] = int(parts[1])
    starts = [(name[:-6], time) for name, time in markers.items() if name.endswith('_start')]
    sched = [(r["ts"], r["ts"] + r["dur"]) for r in rows if r["kind"] == "sched" and r["dur"]>0]
    sched_starts = [a for a, b in sched]
    def running(a, b):
        i = max(0, bisect.bisect_right(sched_starts, a) - 1)
        total = 0
        while i < len(sched) and sched[i][0] < b:
            x, y = sched[i]
            total += max(0, min(b, y) - max(a, x))
            i += 1
        return total / 1e6
    report = {"markers": markers,
              "counter_distribution_units": "Counters ending in ' ns' are converted to milliseconds; counter_first_last retains raw values.",
              "trace_errors": {r['name']:r['dur'] for r in rows if r['kind']=='error'}, "actions": {}}
    bounds = next(r for r in rows if r['kind']=='bounds')
    report['retained_seconds'] = (bounds['dur']-bounds['ts'])/1e9
    for name, start in starts:
        end = markers[name + '_end']
        # History markers only bracket injection. Allow two seconds for response.
        if name in ('undo','redo'):
            end = start + 2_000_000_000
        selected = [r for r in rows if start <= r['ts'] < end]
        latches = [r['ts'] for r in selected if r['kind']=='latch']
        gaps = [(b-a)/1e6 for a,b in zip([start]+latches,latches+[end])]
        scopes = {}
        for r in selected:
            if r['kind']=='slice' and (r['name'].startswith('capy.') or r['name'] in ('AllocateCommandBuffers','QueueSubmit','QueuePresentKHR','AcquireNextImageKHR') or r['name'].startswith('Choreographer#doFrame')):
                key = 'Choreographer#doFrame' if r['name'].startswith('Choreographer#doFrame') else r['name']
                scopes.setdefault(key, []).append(r)
        counters = {}
        for r in selected:
            if r['kind']=='counter':
                counters.setdefault(r['name'],[]).append(r['dur'])
        report['actions'][name] = {
            'fully_retained': bounds['ts'] <= start and end <= bounds['dur'],
            'seconds': (end-start)/1e9, 'owner_cpu_ms': running(start,end),
            'canvas_latches': len(latches), 'latches_per_second':len(latches)/((end-start)/1e9),
            'latch_gap_ms': distribution(gaps) if name not in ('undo', 'redo') else None,
            'first_latch_ms': (latches[0]-start)/1e6 if latches else None,
            'scopes': {k:{'wall_ms':distribution([r['dur']/1e6 for r in v]),
                          'running_ms':distribution([running(r['ts'],r['ts']+r['dur']) for r in v])} for k,v in scopes.items()},
            'counters':{k:distribution([x/1e6 if k.endswith(' ns') else x for x in v]) for k,v in counters.items()},
            'counter_first_last':{k:[v[0],v[-1]] for k,v in counters.items()},
        }
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    main()
