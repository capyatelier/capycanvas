# Tablet prediction dataset bank

Each independent recording is `DEVICE.capystrokes` with a matching
`DEVICE.expected.json`. Use a descriptive lowercase name and a session suffix
for additional recordings from the same device. Optional `DEVICE.labels.json`
annotations are bound to the recording SHA256; labels describe shape, not
recognized characters or predictor inputs. Keep devices and sessions separate
when splitting training and evaluation data.

The Rust bank test and Python runner discover every recording automatically.
The benchmark defaults to Optimized regardless of the setting captured on the
device. `--algorithm previous` evaluates Previous on the same input and clocks.
Device names are provenance labels, not hardware IDs inferred from input events.

## Current recordings

| File | Capture | Contacts | Predictor samples | Queries | Compressed bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| `wacom-pro-27.capystrokes` | Wacom Pro 27, GTK; name supplied by the user | 81 | 22,029 | 10,985 | 1,353,690 |
| `movink14.capystrokes` | Wacom Movink 14 (DTHA140), Android | 89 | 20,879 | 10,272 | 1,312,397 |

This replaces the former `pen-20260921` fixture. Its SHA256 is
`dc30c7cc1cfb60beda7b37919175181d76c950243b81d8791cee0d979b960d86`.
The 132.345-second capture contains raw tablet deliveries, predictor samples,
corrections, policies and queries. Recorded clocks do not measure presentation
latency. Its reviewed summary includes the continuity refinement's small
instantaneous-error tradeoff, documented in the evaluation guide below.

`movink14` is the user's replacement capture, saved as
`stroke-recording (1).capystrokes`, not the earlier unsuffixed file. SHA256:
`b001b18ef51846a02a1aee1b10731e454aa578d72a57139ce7112f5e66ba9c15`.
It contains 100.284 seconds of capture and 28,245 raw events, with no native
predictions. The shared predictor used a 16 ms requested horizon.

## Replay and validate

```sh
cargo build -p layer-engine --release --offline --features prediction-bench --examples
cargo test -p layer-engine --offline --features prediction-bench collected_recording_bank
python3 tools/prediction/replay-bank.py --output artifacts/strokes/tablet-bank
# Full geometry, temporal metrics and regression checks; requires NumPy:
python3 tools/prediction/replay-bank.py --output artifacts/strokes/tablet-bank --analyze --check
python3 tools/prediction/replay-bank.py --algorithm previous --output artifacts/strokes/tablet-bank-previous --analyze --check
python3 tools/prediction/preview-player.py artifacts/strokes/tablet-bank/wacom-pro-27
```

For a new recording, run `replay-bank.py --output artifacts/strokes/tablet-bank
--create-baselines --analyze`. This creates only missing baselines. Review the
provenance, results and preview episodes before checking in data and sidecars.
Never update an existing baseline merely to make a failing change pass.
Run it again with `--algorithm previous` to add that selection's missing baseline.

The expected JSON version 1 binds the recording hash to `summary` (counts,
accuracy, coverage, useful horizon) and `correction_stability` (version 5 temporal
snapshot) for Optimized. `alternatives.previous` holds the same fields for
Previous; the Rust bank test checks both. Historical fields such as
`error_step_rms_px` are endpoint diagnostics, not flicker measurements.
Version 5 additionally follows the initial preview through measured-ink
settlement and disappearance. These temporal references were recomputed from
frozen pre-change outputs: `9ccc9b22` for Wacom Pro 27, and `ba9b91e0` for each
Movink selection. Other temporal values and budgets retain their old references.
After passing those frozen guards, Optimized's mean speed-weighted transient
cost bound was tightened to preserve this fix's measured improvement. Other
cost thresholds and Previous's values remain at their pre-change references.

Full analysis writes `DEVICE/analysis/metrics.json`, `severity.csv`, compressed
per-query signals and `regression-snapshot.json`. The gate protects ordinary
correction shock, oscillation, straight retreat, common-clock pen tracking,
geometric error, braking and scoring eligibility. An optional comparison takes
saved frame exports from an earlier revision. The replay CLI also accepts
`--algorithm previous|optimized` for comparing the two settings choices. See [recording and evaluation](../../../../docs/development/stroke-recording.md)
for metric definitions, capture format, host tests and limitations.
