# Web and Android print proofing validation

2026-09-17. Branch `color/proof-web-android` starts at fetched `origin/main`
`173f760f`. The accepted GTK reference is merge `1cb1ebb8`, with its
[review workflow](../development/color-management-m3-gtk-review.md) and
[qualification](color-management-gtk-m3-validation.md). This port follows that
workflow. It enables Web and Android only; HDR and other hosts are outside scope.
User review is pending; these are implementation and automated-test results.

The later [navigation investigation](color-management-proof-navigation-investigation.md)
isolates an avoidable GPU shader cost and records the optimized builds: Web proof
navigation improves from about 78 to 100 updates/s on the same tablet. The original
measurements below remain the baseline; they do not describe the updated shader.

## Implementation

- `1c89f091` shares proof preparation, adoption/preservation requirements, stale
  identity checks, defaults and viewing status in `layer-ui::proof_workflow`.
- `494226d5` adds Web/Android setup, profile-library integration, worker ownership,
  viewing presentation and journey tests. Rust retains ICC validation, recipes,
  transforms, LUT generation, rendering, actions, history and native serialization.
- `c381c4c1` fixes display-transform recovery and Web dialog worker ownership.
  Web canvas presenters now use the document's working space. Retained Navigator
  surfaces follow document/color adoption and are reattached after device loss.

The three View commands remain together, using the accepted shared shortcuts
(`Ctrl+Alt+P`, no default setup shortcut, `Ctrl+Shift+Y`). First use opens setup;
viewing toggles are temporary. A replacement is prepared before preservation and
adoption. The old embedded ICC stays selectable as Document Profile; successful
Apply first saves its exact bytes locally. Cancel does not copy it. A storage
failure leaves the old recipe and history intact. Only the active target travels
in `.capy`; proof selection never selects an export profile.

Web's existing raster worker transport serialized `Document` separately from
its native archive index, losing the serde-skipped proof recipe. The transport
now carries that recipe explicitly, restoring it before native archive writing.
No new file format or alternate ICC archive path was introduced.

Each host owns one preparation lane and one retained CPU LUT. Web transfers a
bounded typed sample buffer from a disposable Wasm worker, terminating that
worker on completion, cancellation or timeout. Android uses a coroutine lane
and native atomic cancellation; native handles outlive their worker. Both reject
results after document/working-space/recipe/request changes. Device replacement
recreates GPU resources from the retained CPU result; lifecycle suspension
cancels pending preparation. Setup commits lock dismissal during durable profile
preservation. A late Web dialog-close callback can only clean up its own work.

The shared grids are 65³ or 129³, five Float32 values per sample: 5,492,500 or
42,933,780 bytes per CPU/GPU table. Canvas and Navigator share the GPU table.
Worker transport verifies dimensions, finite samples and dark-grid compatibility.
The bound does not describe total app memory or ICC-library scratch allocation.

## Automated evidence

Raw logs and generated fixtures are local under `artifacts/color-m3-web-android/`;
ICC/photo fixtures and binaries are excluded from source control.

| Check | Result |
| --- | --- |
| Shared UI library | 454 tests passed (`shared-ui.log`) |
| Bounded LUT transport, invalid dimensions/dark grid/NaN and cancellation | Passed (`worker-transport.log`) |
| Shared hardware GPU/CPU proof parity, artwork and export invariance | Passed (`shared-gpu-proof-hardware.log`) |
| Web package graph / asset fingerprint tests | 14 passed (`web-package-tests.log`) |
| Desktop hardware Chrome, final static package | Passed (`web-package-final3-proof.log`) |
| Tablet Chrome, final source build | Passed, no browser errors (`tablet-web-final4-journey.log`) |
| Native Android UI/JNI journey, actual worker cancellation and stale result | Passed (`android-final-journey.log`, 29.79 s) |
| Web-created P3/U16 `.capy` opened/proofed/resaved/exported on Android, empty library | Passed (`android-cross-port-journey.log`, 33.867 s including full native journey) |
| 61 MP normal/proof navigation and separate memory runs | Completed on connected tablet |

Both host journeys exercise first-use cancellation/defaults, retained Document
Profile selection, cancellation without preservation, failed preservation and
retry, exact original ICC preservation, painting and edit undo/redo, clean view
toggles, exact histograms and PNG bytes with proof/warnings on/off, save/reopen
with the entire local library removed, and device recovery. Android also recreates
the Activity. Shared tests exercise recipe undo/redo and stale adoption. Web
captures canvas and Navigator, verifies both change on comparison and both remain
live after GPU replacement. The synthetic portable fixture is 513×257 P3/U16.

Export/backing/histogram checks remain exact. Screenshot comparison after device
recreation allows max 8 code values and mean 0.5 across RGBA to accommodate
fractional-DPR antialiased edge rounding in Android Chrome. The observed initial
failure had identical solid colors with a few edge differences up to 6. This
presentation tolerance does not relax the shared GPU/CPU numerical contract.

Earlier logs retain test-harness failures: same-named builtin/ICC selection,
concurrent clipped screenshots, native reload confirmation, and starting a file
command before workspace readiness. They also retain the real serialization,
retained-transform and dialog-cleanup issues fixed above. The general legacy
`test.mjs` smoke path stops at its absent `[data-brush="4"]` selector; the focused
proof journey and package graph pass. Two packaged-test attempts reached an
exited localhost server, then passed after restarting it; those blank-page runs
are not evidence of GPU failure.

## Connected-tablet performance

Physical Wacom MovinkPad 14 / DTHA140, Android 15, serial `5ll21u1002931`.
Android uses Vulkan; Chrome 152 uses WebGPU and its existing desktop-site mode.
No browser flags were changed. The existing documented photo fixture is
9504×6336 (61 MP), SHA-256
`3aac9c9b8b34c38a5e0121f16ad1ec806e92a19e15ee5e1128f36d987e888054`.
The host document was sRGB/U8. Profile: local Krita `cmyk.icm`, Chemical proof,
relative colorimetric + BPC + black ink. Native viewport 2880×1800; Web fullscreen
uses the same device with DPR 1.75. Gestures are synthetic pan/zoom/rotation through
normal host scheduling. These are CPU/callback measurements, not input-to-photon
latency or colorimetric display measurements. Raw first and warm runs are retained.

| Path, three 361-input runs | CPU frame p95 | Input callback p95 |
| --- | --- | --- |
| Android normal | 8.14 / 6.37 / 6.00 ms | 8.33 / 8.33 / 8.33 ms |
| Android proof | 7.48 / 7.65 / 7.29 ms | 16.67 / 8.33 / 16.67 ms |
| Web normal | 2.8 / 1.8 / 1.7 ms | 275 / 8.4 / 8.4 ms |
| Web proof | 5.6 / 4.8 / 5.5 ms | 33.3 / 33.2 / 33.3 ms |

Android submitted 194/342/345 normal frames and 345/332/341 proof frames. Web
submitted 362/361/361 normal and 361/361/361 proof frames. Android submission p95
was 16.67 ms in all six runs. Web normal cold navigation had substantial stalls
(p99 callback 575 ms), while warm normal runs each had two intervals over 12 ms.
Proof runs had 96–100 such intervals; sustained 120 Hz proof navigation is **not
qualified**. Do not average the cold stalls away or infer zero dropped frames.

Native preparation took 308 ms, with 38 UI callbacks at approximately 8.33 ms.
Web Apply-to-ready took 916 ms, with 100 RAF callbacks, one 80 ms main-thread
long task and a largest recorded RAF gap of 75 ms. Preparation is cancellable and
bounded, but this is not a zero-jank claim. Photo import took 3.21 s native and
5.71 s Web in these runs. The separate sampled-memory run has its own timings.

Native sampled process peak: 764.9 MiB PSS / 912.8 MiB RSS; final samples about
668–670 MiB PSS. System available memory fell to 851.7 MiB and ended near 1240 MiB.
These are approximately one-second samples, not guaranteed peaks. Native tracked
canvas storage was 1357.2 MiB; Web 1313.0 MiB, plus the separate 5.24 MiB proof
presentation buffer and 5.24 MiB CPU LUT. Host renderer telemetry currently omits
the presenter-owned proof allocation, so add that buffer when interpreting those
rows. Process PSS also omits some driver/GPU allocations; neither number is total
GPU residency. The memory sampler runs separately from navigation timings.

The separate Web memory run completed with no runtime exceptions. Summing all
five Chrome processes reported by `dumpsys meminfo --package com.android.chrome`,
PSS rose from 900.5 MiB to a sampled peak of 2231.1 MiB and ended at 2058.1 MiB;
RSS peaked at 2815.2 MiB. System available memory started at 4520.8 MiB, reached
1684.8 MiB and ended at 1857.3 MiB. Other user tabs stayed open and contribute to
these whole-browser totals. This short run includes normal recovery/autosave and
a screenshot; the final samples are not a demonstrated long-term steady state.
The roughly 26 MB JavaScript heap estimate excludes Wasm/GPU allocations and is
not used as total browser memory. Raw evidence: `web-memory-samples.json` and
`web-memory-run.log`. An initial memory attempt issued Open before full workspace
readiness; the CLI now waits for the workspace and command before starting.

## Limits and reproduction

Not qualified: calibrated displays or physical prints; Firefox/Edge/Safari;
other Android devices or low-RAM Android; worst-case 61 MP P3/ProPhoto U16 with the
129³ grid; OS process death during the narrow preservation commit; every OEM
background policy; prolonged edit/export contention while preparing; hardware
input-to-photon latency. The full shared numerical/profile corpus is inherited
from GTK; the host journeys use one CMYK target and a portable RGB ICC. Desktop
large-document memory was not measured. HDR and other-platform work are excluded.

Build commands, fixture setup, device commands and user review steps are in the
[Web/Android review guide](../development/color-management-m3-web-android-review.md).
