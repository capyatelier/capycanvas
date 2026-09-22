# Tablet prediction dataset bank

Each independent recording is `DEVICE.capystrokes` with a matching
`DEVICE.expected.json`. Use a descriptive lowercase name and a session suffix
for additional recordings from the same device. Optional `DEVICE.labels.json`
annotations are bound to the recording SHA256; labels describe shape, not
recognized characters or predictor inputs. Keep devices and sessions separate
when splitting training and evaluation data.

The Rust bank test and Python runner discover every recording automatically.
Both run the sole production predictor, Smooth Motion. Device names are
provenance labels, not hardware IDs inferred from input events.

## Current recording

| File | Capture | Contacts | Predictor samples | Queries | Compressed bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| `wacom-pro-27.capystrokes` | Wacom Pro 27, GTK; name supplied by the user | 81 | 22,029 | 10,985 | 1,353,690 |

This replaces the former `pen-20260921` fixture. Its SHA256 is
`dc30c7cc1cfb60beda7b37919175181d76c950243b81d8791cee0d979b960d86`.
The 132.345-second capture contains raw tablet deliveries, predictor samples,
corrections, policies and queries. Recorded clocks do not measure presentation
latency. Its reviewed summary preserves the user-approved stable model's output.

## Replay and validate

```sh
cargo build -p layer-engine --release --offline --features prediction-bench --examples
cargo test -p layer-engine --offline --features prediction-bench collected_recording_bank
python3 tools/prediction/replay-bank.py --output artifacts/strokes/tablet-bank
# Full geometry, temporal metrics and regression checks; requires NumPy:
python3 tools/prediction/replay-bank.py --output artifacts/strokes/tablet-bank --analyze --check
python3 tools/prediction/preview-player.py artifacts/strokes/tablet-bank/wacom-pro-27
```

For a new recording, run `replay-bank.py --output artifacts/strokes/tablet-bank
--create-baselines --analyze`. This creates only missing baselines. Review the
provenance, results and preview episodes before checking in data and sidecars.
Never update an existing baseline merely to make a failing change pass.

The expected JSON version 1 binds the recording hash to `summary` (counts,
accuracy, coverage, useful horizon) and `correction_stability` (version 2 temporal
snapshot). Historical summary fields such as `error_step_rms_px` are endpoint
error diagnostics, not flicker measurements. Snapshot version 2 evaluates the
current model independently of any retired model's shorter horizon.

Full analysis writes `DEVICE/analysis/metrics.json`, `severity.csv`, compressed
per-query signals and `regression-snapshot.json`. The gate protects ordinary
correction shock, oscillation, straight retreat, common-clock pen tracking,
geometric error, braking and scoring eligibility. An optional comparison takes
saved frame exports from an earlier revision; obsolete algorithms are not kept
in production. See [recording and evaluation](../../../../docs/development/stroke-recording.md)
for metric definitions, capture format, host tests and limitations.
