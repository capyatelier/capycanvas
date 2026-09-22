# Stroke recording and prediction datasets

Open **Diagnostics → Start stroke recording**, draw normally, then choose
**Stop stroke recording**. GTK and Android open the system save dialog. Web uses
the system file picker when available, with a system share sheet or download UI
as the fallback. Cancelling or failing a save retains the recording;
the button becomes **Save stroke recording**. Recording stops automatically at
ten minutes or the 32 MiB capture budget, including when Diagnostics is hidden.
An automatic web stop offers a Save button because browser file pickers require
a user gesture. Recordings remain in memory until saved or the window is closed.

Capture follows the window across document switches. It records canvas input
delivered by the platform adapter, including coalesced/history samples, hover,
pressure before brush mapping, tilt, twist, distance, device/contact identifiers,
sequence tokens, phase, flags and native predictions delivered to the engine.
These are platform API values converted to shared units, not USB/HID packets.
Unavailable axes remain the adapter's zero values. Input already filtered by
the OS/browser cannot be recovered. Browser hover uses the cursor event stream;
stroke history uses the coalesced/raw pointer stream.

The companion predictor log captures the exact processed real/native/stationary
samples, corrections, pressure observations, resets, policy and physical-surface
transform at each query, and actual frame/requested timestamps. It makes replay
independent of brush settings, input batching and display cadence on the replay
machine. Raw records retain the input view transform and pressure curve. Contact
end markers distinguish cancellation from a recording interrupted mid-stroke.
If capture starts during an existing contact, raw input is retained immediately;
predictor replay begins with the next full contact. Empty recordings and contacts
without prediction queries are valid.

No screenshot or rendered prediction is stored. Positions and pen input can
still reveal what was drawn. There is no upload or automatic on-disk retention.

## Binary format v2

`.capystrokes` starts with the eight ASCII bytes `CAPYPEN2`, followed by one gzip
member. The decompressed stream contains repeated `[u32 little-endian length,
payload]` frames. Payloads use the pinned bincode 2.0.1 serde codec with its
standard configuration: little endian, variable-length integers, exact IEEE
floating-point values. Enum and field ordering in
`crates/layer-engine/src/recording.rs` and `recording/schema.rs` is the versioned
wire contract. Changing that order or schema requires a new format version.

Records are metadata, raw input, contact begin, predictor event, contact end,
and footer. Metadata is a JSON string. The footer contains the preceding record
count, elapsed duration in nanoseconds and stop reason. Gzip's CRC, framing,
counts and bounded decoding reject damaged/truncated recordings. The 32 MiB
limit applies before compression. All clocks are monotonic, never wall time.
Raw sample timestamps retain the host's nanosecond clock; arrival timestamps
are relative to recording start. Predictor times are contact-relative. Preserve
delivery order, repeated timestamps, backward timestamps and prediction flags.
Never sort inputs before replay or use native predictions as ground truth.

Capture allocates its bounded byte buffer when started, appends binary records
without per-sample allocation, and performs no compression in input callbacks.
GTK and Android compress on a worker when saving. Web uses asynchronous browser
gzip compression, with a Rust fallback for browsers without CompressionStream.
Once a successful save is acknowledged, the buffer is released.

## Replay, training export and conversion

From the repository root:

```sh
cargo run -p layer-engine --release --features prediction-bench \
  --example prediction-replay -- capture.capystrokes > prediction.csv
cargo run -p layer-engine --release --features prediction-bench \
  --example stroke-recording -- dump capture.capystrokes > records.jsonl
cargo run -p layer-engine --release --features prediction-bench \
  --example stroke-recording -- convert old.jsonl.gz converted.capystrokes
cargo test -p layer-engine --features prediction-bench --lib
```

`dump` emits every record as JSON Lines, including raw input, for Python or
other training pipelines. Split training and evaluation by recording, not by
neighboring samples from the same stroke. Keep hover, predicted input and
interrupted contacts distinguishable when preparing labels.

Replay runs the same Smooth Motion state machine as the application, including
its corrections and native-prediction precedence. It writes per-query CSV to
stdout and summary diagnostics to stderr; `--frames frames.jsonl` additionally
exports the entire preview and causal actual-sample replacements. Both binary
v2 and legacy v1 JSONL (optionally gzip compressed) are readable. Conversion
preserves legacy samples and queries without inventing raw inputs or missing
clocks; metadata identifies these limitations. The v2 policy wire layout keeps
one reserved integer in the retired algorithm slot so existing captures remain
readable. It never selects a runtime algorithm.

## Evaluating visible stability and tracking

```sh
cargo build -p layer-engine --release --features prediction-bench --examples
python3 tools/prediction/replay-bank.py --output artifacts/strokes/bank --analyze --check
python3 tools/prediction/preview-player.py artifacts/strokes/bank/wacom-pro-27
# Optional comparison against a saved export from another revision:
python3 tools/prediction/analysis.py records.jsonl frames.jsonl \
  --before previous-frames.jsonl --output artifacts/strokes/comparison
```

Full analysis needs NumPy. The interactive player is a standalone HTML file;
`--before previous-frames.jsonl` adds a synchronized comparison pane. Comparisons
validate contact/query IDs, timing, delivered inputs and native precedence.
Generated reports, frame exports and players belong in ignored `artifacts/`.

Endpoint RMS and endpoint-error changes are useful diagnostics, **not flicker
scores**. A frame-to-frame change can be correct convergence or normal forward
travel. The evaluation therefore measures these distinct behaviors:

| Behavior | Measurement |
| --- | --- |
| Oscillating preview | Opposing residual revisions over three frames at identical future times, integrated over the body and separately at the sensitive tip |
| Abrupt correction during ordinary motion | Whole-preview correction acceleration at fixed future times, excluding forward pen travel; sudden turns/braking are classified separately |
| Unproductive correction or one-frame flash | Displacement that does not reduce geometric error, and incorrect ink withdrawn on the next frame; report body and tip |
| Sudden loss of preview length | Retreat beyond actual pen travel on straightish motion, plus disappearance/length lost |
| Ghosts at curves, stops or reversals | Distance from all predicted points to the eventual local path, including braking exposure |
| Stable but lagging preview | Uncovered pen distance and signed endpoint lag at common query+0/8/16/24 ms clocks, independent of a model's chosen horizon |

Classification uses seven ground-truth positions at 8 ms intervals around each
query. Overlapping 16 ms chords reduce quantization noise. Turn magnitude,
changes/reversals in curvature and path efficiency distinguish steady lines,
smooth curves and changing-direction motion. Speed is a separate axis: below
600 px/s is slow, below 1,400 px/s medium, otherwise fast; below 60 px/s is micro
motion. Thus a slow straight line is not penalized as an erratic detail stroke.
Ground-truth classification is retrospective and is never available to the
predictor. All distances use the recorded physical-surface transform.

Reports include severity counts, duration, episodes and eligible time. Query
counts are windows, not independent mistakes. Gaps and missing truth remain
unscored rather than being filled with invented future input. Version 2 of the
stability snapshot scores each model's entire available common future interval;
optional paired comparisons restrict both models to identical future support.
The per-tablet regression gate guards stability, tracking, ghosts, braking and
scoring eligibility together. It must not be passed by shortening the preview.
These captures do not contain GPU presentation timestamps, brush raster, opacity
or texture; centerline metrics and display-delay sweeps do not establish actual
pen-to-photon latency or replace physical pen assessment.

Smooth Motion preserves the user-approved stable model exactly. Its 100 ms
history was selected after testing 100 and 200 ms: it retains ordinary motion
continuity while adapting sooner to changes. Smoothing prioritizes correction
continuity and may slightly increase instantaneous geometric error. The reviewed
baseline records that tradeoff, rather than claiming every metric improves.

See the [tablet bank guide](../../crates/layer-engine/tests/data/README.md) for
capture naming, hash-bound baselines and adding recordings from other tablets.

## Host regression checks

- GTK: `native_stroke_recording` and `native_prediction_settings`, run using
  `tools/performance/gtk-raster.sh` with isolated settings and Wayland display.
- Android: `AndroidHostTest#strokeRecordingSavesRawStylusInput` exercises stylus
  input, the real system save dialog, cancellation and retry.
- Web: desktop and device harnesses accept `--stroke-recording`; this covers
  browser pen delivery, cancellation, provider failure, retry and binary export.
- Shared: `cargo test -p layer-engine --features prediction-bench --lib`,
  `cargo test -p layer-ui --lib`, and
  `python3 -m unittest discover -s tools/prediction -p 'test_*.py'`.

Device harnesses must target an explicit device serial and isolated application
state. Synthetic stylus delivery verifies the input path and save controls, not
physical pen feel or sensor accuracy.
