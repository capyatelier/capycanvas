# Web/Android GTK parity audit — 2026-09-19

This follow-up starts at `caf46ebf` and incorporates current main's `cb577569`
portable-color fixes. It checks the rendered Color and Proof panels and their
editing, history, persistence and delivery behavior against GTK. The earlier
[phase-4 host report](color-management-web-android-m4.md) remains the record for
large-workload qualification and unsupported capabilities.

## Findings and changes

| Surface or workflow | Finding | Result |
| --- | --- | --- |
| Starting layout | The regular tablet app was an older build; only the isolated HDR app had been updated. | Deliver both packages. Real Window → Workspaces → Restore Starting Layout tests verify adjacent Color/Proof tabs in Paint and Photo on both hosts. Sketch retains GTK's different starting arrangement. |
| Browser menus | Refresh replaced an open submenu; popover overflow could enlarge its title-bar item and hide the menu. | Retain the menu path and measure the label allocation independently of its popup. |
| Color panel | Reviewed placement and shared geometry were already present in main. | Verify upper-right Edit Color, no palette footer, HDR intensity arc, touch/pen and tagged color entry. |
| Edit Color | Host forms differed from GTK's grouped entries and Base/Adjusted layout; property colors did not derive GTK's initial HDR EV. | Grouped Model/EV/component rows, contiguous preview patches, partial-entry and out-of-range EV rejection, and shared definition/gamut/above-white feedback. Preserve unchanged HDR samples and explicit EV. |
| Proof contents | Extra header/Close, accent-colored modes, Material pills, loose rows and inconsistent Print defaults. | GTK's Off/SDR/Print strip, neutral selection, compact Profile/Simulate/Intent rows, BPC and Gamut warning, and no implicit first-use profile. Android Properties choices also use the GTK inline label/value arrangement. |
| SDR controls | Host keyboard increments/reset, key-repeat history, focus cancellation and readout reconciliation differed. | Shared Rust numeric policy, arc quantization, per-control focus, one undo per held key/contact, Escape/focus/disposal cancellation, and accurate rapid undo readouts. |
| Dial illustration | Synchronous host generation and browser canvas loss could leave a blank illustration. | Cache GTK's shared 512² illustration off the UI thread; retain CPU-backed browser pixels across panel moves. |
| Print draft | Independent Android panel instances could race. Hidden Web panels could miss state changes. Saved profile lists became stale. | One native controller per editor; reconcile pending retained forms even when hidden; refresh libraries on selection. Off cancels work and rejects late publication. |
| Export during Print change | A file request marked the document busy before a pending profile edit committed. | Retire the unstarted options request, finish the edit while idle, and request export options at the resulting document revision. No file capture/write has begun at this point. |
| Browser GPU recovery | A retired compiler could retain the scheduling lane or publish a late failure into its replacement. | Generation-bound compiler scheduling; a deterministic real-browser test holds completion across replacement and releases a late failure. |

Hosts retain native input capture, accessibility, file transport and worker
scheduling. Shared Rust owns color parsing, preview mapping, recipe validation,
proof LUT construction, dial geometry/value policy and document/workspace history.
The drag convention is unchanged: panel tabs move after slop, with no hold;
ordinary reorderable tiles still require a hold.

## Evidence and reproduction

The local review bundle is `artifacts/color-gtk-parity/review/`. Its README,
source/build manifest and checksums identify the runnable Web package, regular
and isolated Android APKs, test APK, screenshots and numerical/measurement
records. `visual-review.html` places GTK and host captures together; these are
visual references, not an assertion of identical font rasterization or values
in independently edited documents.

Real-browser workflows use hardware WebGPU, actual Wasm workers/codecs/storage,
and browser mouse/touch/pen dispatch. File-picker handles are supplied by the
harness. Tablet instrumentation uses Compose, JNI, Vulkan and Android input.
No injected-input timing is represented as physical pen-to-photon latency.

```sh
cargo test --locked -p layer-ui --lib
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen LAYER_PROOF_WORKSPACE=1 tools/performance/workspace-motion.sh web --hdr
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --proof
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --shared-workflows
LAYER_WASM_BINDGEN=/path/to/wasm-bindgen tools/performance/workspace-motion.sh web --package --package-offline
cargo run --release -p layer-color --example validate_sdr_delivery -- MASTER.exr SDR.png RENDITION.json
python3 tools/validation/hdr_reference.py verify-delivery DELIVERY_DIRECTORY
```

`validate_sdr_delivery` independently reconstructs the host's complete image
mapping using the same Rust mapper/local-tone guide that GTK uses and compares
all sRGB8 output channels. It is a host integration parity check, not an
independent oracle for that shared algorithm. The FFmpeg/PQ check separately
validates transfer/primaries metadata and encoded PQ values against decoded EXR.

Physical HDR surfaces and gain-map output remain unavailable on Web/Android.
The browser retains the explicit 12,000,000 HDR-pixel admission limit without
reducing source precision. Existing cold Float32 heartbeat misses, sustained
thermal coverage and physical pen/display qualification remain open. These
panel and workflow improvements do not claim whole-product GTK parity or close
the global phase-4 acceptance checklist.

## Validated milestone

- 486 shared UI tests pass. GTK's actual compact/floating Proof controls and
  immediate tab drag pass; captured GTK controls are the visual reference.
- Desktop and tablet Chrome cover restored layouts, HDR numeric entry, pen/touch
  editing, one-step undo, key repeat, Escape/focus cancellation, panel/drawer
  movement, exact native save/reopen, recovery, EXR/PQ/SDR output and Print.
  Print includes first-use defaults, ICC preservation failure/retry, pending
  Export, portable profiles, canvas/Navigator viewing and device replacement.
  Recovery images use the same camera before/after GPU replacement; reopening
  legitimately refits the camera and has separate exact data/export checks.
- Native tablet: all four HDR/Print/workspace/restore workflows pass, followed
  by the EV range regression and dense 24 MP measurement. The regular
  `art.capycanvas` package is installed with `adb install -r`; its actual Window
  menu restores adjacent Color/Proof tabs. Existing app data and brush settings
  remain in place. `regular-android-restored-proof.png` records this result.
- Desktop SDR regressions pass: tagged sources, assignment/conversion, depth,
  exact history/reopen, import/paste, all six photo corrections, saved ICC and
  export presets, flattened copy and output previews. The packaged Web build
  passes root/nested offline use, asset upgrades and failed-update recovery.
- All three hosts' 196,608-pixel SDR deliveries differ from GTK's shared mapping
  by at most 0.501 sRGB8 code. Independently decoded PQ metadata matches BT.2020
  / ST2084, with zero PQ16 code error against the reference calculation.

Final desktop 12 MP measurements: Float16 ready in 3.00 s at 1.50 GiB peak PSS;
Float32 ready in 4.52 s at 1.86 GiB. Warm submission p95 is 0.1 ms; observed
histogram/Open/export cancellation is at most 46 ms. The Float16 cold heartbeat
reaches **104.3 ms**, narrowly missing the declared 100 ms desktop limit;
Float32 reaches 85 ms in this run. No tolerance or budget was relaxed.

Native 24 MP Float16 is ready in 7.94 s at 0.65 GiB peak process PSS; maximum cold
heartbeat is 112 ms and cancellation at most 342 ms. Warm input CPU p95 is
0.030–0.040 ms; pen queue p95 is 3.52–3.69 ms, touch queue p95 0.217 ms.
These are input processing/queue measurements, not physical display latency.
Raw timelines and the final tablet-browser measurements accompany the bundle.
PSS sampling can miss short peaks and does not account for every driver allocation.

Tablet Chrome 12 MP is ready in 5.96 s (Float16) and 7.78 s (Float32), with warm
input submission p95 0.2 ms and observed cancellation at most 86 ms. Cold
heartbeat maxima **352.5 / 275.6 ms** exceed the 200 ms tablet budget. Sampled
all-Chrome-process peaks **2.35 / 2.60 GiB** exceed the 2 GiB budget. These memory
figures include existing tabs, isolated renderers and the GPU process; they are
not a measurement of this document alone. Existing user tabs were not closed.
The functional workflow passes do not turn these qualification misses into passes.
