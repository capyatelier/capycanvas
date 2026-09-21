# Pen prediction datasets (v1)

One `NAME.jsonl.gz` per recording; gzip-compressed UTF-8 JSON Lines, with one
header followed by one contact per line. Replay streams one contact at a time.
Add a matching `NAME.expected.json` accuracy baseline to include a recording in
the dataset-bank regression test. Keep recordings independent when splitting
training and evaluation data.

The header contains `format: "capy-pen-dataset"`, `version: 1`, total `contacts`
and `events`, and optional `metadata`. Counts are checked; unsupported versions,
duplicate identities, malformed records and incomplete recordings are errors.
Each contact contains `id`, `policy`, ordered `events`, and `cancelled`.
`policy` holds the predictor `config` and a six-element document-to-physical-
surface transform. The Rust schema is in `src/prediction_bench.rs`.

Samples are compact arrays `[elapsed_us, x, y, pressure, tilt_x, tilt_y, twist]`:
time is relative to contact start, positions are document pixels, pressure is
normalized, and angles are radians. Event objects have one key:

- `sample`, `predicted`, `stationary`: a sample array. Real samples clear native
  predictions; native history is bounded to 32 points. Stationary samples append
  without clearing native history, matching the engine.
- `replace`: `[sample_index, sample]`, a correction of existing real input.
- `observe`: `[elapsed_ns, raw_pressure, tool, flags]`. Raw pressure precedes the
  brush pressure curve; its clock is contact-relative nanoseconds. Tool names
  and flag bits use the engine's `ToolKind` and `SampleFlags` representations.
- `query`: `[query_id, frame_elapsed_us, requested_elapsed_us]`. IDs are unique
  within a contact and stable across candidate replays.
- `policy`: a replacement policy, preserving input history.
- `reset`: `null`, resetting predictor state while preserving input history.

Event order is delivery order. Preserve duplicate/out-of-order timestamps,
corrections, query timing, native predictions and resets. Never sort input before
replay. Stored data contains no predicted outputs, screenshots or UI diagnostics.

From the repository root:

```sh
cargo test -p layer-engine --release --features prediction-bench --lib
cargo run -p layer-engine --release --features prediction-bench \
  --example prediction-replay -- crates/layer-engine/tests/data/pen-20260921.jsonl.gz \
  > /tmp/prediction.csv
CAPY_PREDICTION_SCORE=1 cargo test -p layer-engine --release --lib \
  -- --nocapture --test-threads=1 > /tmp/synthetic.log 2>&1
CAPY_PREDICTION_SCORE=1 cargo test -p layer-engine --release \
  nonperiodic_maneuver_holdout -- --ignored --nocapture --test-threads=1
python3 tools/prediction/compare-synthetic.py before.log after.log comparison.json --check
```

Replay writes per-query CSV in physical pixels and a JSON summary to stderr.
An optional final `trajectory` argument overrides the algorithm only. The
feature-gated `prediction_bench::replay` API accepts any buffered dataset reader
and CSV writer; production builds contain neither replay nor capture code.

Accuracy compares each prediction with actual input at its target timestamp,
interpolating only within gaps of at most 32 ms. Missing truth is ungraded.
Flicker is the change in this error between eligible queries up to 50 ms apart;
actual motion scores zero. Bands are 4–8, 8–16, 16–32 and ≥32 physical pixels,
requiring at least 4 or 8 pixels of endpoint error respectively. Report accuracy,
flicker, coverage and useful horizon together; shortening prediction alone is
not an improvement. Compare candidates on identical query pairs and separately
at matched target timestamps when evaluating motion estimation alone.

`pen-20260921` preserves all 6,530 real samples and 3,266 queries from 14 contacts
in the collected GTK recording. Its header retains the original capture's SHA-256.
It predates the GTK clock-alignment fix: sample-relative and display-relative
horizons differ, and neither is measured pen-to-photon latency. The baseline is
for all eligible pairs in this fixture, not a subset shared with older experiments.
