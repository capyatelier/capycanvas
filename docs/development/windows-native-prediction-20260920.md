# Windows native stroke prediction — 2026-09-20

Windows already creates Microsoft.UI.Input.PointerPredictor on the independent
canvas input thread. The shared UiSession capability switch omitted Windows,
so it discarded the successful host capability report. Preferences said
unavailable and live preview used the engine fallback instead of OS predictions.

Windows now honors that report, like Android and Web. Native prediction remains
on by default, can be disabled in Preferences > Pen & Input, and disables the
manual prediction-time slider while selected. Hosts which cannot create the
predictor continue to report unavailable. The real input path, pressure samples,
presentation mode, and saved strokes are unchanged. Prediction affects only the
replaceable preview and adds no wait for future input.

Microsoft documents PointerPredictor for independent SwapChainPanel input and
requires at least ten input points before predictions are returned:
[PointerPredictor](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.input.pointerpredictor?view=windows-app-sdk-1.8),
[GetPredictedPoints](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.input.pointerpredictor.getpredictedpoints?view=windows-app-sdk-1.8).
The existing Windows host requests 16 ms of prediction; the shared preview
interpolates those samples for presentation. This does not smooth or delay the
real stroke samples.

## Verification

The Windows expectation in the existing cross-platform capability test failed
before the one-line capability fix and passed afterward. The project-adoption
matrix now includes Windows: opening and recovery preserve the receiving
window's capability, native-prediction preference, and fallback policy.

* `cargo test --locked -p layer-ui prediction`: 3 tests passed.
* `cargo test --locked -p layer-engine prediction`: 9 tests passed, including
  native preview geometry, presentation interpolation, fallback selection, lift
  handling, and committed strokes excluding prediction.
* Release Rust DLL and WinUI build passed.
* `apps/layer-windows/scripts/exercise-prediction.ps1` launches an isolated app,
  verifies the native toggle and dependent slider, preserves the choice across
  reopening Preferences, and checks that mouse, touch, and pen input do not
  disable the predictor. OS-injected pen exercises actual native prediction.

CAPY_LATENCY_TRACE now records native/fallback preview-frame totals once at
shutdown. They accumulate around rendering so closing or switching a document
cannot erase them. No counter file is written in normal app runs, and there is
no per-frame disk I/O.

## 61 MP / 18 px qualification

The deterministic 9504 x 6336 image, 18 px G-Pen, 240 Hz synthetic pen, ten-second
stroke, and timing definitions are the same as the earlier
[Windows latency qualification](windows-pen-latency-20260920.md).
Both builds use Immediate presentation with maximum frame latency one.
The connected test output was the Surface Panel, 2256 x 1504 at 60 Hz, on Intel
Iris Xe. These are OS-injected-input software measurements, excluding physical
digitizer latency, pixel scanout position, and panel response. They are not an
input-to-photon measurement or physical Wacom acceptance.

| Run | Frame p50 / p95 (ms) | Queue p50 (ms) | Observation bound p50 (ms) | Matched observations |
| --- | ---: | ---: | ---: | ---: |
| Previous build, first | 2.23 / 4.17 | 1.21 | 5.57 | 2182/2401 |
| Corrected capability, first | 2.42 / 4.57 | 1.26 | 5.45 | 2165/2401 |
| Final build, first | 2.82 / 8.04 | 2.15 | 35.90 | 741/2401 |
| Previous build, repeat | 2.24 / 4.42 | 1.20 | 5.53 | 2191/2401 |
| Final build, repeat | 1.98 / 4.36 | 1.51 | 29.85 | 369/2401 |

All valid runs delivered and consumed 2,401/2,401 real samples, without input
validation errors or trace overflow. The final build recorded 2,504 and 3,493
native prediction frames on the imported document. The separate native UI
fixture recorded 428 native frames and passed the toggle and device-transition
checks. Exact counters, binary/fixture hashes, and CSV hashes are in the
[measurement record](native-prediction-windows-20260920.json).

These runs establish functional native prediction and complete real-input
delivery. They do not establish faster frame generation or a physical latency
improvement. The final-build presentation observation bounds are higher, with
much lower matched-present coverage; the software observations alone cannot
establish whether physical latency changed. The table retains those results
rather than treating unmatched presents as successfully displayed. Native
prediction changes where the preview reaches; measuring its visible benefit
requires a physical pen and high-speed camera.

The 2000 px brush rendering optimization measurements remain in the earlier
latency report; this follow-up measures the requested 18 px input workload.
The discarded control-repeat capture injected no pen data after a harness
continuation error. Its clean replacement and the final repeat allowed more
shader-startup time; all timed strokes and analysis settings were identical.
