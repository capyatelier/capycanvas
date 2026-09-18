# Shared filter preview scheduling — 2026-09-17

Follow-up to the [61 MP memory investigation](filter-preview-tablet-memory-2026-09-17.md).
The texture reuse correction is `1724e5c6`. This change speeds completion and moves
preview lifecycle policy into Rust. It also integrates the subsequent Apple
cancellation/save correction (`02be7ad1`), preserving its file-storage changes,
and the later shared dry-brush optimization (`e350a585`).
Used the local [Android](../development/android.md) and [Web](../development/web.md)
development guides.

## Cause and behavior

Android's old `delay(200)` ran even while a preview was pending. A completion poll
can submit the next four-tile source-probe chunk, so that delay was repeatedly
inserted into the dependency chain. The 9504 × 6336 photo contains 950 tiles and
needs 238 chunks. Small bitmap conversion was not the bottleneck.

An earlier instrumented A/B/B/A run measured 40.085–40.791 seconds from request to
atlas with the old cadence, versus 5.547–5.937 seconds with frame-paced completion.
The atlas SHA-256 and tracked GPU allocation were identical across those runs.
Panel opening to observing the bitmap was 40.674–41.398 seconds before the change.
The shared-controller build completed that user-visible sequence in **6.109 seconds** on the final main integration (6.043–6.358 seconds in
the preceding shared-controller runs).

New work still waits for 200 ms of idle time. Pending work is serviced on Android's
real Choreographer clock. Both admission and completion service yield to input,
active strokes, gestures and unfinished document edits. Cancellation drops stale
callbacks without waiting for the GPU; each renderer poll advances at most four
source tiles. Already submitted GPU work cannot be preempted, so testing first
contact and drawing while a probe is pending remains necessary.

## Shared ownership

[`layer-ui/src/filter_previews.rs`](../../crates/layer-ui/src/filter_previews.rs)
owns source/document/GPU generations, admission, batching (eight rows), pending
requests, cancellation, retry and retention (64 delivered rows). The renderer
keeps only the current request's pixel rows. A cache key is opaque to hosts;
hosts report actual retained images so failed conversion or a recreated view can
recover. Hidden panels, obsolete visible categories and renderer replacement
retire pending work. Larger projections can upgrade the backing size; smaller
ones reuse it.

Android, Web, GTK, Apple and Windows now submit visible IDs and physical geometry
to that controller. Their remaining code owns native clocks, visible geometry,
thread/queue transfer and image conversion/presentation. Web uses one driver per
editor across panel/drawer views, with independent row storage so an evicted row
can release its atlas. GTK's renderer worker tags queued replies by generation
and yields optional completion work during active canvas rendering.

Apple's former ABI cancellation flag and Swift request bookkeeping are replaced
by shared status and proactive retirement. Source changes still produce no editor
error; genuine failures remain reportable. Its Rust cancellation/retry/ownership
checks pass after integration, and its mounted Swift owner test has been updated
to the new protocol. Windows and Apple native UI builds remain for those platform
agents to run; Linux Rust checks do not establish Swift or WinUI acceptance.

## Physical Android results

Wacom MovinkPad 14 / DTHA140, Android 15, 120 Hz. Same 61 MP JPEG as the memory
investigation. Separate `art.capycanvas.filtertest` app; no production app uninstall,
data reset or drawing overwrite. Debug Kotlin APK with release Rust library.

The responsiveness test injects OS stylus events through the real input path at
approximately 4 ms intervals. Four five-second strokes run in closed/open/open/
closed order; open runs start while a fresh preview is pending. Each timed stroke
must increase the document revision. The test uses the real display clock, records
input/render timing and SurfaceFlinger presentation samples, and waits for a fresh
preview after release.

| Final main integration, p95 | Closed 1 | Open 1 | Open 2 | Closed 2 |
| --- | ---: | ---: | ---: | ---: |
| Input arrival → render owner, ms | 6.471 | 6.038 | 6.039 | 5.629 |
| Input event → render owner, ms | 8.850 | 8.761 | 8.524 | 7.792 |
| Frame render/present CPU, ms | 9.000 | 8.317 | 8.527 | 8.829 |
| Frame callback spacing, ms | 16.667 | 16.667 | 16.667 | 16.667 |
| Recorded frames | 558 | 558 | 557 | 560 |

Fresh previews appeared 5.607 and 5.998 seconds after pen release. The test gates
open-panel p95 input queue and frame CPU against the slower closed run plus 2 ms,
frame spacing plus 0.5 ms, and preview resumption within 20 seconds. Both the
initial candidate and shared-controller runs passed. This is evidence of no
measured drawing regression on this tablet; injected events are not a substitute
for a physical pen feel assessment across devices, brushes and thermal states.

Earlier matched runs had p95 frame spacing of 8.334 ms and frame CPU around
5.7–6.2 ms in both conditions. The final run, after the additional upstream
brush change and sustained testing, was slower in **both** conditions. These
are not controlled cross-commit timing comparisons; no cause for that absolute
variation is established here. The acceptance result is no measured additional
drawing cost from opening Filters in the same build, not guaranteed 120 Hz or
an assessment of the separate brush optimization.

The first All-filters preview retained 1,423,178,228 → 1,493,221,156 tracked GPU
bytes (about 67 MiB increase), unchanged from the texture-reuse fix. Process PSS
was 661,647 KiB. These counters have different scopes and are not added together.

## Verification and follow-up

- Android build/lint and three instrumented tests (all passed again on the final main integration):
  preview pixels/storage,
  drawing responsiveness, and hide/reopen/background/GPU/document replacement.
- Shared UI suite: 459 tests passed before the final extra category/stale-atlas
  regression. Six focused lifecycle tests plus the host-without-GPU check
  cover the final controller.
- Shared GPU previews: four passed, one opt-in benchmark ignored, including
  full-resolution pixel comparisons and bounded chunk/cancellation checks.
- Integrated Apple Rust preview-related suite: eight passed, including source
  cancellation/retry and atlas ownership after editor teardown.
- Native Rust adapters compile on Linux; Wasm builds and web packaging tests pass.
- Desktop browser: all 40 filter workflow/rendering assertions passed. The runner's
  final console check fails on the existing headless Dawn message, `A valid external
  Instance reference no longer exists.` It was not suppressed for this change.
- Tablet Chrome: focused GPU pixel/cache/lifecycle checks pass with no console
  errors, covering cached reopening,
  source/category changes and GPU replacement. The broader desktop-oriented
  adjustments fixture also exercised these rows, but its curve-endpoint mouse
  pickup assertion failed intermittently on the tablet. That separate input check
  is not claimed as passing or changed by this preview fix.
- The full host unit suite has an unrelated existing failure in
  `snapshot::tests::workspace_updates_retain_models_and_match_authoritative_layout_and_history`
  (group y 400 versus 48). Reproduced on an unchanged `1724e5c6` source archive.

Raw device results, A/B timing, build logs and test reports are under the ignored
`artifacts/filter-memory/` directory. `android-preview-main-{idle,drawing,summary}.json`
contains the table above; `android-preview-integrated-*` and
`android-preview-verified-*` record the preceding shared-controller checks. The Apple agent should run its mounted filter owner check and macOS/iPadOS
builds. The Windows agent should build WinUI and exercise simultaneous views,
hide/reopen, first input, document changes and GPU replacement.

The preview algorithm and image resolution are unchanged. This change does not
address document-wide filters' large source allocations, the browser display
cache budget, or scanning every source tile. Those remain the separate memory and
algorithm work identified in the original investigation. Faster completion may
concentrate optional GPU activity while idle; battery/thermal behavior was not
benchmarked. Frequent continuous editing intentionally defers previews until idle.
