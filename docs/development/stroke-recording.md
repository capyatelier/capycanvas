# Stroke recording and prediction datasets

Open **Diagnostics → Start stroke recording**, draw normally, then choose
**Stop stroke recording**. GTK, Android, Windows, macOS and iPadOS open the system save
dialog. Web uses
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
GTK, Android and Windows compress on a worker when saving. Web uses asynchronous browser
gzip compression.
Once a successful save is acknowledged, the buffer is released.

## Replay and training export

From the repository root:

```sh
cargo run -p layer-engine --release --features prediction-bench \
  --example prediction-replay -- capture.capystrokes > prediction.csv
cargo run -p layer-engine --release --features prediction-bench \
  --example stroke-recording -- dump capture.capystrokes > records.jsonl
cargo test -p layer-engine --features prediction-bench --lib
```

`dump` emits every record as JSON Lines, including raw input, for Python or
other training pipelines. Split training and evaluation by recording, not by
neighboring samples from the same stroke. Keep hover, predicted input and
interrupted contacts distinguishable when preparing labels.

Replay runs the current Smooth Motion predictor, including its corrections and
native-prediction precedence, on the recorded inputs, policies and query timing.
It writes per-query CSV to stdout and summary diagnostics to stderr;
`--frames frames.jsonl` additionally exports the entire preview and causal
actual-sample replacements. The v2 policy wire layout keeps
the original integer slot: historical values 0–4 read as Optimized, and new
captures write 3. This includes captures made with the retired Previous choice.
Unknown values are rejected; the rest of each recorded policy is preserved.

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
| Sudden loss of preview length | Missing useful coverage after transporting the old preview by actual pen travel and crediting removal of old error; score body and tip separately on steady lines and curves |
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

The steady-continuation classes additionally require forward 16 ms chords to
retain at least 75% of recent speed. This excludes braking from the cutback
objective without excluding ordinary curvature. Labels depend only on recorded
truth, never pressure or a candidate's output. Severity CSV rows include both
tracking categories and the separate retreat categories.

Reports include severity counts, duration, episodes and eligible time. Query
counts are windows, not independent mistakes. Gaps and missing truth remain
unscored rather than being filled with invented future input. Version 5 of the
stability snapshot follows each initial preview across later frames, including
its settlement into measured ink. A withdrawn tail moves to the remaining
endpoint; it is not discarded from scoring. This catches long → absent → long
flashes and makes short-lookahead revisions visible. Optional paired comparisons
use the common initial preview support. The version migration remeasures the
frozen pre-change outputs, without changing the other reference values or budgets.
The per-tablet regression gate guards stability, tracking, ghosts, braking and
scoring eligibility together. It must not be passed by shortening the preview.
The Rust summary preserves endpoint RMS and prediction coverage guards.
Chosen-horizon means remain diagnostics: their targets change with the
predictor, and shorter horizons can improve them while making tracking worse.
The Python bank gates full-preview severity and common-clock tracking instead. More gradable boundary queries are allowed; lost scoring coverage is not.

These captures do not contain GPU presentation timestamps, brush raster, opacity
or texture; centerline metrics and display-delay sweeps do not establish actual
pen-to-photon latency or replace physical pen assessment.

### Speed-sensitive cost

`speed_weighted` reports physical threshold rates alongside penalties weighted
by `600 / max(speed_px_per_second, 60)`, using the same retrospective truth speed
for both algorithms. An equal displacement costs twice as much at 300 as at
600 px/s, and ten times as much at rest. The floor prevents division by zero.
Cost is the time integral of weighted **squared** displacement divided by actual
eligible seconds, so slowing a trace raises its cost even if every error is
unchanged. This is an explicit optimization preference, not a calibrated model
of perception. Report raw exposure too; the weighted percentage has a different
denominator and must not be described as physical seconds of flashing.

`transient_peak` counts either incorrect ink disappearing next frame or useful
preview reach being withdrawn, anywhere in the preview (tip or body RMS).
`transient_cost` uses the larger of those two displacements per region, with
two-thirds tip and one-third body squared-error cost. It does not add overlapping
failures twice. Both measurements and speed must be available; eligibility is
reported and guarded. Ghost geometry, correction oscillation/shock, and fixed-clock
tracking remain separate to prevent a stable but inaccurate/lagging model from
winning through this score alone.

Smooth Motion uses 100 ms of drawing history. Its continuity controller separates
curve geometry from visible reach: a changing confidence window need not abruptly
withdraw otherwise useful ink. Stop/turn evidence releases that memory promptly.
Falling pressure qualifies a motion alarm; varying pressure alone does not shorten
a steady stroke. Fit-window confidence and physical stopping evidence serve
separate purposes. Native predictions keep their platform precedence.

See the [tablet bank guide](../../crates/layer-engine/tests/data/README.md) for
capture naming, hash-bound baselines and adding recordings from other tablets.

## Host regression checks

- GTK: `native_stroke_recording` and `native_prediction_settings`, run using
  `tools/performance/gtk-raster.sh` with isolated settings and Wayland display.
- Android: `AndroidHostTest#strokeRecordingSavesRawStylusInput` exercises stylus
  input, the real system save dialog, cancellation and retry.
- Windows: `stroke_recordings_save_off_thread_and_release_only_after_delivery`
  and `apps/layer-windows/scripts/exercise-stroke-recording.ps1`, which records a
  controlled pen stroke, saves through the owned picker, checks the CAPYPEN2 gzip
  file, and confirms that a cancelled save keeps the recording for a retry.
- Web: desktop and device harnesses accept `--stroke-recording`; this covers
  browser pen delivery, cancellation, provider failure, retry and binary export.
- Shared: `cargo test -p layer-engine --features prediction-bench --lib`,
  `cargo test -p layer-ui --lib`, and
  `python3 -m unittest discover -s tools/prediction -p 'test_*.py'`.

Device harnesses must target an explicit device serial and isolated application
state. Synthetic stylus delivery verifies the input path and save controls, not
physical pen feel or sensor accuracy.
