# Web and Android phase 4 qualification

This branch starts at `59aff3df` (origin/main, 2026-09-18). The requested review
covers Web and Android's integration of the current shared Rust HDR contracts
and reviewed GTK workflows. It does not qualify physical HDR presentation or
change the global milestone's other-host gates.

## Budgets declared before large-document measurements

Run release Wasm in ordinary hardware WebGPU Chrome, and the release Rust core
inside the isolated Android debug host (`art.capycanvas.hdr`). Use the attached
Wacom tablet on its current power/display settings; capture model, OS, browser,
GPU, memory, battery and thermal state with the results. Do not infer physical
pen latency from injected input or frame cadence.

Workloads: sparse 4K HDR painting; dense 24, 45 and 60 MP HDR content; repeated
navigation/pen contacts; SDR appearance edits; histogram; cancellation; native
save/reopen and export contention. Record cold and warm results separately.
Use three repeated interaction runs where supported. Preserve precision when
rejecting a workload that exceeds the host's admission budget.

| Measurement | Desktop Chrome | Tablet Chrome / native Android |
| --- | --- | --- |
| Warm input submission p95 / p99 | 16.7 / 33.4 ms | 33.4 / 66.7 ms |
| Maximum event-loop heartbeat gap during background work | 100 ms | 200 ms |
| Cancellation to idle | 1 s | 2 s |
| Cold document ready, 24 / 45 / 60 MP | 30 / 45 / 60 s | 45 / 60 / 90 s |
| Peak application process memory, ordinary / concurrent delivery | 3 / 4 GiB | 2 / 3 GiB |
| Accounted renderer allocation, steady | 2 GiB | 1.5 GiB |

Memory figures must distinguish measured process residency, renderer accounting
and unavailable browser/driver allocations. Browser JS heap is not total memory.
For unchanged SDR paths, investigate a reproducible p95/p99 regression above
`max(5%, 0.2 ms)` against the runnable parent and existing fixed baseline. Missing
measurements, missed presentation feedback, untested large combinations and
sustained thermal coverage remain outstanding; they are not implied passes.

## Review record

The final validation report links runnable packages, exact source/build hashes,
commands, screenshots, numerical checks and measured limits. Browser file-picker
handles may be supplied by the harness; codecs, workers, WebGPU, storage and
input dispatch must be real. Native tablet tests must exercise JNI, Vulkan,
Compose and Android touch/stylus dispatch. Physical pen feel and physical HDR
luminance require human/hardware review beyond those automation paths.


## Integration milestone — 2026-09-19

The feature branch `color/phase4-web-android` integrates current main through
`20932ce4`, including the shared Float32/OpenEXR and thumbnail scheduling work.
The implementation uses shared Rust color forms, HDR sample semantics, local
SDR mapping, Proof recipe/history and GTK dial geometry. Host code owns input,
file transport and cancellable background work.

The review bundle is in `artifacts/color-m4-web-android/review/`: runnable static
Web archive, isolated `art.capycanvas.hdr` APK, test APK, launch instructions,
source/build manifest, checksums, screenshots and `measurements.json`. Raw logs,
traces and independently decoded outputs remain beside it. Artifacts are local
build products, excluded from Git. The package has complete dependency notices,
including the original OpenEXR BSD notices and pinned upstream Zlib notice.

### Implemented and verified

- Open independently encoded PQ PNG; edit above-white and negative Float16/Float32
  values; retain exact HDR histogram through pen paint, undo/redo, native
  save/reopen, GPU restart and recovery. The original SDR Open/Print journey
  passed on main and is considered fixed, as requested.
- GTK-style picker: upper-right Edit Color pencil, no palette footer, colored
  HDR intensity arc, editable EV and Base/Adjusted numeric previews. Float32
  documents use their actual depth and range, including histogram stop axes.
- Live Off/SDR/Print, shared circular contrast/balance field, brightness/color
  arcs, reset, keyboard adjustment and cancellation. One gesture is one undo.
  Appearance settings persist without changing HDR raster blobs. Native history
  now refreshes the displayed percentages; browser Proof receives input above
  floating workspace panels.
- Async ICC preparation with cancel, retry and preservation of the previous
  profile. Print simulation and gamut warnings remain view-only. Existing SDR
  source interpretation, profile conversion, PNG/TIFF/JPEG and print workflows
  pass their real-browser regressions.
- Float32 EXR for linear HDR interchange; strict BT.2020 PQ PNG rejects values
  outside its range and blocks Choose in preview. Explicit clipped-PQ delivery
  and authored SDR PNG/TIFF/JPEG use output copies. Gain-map JPEG/AVIF and physical
  HDR surfaces were unavailable in this first integration. Subsequent Android
  PQ and Web extended-canvas milestones below supersede that display limit;
  gain-map delivery remains unavailable.
- Bounded worker capture, generation/stale-result checks, atomic cancellation
  and worker teardown. Web admits at most **12,000,000 HDR pixels**, with an
  actionable rejection preserving the existing master. It never reduces HDR
  precision or image dimensions to admit an oversized file. SDR limits are
  unchanged. Native Android's measured Float16 workload reaches 60 MP.

The first integration was merged to `origin/main` at `32de7e6e`. Its initial
nonmodal Proof surface is superseded by the workspace milestone below.

### Proof workspace milestone — 2026-09-19

Proof is now a regular workspace panel on Web and Android, sharing Color's tab
group in Paint and Photo. The existing host controllers handle tab dragging,
floating, cancellation, layout undo/redo and collapsed-column drawers. The
reviewed GTK reveal action now lives in shared Rust and all three hosts use it:
showing Proof preserves placement, selects its tab and opens its drawer without
closing an already open drawer. This is workspace history, separate from the
document's appearance history.

Web retains one live control view while moving between dock and drawer mounts.
The dial uses shared geometry at the available size, with device-pixel rendering,
so compact tablet panels keep both arcs reachable. Android respects the outer
drawer's scrolling and drains gesture cancellation when a view is removed.
Both hosts refresh print controls after document undo and retain the current
document profile selection. Hidden browser panels do not redraw their dial.

`browser-docking-final-ready.log` and `tablet-browser-docking.log` cover the full
HDR journey with pen tab dragging, workspace undo/redo, touch cancellation and
idempotent drawer reveal. `android-docking-final.log` runs all three native
HDR, print and workspace workflows with real fixture arguments. Earlier compact
dial clipping and Android nested-scroll failures are fixed and superseded by
these runs. Screenshots in the review bundle show docked and drawer controls.
`browser-docking-final-proof.log` checks the SDR/print journey, and
`browser-docking-package-offline.log` checks the rebuilt static package offline.
`shared-proof-reveal-final.log` checks shared reveal behavior on GTK/Web/Android;
`gtk-proof-reveal.log` validates the actual GTK action on hardware Vulkan.

The large-document measurements below remain from the first milestone; they
are not presented as new measurements of the docked controls. Neither this
workspace change nor passing workflow tests remove the failed performance gates
or missing hardware qualification listed below.

### Correctness evidence

`browser-final.log` and `tablet-browser-review.log` exercise actual Chrome
WebGPU, workers, OPFS and browser-dispatched touch/pen. The harness supplies
file-picker handles, not codecs or rendering. Checks include touch cancellation
on the circle and arc, pen gesture undo, rejected oversized Open, export preview,
EXR/PQ/SDR delivery and recovery. Screenshots are `browser-final/proof-sdr.png`,
`tablet-browser-review/proof-sdr.png` and `review/android-proof.png`.

`android-readout-workflows.log` records two passing instrumented tests on the
attached tablet, with explicit `hdrFile` and `proofProfile` arguments. They use
Compose, JNI, Vulkan and Android InputDispatcher stylus contacts. The earlier
invocation without fixture arguments skipped these tests; it is not evidence
of a pass. `browser-final-proof.log` and `browser-final-shared-trace.log` record
SDR/print and full shared file/color workflows. The incomplete earlier shared
run is superseded by the clean traced run.

Shared checks: 482 layer-ui tests plus the new shared reveal regression;
10 core HDR, 8 color HDR and 3 EXR tests;
28 native-host tests pass, one platform-specific test ignored. Package and
pointer unit tests pass. The packaged offline suite is run separately with
`--package --package-offline`: it does not claim the old fullscreen/title-bar
assertions pass. That unrelated full PWA suite still needs updating for current
main's title bar. Its pen-capture assertion was updated to preserve strokes on
capture loss, matching main's reviewed tablet fix.

`tools/validation/hdr_reference.py verify-delivery DIR` independently decodes
EXR and PQ PNG with FFmpeg, then compares all 196,608 pixels using the published
sRGB/BT.2020 matrix and ST 2084 math. Desktop Chrome, tablet Chrome and native
Android outputs have **0 maximum PQ16 code error**, against a declared tolerance
of 1 code. Negative, above-white and explicitly clipped samples are required;
metadata, dimensions and opaque alpha are checked. The current local SDR mapper
has shared reference tests, not a separate external pixel oracle.

### Measured envelope and failed gates

Desktop: Chrome 152, NVIDIA RTX PRO 6000 Blackwell Max-Q, driver 610.57.4,
120 Hz private Wayland compositor. Tablet: Wacom DTHA140/MovinkPad 14, Android 15,
Adreno 7xx, 11 GiB RAM, 120 Hz. Chrome's Desktop Site user agent reports Linux;
its Qualcomm GPU and ADB endpoint identify the actual tablet. Native APK uses
release Rust inside a debug Compose host. Battery was AC-powered, 99%/26.6°C
before and 98%/26.2°C after the matrix.

| Workload | Ready, including SDR guide | Peak sampled process PSS | Largest cold heartbeat gap |
| --- | ---: | ---: | ---: |
| Desktop Web 12 MP Float16, two opens | 2.81 / 2.71 s | 1.43 / 1.54 GiB | 49 / 50 ms |
| Tablet Web 12 MP Float16, two opens | 5.44 / 4.67 s | 1.48 / 1.58 GiB | 199 / 115 ms |
| Desktop Web 12 MP Float32 EXR | 4.71 s | 1.71 GiB | **134 ms** |
| Tablet Web 12 MP Float32 EXR | 7.58 s | 1.60 GiB | **390 ms** |
| Native Android Float16 24 MP | 8.30 s | 0.59 GiB | 89 ms |
| Native Android Float16 45 MP | 17.35 s | 0.83 GiB | 83 ms |
| Native Android Float16 60 MP | 24.58 s | 1.18 GiB | 62 ms |

Float32 cold decode/adoption misses the 100/200 ms heartbeat ceilings despite
meeting memory and total-ready budgets. Warm Float32 runs stay below 41 ms
heartbeat gaps. Native sparse-4K background work has one **202 ms** gap against
200 ms; this is a miss, not rounded into a pass. Native dense 24/45/60 MP warm
and background maxima are 61/64/73 ms. Browser 12 MP Float16 warm maxima are
28 ms desktop and 185 ms tablet. See the JSON for all distributions and samples.

Before the browser guard, 45/60 MP reached 3.68/5.35 GiB sampled process PSS;
a fresh 60 MP run still reached 4.07 GiB. Those workloads failed the declared
memory gates. They are rejected in the delivered Web build. Earlier tablet
`tablet-browser-performance` samples measured only the main Chrome process and
are invalid as total memory; qualified runs sum Chrome and its renderer/GPU
processes, conservatively including other existing tabs.

Across admitted browser workloads, input submission p95/p99 is at most
0.3/0.7 ms. Open cancellation is 6–19 ms and export cancellation 41–68 ms.
Native dense-workload Open cancellation is 1–2 ms, histogram cancellation
288–291 ms and export cancellation 273–277 ms. Native input CPU p99 stays
below 0.09 ms; queue p99 below 6.9 ms. These are application submission/queue
measurements, **not physical pen latency or exact input-to-present latency**.

The unmodified runnable parent `62ae6640` SDR-4K comparison and feature both
have input p95 around 0.1 ms and p99 0.1–0.2 ms; no warm regression exceeds
the declared trigger. This is not a rerun of every original fixed M2 baseline.
PSS sampling can miss short peaks and excludes unreported GPU-driver residency;
renderer counters are separate. SurfaceFlinger traces lack per-input correlation.
Long thermal soak, human pen feel, physical HDR/mixed-monitor behavior, large
Float32 native combinations, long effect chains and all other-host gates remain
unqualified. This report does not close the global phase-4 acceptance checklist.

### Reproduction

```sh
LAYER_PROOF_WORKSPACE=1 LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --hdr
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --proof
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --shared-workflows
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --package --package-offline
LAYER_HDR_WORKLOADS=sparse4k,hdr12.png,hdr12.exr tools/performance/workspace-motion.sh web --hdr-performance
python3 tools/validation/hdr_reference.py verify-delivery artifacts/color-m4-web-android/android-delivery
python3 tools/validation/summarize_hdr_hosts.py artifacts/color-m4-web-android
```

Generate the independent small PQ fixture with `hdr_reference.py generate DIR`,
then copy `ffmpeg-pq.png` to `apps/layer-web/pkg/hdr-pq.png`. Large fixtures are
in `artifacts/color-m4-web-android/fixtures`; performance names resolve under
`apps/layer-web/pkg/`. `LAYER_HDR_REJECTIONS=hdr24.png,hdr45.png,hdr60.png`
extends the real Open workflow with preservation checks. Do not rebuild that
Wasm directory during active browser tests. Build/package and ADB commands,
including the required instrumented-test fixtures, are in the review README.

## Web HDR display and EV drag follow-up — 2026-09-19

Web now qualifies HDR using both `(dynamic-range: high)` and an actual
`rgba16float` WebGPU canvas configuration whose accepted tone-mapping mode is
`extended`. The shared presenter sends signed, extended sRGB to both canvas and
Navigator. The browser owns output tone mapping and brightness; no invented
numeric display headroom or app SDR shoulder compresses the HDR master. Off,
SDR, Print, gamut warnings, document/color adoption and GPU replacement keep
surface configuration and presentation encoding together. SDR analysis remains
available for thumbnails and explicit proofing. Capability changes publish
command availability as well as waking the canvas.

The EV snapback was reproduced with actual Chrome touch input: scrolling
arbitration cancelled the SVG path's pointer and correctly restored the starting
value. `touch-action: none` on the enclosing SVG viewport prevents that unwanted
cancellation. Real pointer cancellation still restores the original EV. Mouse,
touch and pen exercise the arc through browser input dispatch.

The review bundle is `artifacts/web-hdr-ev/review/`. It records source/package
hashes, runnable Web output, desktop and tablet workflows, and independent GPU
pixel-oracle results. Browser checks read one actual submitted swapchain row per
surface with temporary test-only copy usage, verifying above-white output and
standard SDR proofing. Screenshots are SDR captures and do not measure physical
HDR luminance. The picker, layer thumbnails and export comparison canvases remain
SDR previews; the 12 MP browser admission limit and gain-map output limits remain.

The Web encoding follows [WebGPU canvas color management](https://gpuweb.github.io/gpuweb/#canvas-color-management)
and [Chrome's extended canvas configuration](https://developer.chrome.com/blog/new-in-webgpu-129).

The final build incorporates `ebdb33b0` from main: retained GPU tone guides and
Huion Float32 blending qualification. The combined desktop and packaged Wacom
Chrome journeys pass, including pen-contact guide retention and late-result
rejection. A test removes canvas configuration inspection to simulate an older
browser: both views fall back to SDR and recover HDR when the API returns.
Chrome ignores `dynamic-range` emulation, so physical display switching remains
unqualified. Fingerprinted production assets are tested on a clean origin after
stale development worker modules caused a Print startup failure during integration.
