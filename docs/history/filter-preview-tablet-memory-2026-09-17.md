# 61 MP photo / Filters memory investigation — 2026-09-17

Base: `bc9593b2` (Keep web startup responsive during shader compilation).
Device: attached Wacom MovinkPad 14 / DTHA140, Android Chrome
152.0.7977.82. Followed the local [Web](../development/web.md) and
[Android](../development/android.md) development guides. Web reproduction used
an isolated `http://127.0.0.1:8136/` origin and the optimized development Wasm.
The user's reported sequence is **Open a photo, then open Filters with All
filters selected**.

## Finding and correction

`FilterPreviews::probe_batch` in
[`filter_previews.rs`](../../crates/layer-render-wgpu/src/filter_previews.rs)
scans the insertion-point composition to find a useful preview crop. It submits
four 256 px source tiles per completion, but previously **created a new Float32
source-window texture for every tile**. A 9504 × 6336 photo has 38 × 25 = 950
tiles. At the tablet's 403 × 70 preview size, this created **2.89 GiB of texture
storage cumulatively** just to choose the source for three small visible rows.
It did not render the entire filter catalog: the requested rows were Curves,
Levels and Brightness / Contrast.

Rust retained only the latest source handle. However, the vendored wgpu WebGPU
backend's `WebTexture::drop` does not call `GPUTexture.destroy`; it releases the
JS reference and relies on browser collection. Queue completion between chunks
does not force that collection. Small retained Rust allocation statistics thus
hid a large stream of pending browser/driver allocations. This is allocation
churn and deferred reclamation, not growth of the Wasm linear heap.

The correction is in the **shared renderer**, without a web-only branch: retain
one texture sized for a complete tile plus the preview halo and reuse it in
queue order throughout the scan. Partial edge captures write its top-left
prefix. Shader samples remain inside that capture or are rejected as outside
the document. The source selection, full-resolution filter semantics, four-tile
scheduling, cancellation and output caching are unchanged.

## Tablet evidence

Fixture: existing local LensTip `61mp-DSC02494.JPG`, 9504 × 6336
(60,217,344 pixels; sold as a 61 MP camera image). Loaded through the ordinary
Open document/picker controller using a served copy of the file. GPU creation
instrumentation retained descriptors, not GPU resources. Wasm memory was read
from the module's exported memory, not `performance.memory`.

| All filters, first three visible rows | Before | Corrected |
| --- | ---: | ---: |
| Probe-window textures created | 950 | 1 |
| Cumulative probe texture allocation | 2961.11 MiB | 3.28 MiB |
| Preview request to atlas delivery | 7321.5 ms | 6929.0 ms |
| Tracked GPU bytes before preview | 1,376,826,212 | 1,376,826,212 |
| Tracked GPU bytes after preview | 1,446,871,380 | 1,446,871,380 |
| Main Wasm memory before / after | 304,545,792 / same | 303,693,824 / same |

The unchanged retained GPU total is expected: the old code already counted
only its last probe window. Most of the approximately 67 MiB lasting increase
is the source scene's bounded 64-slot Float32 decode cache. The corrected
probe texture is 659 × 326 × 16 = 3,437,344 bytes.

The original All filters request ran approximately 19:00:48–19:00:55 PDT.
Android's low-memory killer killed four background processes at
19:00:54–19:00:55, during that scan. The corrected run at 19:07:36 rendered all
three nonempty 403 × 70 canvases without exceptions or new low-memory kills.
Closing/reopening the panel reused the delivered previews without another scan.
The original foreground test tab survived; the investigation reproduced severe
system memory pressure, not an exact foreground OOM/error screen. The runs are
not a controlled whole-device peak-memory benchmark: background process state
changed, and cumulative allocation is not simultaneous residency.

A 7800 × 7800 blank-document control also completed before the fix. It created
961 probe textures and took approximately 4.7 seconds. The photo-specific path
adds actual source decoding and an existing large display cache.

Local raw evidence is under `artifacts/filter-memory/`: `tablet-fixed.json`,
`tablet-distort.json`, `android-fixed.json`, `low-memory-kills.txt`, and
`chrome-memory.txt`. Chrome
package PSS and renderer tracked GPU bytes have different scopes and must not
be added together or described as total GPU memory.

## Platform scope and remaining work

**Optimize the shared renderer for all platforms.** Android, GTK, Apple and
Windows ultimately call the same `UiSession::request_filter_previews` and
`WgpuRasterizer` preview implementation. Every host incurred the texture
creation churn. Web adds GC-delayed release; native backends have different
resource-retirement behavior, so the exact OOM threshold is not portable.
The correction benefits every host. Native physical-device evidence is recorded
below; Apple and Windows were assessed from their call paths, not run here.

Three additional issues remain beyond the corrected All filters trigger:

1. **Optional previews bypass the live filter memory budget.**
   `render` falls back to `PixelRect::full(extent)` if any requested filter has
   document-wide sampling. A separate Distort-category test requested Chromatic
   Aberration, Kaleidoscope and Swirl and allocated a 9504 × 6336 RGBA32Float
   source: **918.84 MiB**. Tracked GPU allocation rose to 2,410,975,156 bytes.
   At 19:01:27–19:01:36 Android killed more processes, including a background
   Chrome renderer. This is a separate issue, not necessary for the user's All
   filters trigger. Later document-remapping passes can also require two
   full-size scratch textures, whose dimensions currently only grow.
   `scene/windows.rs` has a live-filter pixel budget, but preview source capture
   does not go through that admission path. Add shared preview admission covering
   source dependencies, intermediate textures and in-flight work; reclaim caches
   on cancellation/hiding. Do not silently crop global samples or change exact
   document pixels to fit a budget. An explicitly unavailable or separately
   designed representative preview is preferable to risking document loss.
2. **Browser display admission spends too much independently.**
   `raster_worker::install` uses `photo_memory_budget().encode_bytes` as the
   complete display-cache allowance. With this tablet's `navigator.deviceMemory`
   hint of 8, that is **1.5 GiB**. It admits the full photo's Float32 display
   pyramid, producing about 1.28 GiB tracked GPU allocation before Filters opens.
   Installed/capped device capacity is not remaining memory, and a file encoder
   allowance is not a GPU cache budget. Native Linux/Android/Apple admission
   instead queries driver/system/process headroom, although these are snapshots,
   not aggregate reservations. Introduce a shared memory policy that accounts
   for existing display, source, preview, paint and staging allocations together;
   let hosts supply measured headroom or an explicitly weaker capacity hint.
   Required edits should outrank optional display detail and picker previews.
3. **The scan is still expensive.** Reuse removes churn, not the 950-tile scan.
   Approximately seven seconds is too long for initial picker images. Improve
   the shared search order/early termination using bounds on the existing winner
   ranking, or exploit source occupancy, while preserving insertion scope,
   nonempty-corner preference and native-resolution sampling. Spatial effects
   already in the source can also allocate image-stage windows while probing;
   those lifetimes need the same admission/reuse audit. Native Android also
   imposes a 200 ms delay on each preview poll in `Effects.kt`. Because polling
   advances the four-tile probe, that host interval appears to contribute
   substantially to the 41-second native result below. Distinguish the delay
   before starting optional work from delivery of GPU completions; a shared
   lifecycle should let hosts service pending work promptly.

## What should move out of the web host

The filter algorithms, catalog/category/search decisions, source selection,
compositing, exact crop rendering and atlas readback already live in common
Rust. There is no JavaScript full-photo readback or JS filter implementation
behind this failure. Moving more JavaScript indiscriminately would not fix it.

Two concrete pieces of policy should be shared:

- **Memory admission and prioritization.** Keep browser capability reads and
  native OS/driver queries in adapters. Move the capacity/headroom interpretation,
  per-purpose budgets, combined accounting and reclaim priorities into a common
  policy. The present web-only coupling of JPEG encode allowance to GPU display
  storage is an example of behavior that should not be decided in a host crate.
- **Preview request lifecycle.** `effects.js` implements revision/epoch keys,
  pending request ownership, stale-result filtering and missing-row batching.
  Similar state machines exist in Android `Effects.kt`, GTK `effects.rs`, Apple
  `FilterPreviews.swift` and Windows `FilterPreviews.cpp`, with differing reset
  and error behavior. Extract that state machine into `layer-ui`/`layer-host`,
  including document/GPU-generation changes, cancellation, cache limits and
  failure backoff. Hosts should supply visible IDs and physical row sizes and
  receive completed atlas rows. DOM/native geometry, animation clocks, pointer
  events, bitmap conversion and platform file/worker transport should stay local.

The separate Wasm-worker packing code is primarily transport between independent
heaps and already delegates persisted-project and photo semantics to shared Rust.
It is not the first extraction priority for this bug.

## Validation

- Shared hardware-GPU preview suite: **4 passed, 1 ignored benchmark**. Includes
  exact multipass/catalog pixel comparisons, source/cache behavior, empty-source
  fallback and cancellation. The chunk regression now asserts the same GPU
  texture identity across probe completions, including partial document edges.
- Optimized WebAssembly build passed; physical-tablet All filters results above.
- Native Android on the same tablet: **OK (1 test)**, 53.909 seconds for the
  complete instrumentation run. The actual pane-opening/first-preview interval
  was **41,140 ms**, including UI synchronization; this is not isolated GPU
  execution time. Tracked GPU allocation increased from **1,423,178,228** to
  **1,493,221,156** bytes (66.8 MiB), and the preview contained opaque photo
  pixels. Process PSS at completion was 653,133 KiB, a separate accounting scope.
  This used the corrected shared renderer; no unfixed native A/B run was made.

Native reproduction uses the opt-in
`AndroidRasterTest#largePhotoFilterPreviews` test with `-e filterPhoto true` and
`files/filter-memory-test.jpg` in the test app's private files directory. It opens
the real photo, opens All filters, checks rendered preview pixels and bounds the
lasting preview storage increase to 96 MiB. The test's normal setup isolates
workspace, recovery and color preferences. For this investigation a separate
application ID, `art.capycanvas.filtertest`, also keeps the installed user app
untouched. Build with the Android guide's SDK setup:

```bash
cd apps/layer-android
./gradlew :app:assembleDebug :app:assembleDebugAndroidTest \
  -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.filtertest \
  '-PcapyAppLabel=Capy Filter Test'
```

Install both APKs with `adb install -r`, copy the fixture into the test app's
private `files/` directory using `run-as`, then:

```bash
adb shell am instrument -w -e filterPhoto true \
  -e class art.capycanvas.AndroidRasterTest#largePhotoFilterPreviews \
  art.capycanvas.filtertest.test/androidx.test.runner.AndroidJUnitRunner
```

Read the instrumentation `OK`/`FAILURES` result. The test writes
`filter-memory.json` to its external files directory. Passing this test does not
qualify document-wide previews, arbitrary layered files or every platform's
memory-pressure behavior.
