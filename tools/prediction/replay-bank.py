#!/usr/bin/env python3
"""Replay each tablet recording through Smooth Motion. Never overwrite baselines."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--data-dir',type=Path,default=Path('crates/layer-engine/tests/data'))
    p.add_argument('--bin-dir',type=Path,default=Path('target/release/examples'))
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--frames',action='store_true',help='Export full preview geometry')
    p.add_argument('--analyze',action='store_true',help='Score temporal stability and tracking; requires NumPy')
    p.add_argument('--check',action='store_true',help='Check each device against its reviewed snapshot')
    p.add_argument('--create-baselines',action='store_true',help='Create only missing baselines, for review')
    args=p.parse_args()
    if (args.check or args.create_baselines) and not args.analyze:p.error('--check / --create-baselines require --analyze')
    recordings=sorted(args.data_dir.glob('*.capystrokes'))
    if not recordings:p.error('No .capystrokes files found')
    index={}
    for path in recordings:
        digest=hashlib.sha256(path.read_bytes()).hexdigest()
        expected_path=path.with_suffix('.expected.json')
        expected=json.loads(expected_path.read_text()) if expected_path.exists() else None
        if expected is None and not args.create_baselines:p.error(f'{expected_path}: missing baseline')
        if expected and (expected.get('version')!=1 or expected.get('recording_sha256')!=digest):
            p.error(f'{expected_path}: unsupported baseline or recording hash mismatch')
        out=args.output/path.stem;out.mkdir(parents=True,exist_ok=True)
        command=[str(args.bin_dir/'prediction-replay'),str(path)]
        if args.frames or args.analyze:command+=['--frames',str(out/'frames.jsonl')]
        with (out/'replay.csv').open('w') as csv:
            run=subprocess.run(command,stdout=csv,stderr=subprocess.PIPE,text=True,check=True)
        summary=json.loads(run.stderr);(out/'summary.json').write_text(run.stderr)
        snapshot=None;failures=[]
        if args.analyze:
            from analysis import analyze
            from stability_metrics import check_snapshot
            with (out/'records.jsonl').open('w') as records:
                subprocess.run([str(args.bin_dir/'stroke-recording'),'dump',str(path)],stdout=records,check=True)
            snapshot=analyze(out/'records.jsonl',out/'frames.jsonl',out/'analysis')
            if expected and args.check:
                failures=check_snapshot(expected.get('correction_stability',{}),snapshot)
        if expected is None and args.create_baselines:
            expected=dict(version=1,recording_sha256=digest,device=path.stem,
                          device_label_source='filename',summary=summary,correction_stability=snapshot)
            with expected_path.open('x') as f:json.dump(expected,f,indent=2);f.write('\n')
        if args.check:
            # Match the Rust bank's deterministic accuracy/coverage guardrails.
            old=expected['summary']
            for k in ['contacts','samples','queries']:
                if summary[k]!=old[k]:failures.append(f'{k} changed')
            if summary['accuracy']['graded_queries']<old['accuracy']['graded_queries']:failures.append('lost graded_queries')
            if summary['accuracy']['position_rms_px']>old['accuracy']['position_rms_px']+1e-6:
                failures.append('accuracy position_rms_px regressed')
            if summary['prediction_coverage']<old['prediction_coverage']-.001:failures.append('coverage regressed')
        (out/'checks.json').write_text(json.dumps(dict(failures=failures),indent=2)+'\n')
        sources=[Path('crates/layer-engine/src/feedback.rs'),*Path('crates/layer-engine/src/feedback').glob('*.rs')]
        (out/'source-manifest.json').write_text(json.dumps(dict(
            algorithm='optimized',
            replay_sha256=hashlib.sha256((args.bin_dir/'prediction-replay').read_bytes()).hexdigest(),
            sources_at_replay={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}),indent=2)+'\n')
        index[path.stem]=dict(recording_sha256=digest,algorithm='optimized',summary=summary,failures=failures)
        print(f'{path.stem}: {summary["contacts"]} contacts; {out}',flush=True)
        if failures:raise SystemExit('\n'.join(failures))
    (args.output/'bank.json').write_text(json.dumps(index,indent=2)+'\n')


if __name__=='__main__':main()
