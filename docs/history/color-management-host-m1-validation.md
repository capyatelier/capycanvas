# Raster foundation host validation

This records the Web and Android follow-up to the GTK qualification in
[color-management-gtk-m1-validation.md](color-management-gtk-m1-validation.md).
The GTK and integration checkpoint was pushed to main as `c1ec69e` before these
ports. The old Web comparison is main `1756cfa`, before the raster merge.

## Web

The browser uses the shared encoded sRGB8 renderer, immutable raster revisions,
indexed project format and bounded history. GPU objects remain on their owning
browser event loop. GPU mappings are awaited asynchronously; a dedicated Wasm
worker hashes and compresses bounded tile blocks. A separate worker validates
project input, encodes project/PNG output, and writes recovery transactions.
Ordinary frames do not await those workers. A frame that actually restores
pending tiles is retained and retried without consuming another contact.

Project files are transferred to the validation worker without first copying the
archive into the input owner's Wasm memory. Immutable compressed tiles and source
assets cross independent Wasm memories in blocks, yielding after 4 MiB of copying.
The worker protocol is private: persisted files still use only the shared,
integrity-checked `Project::read`/`Project::write` format. Browser project limits
are 256 MiB source bytes, 512 MiB raw raster samples and 8192 tiles; the device's
texture limit also applies. These limits do not describe total process RSS.

Recovery uses strict IndexedDB transactions on the file worker and a Web Lock
per live tab. A closed tab's checkpoint can be recovered without racing a live
tab. Recovery preserves dirty-state protection and has no user file location.
GPU replacement retains the UiSession and history through `replace_renderer`;
`Suspend` discards unsubmitted contacts, and device loss exposes Restart Canvas.

Validation on Chrome 150 / NVIDIA driver 610.57.04, RTX PRO 6000 Blackwell:

- Release Wasm build; 45 core, 49 engine and 356 UI/session tests passed.
- 12 Web packaging tests passed, including dependency fingerprint rewriting.
  The production PWA build includes all 251 precached files and both raster workers;
  the same offscreen acceptance journey also passes against that packaged build.
- Real hardware WebGPU offscreen acceptance passed: paint and save, exact tile
  hashes after reopen, identical full-canvas PNGs, undo/redo, corrupt archive
  rejection without adoption, renderer replacement, and an abandoned tab's
  IndexedDB recovery after reload. Recovered pixels match and remain unsaved.
- Frame creation sampled 2304 moving frames and 15 contact completions per canvas,
  in three consecutive runs with five contacts each. The first contact warms the
  path and is excluded from moving-frame percentiles. New-backend runs include
  concurrent immutable recovery encoding and IndexedDB publication. Inputs are
  synthetic; timing includes the Wasm host frame/presentation encoding, not GPU
  completion, physical input latency or display presentation.
- On 2048 × 1536, old moving-frame p99 was 0.40/0.30/0.30 ms; new was
  0.50/0.30/0.30 ms. New maximum was 1.20 ms, contact-completion maximum 1.90 ms,
  and concurrent recovery writes took 40.0/12.1/12.2 ms. Steady-state p99 is
  unchanged at the browser clock's 0.1 ms resolution.
- On 6000 × 4000, new moving-frame p99 was 0.40/0.60/0.40 ms; maximum 1.90 ms,
  contact-completion maximum 2.60 ms, and recovery writes took 19.1/29.1/39.9 ms.
  The matching old p99 was 0.40/0.30/0.30 ms; the median increase is 0.10 ms,
  below the predeclared 0.20 ms noise allowance. These are sparse paint workloads; the GTK dense multi-layer qualification
  remains separate and is not a browser memory or mobile performance claim.

**Presentation limitation:** the strict editor screenshot test fails before
painting on both old main and this port in this environment. Chrome reports
`A valid external Instance reference no longer exists` and presents a black
headless canvas while hardware GPU exports remain correct. The explicit
`--offscreen-raster` test mode reports this known failure and tolerates only that
specific diagnostic; every other error fails. It does not qualify display
presentation. The ordinary editor/screenshot suite remains strict. A private
Wayland test also encountered the installed Chrome GTK 4 startup failure; GTK 3
avoids that launch crash but does not establish Vulkan presentation here.

Local evidence is in `artifacts/color-m1/web-raster-recovery.txt`,
`web-before-editor.txt`, `web-before-frame-times.json`, `web-frame-times.json`,
`web-24mp-frame-times.json` and the accompanying benchmark logs. Reproduce with:

```sh
bash apps/layer-web/build.sh
node apps/layer-web/package.test.mjs
LAYER_WEB_URL=http://127.0.0.1:4173 node apps/layer-web/test.mjs --headless --raster --offscreen-raster
LAYER_WEB_URL=http://127.0.0.1:4173 node apps/layer-web/test.mjs --headless --raster-bench --offscreen-raster
# Add --large-raster for a 6000 × 4000 canvas.
```

## Android

Android uses the shared immutable raster/project implementation through JNI.
The render Looper captures a committed snapshot immediately, including during a
live contact; the existing file worker waits for backing and writes the indexed
archive. Storage Access Framework output is encoded completely in private cache
before opening the provider destination. Provider writes still have the
provider's durability/atomicity limits; private recovery uses local atomic rename.

The 15-second/on-stop recovery controller owns one in-flight checkpoint per
window. Private files use the atomic writer extracted from GTK into layer-core:
mode 0600 sibling file, complete encoding, file sync, rename, then directory sync.
Per-window file locks exclude live windows from recovery offers. Recovery adopts
a prepared project as modified with no user file location, writes its new complete
copy, then retires the old copy. Accepted immutable writes can finish after
Activity teardown. Successful manual save/document replacement/explicit close
retires the applicable recovery copy without acknowledging a different edit.

Surface recreation retains the ViewModel/UiSession. Device replacement uses
`replace_renderer`, keeps backed pixels and undo/redo, and retires capture workers
off the render/input Looper. Device errors are recorded by callbacks, observed
before preview/frame GPU use, and suspend the session. Restart Canvas creates a
fresh renderer. File candidates carry a GPU generation and cannot reintroduce a
retired device. A controlled device-destruction test exposed a queued thumbnail
race in the original JNI path; this port closes that path before new GPU use.

Android workspace startup also required an independent prerequisite fix:
Rust's `File::try_lock` returned Unsupported on the tablet. The Android branch
now calls Bionic `flock(LOCK_EX | LOCK_NB)` with the same held-file lifetime;
SQLite ownership/fencing and permanent lock inodes are unchanged. Performance
comparisons use old main `1756cfa` with this same platform lock fix and identical
test harness, in a separate application ID. The user's installed app was retained.

Validation hardware is the connected Wacom MovinkPad 14, Android 15, arm64 Vulkan.
Rust is release-optimized in the debug instrumentation APK; CPU frame times are
native frame creation and host render/present submission, not GPU completion or
physical pen/display latency. The benchmark creates a sparse 6000 × 4000 document,
uses synthetic pen events through the normal render Looper/display callbacks,
warms the first contact, and measures four further 192-move contacts per run.
Three runs include concurrent private recovery writes on the new backend. Tests
wake the tablet before Activity launch and keep the screen on; interrupted/sleeping
runs were rejected. This is not a dense-layer or total mobile memory qualification.

Android's release native/debug APK builds and lint passed with JDK 25. The
private JNI acceptance test passed exact indexed tile hashes and full PNGs after
save/reopen, active-contact save, undo/redo, controlled device destruction and
Restart Canvas, stale-device candidate rejection, corrupt archive rejection,
Activity/surface recreation, and recovery through the production dialog. Native
job tests suppress automatic SAF pickup because the test owns each descriptor;
the system file picker is tested independently.

The first merged tablet batch showed a +0.408 ms median frame-creation p99
increase. Removing per-call failure mutexes (one-time publication instead) and
duplicate device polling reduced that tail. Callbacks are now polled once before
frame resource use; initial surface readiness and file readbacks still drive their
own completion. The optimized build passes the complete raster/device-loss and
SAF file-picker journeys again; the merged kernel-lock exclusivity test also passes.

Final optimized runs measured 2322 display callbacks: frame-creation p95 was
2.330/1.961/1.853 ms and p99 2.719/2.394/2.133 ms; maximum 3.688 ms.
Concurrent recovery publication took 13.45/9.03/9.61 ms on the file worker. Native
host render/present submission p99 was 5.656/4.961/4.624 ms.

The two baseline batches measured 4625 callbacks. Old frame-creation p95 ranged
1.750–2.286 ms and p99 1.931–3.107 ms. Median p95 across the baseline batches was
1.867 ms versus 1.961 ms in the final batch (+0.094 ms); median p99 was 2.242
versus 2.394 ms (+0.152 ms). Both differences are inside the established 0.20 ms
noise allowance. Final frames remained below 8.33 ms, including contact completion
and concurrent new recovery work. Host render/present p99 remains separately
reported, with scheduling/presentation costs beyond native paint creation.
These measurements qualify the stated workload, not every brush or device.
Local data is in `android-before-frame-times.json`,
`android-before-repeat-frame-times.json`, `android-merged-frame-times.json`, and
`android-optimized-frame-times.json` under `artifacts/color-m1/`; final complete
journeys are in `android-optimized-tests.txt`, `android-saf-final.txt`, and
`android-merged-tests.txt`.

Reproduce the native build using `ANDROID_HOME`, JDK 25 and:

```sh
cd apps/layer-android
./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug \
  -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.rastertest
```

Install both APKs in this isolated application ID and run AndroidJUnitRunner
with `AndroidRasterTest`, `AndroidWorkspaceOwnershipTest`,
`AndroidRasterBenchmarkTest`, and
`AndroidFeatureParityTest#documentSafSaveReopenAndExportPreservePaint`.
Benchmark reports go to that app's external files directory. Lint passed with
JDK 25; the initially selected newer JDK crashed the upstream opt-in detector.

## Shared regression checks after the ports

Release checks passed: 45 core, 49 engine, 366 UI/session, 25 native host and
86 native workspace tests. The hardware renderer passed 123 library tests
(18 separate hardware workloads remain ignored) and all three GPU project tests.
GTK's atomic-write failure/PNG test, both recovery failure/cancellation tests,
actual New/Open/Save/Export/autosave/surface workflow and all seven native pacing
workloads passed after extracting the shared writer.

Two further 25-scenario runs sampled 21,840 frames, with no moving or contact-end
frame above 8.33 ms and no reproducible CPU p95 gate breach against the established
pre-change baseline. The first run overlapped an Android cross-compilation; the
second ran after builds completed. Reports retain that scheduling noise instead
of relabeling it as a renderer cost. Local records are
`artifacts/color-m1/hosts-final.md`, `hosts-final-repeat.md`,
`hosts-final-gtk-pacing.json`, and the `hosts-final-*` correctness logs.

## Remaining host migrations

macOS/iPadOS and Windows share the new core/renderer, but their complete host
lifecycle and rendering performance were not qualified here. Comments at the
Apple renderer-assignment/recovery barriers and Windows retirement/document
paths identify deprecated assumptions and the current APIs to use. Existing
Windows `replace_renderer` and shared project calls are explicitly retained.

The final integration merges main through `bc5615e`. Its stopgap browser
compression/encoding on the event loop was replaced by the qualified dedicated
workers, and the parallel Android lock fixes were consolidated with EINTR retry.
Healthy renderer replacement retains main's bounded active-contact reconstruction;
explicit suspension still discards unsubmitted input. Replacement while a prepared
frame awaits raster dependencies now fails without changing either renderer or
consuming more input; the engine test verifies successful retry after submission.
The final packaged Web raster/recovery journey, merged GTK file workflow, and
all seven merged native GTK pacing workloads pass.
