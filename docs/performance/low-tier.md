# Low tier: 60 fps on 12 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: TCL TAB 11 Gen 2 (9465X) with a 1200 × 1920, 60 Hz panel. The canvas
is 4248 × 2832.

- Every row targets **60 fps** unless marked soft.
- Geometry measurements use the 12 MP photo at Fit zoom. Older UI-only rows
  name their own canvas sizes.

## Android panel composition

Measured 2026-10-28 on TCL TAB 11 Gen 2 against `176c68cd6`, using
release Rust, an unminified benchmark host, the 4248 × 2832 reference photo
at Fit, factory density 320, font scale 1, 60 Hz and thermal status 0.
The candidate retains panel content outside the changing constraint lambda;
native libraries are identical. Each recording has three six-second contacts.
The earlier light comparison uses a one-second primer and
baseline/candidate/candidate/baseline order; its rates are diagnostic. The dark
comparison uses a full six-second primer.

| Column-gap resize | Baseline | Retained panel body | Qualification |
| --- | --- | --- | --- |
| Light, mean actual window presents/s across both order pairs | 42.127 | 44.680 | Not met; warming/order spread overlaps the gain |
| Light, mean fresh drawn bounds/s | 35.926 | 38.226 | Not met |
| Light, worst presentation-gap p99, ms | 66.653 | 66.637 | Not met |
| Dark, actual window presents/s in three contacts | 41.792 / 48.780 / 50.563 | 44.888 / 52.088 / 55.193 | Not met |
| Dark, fresh drawn bounds/s | 35.821 / 39.548 / 43.759 | 40.020 / 41.335 / 43.978 | Not met |
| Diagnostic unchanged `PanelControls` compositions | 290 | 0 | Less repeated UI work; not a presentation rate |
| Diagnostic mean UI measure/layout, ms | 8.206 | 6.312 | 23.1% lower; trace overhead applies |

The separate composition regression keeps its observer active without Perfetto.
It fails on the baseline with 442 unchanged control compositions and passes all
three candidate contacts with zero, while recording 235/262/269 changing bounds
and no full snapshots. Both builds perform the same native gesture.

The same three-row native panel-change/size-slider/value fixture records
34.172/37.098/37.316 → 34.731/38.534/41.486 actual presents/s for the slider.
Its p99 ranges are 149.956–166.608 → 149.944–183.202 ms; the worst increase is
one display interval, within the control's contact spread. Value scrubs record
43.195–48.648 → 50.104–50.975 presents/s. Both controls remain below target.
The candidate's light/dark resize matrices pass all 48 retained-content contacts;
the matched full-primer light control passes all 24. Native panel-schema and
displayed-control journeys also pass in both themes.

Light and dark native bridge/source pixel checks pass. Ten chrome crop pairs
per theme differ at three colour channels by at most one level, all within
Navigator imagery; chrome text and edges match. The production refactor is
+16/−16 lines. Matched APKs, raw contacts, traces and crops are retained under
`/mnt/localssd/capycanvas5-android-ui-motion-20261028/{abba-gap,body-acceptance}/`;
candidate APK SHA-256 is
`946d04acf4821d234949ce058cf1c77d570c8146bddb888f471a5b0e6b8adcac`.

The matched Navigator-open canvas controls use the same photo and three
six-second native contacts. Pan and rotation remain 60.023 presents/s in every
contact. Pinch is a shared miss: 47.468–48.673 → 47.812–48.673 presents/s.
Fresh input-attributed stroke completions are 39.117–39.681 →
38.982–39.466 updates/s, within run spread and below the canvas target.
Exact consumed-input-to-GPU-completion pooled p50/p95/p99 is
54.323/70.200/78.513 → 53.983/69.598/75.179 ms. These software intervals exclude
digitizer and panel latency; they do not measure physical pen-to-ink latency.

## Android UI compositing trials

Measured 2026-10-28 on TCL against `10f4b7c40`: 4248 × 2832 photo at Fit,
factory density 320, font scale 1, 60 Hz, thermal status 0, default Low glass,
light theme. Each condition has three measured six-second contacts after a
full-duration primer. Both trials use matched ABBA order, identical native
libraries and test APK `445223341d9beae94f9f67a41c81ec0b7208ea4f3f63458266ecd38a651ee03c`.
The rates are mean actual target-window SurfaceFlinger presents/s across six
contacts per variant; p99 is the worst contact presentation-gap p99.

Trial 1 removes the full live-pane offscreen intermediate and retains the
shadow separately. Trial 2 instead clips a content-only offscreen layer after
rasterization, with the retained shadow outside. Neither was accepted; both
were removed pending a valid numeric gate. Neither is a shipped production
frame-rate improvement. Trial 2 reuses the final trial-1 control as its opening
control; its comparison uses only A2/C1/C2/A3.

| Trial | Row | Control → candidate presents/s | Control → candidate worst p99, ms |
| --- | --- | --- | --- |
| 1 | Size slider | 38.584 → 38.043 | 183.181 → 199.882 |
| 1 | Size value scrub | 47.428 → 45.421 | 183.195 → 183.307 |
| 1 | Flow slider | 51.242 → 50.265 | 149.962 → 149.954 |
| 1 | Flow value scrub | 41.766 → 42.657 | 166.622 → 166.588 |
| 1 | Wheel, mouse | 46.575 → 47.300 | 49.993 → 50.000 |
| 1 | Wheel, touch | 47.024 → 47.978 | 49.885 → 49.932 |
| 2 | Size slider | 37.958 → 38.375 | 183.181 → 183.266 |
| 2 | Size value scrub | 47.781 → 48.241 | 183.195 → 166.614 |
| 2 | Flow slider | 51.822 → 51.369 | 149.940 → 149.960 |
| 2 | Flow value scrub | 44.070 → 41.882 | 166.547 → 166.603 |
| 2 | Wheel, mouse | 46.418 → 47.230 | 49.997 → 49.974 |
| 2 | Wheel, touch | 46.838 → 48.352 | 49.867 → 50.007 |

Trial-1 Flow slider phase means are 51.297/51.187 control versus 50.249/50.281
candidate. Trial-2 Flow value means are 45.177/42.963 control versus
41.809/41.956 candidate. The latter row has substantial contact spread; the 4.96% negative estimate
does not exceed its 5.02% between-control mean spread. It remains an unresolved
tradeoff, not proof of a uniquely attributed regression. Both candidate phases are below both control phases. No further
production candidate was tried under the two-candidate stopping limit.
The original numeric recordings contain no per-frame value timeline, so the
Flow-value presentation delta cannot establish a fresh-value regression.
A subsequent harness records scalar/composed value changes and input positions;
clip ordering is being rechecked with that gate on the current measurement-cache
baseline. The earlier presents-only result remains diagnostic.

The corrected numeric gate replays captured controls, origins and Float pointer
coordinates through shared Rust. The old 1 Hz, ±18–20 dp percent-value scrubs
imply only about 16–20 distinct changes/s and legitimate turnaround plateaus;
they cannot establish a delivery stall or reject clip ordering. The new value
sweep runs at 240 dp/s. A fresh control at `6c6705231`, 12 MP Fit, factory
density/font, thermal status 0 and three warmed contacts records:

| Numeric row | Actual presents/s | Present gap p99, ms | Delivered / input-implied / display-eligible changes/s | Delivered gap p99, ms | Demand-conditioned gap p99, ms |
| --- | --- | --- | --- | --- | --- |
| Size slider | 38.55 | 166.58 | 27.19 / 172.62 / 52.54 | 246.03 | 238.45 |
| Size value | 51.35 | 49.91 | 45.70 / 148.77 / 59.54 | 47.94 | 46.62 |
| Opacity slider | 51.73 | 149.95 | 42.86 / 138.93 / 52.53 | 250.08 | 246.15 |
| Flow value | 57.25 | 33.35 | 50.37 / 59.87 / 59.54 | 49.06 | 45.28 |

Rates are contact medians; tails are the worst contact p99. All twelve exact
replays pass. Display-eligible demand uses an inferred 60 Hz lattice anchored
to actual target-window presents. Draw observations do not establish accepted
input, matching-value latency or fresh SF buffer identity. Observer wall p99
is at most 0.274 ms. The unrelated editor continued using CPU in the background;
its visibility and CPU counters are retained with the run. No production
gain is claimed. Evidence: `artifacts/android-ui-motion/round3/numeric-validation/`
and scratch `round3/small-rounded-abba/A1/numeric-validation/`.

The current control cannot qualify native layer-row opacity: changing opacity
replaces the panel's measurement identity and loses its popup owner. A single-row
repro fails with `ui-layer-row-opacity draw target unavailable`; shared state
still marks the popup open. Earlier 30–31 presents/s results predate this check.
The primary failure and suppressed cleanup timeout are retained in
`artifacts/android-ui-motion/round3/layer-popup-owner-repro.local.md` and scratch
`round3/popup-primary-control/`. No candidate workaround is applied.

Separate diagnostic traces establish RenderThread drawing/flush work and
software clip-mask generation. The wheel has about three large logical
layer-target repaints per moving frame. A bounded Skia trace records about
20 software clip-mask tasks and 20 pixel writes per flush. Individual upload
ownership and physical GPU render-target switches remain unobserved.
Buffer-stuffed frames have adjusted deadlines unavailable in these captures;
their original predicted endpoints do not establish the stage causing a miss.
Dropped buffers are separated from presented frames. The critical-path table,
queries, unknown shares and next untested investigations are in
`artifacts/android-ui-motion/plan.local.md` and its linked reports.

Both variants pass benchmark/debug app/test builds and lint; all ABBA JUnit
runs pass. Final dark-theme/middle-tier matrices, canvas guards and chrome crop
acceptance were not run for these discarded candidates. The proposed 90 Hz collector
extension has not been built or validated on Android and is not shipped code. Exact APKs,
hashes, contacts and p99 samples are retained under
`/mnt/localssd/capycanvas5-android-ui-motion-20261028/critical-trial{1,2}-abba/`.

## Android retained thumbnail projection

The Layers panel retains its thumbnail-map projection until its images or
document epoch change. Unchanged rows can then skip composition during drag.
Matched TCL ABBA at `df1b3b570`, 12 MP Fit, factory density/font and thermal
status 0, uses identical native libraries and test APK `f14cd42e…`:

| Row | Control → candidate actual presents/s | Worst contact gap p99, ms |
| --- | --- | --- |
| Layer reorder | 44.779 → 50.431 (+12.62%) | 49.983 → 49.983 |
| Attached filter reorder | 38.049 → 45.246 (+18.91%) | 83.322 → 83.311 |

Both candidate phase means exceed both controls; control drift is 1.69% and
4.39%, respectively. All eight native fixtures pass. These rows still miss
60 Hz, and their worst-contact tails do not improve. Numeric fresh values are
not applicable to these rows.

Separate composition diagnostics show 68.7% fewer LayerRow entries, about
four → one per LayerPanel entry. A regression assertion fails on control
(`764 row scopes for 191 panels and 3 authored rows`) and passes all candidate
contacts. Clean FrameMetrics animation-phase p50 falls 14.98 → 11.22 ms for
layers and 17.43 → 11.93 ms for filters. These are elapsed phase times; the
RenderThread command-issue phase does not improve. Both-theme red, gradient,
reverse and undo pixel checks pass on control and candidate; all six thumbnail
and row crops are pixel-identical. Relationship captures are also identical
before a shared control swipe failure. Full
contact triples, hashes, costs and acceptance checks are retained under
`artifacts/android-ui-motion/round3/retained-thumbnail-attribution/` and scratch
`round3/retained-thumbnail-{abba,regression,acceptance}/`.

Matched canvas guards pass all eight native fixtures on the 12 MP photo at
Fit with Navigator, OS input and G-Pen 1024. Actual presents/s are pan
60.022 → 60.023, pinch 48.087 → 48.316, rotate 60.022 → 60.023 and stroke
38.879 → 38.743. Completed stroke updates are 38.39 → 38.28/s, within contact
spread; software input-to-completion p99 is 83.843 → 79.541 ms. These do not
measure physical digitizer-to-panel latency. No material canvas regression
is established. The separately reproduced popup-owner and pass-through-swipe
control failures remain unresolved. The full filter journey completes its
light/dark thumbnail checks, then the control times out while revealing Delete
on the remaining row; that later geometry failure is retained separately.

## Android layout-only panel measurement identities

Measured 2026-10-29 on TCL TAB 11 Gen 2: 4248 × 2832 reference photo at
Fit, factory density 320, font scale 1, 60 Hz and thermal status 0. Shared
panel measurement keys retain layout inputs while excluding live values;
Android subscribes to per-panel origins instead of the whole request array.
No clip, shadow or glass change is included. The frozen comparison is based on
the original baseline: control APK `72145997…`, candidate `1ef3909c…`, common test `81ac0612…`.
Each ABBA phase uses a full six-second primer and three six-second contacts.
All measured contacts, including slow contacts, remain included.

| Native motion | Control → candidate median presents/s | Control → candidate mean presents/s | Worst contact present-gap p99, ms |
| --- | --- | --- | --- |
| Color wheel, mouse | 37.547 → 44.521 (+18.6%) | 36.922 → 44.578 | 66.657 → 50.000 |
| Color wheel, touch | 42.800 → 45.146 (+5.5%) | 42.969 → 45.161 | 50.022 → 49.984 |
| Layer reorder | 50.499 → 50.194 | 51.483 → 50.456 | 49.984 → 49.982 |
| Attached filter reorder | 43.480 → 43.171 | 44.507 → 45.471 | 83.268 → 66.663 |

Wheel cadence improves; reorder differences do not establish a stable gain.
All four rows still miss 60/s. Diagnostic wheel contacts record zero unchanged
docked-control composition scopes and zero measurement requests. Three large
logical GPU layer replays remain, with about 20 uploads per flush: this is a
UI-work reduction, not evidence of fewer GPU replays or a passed GPU gate.
[Mouse](../../artifacts/android-ui-motion/round5/final-panel-body-mouse.json),
[touch](../../artifacts/android-ui-motion/round5/final-panel-body-touch.json) and
[Skia counts](../../artifacts/android-ui-motion/round5/final-panel-body-skia-counts.csv)
retain the diagnostic attribution.

The size-slider gate pools three ABBAs, 36 contacts with exact native replay.
Mean actual presents/s is 36.555 → 35.760 (−2.18%); mean delivered scalar
changes/s is 27.287 → 27.018 (−0.98%). Both differences are within the observed
control-phase spread: 35.159–38.268 presents/s and 26.351–28.352 delivered/s.
The tie-breaker's candidate mean, 35.916 presents/s and 27.240 delivered/s,
exceeds its closing control's 35.159 and 26.351. This passes the pooled
non-regression gate without excluding outliers; it does not improve size cadence.
Worst contact p99 across all three comparisons is 183.223 → 183.260 ms for
presents, 259.763 → 258.917 ms for delivered values and 249.317 → 252.103 ms
for demand-conditioned delivery gaps. All remain misses.
[All contacts and tails](../../artifacts/android-ui-motion/round5/final-numeric-three-comparisons.csv)
and [phase means](../../artifacts/android-ui-motion/round5/final-numeric-three-comparisons-summary.json)
retain the complete gate, including the confirmation outlier.

The windowless identity regression passes actual color, brush and layer-numeric
dispatches across platform policies. The candidate rebased onto `752d5d9b1`
has separate APKs (`bb97…` control, final `f280…` candidate, `c238…` common
tests); the rates above remain attributed to the frozen original pair. Retained
rows exposed stale document callbacks: the exact layer-input journey passed
both control runs and failed both initial candidate runs at the first body
hold. Refreshing retained callbacks fixes that case, including both-theme
finger, pen and mouse hold/drag/swipe/cancel contacts.

All eight matched canvas guard fixtures complete. Mean actual presents/s for
pan is 60.023 → 60.023, pinch 48.385 → 48.363 and rotation 60.023 → 60.023;
pinch remains a shared target miss. Software-input stroke is a regression
diagnostic, not brush-tier qualification: native submissions are 38.4 → 38.6/s,
with input-to-completion p50 54.674 → 54.682 ms and p99 85.369 → 81.594 ms.
Physical digitizer latency is unmeasured. All six Palettes/Layers crop pairs
are pixel-identical in light and dark themes, including Layers at 226/300 dp.
Palette history/expansion passes; the Layers relationship fixture retains the
same late pass-through assertion failure in control and candidate, after all
four screenshot postconditions.

The final rebased/callback ABBA completes all 16 fixtures and retains all 60
contacts, with identical common tests and factory display/thermal conditions.
Every candidate wheel contact exceeds every matched control contact. The
following rates are contact means; tails are the worst contact p99.

| Motion | Control → candidate presents/s | Present-gap p99, ms | Delivered values/s |
| --- | --- | --- | --- |
| Wheel, mouse | 37.642 → 44.846 (+19.1%) | 66.722 → 50.059 | — |
| Wheel, touch | 43.088 → 45.406 (+5.4%) | 66.387 → 49.963 | — |
| Size slider | 36.522 → 36.765 | 166.606 → 183.227 | 27.491 → 27.101 |
| Layer reorder | 50.122 → 49.382 | 33.669 → 49.992 | — |
| Filter reorder | 44.567 → 44.997 | 66.655 → 66.883 | — |

Size delivered-gap p99 is 252.494 → 255.501 ms; demand-conditioned p99 is
249.069 → 251.784 ms. This independent integration check remains separate from
the accepted original three-ABBA size gate. Layer phase means A1/B1/B2/A2 are
50.113/49.989/48.775/50.131 presents/s; phase worst p99 values are
33.604/49.992/49.923/33.669 ms. Candidate tails occupy three refresh intervals,
matching the historical shipped-thumbnail 49.983 ms tail; the two current
controls occupy two. The rate difference is within contact spread and does
not establish a reorder gain. No tail investigation or extra campaign is added.
[Final integration contacts and provenance](../../artifacts/android-ui-motion/round5/rebased-integration-summary.json)
retain the exact phase evidence. Final TCL dark and MovinkPad 90 Hz matrices
remain pending.

## Android UI motion baseline

Measured 2026-10-28 on the TCL TAB 11 Gen 2, 4248 × 2832 reference photo
at Fit, factory display size/density 320 and font scale 1, default Low glass,
light theme. The production baseline is `176c68cd6`. The v8 control APK is
`33115e277148d2fb77564808a8e3acc89c2f5a10ed3d2f0e938df149b05c420f`;
its test APK and native hashes are in `v8-artifact-manifest.json`. Older v4–v6
recordings use their retained APK/test pairs; that hash does not identify them.
These are baseline observations, separate from the retained-panel-body
comparison above.

Rates and gaps below use app-owned SurfaceFlinger buffer latches joined to
actual display-frame completion. Continuous native controls with a recorded
window title use that literal window: `art.capycanvas.capycanvas5/art.capycanvas.MainActivity`
for main chrome and `Pop-Up Window` for layer-row Opacity and compact workspace
menus. Older records without a title use the app-owned aggregate; they do not
isolate a popup. FrameMetrics, callbacks and revision counts are attribution,
not presented FPS or proof that every presentation contains a new pose.

`resize-v8-light`, `wheel-v8-light`, `edit-wheel-v8-light`, column feedback and
`drag-v5-light` use full six-second primers and three six-second measured
contacts per input/mode. Native continuous UI controls also retain six-second
primers and three measured contacts. Older layer/grouped-drawer/switcher
fixtures used one-second primers or lack retained full-primer evidence and
remain diagnostic under the current measurement rule. The layer drawer and
other short transitions are diagnostic regardless of journey length.

The canonical UI recording is
`ui-full-complete-light/A-ui-full-complete-summary.json`: all 22 native rows
and their three measured contacts passed in one 714.394-second JUnit run.
It supersedes the earlier partial prefixes that failed later setup. Each row
uses a full six-second primer. Short or discrete transitions still have no
sustained-FPS qualification. Dark-theme completion for this full baseline
matrix remains unmeasured.

| Native motion | Actual presents/s | Presentation gap p99, ms | Fresh bounds/s | Evidence and limits |
| --- | --- | --- | --- | --- |
| Panel/column resize: Brushes, Properties, Navigator, toolbar; mouse/touch | 35.289–59.334 | 33.089–49.989 | 34.146–54.651 | `resize-v8-light` |
| Tab drag, floating panel and docking destination; mouse/touch | 58.814–60.023 | 16.766–33.340 | 51.993–59.284 | `drag-v5-light` |
| Panel drag over Color wheel | 58.815–59.850 | 16.761–33.316 | 47.429–57.404 | `color-overlap-v4-light; primer-limited diagnostic` |
| Committed panel Color wheel; mouse/touch | 46.570–47.742 | 33.555–50.004 | — | `wheel-v8-light; committed native colors change; bounds count does not apply` |
| Edit Color wheel; mouse/touch | 42.774–57.953 | 33.410–50.098 | — | `edit-wheel-v8-light; committed native colors change; bounds count does not apply` |
| Canvas action bar move (real photo Transform Y scrub) | 5.692–7.210 | 483.203–649.862 | — | `ui-full-complete-light` |
| Brush Size slider | 38.358–42.873 | 149.937–166.605 | — | `ui-full-complete-light` |
| Brush Size value scrub | 46.293–50.222 | 49.980–150.088 | — | `ui-full-complete-light` |
| Brush Opacity slider | 51.372–52.255 | 149.944–149.960 | — | `ui-full-complete-light` |
| Brush Opacity value scrub | 34.695–44.843 | 50.065–166.618 | — | `ui-full-complete-light` |
| Brush Flow slider | 49.960–52.124 | 116.666–149.904 | — | `ui-full-complete-light` |
| Brush Flow value scrub | 34.348–41.928 | 166.568–166.618 | — | `ui-full-complete-light` |
| Layer Opacity header slider | 52.910–57.074 | 33.327–49.922 | — | `ui-full-complete-light` |
| Layer Opacity header value scrub | 54.629–56.217 | 33.347–49.978 | — | `ui-full-complete-light` |
| Layer-row two-finger Opacity popup | 30.446–31.309 | 83.271–83.334 | — | `ui-full-complete-light` |
| Layers list scroll | 59.502–59.850 | 16.847–16.882 | — | `ui-full-complete-light` |
| Brushes list scroll | 55.871–58.120 | 33.313–66.639 | — | `ui-full-complete-light` |
| Filters list scroll | 58.120–59.159 | 33.273–33.328 | — | `ui-full-complete-light` |
| Grouped Drawing drawer scroll | 57.946–59.502 | 16.985–33.315 | — | `grouped-scroll-light; primer-limited diagnostic` |
| Compact workspace choices and Visibility submenu scroll | 59.849–60.023 | 16.975–17.007 | — | `switcher-scroll-v6-light; primer-limited diagnostic; factory compact menus, vertical; horizontal strip unmeasured` |

The resize matrix covers 24 contacts. Its lower rates are a baseline miss;
retaining content does not make every moving bound meet 60/s. Fresh bounds
are independently sampled from actual drawn geometry. Layer swipe/reorder
records below retain their historical one-second primer limit and are not silently
promoted to current qualification:

| Layer motion | Recorded actual presents/s | Gap p99, ms | Limits |
| --- | --- | --- | --- |
| Swipe left | 56.217–57.428 | 33.331–33.391 | `layer-swipe-left-native-light`; three six-second contacts, historical one-second primer; primer-limited diagnostic |
| Swipe right | 56.401–58.120 | 33.342–33.379 | `layer-swipe-right-native-light`; three six-second contacts, historical one-second primer; primer-limited diagnostic |
| Layer reorder | 39.324–46.064 | 33.577–49.988 | `layer-reorder-native-light`; three six-second contacts, historical one-second primer; primer-limited diagnostic |
| Attached filter reorder | 37.909–39.497 | 49.883–50.087 | `filter-reorder-native-light`; three six-second contacts, historical one-second primer; primer-limited diagnostic |

Whole columns do not translate while held: Rust applies their docking move on
release. `column-v8-light` samples changing `workspace-drop-hint` bounds and
disappearance while the native grip crosses before/after column targets. It
also requires a native release that changes layout and one Undo that restores
it. Its 36 observed feedback windows are diagnostic; the earlier stationary
grip's ~59 presents/s was not fresh column-body motion.

Short transitions retain their observed draw windows and the first subsequent
actual present bounded by the next request or contact end. The durations and
onsets below are presentation observations, not fresh-pose latency or a
sustained FPS claim. Repeated presentations within a transition stay in the
series; idle gaps between requests are not joined. A single-present transition
has no inter-present gap statistic. Tile dragging uses discrete drop-hint
poses even when the contact lasts six seconds.

| Native transition | Observed windows across three journeys | Observation duration, ms | First present from window start, ms | Gap p99, ms | Recording |
| --- | --- | --- | --- | --- | --- |
| Column docking feedback | 36 | 21.204–38.287 | 5.065–21.204 | 16.373–33.222 | `column-v8-light` |
| Layers/Properties drawer | 39 | 85.619–284.824 | 7.475–28.968 | 16.663–33.390 | `drawer-v2-light` |
| Action bar show/hide | 74 | 46.877–64.540 | 46.877–64.540 | — | `ui-full-complete-light` |
| Action bar show/hide in Zen | 74 | 46.114–47.424 | 46.114–47.424 | — | `ui-full-complete-light` |
| Native Select/Drawing Brush panel-content change | 66 | 74.920–147.237 | 13.041–147.237 | 49.994–66.638 | `ui-full-complete-light` |
| Main menu open/close | 42 | 147.818–829.370 | 10.876–179.575 | 33.306–316.558 | `ui-full-complete-light` |
| Filter submenu enter/return | 30 | 327.705–345.411 | 10.576–12.326 | 16.670–16.869 | `ui-full-complete-light` |
| Grouped tool menu open/close | 42 | 148.326–749.861 | 61.528–529.803 | 16.657–283.243 | `ui-full-complete-light` |
| Native grouped-tool tile drag/drop hint | 3 | 5991.644–6001.745 | 3.957–11.471 | 16.808–16.842 | `ui-full-complete-light` |
| Zen enter/leave | 42 | 415.817–466.107 | 79.550–229.574 | 16.666–99.973 | `ui-full-complete-light` |
| New Drawing open/Cancel | 30 | 264.827–427.732 | 11.257–128.274 | 33.397–199.925 | `ui-full-complete-light` |

Layer/mask selection is instantaneous. `layer-selection-v9-light` passed three
six-second contacts and records 14 deduplicated state-change observations per
contact plus the actual app-window present series and each next present. This
is not a fresh-selection latency measurement and has no sustained FPS.
Expanded configuration, general dialog page traversal and the noncompact
horizontal workspace strip remain unmeasured; New Drawing open/Cancel does
not establish those journeys. Grouped menu/tile checks do not qualify every
possible drawer sibling switch.

The existing five-bubble fixture also passed three contacts at
59.850–60.026 actual app-window presents/s, gap p99 16.820–16.917 ms, with
factory settings and visible neighbor motion; see [Native tool bubbles](#native-tool-bubbles).
This result does not extend to every short bubble opening transition.

Raw traces, fixture snapshots, value variants, interactions, before/after
geometry and per-row completion guards are under
`/mnt/localssd/capycanvas5-android-ui-motion-20261028/baseline-tcl/`, using the
recording names in the tables. Compact summaries are retained in
`artifacts/android-ui-motion/final-baseline-inventory.local.{json,md}`.

## Navigation presentation

Measured 2026-10-24–25 on the reference TCL TAB 11 Gen 2, comparing
`2d7c4d5d7` with the presentation-admission and renderer-cache fix. Both use
optimized Rust release code and the same benchmark host; the final APK is
`55febd3740001e0ca1f89c1761e1995ade72ea5bb8b8aa96da497680c67b623d`.
Each row has three independent eight-second moving contacts, warm required
shaders, actual 60 Hz display mode and thermal status 0. Factory density 320,
font scale 1 and unmodified physical resolution are verified. The inherited
density override of 272 is restored afterward; its earlier runs remain
separate diagnostics. Rates are medians of SurfaceFlinger canvas presentations,
independently matched to the latency ring. The tier photo is 4248 × 2832 at Fit.
The supplemental default-paper rows use the actual startup 1600 × 1200 canvas,
startup camera, Sketch workspace and Coldpress Naturalwhite at strength 100.

| Workload and motion | Baseline fps | Fixed fps | Gain | Worst contact gap p99, ms | 60 Hz rate and gap target |
| --- | --- | --- | --- | --- | --- |
| Default 1600 × 1200 paper, rotate | 60.02 | 60.02 | +0.0% | 16.841 | Met |
| Default 1600 × 1200 paper, pinch | 57.40 | 60.02 | +4.6% | 16.835 | Met |
| 12 MP, Navigator closed, pan | 41.98 | 56.26 | +34.0% | 33.395 | Not met |
| 12 MP, Navigator closed, rotate | 42.27 | 54.52 | +29.0% | 33.420 | Not met |
| 12 MP, Navigator closed, pinch | 39.51 | 48.39 | +22.5% | 49.979 | Not met |
| 12 MP, Navigator open, pan | 45.36 | 60.02 | +32.3% | 16.859 | Met |
| 12 MP, Navigator open, rotate | 45.17 | 59.14 | +30.9% | 33.361 | Not met |
| 12 MP, Navigator open, pinch | 46.79 | 54.41 | +16.3% | 33.516 | Not met |

Default-paper rotation already meets the target in the baseline. The fix brings
default pinch and Navigator-open photo pan to the target. Closed-Navigator
photo rows remain below the 57 presentations/s floor; their viewport GPU work
is about 17.6–17.8 ms per pan/rotation frame. Open-Navigator rotation clears the
rate floor but one contact's 33.361 ms gap p99 exceeds 33.333 ms. The shader and
quality settings remain unchanged. Full per-contact values, native state,
traces, captures and provenance are under
`artifacts/nav-fix-qualification-20261024/low-factory320/`.

### Brush and pending-filter controls

The same factory-scale comparison uses G-Pen 1024 px at pressure 1 and physical
inclination 40°, with nominal 200 Hz input on the 12 MP photo at Fit, Navigator
closed. Fresh input-attributed GPU completions are
42.56 / 42.79 / 39.92 → 41.89 / 42.72 / 42.15 updates/s. Completion-gap p99
is 35.22–39.87 → 35.28–37.97 ms. Throughput is effectively unchanged, and both
builds miss the full 60 updates/s brush requirement.

A nominal one-second pinch during verified pending Gaussian Blur 85 work takes
1.033–1.671 s in the baseline and 1.026–1.031 s with the fix. Final input-queue
p95 remains 84–155 ms, and fresh filtered paint completes only 0.288–0.304
updates/s with 5.3–7.0 s completion-gap p99. Both builds miss filtered-paint and
response targets. These short interruption probes do not qualify sustained
navigation during filtering. Native witnesses and all raw values remain in
`artifacts/nav-fix-qualification-20261024/low-factory320/comparison.json`.

## Operations

### Artistic cell filters

| Filter at insertion defaults, painting into its input | Fresh filtered updates/s | 60/s target |
| --- | --- | --- |
| Dither Spread, Spread 16 px / cells 4 px / Density 100% | Unmeasured | Open |
| Scatter, Spread 20 px / cells 3 px / Density 100% | Unmeasured | Open |
| Crystallize, cells 16 px / Stretch 25% | Unmeasured | Open |

The portable native-pixel algorithms use fixed sampling counts. Their
[contracts and footprints](../reference/runtime-filters.md#dither-spread-scatter-and-crystallize)
apply at every tier. Correctness tests and desktop diagnostics do not qualify
fresh filtered completion rate, age, gaps or settling on this reference device.

### Dither cell sizes

| Cell size, 512 px brush | Completed updates/s on the tier canvas | 60/s target |
| --- | --- | --- |
| 2–50 px | Unmeasured | Open |

Existing Dither measurements use 1 px cells.

### Scatter cell sizes

| Cell size, 512 px brush | Completed updates/s on the tier canvas | 60/s target |
| --- | --- | --- |
| 1–50 px | Unmeasured | Open |

Scatter now uses stable staggered cells and retained stroke-start pickup.
Earlier Scatter measurements do not qualify this algorithm.


### Checked upload comparison

Measured 2026-10-25: `e038e3361` baseline, `c1b215667` checked dab/style upload
growth, then the baseline again. Optimized release/R8 builds use the reference
12 MP photo at Fit, factory density 320 and font scale 1, thermal status 0,
and three warmed five-second gestures per row. Dither paints Photo; the other
rows paint an empty layer above it. Completed canvas-update medians and p99
gaps describe GPU-completed moving work, not scanout.

| Row | Baseline / candidate / baseline, updates/s | Gap p99, ms | 60/s target |
| --- | --- | --- | --- |
| G-Pen 1024 | 51.33 / 50.54 / 51.25 | 29.28 / 28.99 / 29.36 | Not met |
| Spray 1024 | 51.46 / 50.52 / 51.26 | 55.72 / 57.29 / 55.91 | Not met |
| Dither Blender 512 | 32.72 / 31.91 / 32.44 | 76.54 / 76.60 / 75.81 | Not met |
| Wet Round 512 | 41.25 / 41.52 / 41.46 | 33.15 / 33.16 / 33.26 | Not met |
| G-Pen 1024, changing preview | 47.87 / 47.46 / 47.75 | 29.75 / 30.22 / 30.71 | Not met |

Additional candidate/baseline controls reverse the initial small G-Pen, Spray
and Dither throughput costs: 51.44/50.39, 50.97/50.17 and 32.54/32.03 updates/s.
These short controls establish no repeatable slowdown, without establishing
complete equivalence. The changing-preview fixture does not inject genuine
late sensor correction or whole-stroke replay; those rows remain unmeasured.
Exact APK/source provenance, per-contact results and settings are retained in
`/mnt/localssd/capycanvas9-stability-android-artifacts/ad5-reference/`.

Native full required-only startup remains unmeasured; selected dry-brush
preparation is measured under [dry media](#dry-shader-preparation).
Web diagnostics are recorded under
[shared shader preparation](#shared-shader-preparation). Earlier idle-catalog
measurements describe their named builds. See
[startup preparation](responsiveness.md#demand-driven-startup) for the Apple
diagnostic and its limits.

The BUILD32 G-Pen pair measures the M3 candidate captured on `192601dac`.
BUILD28 and selected BUILD20 canvas diagnostics measure frozen `4a2cf6aa0`
binaries. Earlier operation rows and 83/BUILD15 probes apply to their named
binaries. Each row retains its presentation and scope limits.

Overall M3 performance qualification remains pending. Matched release offscreen
runs on the desktop RTX PRO 6000 compare frozen `192601dac` with the authored
candidate. Navigation CPU p95 improves in every comparison, with unchanged
camera/work/cache counters and zero source misses or recomposition. Completed
p95 exceeds 5% in 7/15 cold and 8/15 warmed observations; warmed native-scale
excesses repeat in all three pairs (+0.024–0.178 ms). All p99 shifts stay below
+1 ms. These completion spans include host queue polling, not GPU timestamps.

The 100-repeat sixteen-layer bottom-paint ABBA comparison retains exact PNGs,
frame/work counters and 110,624,776 bytes of capture backing. CPU-submit bounds
pass both temporal pairs. The reverse pair exceeds completed-motion p95 by
7.09% (+0.041 ms), loses 6.23% throughput and adds 1.025 ms to pen-up p99;
the forward pair passes. These excesses remain unresolved and do not qualify
reference-tablet rates or physical input-to-present response. Exact observations
are under `artifacts/format/m3-uninstrumented-34/` and
`artifacts/format/m3-final192-ordinary-fixture/measurements/`. Earlier passing
`4a2cf6aa0` diagnostics remain in their own artifacts and do not override these
current-source results.

Shared codec diagnostic, 2026-10-04, desktop release builds: the committed
`b3f6f8e51` writer versus the manifest-compression follow-up on the same authored
input. A 2,048-fill metadata fixture shrinks from 2,951,109 to 461,626 bytes;
a real GTK recovery file shrinks from 5,417 to 3,570 bytes. Independent ZIP
reads preserve every decoded manifest byte, portable resource identity and
encoded resource range. Pixels and channels are not recompressed.

Seven metadata-only repetitions reduce preparation allocations from 913,050 to
646,741 and phase net heap growth from 82,273,847 to 45,559,412 bytes. Median
prepare/write/open times are 91.04/0.75/92.94 → 77.20/40.33/103.72 ms: compression
trades worker time for smaller files. The small GTK file uses
0.063/0.011/0.185 → 0.052/0.074/0.221 ms. These sequential codec diagnostics
include allocator instrumentation where enabled, exclude fsync and do not
qualify frame timing, total process memory or tablet latency. Full samples and
exact source/binary identities are under
`artifacts/format/m3-metadata-compression-44/`.

Decoded-source ownership diagnostic, 2026-09-29: the 12 MP photo transform
sequence retained 204 decoded tiles throughout after sharing the renderer's
source cache (APK `e91330b3a54e549712e60a89e5727ce31a5c604f1a6421fb5c2edcb119cae867`).
Before this fix, pixel translation retained 1,616 tiles and 2,098 MiB of total
GPU allocations; afterwards it retained 204 tiles and 686 MiB. The full sequence
completed with at least 2.2 GiB system memory available. Allocator and PSS sampling
make this a memory diagnostic, not frame-rate qualification. Records are under
`artifacts/latency-investigation/{current-geometry-4,fixed-geometry-memory-2}`.

The 2026-10-08 navigation rows use three 5 s gestures on the 12 MP photo,
actual SurfaceFlinger presentations, and thermal status 0. The brush/canvas
and matching gesture were warmed; optional full-catalog `shaders_ready` was
false. Source: `067f5cbb7` plus the uncommitted navigation candidate
(`navigation.rs` SHA-256 `2c0e9c5a7eee70c3e881ba9810a286cbe5a49ca1059d23a18a3892c8d1354d79`);
optimized APK SHA-256 `e74cd6b9b8f0dc15e1beb2f5e6ff6348bae20025ebc6e1f6adbd9dca7682d462`.

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Native startup and first-use shader preparation | [Latency limits](responsiveness.md) | Full startup and optional paper-lighting admission unmeasured; first dry selection Pencil 35.53 s, Pointy 35.04 s, Chalk 13.82 s after photo startup | [Dry preparation](#dry-shader-preparation); cold first use remains slow |
| Web startup, fresh origins | [Latency limits](responsiveness.md) | Brush readiness median 26.806 → 22.211 s; complete 29.360 → 24.785 s. First canvas essentially unchanged | [Shared shader preparation](#shared-shader-preparation); default-canvas latency diagnostic |
| Exact histogram, Web, 12 MP | [Latency limits](responsiveness.md) | First request 24.288 → 21.362 s; repeated median 14.717 → 12.103 s. Exact counts match | [Shared shader preparation](#shared-shader-preparation); latency only |
| Levels Auto, Web, 12 MP | [Latency limits](responsiveness.md) | Median 13.229 → 10.861 s; all controls and resulting exact histograms match | [Shared shader preparation](#shared-shader-preparation); latency only |
| G-Pen 220 px, Web shader diagnostic | 60 | **Unqualified; no measured gain.** CPU frame calls 21.79–27.24 → 21.97–26.10/s; GPU diagnostic p95 14.48–26.21 → 16.25–32.24 ms | [Shared shader preparation](#shared-shader-preparation); presentation unmeasured |
| Tool cursor hover, with and without brush size | 60 | Unmeasured on the reference tablet | [Top-tier rendering measurements](top-tier.md#tool-cursors) do not qualify this tier |
| G-Pen 1024 px, Android with idle brush preparation | 60 | **Meets these strokes.** Pending: 75.57–75.87 fresh updates/s, fresh completion-gap p99 22.64–24.01 ms; fully warm: 75.00–76.19/s, p99 22.50–24.20 ms | [Idle brush preparation](#idle-brush-preparation), Perceptual, 100 × 60 px radii |
| G-Pen 1024 px, Web while brush warmup is paused | 60 | **Unqualified; CPU proxy below target.** After the in-flight compile: 17.58–22.30 frame calls/s, interval p99 84.6–148.1 ms; fully warm: 21.36–23.31/s. Presentation unmeasured | [Idle brush preparation](#idle-brush-preparation) |
| G-Pen 1024 px, Web asynchronous GPU preparation | 60 | **Unqualified; CPU proxy below target.** 16.63–21.77 frame calls/s, interval p99 96.4–132.7 ms; input-to-submission p95 29.1–46.7 ms. GPU completion and presentation unmeasured | [GPU preparation](#gpu-preparation) |
| Pan during private session checkpoints | 60 | Unqualified: renderer 60.00–60.12 submissions/s, interval p99 18.70–18.92 ms; screen presentation and thermal status unmeasured | [Session checkpoints](#session-checkpoints) |
| G-Pen 1024 px during private session checkpoints | 60 | Unqualified: 77.6–79.0 fresh completed updates/s, completion gap p99 23.36–24.07 ms; thermal status unmeasured | [Session checkpoints](#session-checkpoints) |
| G-Pen 1024 px, repeated short contacts during checkpoints | 60 | Diagnostic only: 11.02–14.27 MiB process writes per 50 contacts; observed head age max 3.52–4.37 s. Paused motion has no qualified frame rate | [Session checkpoints](#session-checkpoints) |
| Pan: Hand tool, one or two fingers | 60 | **Met for two-finger pan.** 60.02 presented fps in all three contacts, 12 MP photo, Navigator open | [Navigation presentation](#navigation-presentation), 2026-10-25 |
| Pinch zoom | 60 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** 54.41 fps median, 12 MP photo, Navigator open; default paper 60.02 fps | [Navigation presentation](#navigation-presentation), 2026-10-25 |
| Mouse-wheel pan and Ctrl-wheel zoom, including held navigation buttons | 60 | Unmeasured on the reference tablet | [Wheel input contract](../ui/shared-ui.md); desktop correctness checks do not qualify this tier |
| Pan: Hand tool, one or two fingers, M3 BUILD20 | 60 | **Unqualified on current M3.** 59.078–59.729 completed updates/s; completion gap p99 18.821–19.033 ms; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison); older actual presents below |
| Pinch zoom, earlier retained-Navigator revision | 60 | Photo, Navigator open: screen 59.40 presents/s, p99 ≤16.83 ms; viewport 59.90 fresh completed updates/s | Retained-Navigator navigation below |
| Two-finger rotate | 60 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** 59.14 fps median, 12 MP photo, Navigator open; one gap p99 33.361 ms exceeds 33.333 ms | [Navigation presentation](#navigation-presentation), 2026-10-25 |
| Rotation snapping: two fingers and Rotate View | 60 | Unmeasured on the reference tablet | [Shared snapping behavior](../ui/shared-ui.md#drawing-and-navigation) |
| Smooth Zoom tool, left/right | 60 | **Current binary unmeasured; earlier 1.0.11 missed:** 43.33–43.82 presented fps, p99 99.95–99.96 ms, 12 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized APK `10e734af`, navigation source `87b79f29`; `artifacts/navigation-controls/zoom-audit-20261009/final/low/` |
| Smooth Zoom tool, up/down | 60 | **Current binary unmeasured; earlier 1.0.11 missed:** 43.43–43.56 presented fps, p99 99.96–116.21 ms, 12 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized APK `10e734af`, navigation source `87b79f29`; `artifacts/navigation-controls/zoom-audit-20261009/final/low/` |
| Footer zoom and rotation sliders | 60 | Unmeasured on the reference tablet with slider snapping enabled | [Shared slider behavior](../ui/shared-ui.md#drawing-and-navigation) |
| Navigator drag | 60 | | |
| Pan with Navigator open, checkerboard shader | 60 | Diagnostic: 59.71–59.91 renderer submissions/s; submission interval p99 18.81–19.55 ms. Presented FPS unmeasured; target unqualified | 2026-10-17, three warmed 5 s gestures, 12 MP tier photo, thermal 0; release APK `e38cf984`, base `5cee14be0` plus shared white/gray checker colors; `artifacts/checker-android/low/` |
| Brush-cursor hover | 60 | | |
| Placed-photo translation | 60 | Navigator closed: screen 59.3 presents/s, p99 16.9 ms; renderer 122.9 completed updates/s | Earlier direct-presentation comparison below, `photo-translate-drag` |
| Placed-photo corner resize | 60 | Navigator open: screen 59.4 presents/s, p99 ≤17.0 ms; renderer 104.2 fresh completed updates/s | Current drag comparison below |
| Pixel transform corner resize: Free | 60 | Navigator open: 60.022 presented frames/s, p99 16.87–16.89 ms | [Navigator-open transform qualification](#navigator-open-transform-qualification), 2026-10-10 |
| Pixel transform: Uniform, Skew or Rotate | 60 | | |
| Pixel transform translation | 60 | Navigator open: 60.022–60.023 presented frames/s, p99 16.70–16.87 ms | [Navigator-open transform qualification](#navigator-open-transform-qualification), 2026-10-10 |
| Pixel transform: Distort | 60 | Navigator open: steady 60.023 presented frames/s, p99 16.88–16.91 ms; first contact below target | [Navigator-open transform qualification](#navigator-open-transform-qualification), 2026-10-10 |
| Pixel transform: Perspective | 60 | | |
| Pixel transform: Warp | 60 | **Not met.** Navigator open: steady 55.94–56.78 presented frames/s, p99 33.35–33.38 ms; first contact 47.09/s | [Navigator-open transform qualification](#navigator-open-transform-qualification), 2026-10-10 |
| Crop corner drag | 60 | Navigator closed: screen 59.2 presents/s, p99 16.7 ms; renderer 130.1 completed updates/s | Earlier direct-presentation comparison below, `crop-handle-drag` |
| Pixel resize after placing the photo at 45% size | 60 | Navigator closed: screen 58.6 presents/s, p99 16.8 ms; renderer 224.6 completed updates/s | Earlier direct-presentation comparison below, `scaled-photo-pixels-handle-drag` |
| Selection translation, full canvas | 60 | Renderer 136–139 submissions/s; GPU interval p99 14.3–17.3 ms (6000 × 4000) | Canvas-bar `selection-handle-drag` and `selection-distort-drag`, 2026-09-27 |
| Move tool layer drag | 60 | | |
| Object Layer placement, Move and Scale/Rotate | 60 | Current Object-layer simplification unmeasured on the reference tablet | [Qualification gap](known-gaps.md) |
| Vector layer: G-Pen 24 px on 10,000 paths | 60 | **Met.** 83.8–84.1 completed updates/s, gap p99 27.1 ms | [Vector layers](#vector-layers), 2026-10-20 |
| Vector paths: Move drag, 1 / 100 / 5,000 paths | 60 | **Met.** 59.6–60.0 presented/s, p99 ≤16.9 ms, start 53–85 ms | [Vector layers](#vector-layers), 2026-10-20 |
| Vector paths: Scale/Rotate, Edit Points, eraser, Outline width scrub | 60 | Scale, Rotate of 100 paths, every eraser, Edit Points (59.8–60.0/s) and the 100-path width scrub (60.0) **met**; Rotate of 5,000 (55.4–55.6/s) and the 5,000-path width scrub (10.4–11.2) **not met** | [Vector layers](#vector-layers), 2026-10-20 |
| Navigation over a crowded vector layer | 60 | **Not met.** Pan 43.1–43.3, pinch 33.4–33.9 (5.8–28.6 with 5,000 paths selected), rotate 41.5–42.1 presented/s | [Vector layers](#vector-layers), 2026-10-20 |
| Move selected pixels: whole image | 60 | Screen 59.1 presents/s, p99 16.8 ms; renderer 81.0 completed updates/s | Two-page-refinement qualification below, `move-all-drag` |
| Move selected pixels: partial selection | 60 | Screen 59.4 presents/s, p99 16.8 ms; renderer 134.2 completed updates/s | Two-page-refinement qualification below, `move-part-drag` |
| Move selected pixels: Leave Copy | 60 | Screen 59.3 presents/s, p99 16.7 ms; renderer 131.1 completed updates/s | Two-page-refinement qualification below, `move-part-leave-copy-drag` |
| Marquee, Lasso or Polygon drag | 60 | | |
| Enclose and Fill: loop drag and navigation during completion | 60 | Unmeasured on the reference tablet | Shared lasso overlay; GPU region discovery starts on release. Desktop checks do not qualify this tier |
| Selection Brush or Quick Mask, 1024 px | 60 | | |
| Grow, Shrink or Feather drag, full canvas | 60, soft | | |
| Paper Scale drag with relief preview | 60 | Unmeasured on the reference tablet | Canvas-bar `paper-scale-drag`; [Paper rendering](../development/paper.md#rendering-and-performance) |
| Pointwise adjustment slider: Exposure | 60, soft | **Not met.** Current M3 12.131–32.686 completed updates/s; completion gap p99 63.347–281.970 ms; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison) |
| Pointwise chain: Levels, Vibrance, Exposure slider | 60, soft | **Not met.** Screen 57.3 presents/s, p99 33.3 ms; renderer 25.1 completed updates/s, with a 13.9–31.7 range | Pointwise graph comparison below |
| Threshold slider | 60, soft | **Not met.** Current M3 3.362–3.789 completed updates/s; completion gap p99 260.867–338.872 ms; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison) |
| Other pointwise adjustment sliders | 60, soft | | |
| Gaussian Blur slider, small radius | 60, soft | **Not met.** Screen 50.0 presents/s, p99 33.4 ms; renderer 23.5 completed updates/s | Two-page-refinement qualification below, Navigator open |
| Gaussian Blur slider, large radius | 60, soft | **Not met.** Screen 42.7 presents/s, p99 50.0 ms; renderer 15.5 completed updates/s | Two-page-refinement qualification below, Navigator open |
| Saved spatial lengths beyond editor bounds, including Gaussian sigma >85 | 60, soft | Unmeasured | Full authored lengths need moving-frame qualification; earlier capped-value measurements do not apply |
| Other neighbourhood filter sliders: Unsharp Mask, Edge-Preserving Smooth | 60, soft | | |
| Denoise Strength | 60, soft | **Not met.** 0.199 completed updates/s; whole-screen proxy 27.86–30.72 presents/s, p99 99.96 ms | [Fixed filter controls](#fixed-filter-controls), 2026-10-04 |
| Domain Warp Distance, animation frozen | 60, soft | **Not met.** 0.199 completed updates/s; whole-screen proxy 54.14–54.32 presents/s, p99 66.64–83.30 ms | [Fixed filter controls](#fixed-filter-controls), 2026-10-04 |
| Saved-selection overlay opacity | 60 | **Unqualified.** 7.78–21.71 completed updates/s; whole-screen proxy 59.21–59.62 presents/s, p99 16.85–33.33 ms; independent canvas presentation unmeasured | [Fixed filter controls](#fixed-filter-controls), 2026-10-04 |
| Animated or warping filter: Domain Warp, Ripple | 60, soft | | |
| Fill layer or gradient-fill edit | 60, soft | **Not met** for Solid Color opacity: screen 47.44 presents/s, p99 ≤49.98 ms; renderer 32.51 fresh completed updates/s. Gradient unmeasured | [Solid Color fills](#solid-color-fills) |
| Paper Texture visibility, Light and Shows through paint drag | 60 | Unmeasured on the reference tablet; a mid-tier-class XP-Pen updates a 12 MP canvas 59.8–69.2 times/s | [Paper lighting diagnostic](mid-tier.md#paper-lighting-diagnostic) |
| Navigation with Shadows/Highlights or Clarity | 60 | **Unqualified on current M3.** 52.398–59.826 completed updates/s; first contact below 57/s for both guides; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison) |
| Navigation with proof or tone guide shown | 60 | | |
| Gradient drag | 60 | | |
| Figure or ruler drag | 60 | | |
| Layer opacity scrub | 60 | Current native row popup cannot qualify: its owner disappears during the primer while shared state remains open. Earlier header slider 52.910–57.074 actual presents/s, value 54.629–56.217/s and popup 30.446–31.309/s are historical. Earlier Solid Color workload is separate | [Current Android control check](#android-ui-compositing-trials); [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28; [Solid Color fills](#solid-color-fills) |
| Layer reorder drag | 60 | Matched Android ABBA improves 44.779 → 50.431 actual presents/s; worst-contact gap p99 remains 49.983 ms. Target still missed | [Retained thumbnail projection](#android-retained-thumbnail-projection), 2026-10-28 |
| Layer row swipe | 60 | Android older native swipe diagnostics: left 56.217–57.428, right 56.401–58.120 actual presents/s; p99 up to 33.391 ms; short-primer limits apply | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28  |
| Workspace-manager row swipe and reorder | 60 | Unmeasured on reference hardware; functional input checks do not qualify moving-frame timing | [Workspace manager](../ui/default-workspaces.md#workspace-manager) |
| Layer thumbnail selection animation | 60 | Android instantaneous layer/mask selection: 14 observed state changes per contact; next-present observations only, no fresh-selection latency or sustained FPS | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28  |
| Attached filter drag inside a clipping run | 60 | Matched Android ABBA improves 38.049 → 45.246 actual presents/s; worst-contact gap p99 83.322 → 83.311 ms. Target still missed | [Retained thumbnail projection](#android-retained-thumbnail-projection), 2026-10-28 |
| Navigation with 8 visible paint layers | 60 | | |
| Drawing with 8 visible paint layers, G-Pen 1024 px | 60 | **Not met.** Navigator open, Fit: 50.12 fresh updates/s (48.65–51.65), completion gap p99 29.30–32.87 ms | Retained-Navigator painting below; seven photo layers and one drawing layer |
| Panel, tab, column or toolbar drag and docking | 60 | Android full-primer tab/panel contacts record 58.814–60.023 actual presents/s and 51.993–59.284 fresh bounds/s; column feedback is discrete. Other hosts retain their existing measurements | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| GTK Tool Set hover and press animation | 60 | Unmeasured on reference hardware; the short desktop GTK diagnostic does not qualify this tier | [GTK tool caption hover](top-tier.md#gtk-tool-caption-hover), 2026-10-26 |
| Panel or column resize | 60 | **Pre-existing gap; unqualified.** Android full-primer matrix: 35.289–59.334 actual presents/s and 34.146–54.651 fresh bounds/s; [current baseline](#android-ui-motion-baseline). Web: original 8.43–8.63 bounds updates/s; SVG 21.40–23.42 geometry updates/s alongside 21.11–23.14 total Chrome presents/s GTK compact Tool Set and expanding divider revisions remain unmeasured on this tier. Windows chrome rail scrolling and edge arbitration are unmeasured on reference hardware (2026-10-28). | [Native column bridges](#native-column-bridges); [Web SVG column bridges](#web-svg-column-bridges); [GTK Tool Set density](top-tier.md#gtk-tool-set-density), [GTK toolbar dividers](top-tier.md#gtk-toolbar-divider-resize), 2026-10-26 |
| Pen pressure utility and curve drag | 60 | Unmeasured on reference hardware | [Pen pressure measurement protocol](../ui/pen-pressure.md#validation); GTK desktop measurements do not qualify this tier |
| Android tool bubble drag through four neighbours | 60 | **Met.** Cached native shadows: 59.677–60.023 actual app-window presents/s; p99 16.864–17.009 ms, factory display settings, default glass, 12 MP photo at Fit | [Native tool bubbles](#native-tool-bubbles), 2026-10-28 |
| Web tool bubble drag through four neighbours | 60 | Unmeasured after the bubble release fixes; desktop browser journeys do not qualify the TCL reference tablet | [Web checks](../development/web.md#tests), 2026-10-24 |
| Windows tool bubble drag through four neighbours | 60 | Unmeasured on reference hardware; Surface Pro / Adreno X1-85 functional checks do not qualify timing (PresentMon ETW access denied) | [Windows tool bubbles](../development/windows.md#tool-bubbles) |
| Windows HDR color bubble drag and intensity scrub | 60 | Unmeasured on reference hardware; native input and pixel checks do not qualify presentation timing (PresentMon ETW access denied) | [Windows tool bubbles](../development/windows.md#tool-bubbles) |
| HDR color bubble drag and intensity scrub | 60 | Unmeasured on reference hardware; other tablets and workstation diagnostics do not qualify this tier; Apple reference and native overlay timing unmeasured; 61 MP Mac canvas diagnostic only | [Web and Android HDR color bubbles](top-tier.md#web-and-android-hdr-color-bubbles); [Apple HDR diagnostic](top-tier.md#apple-hdr-color-bubble-diagnostic), 2026-10-24 |
| Drawer open and close | 60 | Android Layers/Properties uses observed draw-clock transitions; 39 diagnostic windows, 85.619–284.824 ms. Earlier timestamp-join fault is superseded | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| Zen fade, expanded configuration and dialog page transitions | 60 | Android Zen and New Drawing open/Cancel have fresh diagnostic observations; expanded configuration and general page traversal remain unmeasured. Windows live-theme header replacement remains unmeasured | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28; [shared chrome](../ui/shared-ui.md#window-chrome-and-zen-mode) |
| Grouped tool menus, drawer switching and tile drag | 60 | Android native grouped menus and tile/drop hints have fresh diagnostic observations; general sibling switching remains unmeasured | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| Grouped Drawing drawer scrolling | 60 | Android actual presents 57.946–59.502/s, p99 16.985–33.315 ms; older short primer, diagnostic | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| Colour wheel or picker drag | 60 | Android full-primer committed wheel 46.570–47.742 actual presents/s, p99 33.555–50.004 ms; Edit Color 42.774–57.953/s, p99 33.410–50.098 ms; target missed | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| Slider and value scrub: size, opacity, flow | 60 | Android actual native slider/value contacts are recorded; canonical native batch passed; numeric controls remain below target | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| Toolbar Size/Strength: Show value and Expand to fit | 60 | Unmeasured on reference hardware; functional platform journeys do not qualify this tier | [Slider display checks](../ui/toolbar-components.md#checks), 2026-10-25 |
| Canvas action bar show, hide and move | 60 | Android actual photo-transform bar movement 5.692–7.210 presents/s; short show/hide observations remain diagnostic; canonical native batch passed | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 |
| Tool Options or panel content change | 60 | Android native Select/Drawing Brush content changes: 66 diagnostic windows, observation durations 74.920–147.237 ms; completed canonical successful batch | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28  |
| List scrolling: layers, brushes, filters | 60 | Android actual presents: Layers 59.502–59.850/s, Brushes 55.871–58.120/s, Filters 58.120–59.159/s; canonical native batch passed. GTK desktop diagnostic remains separate. Windows chrome rail scrolling and edge arbitration are unmeasured on reference hardware (2026-10-28). | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28 ; [GTK layer input diagnostic](top-tier.md#gtk-layer-input-diagnostic) |
| Workspace choices: horizontal scroll | 60 | Current factory display chooses compact vertical native menus: choices 59.849–60.023 actual presents/s; horizontal strip remains unmeasured by this matrix; short primer limits qualification | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28  |
| Workspace visibility checklist: vertical scroll | 60 | Factory compact native submenu: 60.021–60.023 actual presents/s, p99 up to 16.995 ms; short-primer diagnostic | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28  |
| Menu open and close | 60 | Android menu/submenu native transitions have fresh diagnostic observations; no sustained-FPS claim | [Android UI motion baseline](#android-ui-motion-baseline), 2026-10-28  |
| Interface language change | 60 | Current lifecycle binary unmeasured. Earlier German checkpoint: cold publication 169.2–195.5 ms; warm 144.4–194.4 ms; preparation-only maximum 4.095 ms | Matched Web language checkpoint below; no tier qualification |

## Native tool bubbles

The factory-settings comparison measured 2026-10-28 uses the TCL reference
with its physical 1200 × 1920 display, density 320 and font scale 1. The target
4248 × 2832 photo has one empty paint layer above it at Fit, with default glass.
Release Rust and unminified, nondebuggable benchmark APKs run under the same
private application ID. One six-second contact warms the native five-bubble
path, followed by three measured six-second contacts. All four neighbors
visibly move. SurfaceFlinger actual app-window presents exclude 100 ms at each
contact edge; refresh is 60 Hz and thermal status stays at 0.

| Build | Actual presents/s, three contacts | p99 interval, ms | Result |
| --- | --- | --- | --- |
| Control `0ff9b5fc3`, APK `7c6741c7` | 59.506 / 59.850 / 59.850 | 16.973615 / 16.770846 / 16.938461 | Met |
| Cached native shadows, APK `3a7bf7d7` | 59.677 / 60.023 / 60.020 | 16.864000 / 17.008923 / 17.006077 | Met |

Both pass the unchanged 57 presents/s floor and 33.3 ms p99 limit. The candidate
uses `fe47fa222` plus the Android shadow cache; it retains the same shadow
contours and gradients. The collector now records and checks factory display
settings and applies the actual rate and p99 limits with `--target-hz`.
Source/APK hashes, matched raw presentations, contact windows and settings are
in `artifacts/remaining-bubble-failures-20261028/`.

The native capture and edge comparisons below are diagnostic. Their saved
display dumps show a density override of 272 rather than the physical 320.
They were reclassified on 2026-10-28 and do not qualify tier performance.

The native capture recheck measured 2026-10-27 uses the same reference TCL,
4248 × 2832 photo, empty paint layer, Fit view and default glass. Both builds
use release Rust and unminified benchmark APKs under a private application ID.
One six-second contact warms the five-bubble path, followed by three measured
six-second native touch contacts. Actual SurfaceFlinger app-window presents
exclude 100 ms at each contact edge; all four neighbors visibly move. Display
refresh remains 60 Hz and thermal status remains 0.

| Build | Actual presents/s, three contacts | p99 interval, ms | Result |
| --- | --- | --- | --- |
| Baseline `cb7b5b35a` plus renderer lint edits, APK `7bc941ea` | 59.679 / 59.849 / 60.022 | 16.954154 / 16.780846 / 16.908923 | Diagnostic |
| Startup guard and native capture fix, APK `060b1d33` | 59.507 / 59.850 / 59.850 | 16.977846 / 16.948923 / 16.916538 | Diagnostic |

At the overridden density, both builds clear the rate and gap limits. The candidate
uses `43f581594` plus the Android native-card capture change. Frozen APK/test
hashes, the baseline renderer patch and candidate production patch, contact
windows and actual presents are retained in
`artifacts/baseline-fixes-20261027/android-low-motion-baseline/`,
`artifacts/baseline-fixes-20261027/android-low-motion-candidate-final/` and
the adjacent provenance files.

Measured 2026-10-27 on the TCL reference tablet with the 4248 × 2832 tier
photo, one empty paint layer, Fit view and default workspace glass. Both builds
use release Rust and the unminified benchmark APK under a private application
ID. `AndroidBubbleBenchmarkTest#fiveBubbleMotion` warms one six-second contact,
then drags one bubble through four visibly displaced neighbors in three
six-second native touch contacts. The collector uses actual SurfaceFlinger app
window presents and excludes 100 ms at each contact edge. Display refresh is
60 Hz and thermal status remains 0 throughout.

| Build | Actual presents/s, three contacts | p99 interval, ms | Result |
| --- | --- | --- | --- |
| `origin/main` `a4068913f`, APK `bfdb8897` | 59.679 / 59.850 / 59.845 | 16.960308 / 16.952231 / 17.015692 | Diagnostic |
| Edge dismissal, APK `8821464e` | 59.505 / 60.023 / 60.023 | 16.829308 / 16.821231 / 16.916539 | Diagnostic |

Both clear the rate and gap limits at the overridden density. Raw contact windows,
presentation timestamps, thermal records and APK hashes are retained in
`artifacts/bubble-edge/android-low-baseline/`,
`artifacts/bubble-edge/android-low-candidate/` and the adjacent provenance files.

## Layer action reveal

Measured 2026-10-24 on the reference TCL at `889e89b14` plus the fixed-width
Android action strip, using the release-profile benchmark APK. The 4248 × 2832
Sony photo has one empty paint layer above it at Fit, with the default workspace
and panel glass. `AndroidTitleBarTest#layerSwipeFrameTiming` uses native touch
input, a one-second warm-up and three five-second gestures per theme.
Each gesture oscillates 24–64 dp left from rest; full reveal and the release
animation remain unmeasured.

| Theme | Moving window frames/s, three runs | Vsync interval p99, three runs (ms) |
| --- | --- | --- |
| Light | 59.6 / 57.8 / 59.6 | 16.685 / 33.392 / 16.681 |
| Dark | 59.4 / 58.6 / 59.4 | 16.733 / 33.309 / 16.687 |

FrameMetrics samples count only changed row geometry. Rates divide those samples
by each five-second motion window; interval p99 uses nearest rank between
consecutive moving samples. All six runs have zero dropped metric callbacks and
thermal status 0 before and after each theme. The rates exceed the 57 fps floor,
but the second light run exceeds the 33.3 ms interval limit. These partial-swipe
measurements do not qualify the full gesture. Raw arrays, thermal readings and
captures are in `artifacts/layer-swipe-fixed/`.

## Drawer timestamp diagnostic

Measured 2026-10-24 on TCL TAB 11 Gen 2 with the 4248 × 2832 reference photo,
Fit zoom, default Low glass and thermal status 0 before and after. The benchmark
APK is `69fa6b0bd33458cd07e415832dcb8cafb2562b7b34ba0a4c5cde1228edb9d490`,
based on `98f562dd0` plus the interaction adapters. The source and test hashes
are retained in `artifacts/gtk-interaction-fixes/android-benchmark-diagnostic-artifact-manifest.json`.

After the existing warm-up, one five-second light-theme Layers/Properties run
records 33 native footer transitions, 261 FrameMetrics and 159 draw-geometry
samples. Only 105 frame timestamps match draw geometry exactly (40.2%, below
the harness's unchanged 95% requirement). Intended-vsync matching does not
recover the missing correspondence. The raw recording is written before the
assertion so this failure preserves its evidence.

No moving presentation rate is qualified from this run. The remaining repeats
and dark theme are not measured after the instrumentation failure. Zen fades,
expanded configuration and dialog page transitions also remain unqualified.
Raw samples and the diagnostic summary are under
`artifacts/gtk-interaction-fixes/`; newer source requires a new measurement.

## Touch navigation

Measured 2026-10-24 on the reference TCL with `dbdaab756`, using the benchmark
APK and optimized Rust `dev-perf` profile. Each motion has three warmed
five-second injected native two-finger gestures over the 4248 × 2832 Sony photo
and one empty paint layer, at 29.8008% Fit zoom. Navigator is open, the default
workspace has six glass regions, and the display uses its default 60 Hz setting.
Thermal status is zero at each motion's setup and after the final rotation.

`android-viewport-presents.py` joins canvas SurfaceView buffer numbers to
SurfaceFlinger actual-present times during each moving window. All three
motion rows above miss the rate target. Raw traces, gesture JSON, thermal data,
APK and source hashes are in `artifacts/scrub-navigation-20261024/`.

## Split transform programs

Measured 2026-10-09 PDT on the reference TCL, based on `ee4805dfe` with split
transform programs, optimized arm64 Rust and benchmark APK SHA-256
`c15ec9bfb7607041002b636939376c7bae8325589f0d19488061d61d53ffb6f2`.
The 4248 × 2832 Sony photo is rasterized into a paint layer. Blending is
Perceptual, the camera is at Fit, Navigator is closed, and thermal status is
zero before and after. Each steady row has three five-second OS-injected contacts.
Private Perfetto traces join SurfaceView buffer identities to SurfaceFlinger
display completions during the actual contact window; renderer submissions and
GPU completions are separate counters.

| Motion | Actual presented frames/s | Presentation-gap p99 | GPU completed updates/s |
| --- | ---: | ---: | ---: |
| Free translation | 60.022–60.023 | 16.70–16.87 ms | 148.95–149.48 |
| Distort, after first contact | 59.82–60.02 | 16.81–16.89 ms | 123.20–124.96 |
| Warp, after first contact | 59.82–60.02 | 16.74–16.93 ms | 62.78–63.63 |

These steady contacts meet the 60 Hz display-paced criteria. The first Distort
contact reaches 50.35 presented frames/s with a 233.25 ms p99 gap; the first
Warp contact reaches 55.17/s with a 33.39 ms p99 gap. Neither cold contact
qualifies. Transform entry reaches published state in 487–526 ms and drains
pending rendering in 511–554 ms. Free corner-resize setup failed before
measurement because its primer did not change geometry; its earlier row is
unchanged. These measurements do not qualify Web preview, selection transforms,
or physical input-to-photon latency.

Evidence: `artifacts/transform-root-cause/optimize/android-tcl/motion-summary.json`
and its per-contact reports and traces. Renderer telemetry records zero overview
views; these contacts qualify this layout only. The later raw-mask source
correction changes snapshot capture, not these preview programs.

### Navigator-open transform qualification

Measured 2026-10-10 PDT on the reference TCL, with the same 4248 × 2832 photo,
Perceptual blending, default Paint workspace and memory policy, Fit zoom
0.194999978, two visible layers and Navigator explicitly open. Stats and renderer
profiling are closed; thermal status is zero before and after. Presentation
uses the SurfaceView/SurfaceFlinger clock join described above, with three
five-second contacts per steady row.

| Motion | Actual presented frames/s | Presentation-gap p99 | Result |
| --- | ---: | ---: | --- |
| Free translation | 60.022–60.023 | 16.70–16.87 ms | Meets |
| Free corner resize | 60.022 | 16.87–16.89 ms | Meets |
| Distort, after first contact | 60.023 | 16.88–16.91 ms | Meets |
| Warp, after first contact | 55.94–56.78 | 33.35–33.38 ms | Misses |

The first Distort contact reaches 55.78/s, p99 33.33 ms. The first Warp contact
reaches 47.09/s, p99 266.62 ms. Neither is a qualifying contact. Warp's renderer
owner takes 6.84–7.17 ms at p50, versus about 3.4 ms for Free and Distort;
submit-to-completion spans include queue and polling and are not GPU timestamps.

An extra affine/projective split within mesh source adapters was tried and
removed: steady Warp remained 56.55–56.78/s with p99 33.34–33.38 ms. Photo,
camera, geometry, layout and thermal conditions matched; available-memory-based
admission budgets differed by approximately 2.3%. The result did not justify
additional cached variants. The retained implementation has one mesh family.

Free, Distort and retained Warp measurements use optimized APK SHA-256
`bdb570ad9061c7abbd7da92e8bb4f4b21a006eb7a77028a856f14aa62f0381ea`,
base `6354498396`. Resize uses experimental APK
`142e571f3e46b9f279d8cd7c6d73abe8b53fe64521c9346d6cb1e31141f2f7db`,
base `fededc259`, whose affine shader is unchanged by the discarded mesh split.
The corrected benchmark waits for Reset's original bounds to publish before
sampling its next corner. Earlier resize attempts used stale translated bounds
and did not qualify. No coordinate offset or timing workaround was added.

Evidence: `artifacts/transform-root-cause/optimize/android-tcl-rebased-final/performance/`
and `android-tcl-mesh-qualified/performance/`, including their motion summaries,
per-contact reports and raw traces; `android-tcl-mesh-qualified/warp-comparison.json`
records the discarded split. These rows do not qualify Web motion or physical
contact-to-photon latency.

### Web Apply

Measured 2026-10-10 PDT on TCL Chrome 154.0.8037.126, optimized `dev-perf` builds,
comparing baseline `dfbdb6ff7` with `44e0bb5ba` plus transform specialization.
The same saved 2048 × 1536 drawing has two authored Pencil tiles and no wetness;
tile fingerprints, descriptors, byte counts and the 30° Linear transform match.
Each build uses its own fresh origin and the same instrumentation. Driver shader
caches are uncontrolled. First Apply immediately follows the first angle edit,
so its wall time includes cold preview readiness and a fresh worker.

| Work | Before | After |
| --- | ---: | ---: |
| First Apply, cold preview and worker | 65.925 s | 5.378 s |
| Repeated Apply, same worker | 2.243 s | 0.890 s |
| First worker pipeline preparation | 42.443 s | 1.155 s |
| All worker mapped buffer calls / bytes | 340 / 25.82 MB | 19 / 1.32 MB |

The complete 31-case matrix covers affine, projective and mesh mapping with
Nearest, Linear, Bicubic and Lanczos, enlargement and shrinking, wet paint,
linked masks, retained geometry and Pencil acceptance. All pass exact history
and cancellation checks without blocking pipeline calls or GPU loss. The final
12-case follow-up covers affine area arithmetic and compiled-module sharing.
Free shrinking takes 2.126–3.822 s first and 1.013–1.062 s with its worker reused;
Distort/Bicubic shrinking takes 4.165 / 1.925 s and Warp 3.815 / 1.700 s.
Default-one masks retain full capture: mask-only Warp takes 5.837 / 4.600 s,
with normal worker retirement before that repeat. These are Apply diagnostics
on a 3 MP sparse drawing, not Web 12 MP moving-frame qualification or peak GPU
memory measurements.

Baseline Wasm SHA-256:
`38fa3ff549ccfaef8813c69b809dcd1034ef84672ea586ff8732362150c11b02`.
Final comparison Wasm SHA-256:
`d2f86c1fb95c6f761b3ed6bd8b702c54d17791f2f18374b12301939d04684ddd`.
Both use `dev-perf`; the candidate explicitly disables Cargo incremental builds,
while the old build manifest does not record that override. Evidence resides in
`artifacts/transform-root-cause/optimize/` with immutable builds, input witnesses,
raw traces and per-case summaries.

## Shared shader preparation

Measured 2026-10-10 on TCL TAB 11 Gen 2, Chrome 154, hardware Mali-G52 WebGPU,
optimized `dev-perf` builds. Baseline `c22937b89` is compared with the shader
candidate on that base. Every accepted reload verifies the actual served Wasm
hash with browser network caching disabled. Only owned origins and document
state are used; driver shader caches are uncontrolled.

The photo is 4248 × 2832 (12,030,336 pixels). Exact histogram requests keep the
waveform hidden. Levels Auto waits for the accepted controls and a stable exact
histogram. The guide comparison edits Shadows/Highlights or Dehaze through
35%, 0%, 35%, waits for shared Properties to finish updating, then waits for
pipeline readiness and the GPU queue. Exact histogram capture is timed
separately. The camera and work area match between builds.

| Work | Before | After | Scope |
| --- | ---: | ---: | --- |
| First canvas, fresh-origin startup | 7.717 s | 7.775 s | Median of three; essentially unchanged |
| Document readiness, fresh-origin startup | 14.230 s | 14.069 s | Median of three; essentially unchanged |
| Brush readiness, fresh-origin startup | 26.806 s | 22.211 s | Median of three; 17.1% less time |
| Complete startup, fresh origins | 29.360 s | 24.785 s | Median of three; 15.6% less time |
| First Gaussian insertion, 512² | 7.516 s | 5.586 s | Median of three fresh-origin pairs; 25.7% less time |
| Repeated Gaussian insertion, 512² | 306.5 ms | 295.6 ms | Median of three; 3.6% less time, candidate range 295–520 ms |
| Exact histogram, first request | 24.288 s | 21.362 s | One cold-worker request |
| Exact histogram, repeated | 14.717 s | 12.103 s | Median of two further requests |
| Levels Auto | 13.229 s | 10.861 s | Median of three; identical controls and histograms |
| Shadows/Highlights edit | 664.4 ms | 670.2 ms | Median of three; no meaningful response improvement |
| Exact histogram after Shadows/Highlights | 20.487 s | 13.249 s | Median of three; identical full counts and displayed artwork crops |
| Dehaze edit | 676.5 ms | 689.5 ms | Median of three; no meaningful response improvement |
| Exact histogram after Dehaze | 18.597 s | 13.211 s | Median of three; identical full counts and displayed artwork crops |
| Gaussian Blur edit, 12 MP displayed output | 485.4 ms | 442.5 ms | Median of six; matching artwork crops and final histogram, background work remains pending |
| Clone → Healing → Spot Healing readiness | 7.975 / 0.154 / 0.164 s | 7.240 / 0.823 / 0.670 s | One ordered sequence; baseline prepares healing during Clone |
| Motion Blur edit, 1536² diagnostic | 433.9 ms | 427.1 ms | Median of six in-page edits; no material gain |
| Unsharp Mask edit, 1536² diagnostic | 486.5 ms | 417.0 ms | Median of six in-page edits; 14.3% less time |
| Bloom edit, 1536² diagnostic | 843.2 ms | 746.6 ms | Median of six in-page edits; 11.4% less time |

Mode-specific retouch preparation reduces the first Clone wait by 9.2%, but
defers compilation to the first Healing and Spot Healing selections. Total
sequence time increases from 8.294 to 8.732 s in this observation. This does not
establish a universal first-use improvement. The procedural field cache is
excluded: matched small-canvas response gains do not justify its extra
compilation and memory.

The startup and first-use comparison alternates three pairs of fresh origins,
with the default 2048 × 1536 drawing before inserting Gaussian on a 512² fixture.
No recovery dialogs or resumed-document counter resets occur; the first startup
marks define readiness. All six resulting histograms and artwork crops match.
Historical sub-second first Gaussian observations use different cache/workflow
state and are excluded from this first-use comparison. Fresh origins clear
application state, while driver caches remain uncontrolled.

The painting diagnostic uses three 6 s CDP pen replays on the same 12 MP photo,
220 px G-Pen, zoom 0.195 and a 920.4 px work area, with Stats GPU timing enabled.
CPU frame-call rates are 21.79 / 26.24 / 27.24 → 21.97 / 24.74 / 26.10 Hz.
GPU timestamp p50 is 8.13 / 6.55 / 6.23 → 9.18 / 7.08 / 6.55 ms; p95 is
26.21 / 14.48 / 14.68 → 32.24 / 18.09 / 16.25 ms. The camera and brush match,
and the device reports thermal status zero. These observations show no sustained
painting benefit and include diagnostic overhead.

These are application latency and correctness diagnostics. No physical
input-to-present or SurfaceFlinger presentation measurements qualify the tier's
60 fps target. A strict whole-engine retirement endpoint can stall for 12 MP
Gaussian edits on both builds; it includes background cache and Navigator work.
Such observations are excluded from isolated filter latency results. The Gaussian
row measures displayed output and queue completion while separately recording
pending background work; it does not establish whole-engine retirement. Protocol-
created Android browser targets that fail to evaluate plain JavaScript are also
excluded; accepted runs use foreground native Chrome tabs.

Baseline Wasm SHA-256:
`21024c43265020ed114555e12c76055136c41bf887349a0e862de6c2ab47fafb`.
Candidate Wasm SHA-256:
`4e2290f6f9f183e318ff269cc93c66116297a4b06fb99a9ae7d53eedacee0bb8`.
Raw samples, immutable builds, pixel comparisons and source hashes are under
`artifacts/shader-optimization/`; `implementation-audit.tsv` records every shader
and builtin filter conclusion.

## Idle brush preparation

Measured on the reference TCL on 2026-10-07–08 with the 4248 × 2832 photo,
Perceptual blending, Fit, G-Pen 1024 px, default workspace and Navigator,
Stats closed and thermal status zero. The shared compiler prepares the selected
brush first, then admits one speculative job at a time after one second without
input and while the canvas engine has no continuous work. An admitted driver
call cannot be canceled.

Android uses an optimized release APK with R8, OS-injected 240 Hz stylus input,
pressure 1 and a 100 × 60 physical-pixel radius path. The selected build's
qualification consists of three warmed five-second strokes after an undone
primer, with tracing disabled. The baseline has three such batches in separate
GPU lifetimes, after the representative tool-preparation probes.

| Android build | Fresh completed updates/s, median (range) | Canvas completed updates/s, median | Fresh completion-gap p99, range |
| --- | ---: | ---: | ---: |
| Before idle preparation | 74.87 (70.10–76.60) | 74.87 | 22.15–27.77 ms |
| Selected build, catalogue pending | 75.72 (75.57–75.87) | 75.72 | 22.64–24.01 ms |
| Selected build, fully warm | 75.05 (75.00–76.19) | 75.05 | 22.50–24.20 ms |

Both selected workloads meet the 60 completed-updates/s and 33.3 ms p99 limits.
The matched pair shows no measurable penalty with preparation pending; the
difference from baseline is also within the observed run spread. The pending run
enters with an incomplete catalogue after 500 ms idle; the warm control waits
for completion before selecting the brush. Neither uses tool-preparation probes or
tracing. This qualifies these G-Pen strokes, not every brush, navigation or
physical pen latency. Native pauses between contacts are about four seconds,
unlike the short Web gaps below.

Nine further five-second strokes after immediate tool-selection probes reach
71.94–76.79 fresh updates/s, median 75.30, with completion-gap p99
22.79–30.64 ms. All nine meet the stroke limits. Required compilation renews
the quiet interval before the compiler can resume speculative work, preserving
the newly ready tool's opportunity to receive input.

At those nine stroke ends, median allocated/reserved GPU resources remain
793.49/813.40 MB in both baseline and selected builds. Median process PSS rises
from 1,221,378 to 1,318,024 KiB, an observed 94.4 MiB increase. These sequences
use the same photo, primer, Undo and path; the selected build also repeats tool
selection and still has optional work pending. This does not isolate shader
retention from ongoing compilation.

The selected build's probe-free controls retain 792.3–793.5 MB allocated GPU
resources while preparation is pending and 794.1 MB when fully warm, with
813.4 MB reserved in both cases. Their three stroke-end PSS snapshots are
1,338,555–1,358,081 KiB pending and 1,188,284–1,208,462 KiB fully warm.
These are retained observations, not sampled peaks. GPU accounting and PSS
overlap and must not be added.

Chrome 154 uses hardware WebGPU on ARM Bifrost, a visible focused tab and the
`web-release` build. Each comparison opens a fresh photo and undoes a 1.5-second
primer. CDP injects nominal 200 Hz pen input with varying pressure over a
266 × 173 physical-pixel radius path. Each stroke lasts five seconds; actual
gaps are about 600–700 ms. These are CPU frame calls and event-to-submission
latencies, not fresh GPU completions, screen presents or physical pen latency.

| Web condition | Frame calls/s, each stroke | Input-to-submission p95, each stroke | Frame-call interval p99, each stroke |
| --- | --- | --- | --- |
| Baseline, fully warmed | 20.98 / 22.56 / 22.57 | 40.7 / 27.1 / 29.0 ms | 99.8 / 77.8 / 78.8 ms |
| Selected build, fully warmed | 21.36 / 23.12 / 23.31 | 26.8 / 27.0 / 28.8 ms | 88.0 / 109.1 / 78.2 ms |
| Selected build, start during a speculative compile | 17.38 / 21.75 / 17.58 / 21.96 / 22.30 | 36.7 / 28.3 / 28.6 / 28.2 / 29.9 ms | 116.8 / 87.2 / 148.1 / 84.6 / 102.9 ms |

The first stroke overlaps a 4.750-second material-pipeline compilation and is
24.9% below the fully warm median. Its generic pipeline label does not identify
the brush family. The following four contacts have a median 21.85 calls/s versus
23.12 fully warmed, a remaining 5.5% difference. During the third contact, a
separate document/writeback pipeline batch finishes; its largest frame-call gap
is 570.7 ms and starts after the recorded asynchronous calls have finished.
That timing does not establish the stall's cause. No further asynchronous
pipeline starts during the final two contacts. All five strokes are retained
above, and both runs report no errors. This comparison does not establish zero
contention or qualify the 60 fps target.

With a 200 ms admission interval, speculative compilation restarted in
each gap: three matched primed strokes measured 18.57–19.54 calls/s and
35.7–40.1 ms input-to-submission p95, versus 22.25–23.57 calls/s and
26.7–33.5 ms after full warmup. The one-second interval prevents that repeated
overlap while retaining required-shader priority.

The baseline source is `fe15a1c42`; the selected build adds idle brush preparation
on `9f9342d91`. Its queue prepares common brushes, then transforms, then specialty
brushes. Baseline APK SHA-256:
`f9613708eb5a07790bdfc84f201d922234475af8c3efc920c9d1a60439e02fd1`.
Selected APK SHA-256:
`aaa3da579210b916ec1fe2b04e208fa8ce4fd05707a0c59bbb2073c304741ceb`.
Native raw records are in `artifacts/shader-warmup/baseline/android/probes-0-{0,1,2}`
and `candidate5/android/{pending-untraced,warm-untraced,probes-0-{0,1,2}}` under
the same artifact root. Baseline Web Wasm SHA-256:
`f2afeaea8117fb305abf468a16788b4a1b213d82c68d4ad441909b033d3c84fb`.
Selected Web Wasm SHA-256, verified from the loaded response bytes:
`26c9c5e16719c77eb7ad98ee25981b43a1621b14228922a44f294d264d47e914`.
Raw samples and source patches are under
`artifacts/shader-warmup/baseline/web-warm-primed` and
`candidate5/web-{pending,warm}-primed` under that root.
Earlier admission experiments and harness deadline failures are retained
separately in `candidate{,2,3,4}`. Startup and tool-readiness measurements
are in [Responsiveness](responsiveness.md#idle-brush-preparation).

## Fixed filter controls

Measured 2026-10-04 on the reference TCL tablet with the 4248 × 2832 Sony photo
at Fit, in a release Rust benchmark APK, with three warmed five-second scrubs
per control. Denoise uses a fixed two-pixel neighborhood; Domain Warp uses three
noise octaves and a frozen animation phase. Thermal status was 0 before and
after both builds.

The baseline is `7fcca04e5`, with only the benchmark harness added. The candidate
is the same commit plus the artwork-contract changes, source patch SHA-256
`fbe1e761a0b3b6f5912a58f142aa94ba0533c5b1e2033b3180335b212c8beda2`.
Baseline and candidate APK SHA-256 values are
`571fe6a9150efe14fc27c3dc877b0a868304896f944e0126d8b0335a51396398` and
`c375c0760ad0ded0d328adc3acfcee463ba821be03c4d064ad9d4c9a457811a6`.

| Control | Baseline screen presents/s | Candidate screen presents/s | Baseline / candidate screen interval p99 |
| --- | --- | --- | --- |
| Denoise Strength | 30.21 / 31.55 / 31.13 | 29.50 / 30.92 / 31.44 | 99.88–116.63 / 83.44–99.98 ms |
| Domain Warp Distance | 52.90 / 52.52 / 52.47 | 53.72 / 53.35 / 54.10 | 66.66–83.29 / 66.66–66.81 ms |

Both builds record only one completed renderer update per contact, or
0.198–0.199 updates/s; there are too few completions for a gap p99. The screen
measurements include native controls and do not establish independent canvas
presentation rates: the retained canvas SurfaceFlinger layer reports one event
per contact. Neither build meets the target. This comparison finds no measurable
regression from fixing the kernels; it does not establish a hardware-limit waiver.
Source manifests, APK identities, thermal records and individual samples are in
`artifacts/format-stabilization/android/`, including `baseline/report.json` and
`filter-final-report.json`.

The final production changes were measured again after integrating `bd6532ada`,
with source patch SHA-256
`fa400bde6693b2e3b72c774095c10b90e07dfb488ae38f8bc9c772b64ce00991`,
APK SHA-256
`92e7818f24dccfcfd7d66447b7e39fd4bd4cd177d12ee17b12c1bbc45357b867`
and test APK SHA-256
`3b0ba43ec2b1d1bac03872c5efa57c5ad900fa137f0a71537ba3c621f51b54c6`.
The same three warmed five-second scrubs used the dark theme; light-theme
scrubs passed separately. Thermal status stayed 0, CPU/GPU temperature rose
from 51.1 to 57.6 °C and skin temperature from 31 to 38 °C.

| Control | Completed updates/s | Completion gap p99 | Whole-screen presents/s | Screen interval p99 |
| --- | --- | --- | --- | --- |
| Denoise Strength | 0.199 / 0.199 / 0.199 | Too few completions | 27.86 / 30.32 / 30.72 | 99.96 / 99.96 / 99.96 ms |
| Domain Warp Distance | 0.199 / 0.199 / 0.199 | Too few completions | 54.32 / 54.16 / 54.14 | 66.64 / 83.30 / 66.65 ms |
| Saved-selection overlay opacity | 13.13 / 7.78 / 21.71 | 231.20 / 335.53 / 71.16 ms | 59.21 / 59.62 / 59.41 | 33.33 / 16.85 / 33.31 ms |

Overlay scrubs changed 71, 96 and 54 opacity values while preserving the authored
checkpoint and modified state. Completion counters are diagnostic; they do not
prove independently presented canvas frames or stale pixels. The whole-screen
and native-window traces cannot qualify canvas motion. Native UI presentation
was 59.36–59.44 Hz with p99 35.72–38.75 ms, exceeding the 33.33 ms limit.
The overlay remains unqualified, and neither filter meets its target. Mid and
top tiers remain unmeasured for this change. Raw traces, thermal records and
individual samples are under
`artifacts/format-stabilization/android/integrated-perf/`; its `report.json`
contains the exact values.

## Layer attachment qualification

The current attachment implementation is unmeasured on the TCL reference
tablet. No-effects painting, ordinary clipping and painting above an attached
Gaussian remain unqualified on this tier. The reference tablet was reserved
by another session during the matched mid-tier comparison.

The frozen pre-attachment source, APKs and baseline records remain under
`artifacts/layer-attachment-baseline-a918057/`. They do not qualify the current
implementation; its Gaussian slider also differs in geometry and sampled values.

## Solid Color fills

Measured on 2026-10-03 on the TCL reference tablet, thermal status 0 before
and after each scenario, default 60 Hz display settings and panel glass.
The benchmark uses release Rust, the 4248 × 2832 Sony downscale at Fit,
default Photo workspace and open Navigator. Each scenario has a priming gesture
undone before three native input gestures: six seconds for opacity and five
seconds for navigation and painting. Painting uses Perceptual blending,
G-Pen 1024 px, default prediction and a 480 × 280 px stylus ellipse.

| Motion | Fresh completed updates/s, median (range) | Completion gap p99, range | Screen presents/s, median (range) | Screen gap p99, range | Status |
| --- | ---: | ---: | ---: | ---: | --- |
| Maskless Solid Color opacity over photo | 32.51 (31.54–36.56) | 58.96–103.79 ms | 47.44 (46.16–49.97) | 33.41–49.98 ms | **Not met** |
| Two-finger pan | 59.71 (59.69–59.79) | 18.72–19.31 ms | 59.31 (59.29–59.40) | 16.67–16.70 ms | Met |
| Pinch | 59.83 (59.74–59.91) | 19.21–20.43 ms | 59.43 (59.35–59.51) | 16.68–16.86 ms | Met |
| G-Pen 1024 px | 95.77 (89.67–96.53) | 23.43–27.60 ms | 59.28 (58.46–59.39) | 16.72–17.03 ms | Meets measured stroke criteria |

Navigation and painting place one empty paint layer above the photo and the
new ordinary white fill named Paper. Opacity inserts a maskless Solid Color
above the photo and scrubs its layer control; observed values span 22.1–71.8%.
Screen rates come from distinct SurfaceFlinger display actual-present timestamps
inside the moving windows. Android shared-demand presentation does not emit a
canvas surface frame for every update, so these are screen rates rather than
independently matched canvas presents. Renderer rates count nonempty GPU
completions inside motion; painting additionally requires distinct consumed
real input. This qualifies the measured motions, not every brush diameter or
resumed-contact latency.

The matched opacity baseline at `722eeca2d` uses the same measurement fixture
and control locator. Its fresh rate is 28.38 updates/s (26.22–34.57), completion
gap p99 66.92–80.68 ms, and screen rate 45.31 presents/s (42.72–49.61), screen
gap p99 33.40–50.00 ms. Both revisions miss the opacity target. The higher
candidate median does not establish a general improvement; its worst completion
gap is longer. There is no soft-target waiver.

Candidate APK SHA-256:
`3f96c2549631b0f3c4f240a0ca332d0ff787a701c36118b77537f89225a61904`.
Baseline APK SHA-256:
`acaf4c0e93730a523908ae39a46d96698002a5e967a4a058e8487d5a59cab1936`.
The candidate is `722eeca2d` plus the Paper-to-Solid-Color refactor.
Raw traces, gestures, fixture data, thermal records and binary hashes are in
`artifacts/android-fill-measured/` and `artifacts/android-fill-baseline/`;
`artifacts/analyze-fill.py` and `artifacts/analyze-fill-baseline.py` reproduce
the tables. Mid and top tiers were not measured for this renderer revision.

## Grouped tool drawer scrolling

Measured on the TCL reference tablet on 2026-10-10 UTC, thermal status 0 before and
after, 60 Hz default display settings. The 4248 × 2832 Sony photo has one empty
paint layer at Fit (25.5508%), default Photo panels and glass. The grouped Drawing
drawer receives one priming gesture and three six-second native finger gestures.
Actual moving windows span 5.964–5.981 seconds.

| Run | Moving UI FrameMetrics, Hz | Interval p99, ms | Moving draws / matched distinct vsyncs | Maximum draw-to-vsync lag, ms |
| --- | ---: | ---: | ---: | ---: |
| 1 | 60.02 | 16.739 | 359 / 359 | 13.84 |
| 2 | 58.85 | 33.328 | 353 / 353 | 12.37 |
| 3 | 58.85 | 33.319 | 352 / 352 | 8.70 |

Only draws with changed scroll offsets count. Their monotonic timestamps match
the nearest preceding `FrameMetrics` vsync; duplicate callbacks count once.
A 200 ms callback drain retains late metrics without extending the motion window.
All draws match within one frame, with zero lost callbacks or unmatched draws.
These UI FrameMetrics meet the 57 Hz floor and 33.333 ms p99 limit. No
SurfaceFlinger trace was collected; drawer opening, sibling switching, menus,
tile dragging and other tiers remain unqualified.

The unminified benchmark uses release Rust from `ee4805dfe` plus the sidebar refactor.
App APK SHA-256: `ea695ce0fdb968097e43e83b84b0cb68bd72a54ce0cae5aa25405df00f7d00a4`.
The Android/shared diff SHA-256 is `1e5e883bfc5a4176dac853b6350d454d7fea80ac973056fba4de030a62139547`.
Raw gestures, thermal records, photo/test hashes and the report are retained in
`artifacts/sidebar-widths/grouped-drawer-benchmark/`.

## Live paint icon diagnostic

Measured on the TCL reference tablet on 2026-10-03, thermal status 0 and default
glass, with a blank 2048 × 1536 drawing. Three warmed five-second committed-wheel
gestures per input update the visible paint icon. Both APKs use the same release
Rust library, including the shared paint-pair view; the comparison changes the
Android icon and compact-control presentation.

| Input | Baseline UI Hz range / median | Updated UI Hz range / median | Maximum interval p99, baseline → updated |
| --- | --- | --- | --- |
| Mouse | 49.01–50.23 / 49.60 | 48.24–51.16 / 51.03 | 49.97 → 49.98 ms |
| Touch | 53.34–55.07 / 54.17 | 52.80–54.39 / 53.66 | 33.35 → 33.38 ms |

Both builds miss the 57 Hz floor in this diagnostic. These are distinct intended
UI vsyncs observed through FrameMetrics, with changing paint values checked during
draws. They do not establish actual presentation or qualify the 12 MP photo
workload. Reference-canvas qualification remains outstanding.

The APKs precede the unrelated About dedication integration onto `f38ebc4f1`.
Baseline APK SHA-256 is
`eb5fde3d8cfc839478575a131dc589e2e6df94a9e6d5609de75f1fcc23ced566`;
updated APK SHA-256 is
`9a797e40e91e000da15aa2c610b375c55fdf7fceff564ea43d14a6bd117078a9`.
Raw samples, thermal records, APKs and provenance are in
`artifacts/paint-pair-android/{baseline,final-pre-rebase}/` and the adjacent
`baseline-motion.txt` and `candidate-motion-pre-rebase.txt` logs.

## Workspace switcher scrolling

Measured on the TCL reference tablet on 2026-10-03, thermal status 0 before and
after, 60 Hz default display settings. The 4248 × 2832 Sony photo has one empty
paint layer at Fit (15.9703%), default Paint panels and glass. The fixture uses a
small, left-aligned switcher-only header and 30 custom long names plus the three
included workspaces. Each surface has one priming gesture and three five-second
native touch gestures.

Both measured surfaces meet the 57 presents/s floor and 33.3 ms p99 limit.
Every moving draw sample matches an actual app or popup frame, whose display
token joins the SurfaceFlinger actual-present timestamp. Canvas surfaces and
unchanged draws are excluded. These results qualify this long-list fixture;
other header configurations, opening latency and other tiers remain unmeasured.

The unminified benchmark variant uses release Rust from `f224ef05e` plus Android
patch SHA-256 `5a0abb5b8f59fbef43f7d2dc4fcd09be2dd4fdb9cb46cc053911f369a69fa417`.
App APK SHA-256:
`b58c13f842eb77fd21d72acbff4c4a0b19728d57c563f79c4c9d84d14798803e`.
Raw gestures, trace, analysis, source patch, APKs and provenance are retained in
`artifacts/workspace-switcher-validation/android-switcher-benchmark/`.

## Language preparation

Measured on the TCL TAB 11 Gen 2 on 2026-10-02 with Release Wasm from
`19a086a37`, Chrome 154 and a cross-origin-isolated timer. Three fresh modules
per language cover all five catalogs and verify all 2,904 static labels.
The comparison replaces whole-resource parsing with complete Fluent groups
bounded to 32 entries and 4096 bytes.

| Preparation interval | Whole resources, maximum | Bounded groups, maximum |
| --- | ---: | ---: |
| One step, English already initialized | 2.920 ms | 2.020 ms |
| One step, fresh module | 5.325 ms | 4.190 ms |
| Production Web 2 ms deadline loop, English initialized | Unmeasured | 3.140 ms |

Web startup initializes English before a live switch. The fresh-module maximum
is the first parser/JIT invocation; other fresh-module runs stay at or below
1.990 ms. Total isolated preparation cost does not materially improve: foreign
catalogs take approximately 26–35 ms before grouping and 30–34 ms afterwards.
The smaller indivisible parse work allows more frequent deadline checks.

These elapsed intervals include scheduling and JIT. The batch probe runs the
production Rust deadline loop, excluding publication, view generation and
JavaScript serialization. It has no canvas workload and establishes neither
UI frame rate nor input-to-paint latency. Raw results and source/catalog hashes
are under `artifacts/localization-live-switching/production-preparation-*`;
the comparison and limits are in `preparation-quantum.local.md` there.

## Fifteen-language diagnostic

Measured 2026-10-03 on the TCL reference tablet with release Wasm based on
`155c7e1c0`, Chrome 154 and English initialized. Each of the ten added languages
was prepared once, verifying all 2,972 static labels; German was then reused.

| Interval | Measured |
| --- | ---: |
| Ten cold preparations, summed calls | 18.240–27.400 ms |
| Maximum isolated production batch | 2.355 ms |
| Cached German preparation | 0.030 ms |
| Stock cached English publication / first associated GPU completion | 293.590 / 255.655 ms |
| Candidate cold German publication / first associated GPU completion | 190.340 / 354.650 ms |
| Final preparation call plus shared publication, stock / candidate | 7.220 / 12.805 ms |

Preparation totals exclude waits between tasks and module fetch/instantiation.
Each publication sample precedes one five-second CDP pen motion on the 12 MP
photo. Publication includes host apply; GPU completion is associated with input
at the JavaScript boundary, not physical ink or scanout. The final preparation
call also publishes shared state; its duration is not an isolated parsing batch.
Different language/cache states and single samples establish no improvement or
frame-target result.
Desktop native preparation was 3.010–4.329 ms, a supporting diagnostic rather
than tier hardware. [Raw records and exact binary/source hashes](../../artifacts/localization-expansion/preparation/current155/focused-summary.local.json)
and the [unchanged build/deployment guard](../../artifacts/localization-expansion/preparation/current155/final-focused-sourceguard.local.json)
identify this minimal candidate; earlier broad measurements are excluded.

## Matched Web language checkpoint

Measured on 2026-10-03 on the TCL reference tablet, Chrome 154, release Wasm,
with a 4248 × 2832 photo, one empty paint layer above it and Paper, G-Pen
1024 px, Fit 15.9703%, Navigator open and default glass. The viewport is
1920 × 996 physical pixels, 1129 × 586 CSS pixels. Three fresh app modules per
build each perform one cold and two cached German switches after restoring
English. Camera, workspace, brush, settings, color, layers and contained pen
trajectory match. Thermal status was zero before and after the measured sequence.

| Interval | Baseline | Measured `6318d9c7` checkpoint |
| --- | ---: | ---: |
| Preparation-only call maximum across three cold switches | 3.100 ms | 4.095 ms |
| Cold preparation, sum of calls | 28.34–30.12 ms | 30.91–31.64 ms |
| Cold final call including shared publication | 10.66–12.90 ms | 9.75–11.04 ms |
| Cold host publication | 189.18–202.37 ms | 159.45–184.72 ms |
| Cold total publication | 201.74–215.28 ms | 169.19–195.49 ms |
| Cached total publication | 182.88–230.68 ms | 144.35–194.36 ms |
| Cold first associated GPU completion after resumed pen input | 320.08–346.99 ms | 421.57–557.21 ms |
| Cached first associated GPU completion after resumed pen input | 191.77–380.96 ms | 216.88–366.50 ms |

Cached switches have no preparation-only calls. The final preparation call
publishes shared state and is excluded from parsing maxima. The measured cold
maxima are 2.710, 2.950 and 4.095 ms; these elapsed intervals do not establish
the CPU budget. Publication is faster in this sequence, while cold resumed GPU
association is slower. No general responsiveness improvement is established.
Five-second CDP pen motions after publication reach 16.35–17.66 completed
updates/s cold and 18.95–21.32/s cached. GPU callbacks pair with input at the
JavaScript boundary; they do not measure consumed ink, physical latency or
canvas scanout and do not qualify the tier target.

A separate instrumented checkpoint trace attributes 52.1 ms to style updates
and 26.4 ms to layout within a 213.5 ms cold host publication, with no GC or
compilation span in that window. A copy-only experiment removing repeated tab
and proof refresh did not improve cached timings consistently, so production
preparation scheduling remains unchanged. Trace timings are diagnostic and
are not pooled with the matched runs.

Baseline is `0f6b5b708`; measured checkpoint Wasm SHA-256 is
`6318d9c79e92af194d3f4cef331275dbbdaf237c23399f9c8b0f1c408e4d8933`.
Both binaries' embedded producers identify rustc 1.96.0 and wasm-bindgen
0.2.128. [Repeated raw records and summary](../../artifacts/localization-resolution/performance/final-matched-summary.local.json),
[matched fixture checks](../../artifacts/localization-resolution/performance/final-matched-state-check.local.json),
[deployment and source guard](../../artifacts/localization-resolution/performance/final-deployment.local.json)
and [separate attribution](../../artifacts/localization-resolution/performance/attribution-summary.local.json)
retain exact scope, hashes and mixed results. [Runtime byte equivalence](../../artifacts/localization-resolution/performance/final-stack-runtime-equivalence.local.json)
identifies the same measured production bytes after the shared-host catalog
allocation correction. Earlier single-language samples and intermediate
candidates remain separate checkpoints.

The subsequent lifecycle build, Wasm SHA-256
`2282c166274739cb0715960de5a56b88543d6b83e8d00d4cd58875c9bca1102e`,
is **unmeasured on reference hardware**. Its [deployment and source guard](../../artifacts/localization-resolution/performance/final-lifecycle-deployment.local.json)
verify the exact release runtime; only Wasm differs from the measured
checkpoint. These retained timings do not establish current-binary performance
or qualify any tier target.

The rebased release, Wasm SHA-256
`313264f2fed8eef1285f2d010a820ba6286c18566ccf46eb5c4ab68c4b5d941c`,
also remains unmeasured on reference hardware. Its [runtime and source checks](../../artifacts/localization-resolution/web/acceptance-tool-variations-final.local.json)
qualify the recorded functional journeys only.

## Web workspace diagnostic

Measured 2026-10-19 on the reference TCL in Chrome 154.0.8037.126, thermal 0,
1129 × 586 CSS pixels at DPR 1.7, default low glass. The frozen baseline is
`a46600c8d`, Wasm SHA-256 `4dfb5f6e7fe90fb70f4f98d2596fddf841943eb398227e4c9025a5e81c942ade`.
The 4248 × 2832 tier photo has one empty paint layer above it at Fit zoom.
These CDP mouse diagnostics use a custom workspace and three warmed five-second
contacts per row, sampled through animation callbacks.

| Motion | Changing geometry or placement updates/s | Gap p99 across contacts |
| --- | ---: | ---: |
| Left column resize | 8.43–8.63 | 150.0–183.3 ms |
| Right column resize | 8.77–8.83 | 166.5–183.3 ms |
| Navigator column resize | 8.55–8.83 | 166.7–183.3 ms |
| Floating group drag | 18.45–19.41 | 116.6 ms |
| Floating Navigator drag | 21.49–22.36 | 66.8–83.2 ms |

These geometry rates miss the 57 Hz display-paced floor. Raw SurfaceFlinger
actual-present records exist, but their clock alignment does not establish
moving-frame cadence. Floating Navigator contacts also fail the retained-model
probe's zero-`layout()` expectation with one call. The camera-motion probe has
no accepted samples because its instrumentation throws while serializing a
BigInt. No presented FPS or tier qualification follows from these diagnostics.
The current Navigator-minimum candidate, Wasm `938121939f4575295d040c7d52c2b81b082fa2c74683e7fa8566a7945c0dd4d2`,
has no matched low-tier performance measurement. [Raw baseline records and
bounded summary](../../artifacts/navigator-layout-fix/tcl/baseline-sf-performance/summary.json)
retain the scope and failures.

## Current drag comparison

Measured 2026-10-01 on the TCL reference tablet, 4248 × 2832 Perceptual photo,
17.699% Fit zoom, Navigator open, Stats closed and default glass. Both builds
use release Rust, the same work area and three warmed five-second gestures per
row, with thermal status zero. The composed cases transform a translucent photo
over another photo, where direct root presentation is ineligible.

| Journey | Before, fresh completed updates/s | Retained Navigator, fresh completed updates/s | Speedup | Screen presents/s | Screen gap p99 across runs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Pixel transform resize: Free | 73.28 | 150.69 | 2.06× | 59.44 | 16.70–16.84 ms |
| Pixel transform: Distort | 64.52 | 133.97 | 2.08× | 59.41 | 16.70–16.86 ms |
| Placed-photo corner resize | 91.28 | 104.20 | 1.14× | 59.37 | 16.70–16.96 ms |
| Pixel transform: Warp | 62.45 | 66.66 | 1.07× | 59.45 | 16.71–16.88 ms |
| Two-photo composed Free resize | 40.24 | 40.78 | 1.01× | 40.98 | 33.38–50.01 ms |
| Two-photo composed Distort | 33.49 | 34.22 | 1.02× | 34.62 | 33.61–50.00 ms |

Fresh counts match completed submissions to newly consumed host input, excluding
thumbnail-only refreshes; they are not a distinct-transform-pose oracle. Raw
completed rates are 151.09/s for Free and 133.97/s for Distort. Fresh completion
gap p99 spans 9.35–10.01 ms and 9.33–9.96 ms, respectively. Median render-owner
CPU time falls from 6.19 to 3.35 ms for Free and from 5.80 to 3.26 ms for Distort.

The four single-photo rows meet the screen-cadence criteria. SurfaceFlinger
actual-present records include native UI, and this tablet exposes no separate
SurfaceView timeline, so they do not independently establish canvas scanout.
The two-photo composed cases remain below the 60 Hz target. One composed-Free
candidate run has a 55.65 ms completion-gap p99 and 50.01 ms screen-gap p99;
the other two runs have approximately 30 ms and 33.4 ms respectively. The
original samples remain in the comparison.

Three additional full-sequence runs with only scheduler and ART tracing retain
normal throughput: composed Free reaches 40.79, 40.81 and 41.23 fresh updates/s,
with completion-gap p99 29.46–33.00 ms and screen-gap p99 33.36–33.38 ms.
Across all six final-build runs its median is 40.80/s, versus the baseline's
40.24/s. These traces contain no app scopes or GPU diagnostic queries. Their
largest completion gaps overlap running and runnable render-owner time, with
no overlapping app GC pause. This supports unchanged composed throughput but
does not explain the original 55.65 ms tail or establish absence of a latency
regression. Records and aligned scheduler intervals are in
`artifacts/validation/candidate-clock-gc`.

Baseline is `f820bb89c`, APK SHA-256
`645a11a3c86d8d194910f66f50636c6066ac5dd9b5ba867c674ec40fbca879a7`.
The retained-Navigator build includes the shared session-clock correction,
APK SHA-256
`32cf373145f6c356713098726015ebc8454fa46b85e27532ed628dcfa0a876b6`.
Records, raw input/completion samples and complete presentation traces are in
`artifacts/validation/{baseline-final,candidate-clock-final}` in the
`navigator-implementation` worktree; its `candidate-clock-source.patch` records
the measured source. Separate diagnostic captures include phase timing and
memory; they do not qualify the rates above. These transform results do not
establish a general brush improvement or qualify another tier.

### Cost and expected throughput

Separate captures of the same builds and motions isolate the removed work.
Times below are mean GPU intervals; Navigator refreshes are included in total
render time, and main mip generation is included in main composition.

| GPU interval | Free before | Free retained | Distort before | Distort retained |
| --- | ---: | ---: | ---: | ---: |
| Main composition | 8.198 ms | 0.002 ms | 9.672 ms | 0.002 ms |
| Main mip generation | 2.396 ms | None | 2.654 ms | None |
| Total render | 8.243 ms | 0.158 ms | 9.643 ms | 0.175 ms |
| Presentation | 5.039 ms | 6.206 ms | 5.418 ms | 7.052 ms |
| Navigator, per refresh | — | 0.679 ms | — | 0.806 ms |

Rendering and presentation execute sequentially on the same GPU queue. Using
`1000 / (mean render ms + mean presentation ms)` gives an approximate remaining
work ceiling of 157.1 updates/s for Free and 138.4/s for Distort. Qualification
reaches 95.9% and 96.8% of those estimates. The model predicts 2.09× and 2.08×
speedups, compared with the measured 2.06× and 2.08×. These are workload estimates,
not hardware peak claims: intervals include scheduling gaps, bounded timestamp
rings omit some observations, and presentation observations have no matching
render submission ID. The baseline estimates of 75.3/s and 66.4/s are also close
to its measured 73.3/s and 64.5/s.

Navigator refreshes at 18.92 Hz for Free and 18.75 Hz for Distort. CPU refresh
work averages 0.692 and 0.681 ms. After the first refresh, pending artwork age
has p99 59.5 and 58.0 ms, with maxima 62.9 and 59.7 ms; both settle with no
pending thumbnail. The shared session clock ignores zero-time preparation and
preserves the last timestamp across untimed flushes, avoiding device-uptime
rounding that previously reduced the nominal 20 Hz cadence.

The retained image is 266 × 177 pixels, with 753,312 logical pixel bytes and
32 bytes of geometry. Allocated GPU memory increases by 0.78 MiB for Free and
0.80 MiB for Distort; reserved GPU memory is unchanged. This removes repeated
main-image writes, but warm cached allocations remain, so it does not establish
an allocation saving. Cost and memory records are in
`artifacts/validation/{baseline-diagnostic,candidate-clock-diagnostic}`.

### Reduction-encoder cleanup verification

A matched check uses the retained-Navigator build above and the shared reduction
encoder with identical Free/Distort fixtures, three warmed five-second gestures,
Navigator open and thermal status zero. Before rebasing, Free changes from 149.74
to 150.35 fresh completions/s; Distort from 132.94 to 133.16/s. That cleanup APK is
`3c8ee7e43906e68dc06dc2bae43ce1ad8f748ba80991e098b5e3b0753e3c1be1`;
raw runs are in `artifacts/validation/cleanup/{before,after}` in the same worktree.

The final build includes upstream `cb1ad24fb` spatial composition and buffered
Android presentation changes. Three new runs give Free 149.72 fresh completions/s
(135.54–152.39) and Distort 133.36/s (132.97–133.69). Screen rates are 59.49 and
59.40/s, with p99 gaps at most 16.88 and 16.89 ms. Fresh-completion gap p99 ranges
are 9.75–19.82 and 9.97–10.46 ms. Both retain the screen-cadence target; differences
from the saved before-build medians establish no throughput improvement. All slow
Free samples remain in the records. Final APK SHA-256 is
`74d3cf3bd7b06adabc74d321ffb95e6de386827093ab7d4b4f0edf41aee3d7af`;
raw runs are in `artifacts/validation/cleanup/post-rebase/after`.

### Retained-Navigator navigation

The retained-Navigator APK (`32cf373145f6`, before cleanup and buffered-presentation
changes) completes three warmed five-second two-finger pans and pinches on the
12 MP photo with a preparation stroke, Navigator open, Stats closed and 15.9703%
Fit zoom. Pan reaches 59.83 fresh viewport completions/s and a median 59.43 screen
presents/s; pinch reaches 59.90 and 59.40/s. Screen interval p99 ranges from
16.74–16.83 ms for pan and 16.76–16.83 ms for pinch. Both meet the screen-cadence
criterion, subject to the SurfaceFlinger accounting limit above. Fresh completion
gap p99 is 19.09–19.62 ms for pan and 19.94–21.56 ms for pinch.

That build uses FIFO without a retained surface target. The viewport harness
captures presenter GPU timestamps in these runs;
fresh counts use completed host input calls. Records are in
`artifacts/validation/candidate-clock-navigation`, including all three gesture
windows per motion and `matched-summary.json`. These measurements establish
photo navigation at that revision, not a before/after speedup.

### Retained-Navigator painting

Matched three warmed five-second G-Pen strokes use the 12 MP photo, Perceptual
blending, 1024 px size, 200 Hz input, prediction, Navigator open, Stats closed
and default glass. Both builds use 15.9703% Fit zoom and the same 500 × 280 px
ellipse. Its full brush footprint remains inside the photo. The eight-layer
case has one drawing layer over seven photo layers, six at 35% opacity.

| Stroke | Before, fresh updates/s | Retained Navigator, fresh updates/s | Before completion gap p99 | Retained completion gap p99 |
| --- | ---: | ---: | ---: | ---: |
| One photo at Fit | 60.92 (60.49–61.37) | 61.14 (60.69–61.70) | 27.02–28.18 ms | 28.98–29.50 ms |
| Eight visible layers at Fit | 51.47 (49.46–51.68) | 50.12 (48.65–51.65) | 30.22–38.03 ms | 29.30–32.87 ms |

The single-photo stroke meets the 60/s target. The eight-layer case remains
below target; its median falls 2.6%, with overlapping run ranges and a lower
worst p99 completion gap. This does not establish a general brush improvement.
Earlier 100% zoom comparisons are separate workloads and do not qualify Fit.

Optimized baseline APK SHA-256 is
`cea0ded9608d7543f53da150275cf7593551eeeb1503c064a3d9ed71264e636a`;
the final optimized APK is
`0a3ca41f35806af34f642e46a8e10194746768abf3e5b6e724ceaa59eb333814`.
Both use release Rust. Records are in
`artifacts/validation/{baseline-brush,candidate-clock-brush}`, with native
consumed-input traces, completed submissions, geometry and final screenshots.

### Transform reach check

Measured 2026-10-25 on the TCL with `AndroidCanvasBarBenchmarkTest`, the
4248 × 2832 tier photo, opened as the drawing or placed in a new one, at Fit
zoom with Navigator open, release builds of the
parent (control) and the transform reach check (candidate), four runs per build
in ABBA order and thermal status 0 throughout. The per-drag page check adds no
measurable cost.

| Journey | Control completed updates/s | Candidate completed updates/s | Owner CPU p50, control → candidate |
| --- | ---: | ---: | ---: |
| Placed photo, Free resize | 44.54 (44.38–44.91) | 44.45 (44.31–44.51) | 5.34 → 4.90 ms |
| Placed photo, Distort | 38.37 (38.28–38.48) | 38.42 (38.27–38.50) | 3.89 → 4.04 ms |
| Opened photo, Free resize | 45.56 (45.25–45.68) | 45.16 (45.06–45.43) | 4.94 → 4.78 ms |
| Opened photo, Distort | 38.93 (38.87–39.08) | 38.91 (38.69–39.11) | 3.91 → 3.91 ms |

Both builds remain below the 60 Hz target on these rows.

## Earlier direct-presentation comparison

Measured 2026-09-29 on the 12 MP Perceptual reference photo at Fit: release Rust,
default glass, Stats closed and three warmed five-second gestures per row.
Thermal status is zero before and after all runs. Both renderers use the same
600 ms warm-up through the complete motion range and the same test APK.
The harness explicitly sets Navigator visibility and asserts the overview count.
Opening Navigator also changes the work area and Fit camera, so each comparison
pairs the same panel configuration. Values are medians across runs.

| Journey | Before, Navigator closed | Direct presentation, Navigator closed | Before, Navigator open | Direct presentation, Navigator open |
| --- | ---: | ---: | ---: | ---: |
| Placed-photo translation | 96.45 | 122.86 | 83.30 | 98.48 |
| Placed-photo corner resize, bar hidden | 94.82 | 113.20 | 82.37 | 89.27 |
| Placed-photo corner resize, bar visible | 96.58 | 113.25 | 90.59 | 89.88 |
| Pixel transform translation | 89.41 | 175.10 | 79.47 | 80.30 |
| Pixel transform resize: Free | 87.82 | 169.59 | 78.80 | 72.47 |
| Pixel transform: Distort | 85.92 | 152.25 | 77.33 | 63.85 |
| Pixel transform: Warp | 46.53 | 70.03 | 38.64 | 61.54 |
| Crop corner drag | 129.23 | 130.05 | — | — |
| Pixel resize after 45% placement | 117.78 | 224.55 | — | — |

Values are GPU-completed nonempty updates/s. In these earlier builds, single-input
affine and perspective previews sample the retained source directly during
presentation when Navigator is closed, avoiding the intermediate composition. With Navigator open, both
views share the existing materialized composition and preserve preview quality.
Placement drags also defer exact refinement until Apply or Cancel.

All 27 closed-Navigator gestures and all 21 open-Navigator gestures pass the
60 Hz screen-cadence criterion. Closed-panel median screen rates span
58.64–59.46 presents/s, with interval p99 16.71–16.94 ms. The old Warp fails in
both configurations. Closed-panel pixel transforms and placement exceed the old
compositor, including crop after specializing presentation by source type.
In these builds with Navigator open, Distort is 17.4% slower, Free resize 8.0% slower
and visible-bar placement resize 0.8% slower. Meeting screen cadence does not
close these throughput gaps.

Screen rates use SurfaceFlinger actual-present timestamps. This device provides
no separate SurfaceView timeline, so screen records do not independently
establish canvas scanout. Renderer counts exclude empty updates and terminal
polling. Setup waits for command availability outside timing. No allocator or
process-memory sampler runs inside qualification; a separate safety guard
observes system available RAM. The first specialized-presentation Navigator
run lacked continuous RAM sampling because its monitor could not locate adb;
its frame records are complete, but it provides no continuous memory bound.

Old renderer: `7382bd260` with read-only benchmark observers, APK SHA-256
`d60230a701dd4edcd6e7e0346898893122669da72447784304b72f27d4e3c718`.
Closed-panel transforms and placement: `e41ee80fe` plus direct transform
presentation, Navigator materialization and placement refinement scheduling,
APK SHA-256
`ede35aa5b23bd8770d1207929e1b633a25b481f7987eb7201645125a11e013e3`.
Raw runs are in `artifacts/latency-investigation/{old-closed-fixed,deferred-9-closed-fixed}`
and `old-navigator`. Later crop and open-Navigator rows include fragment
materialization and source-specific main-view shader entries, APK SHA-256
`1dbac0ef1dcf9208709ba3e7df71f0a028e99d6d25e4c60356e7413a270f1208`.
Their records are `present-18-crop` and `present-18-navigator`, including its
`extra` directory. The open-Navigator screen medians span 59.35–59.49/s,
with p99 intervals 16.74–16.92 ms. Earlier default-panel records retained
Navigator and are not evidence for a closed-panel workload.

### Two-page-refinement qualification

Measured 2026-09-29 on the 12 MP Perceptual photo at Fit, Navigator open, Stats
closed, default glass and three warmed five-second gestures per row. Thermal
status remains zero. This is the build used for the current brush and retouching
records, APK SHA-256
`ab0d8facb88f67cfc8da05284c70a639f4e7b0815aaed35ed79029e7365f4333`:
unminified Java for the white-box harness, optimized release Rust. Screen
observations use SurfaceFlinger actual-present times; they include native controls
and do not independently establish canvas scanout. No allocator or PSS sampler
runs during qualification. Available memory remains above 1 GiB.

| Journey | Completed updates/s, median | Completion gap p99, median | Screen presents/s, median | Screen interval p99, median |
| --- | ---: | ---: | ---: | ---: |
| Move whole image | 81.02 | 16.37 ms | 59.12 | 16.82 ms |
| Move partial selection | 134.19 | 12.36 ms | 59.44 | 16.85 ms |
| Move partial selection, Leave Copy | 131.10 | 14.52 ms | 59.26 | 16.74 ms |
| Placed-photo translation | 95.68 | 17.51 ms | 59.48 | 16.94 ms |
| Placed-photo resize, bar hidden | 87.13 | 18.18 ms | 59.38 | 16.92 ms |
| Placed-photo resize, bar visible | 88.50 | 17.50 ms | 59.48 | 16.95 ms |
| Pixel translation | 80.64 | 16.10 ms | 59.34 | 16.91 ms |
| Pixel resize: Free | 72.79 | 17.16 ms | 59.42 | 16.82 ms |
| Pixel transform: Distort | 63.80 | 17.85 ms | 59.42 | 16.86 ms |
| Pixel transform: Warp | 62.44 | 18.64 ms | 59.45 | 16.73 ms |
| Gaussian Blur, small radius | 23.48 | 71.39 ms | 49.95 | 33.37 ms |
| Gaussian Blur, large radius | 15.51 | 124.01 ms | 42.67 | 49.98 ms |

The Move, placement and transform screen-cadence measurements pass. In this build,
Navigator-open Distort, Free and visible-bar placement are below the old matched control's
77.33 / 78.80 / 90.59 completed updates/s. Both Gaussian rows fail the soft motion
target; no hardware calculation grants a waiver. The older spatial-filter
comparison uses a different camera and panel configuration. These runs contain
Gaussian sliders; they do not requalify the Exposure rows.

Raw records are `artifacts/latency-investigation/two-25-geometry`;
the analysis is `two-25-geometry-summary.txt`.

### Warp contour qualification

Three subsequent matched runs with curvature-bounded selection contours raise
Warp from 47.88 to 65.06 completed updates/s. Screen cadence is 59.37 presents/s
with a 16.84 ms p99 interval; all three runs meet the screen target, with thermal
status zero. This removes per-source-pixel contour subdivision: the previous
12 MP rectangular selection generated 14,160 points even along straight edges.
The editable grid and the pixel resampler are unchanged.

Build: `255309151` plus the contour fix, APK SHA-256
`643f97510b32802ab72f2ee9c30a7c52e0b1a104c2b044a2aaeb39cd33e9c96d`.
Raw runs: `artifacts/latency-investigation/warp-contour-warm`.
The cursor attribution trace is `cursor-count-profile`; it measures about
14,200 line segments during Warp versus eight during Distort.

### Earlier selected-pixel Move measurements

Transform damage build: 2026-09-28, `2e04caa8` plus incremental cut
invalidation, APK SHA-256
`b614cd5942d2165ac6054497f96873c215ed6b09863659453a4b337dd00a248b`.
Three runs alternate before/after order, with the same private harness, 12 MP
photo, Fit camera, release profile and default glass. Thermal status is zero
before and after all six runs. The presentation-accounting limits above apply.
Raw results and the source patch are under
`artifacts/display-production/transform-cut-paired-*` and
`transform-cut-damage-whitebox-source.patch`.

Avoiding repeated invalidation of the stationary cut improves pixel translation
76.64 to 84.07 completed updates/s, resize 71.13 to 77.68, whole-image Move 78.60
to 86.63, and partial-selection Move 76.98 to 101.28. Leave Copy, which already
keeps the original, changes 72.18 to 73.32. Distort and Warp remain essentially
unchanged at 62.01 and 45.87. Viewport GPU medians fall from 5.60 to 5.03 ms for
translation and 5.61 to 4.96 ms for resize. Old-main transform throughput still
exceeds this build; these gains do not close the geometry regressions.

## Pass Through navigation

Measured 2026-09-28 on a 4248 × 2832 generated solid-color fill with a Black &
White adjustment inside a Pass Through group. Three warmed five-second touch
gestures per motion use release Rust, default glass and thermal status 0 before
and after. These measurements do not qualify photo-backed or multilayer navigation
and have no matched old-renderer control.

| Motion | Completed updates/s | Completion gap p99 | Screen presents/s | Screen gap p99 |
| --- | --- | --- | --- | --- |
| Pan | 59.79 | 18.9 ms | 59.39 | 16.7 ms |
| Pinch | 59.79 | 20.6 ms | 59.39 | 16.8 ms |

Screen cadence meets the 57 presents/s measurement floor for this workload.
The screen-present accounting limits above apply. Renderer-owned storage is
211.7 MiB for pan and 229.6 MiB for pinch; neither is process RSS. Median last
completion after input ends is 74 ms and 29 ms respectively.

Candidate is `cbcf0aec` plus the shared Pass Through traversal port, APK SHA-256
`3cd483d10defd2ffc9cd8115dc2329173dbc5b70054f9dd4ac7d99901b539d49`.
Raw runs, traces and source provenance are under
`artifacts/display-production/pass-through-ready-tcl` and
`pass-through-ready-provenance.json`. Display-frame records identify screen
presents: the pinch trace omits the SurfaceFlinger process name, but its display
frame tokens and PID match the adjacent named pan trace.

## Pointwise filter composition

Measured 2026-09-28 on the 12 MP photo at Fit, with three alternating pairs of
warmed five-second stylus scrubs, release Rust, default glass and thermal status
0. The harness closes panel configuration, waits for shader readiness after
priming and verifies changing parameter values during half-second triangle
motion. Both builds use the same camera and test APK.

| Slider journey | Previous filter executor, completed updates/s | Display graph, completed updates/s | Speedup | Graph completion gap p99 |
| --- | --- | --- | --- | --- |
| Exposure | 1.59 | 30.84 | 19.36× | 52.6 ms |
| Levels, Vibrance, Exposure chain | 1.19 | 25.08 | 20.99× | 70.1 ms |

Neither journey meets the motion target. Screen presents include native controls
and do not independently establish canvas presentation cadence: the chain's
screen rate includes slider-only changes between completed canvas updates.
Exposure ranges from 29.86 to 31.69 completed updates/s. The chain ranges from
13.94 to 31.66, with individual completion-gap p99 values of 63.6, 285.4 and
70.1 ms. The slow repeat remains in the results; thermal status stays zero and
its viewport GPU cost remains comparable. Renderer-owned storage falls from
579.8 to 306.5 MiB; this is not process RSS.

A separate profile measures composition at 5.6 ms for Exposure and 7.9 ms for
the chain, plus viewport observations of 8.2 and 8.5 ms. Normal-run traces also
show substantial native model publication work: 419 calls consume 2.46 s in one
chain gesture and 3.16 s in the slower repeat. GPU cost alone does not explain
the journey rate; shared state publication and scheduling remain optimization
work. The profiled captures verify both themes with panel configuration closed.

The control is production `7cad88dd`, before pointwise graph admission, using APK
SHA-256 `574bac2557e30ce43444a5657de969719052975f285e3d2639e879e3eec7a977`.
Candidate is the spatial graph build based on `2de3d0ca`, APK SHA-256
`f5dda5c758ce1e79294cb2ad86b5df7572c930d6dcc068dba80d5e5ca9d3e1e1`.
Raw results and traces are under `artifacts/display-production/pointwise-workspace-paired`.
Source and harness hashes are in `spatial-hardware-provenance.json` and
`spatial-workspace-provenance.json`. Earlier pointwise runs used an expanded
panel configuration and are superseded by this closed-panel fixture.
These compare filter execution within the production branch, not against a fresh
old-main build. Other filters and high-frequency preview quality remain unqualified.

## Spatial filter composition

Measured 2026-09-28 on the 12 MP photo at Fit, with three alternating pairs of
warmed five-second stylus scrubs. Both builds use release Rust, the same test
APK, default glass, zoom 0.1769915 and thermal status 0 before and after every
run. The harness closes panel configuration, waits for shader readiness and
records changing parameter values strictly inside the slider limits. Its
half-second triangle motion avoids a slow gesture's numeric-step cadence limit.

| Gaussian radius range | Previous executor, completed updates/s | Display graph, completed updates/s | Speedup | Graph completion gap p99 |
| --- | --- | --- | --- | --- |
| 2.6–4.9 document pixels | 1.21 | 30.37 | 25.08× | 56.5 ms |
| 14.2–16.5 document pixels | 0.39 | 17.71 | 45.68× | 93.7 ms |

Neither journey meets the motion target. Screen presents include native controls
and do not independently establish canvas presentation cadence. Renderer-owned
storage falls from 894.9 to 318.0 MiB; this is not process RSS. These compare
filter execution within the production branch, not against a fresh main build.

The graph evaluates 1062 × 708 pixels instead of 4248 × 2832, and prepares its
separable kernel at that scale. This reduces the paired sample count by roughly
48–55× over these radii. Two RGBA32Float passes still read and write at least
48.1 MB, giving an optimistic copy floor of 4.8 ms at the device's calibrated
10 GB/s. This floor excludes additional texture reads, shader arithmetic,
composition and presentation; it is not a filter-throughput prediction.

A separate profiled run gives submission-aligned composition medians of
16.1 ms at small radius and 43.2 ms at large radius. Viewport GPU observations
have medians of 9.5 and 9.6 ms respectively. These costs explain the frame-budget
miss; they do not establish a hardware limit or qualify a soft-target waiver.

Control is production `2de3d0ca`, APK SHA-256
`3cd483d10defd2ffc9cd8115dc2329173dbc5b70054f9dd4ac7d99901b539d49`.
Candidate is the spatial graph build based on that commit, APK SHA-256
`f5dda5c758ce1e79294cb2ad86b5df7572c930d6dcc068dba80d5e5ca9d3e1e1`.
Raw runs and traces are under `artifacts/display-production/spatial-workspace-paired`
and `spatial-workspace-profile`. Source and harness hashes are in
`spatial-hardware-provenance.json` and `spatial-workspace-provenance.json`;
the operation model is `spatial-cost-model.json`. Light and dark captures verify
the closed-panel photo fixture. Earlier runs with panel configuration expanded
or a zero-clamped or slow cosine trajectory are diagnostic only.

## Sources larger than the canvas

This diagnostic places the 12 MP photo at original size over a 1062 × 708 canvas
and moves it at reduced zoom. It measures source-window retention, not the
12 MP canvas target above. Three five-second gestures alternate before/after
order on the TCL at thermal status 0, using the same test APK and private app.

| Measurement | Full source images | Bounded source windows |
| --- | --- | --- |
| Completed updates/s | 139.06 | 137.71 |
| Completion gap p99 | 12.80 ms | 12.99 ms |
| Screen presents/s | 59.57 | 59.39 |
| Renderer-owned storage | 501.4 MiB | 321.4 MiB |

The 1.0% throughput difference lies within these runs' overlapping ranges;
renderer-owned storage falls 35.9%. This is not a process-memory measurement.
Windows retain nearby motion up to twice the requested pixel storage. Returning
to covered pixels preserves textures and avoids repeated source decoding.

Measured 2026-09-28 against `d99823d2`; candidate APK SHA-256
`7056d5dc4c92fcb3b2c07936ad027c901150f8844f1b42643feee6eab779ff5f`.
Source patch, immutable APKs, raw results and traces are under
`artifacts/display-production/source-window-oscillation-*`. The screen-present
accounting limits in the geometry section apply.

## BUILD32 G-Pen comparison

Measured on 2026-10-04 UTC with clean `192601dac` baseline BUILD29 and M3
candidate BUILD32 on the low-tier reference tablet. The contained G-Pen 1024 px
workload uses 4248 × 2832, Fit 15.9703%, dark theme, Linear blending, Navigator
open, Stats closed, Paper hidden, prediction enabled, pressure 1 and 200 Hz
injected input. Three warmed ten-second contacts retain all captured samples.
Logical setup matches; the nominal full tip stays at least 17.44 px inside the
photo. Both invocations finish successfully, with thermal status zero before
and after and no pending composition or edits at contact start.

| Build / contact | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline29 / 1 | 65.306 | 26.870 | 52.038 | 13.998 |
| Baseline29 / 2 | 64.870 | 27.905 | 50.290 | 14.252 |
| Baseline29 / 3 | 65.753 | 27.743 | 50.473 | 14.120 |
| Candidate32 / 1 | 64.416 | 25.855 | 51.523 | 14.231 |
| Candidate32 / 2 | 64.687 | 25.810 | 49.326 | 14.048 |
| Candidate32 / 3 | 65.539 | 24.960 | 51.480 | 13.921 |

All three candidate contacts meet 60 fresh completed updates/s and the 33.3 ms
fresh-gap limit. Throughput changes by −1.36 / −0.28 / −0.33%, owner CPU p95 by
+1.66 / −1.43 / −1.41%, and callback wall p95 by +4.02 / +0.20 / −2.64%, within
the measured 5% bounds. Response p99 changes by −0.515 / −0.964 / **+1.007 ms**.
The third pair's exact +1.006776 ms exceeds the +1 ms common bound by
0.006776 ms. The low-tier stroke result passes; the complete common comparison
gate remains open. This forward-order batch alone does not establish repeatability, other
workloads or overall M3 qualification; the reverse check follows below.

The reverse candidate→baseline check uses the same APKs and matching logical
setup. All three additional candidate contacts meet the tier limits. The
response excess repeats: candidate-minus-baseline p99 is **+1.192 / −3.075 /
+1.778 ms**; contacts 1 and 3 exceed +1 ms. Throughput changes by
+1.13 / −1.17 / −1.34%, owner CPU p95 by −1.62 / −0.28 / +3.39%, and callback
wall p95 by −2.00 / −0.40 / +1.38%, within 5%. Both orders remain separate;
averaging them or reversing their order does not clear the forward failure.

| Reverse build / contact | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Candidate32 / 1 | 65.713 | 27.643 | 54.054 | 13.725 |
| Candidate32 / 2 | 64.836 | 25.580 | 48.107 | 14.094 |
| Candidate32 / 3 | 64.295 | 26.772 | 51.993 | 14.470 |
| Baseline29 / 1 | 64.978 | 26.269 | 52.863 | 13.951 |
| Baseline29 / 2 | 65.603 | 27.053 | 51.183 | 14.133 |
| Baseline29 / 3 | 65.169 | 26.977 | 50.215 | 13.995 |

Tracked renderer residency is 805.641 MiB, except candidate contact 3 at
805.688 MiB; all boundaries satisfy max(16 MiB, 5% of baseline). Both retain
204 source slots. Reverse candidate residency is 805.641 MiB throughout; reverse
baseline contact 3 is 805.688 MiB. All reverse boundaries also satisfy the bound,
and thermal status remains zero. Dynamic cache budgets differ, so this does not establish
identical admission. Staging inventories and process RSS/high-water snapshots
are retained; continuous renderer/import/driver peaks are unmeasured. Fresh
completion and response metrics end at GPU callback service; presentation,
physical pen latency and GPU execution timestamps are unmeasured.

Exact APK SHA-256, baseline then candidate:
`e7ddce0b37f254df4fcdee4c502bc2220c5a2f050e644060bc4a47a06067e429` /
`acccdc9d9464c2ca2db5fc3291dc252b7affe683ca9cadb7628ffef58863a51c`.
Provenance is in `artifacts/format/m3-baseline192-android-build-29/provenance.json`
and `m3-candidate192-android-build-32/provenance.json`; raw captures, exact setup,
staging and every per-contact comparison are in `m3-low-brush-35/analysis.local.md`
and `analysis.json`. These APKs do not qualify later source edits. Earlier 4a
BUILD28 and BUILD20 results remain scoped to their binaries.

## BUILD28 G-Pen comparison

The frozen M3 candidate BUILD28 and clean `4a2cf6aa0` baseline BUILD20 use the
same contained G-Pen 1024 px workload on 4248 × 2832. Logical setup matches the
earlier BUILD20 pair exactly: Fit 15.9703%, dark theme, Linear blending, Navigator
open, Stats closed, Paper hidden, prediction enabled, pressure 1, 200 Hz injected
stylus input and three warmed ten-second strokes. Screen semiaxes are 240 × 120 px;
the nominal full brush tip remains at least 17.44 px inside the photo. Both
instrumentation logs finish successfully; thermal status is zero before and after
each invocation. All strokes begin without pending composition or edits.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline20 / 1 | 65.186 | 25.815 | 50.922 | 13.923 |
| Baseline20 / 2 | 65.311 | 27.031 | 50.911 | 14.288 |
| Baseline20 / 3 | 64.964 | 25.788 | 51.601 | 14.573 |
| Candidate28 / 1 | 64.926 | 27.596 | 50.544 | 13.957 |
| Candidate28 / 2 | 65.022 | 25.959 | 49.502 | 14.109 |
| Candidate28 / 3 | 65.173 | 26.324 | 51.113 | 14.122 |

Every stroke meets 60 fresh completed updates/s and the 33.3 ms fresh gap limit.
Candidate throughput changes by −0.40 / −0.44 / +0.32%, owner CPU p95 by
+0.25 / −1.25 / −3.10%, and callback wall p95 by +0.44 / −2.38 / −2.41%.
Response p99 changes by −0.378 / −1.409 / −0.489 ms. The measured 5% CPU and
throughput bounds and +1 ms response bound hold in all three matched pairs.
These observations preserve the passing low-tier stroke row; they do not erase
the earlier BUILD20 +1.371 ms response sample or establish other workloads.

Tracked renderer residency is 805.641 MiB in both builds, with 204 decoded
source slots. Boundaries remain within max(16 MiB, 5% of baseline); continuous
renderer/import/driver peaks and later edit/undo/output lifetimes are unmeasured.
Rates count nonempty completions consuming fresh input inside contact, excluding
refinement. Response ends at GPU completion callback service. Neither metric
establishes actual screen presentation or physical pen latency; GPU execution
timestamps are absent.

Exact APK SHA-256, baseline then candidate:
`2dd557379b1cac6ac51ce813f8a168a46cd9a460249846efe08a8e79b5801af2` /
`b3e8bfe49f8e97bee101eb223a3c01debc540be34507c95e4e343eba470c4a5d`.
Provenance is in `artifacts/format/m3-baseline4a-android-build-20/provenance.json`
and `m3-candidate-android-build-28/provenance.json`; raw strokes, thermal records,
exact setup checks and per-stroke analysis are in
`artifacts/format/m3-low-brush-28/`. BUILD20 canvas diagnostics below and earlier
reference measurements remain scoped to their named binaries. The interrupted
mid-tier trace setup supplies no timing comparison or qualification evidence.

## BUILD20 selected canvas comparison

The matched 4248 × 2832 photo uses Navigator, dark theme, SDR and default glass.
Cameras and logical setup match in all fifteen contact pairs. Configured display
budgets differ; equal logical setup does not imply equal live headroom at cache
admission. Each Exposure contact records 63–93 distinct observed values and each
Threshold contact 83–106; parameters change in both builds. Pan has no
camera-after trace, so the input gesture does not establish every rendered pose.

| Motion / contact | Completed updates/s, baseline → M3 | Completion gap p99, ms, baseline → M3 | Owner CPU p95, ms, baseline → M3 |
| --- | --- | --- | --- |
| Pan, no effect / 1 | 59.154 → 59.078 | 19.087 → 19.033 | 4.387 → 5.184 |
| Pan, no effect / 2 | 59.885 → 59.729 | 19.053 → 18.961 | 4.891 → 5.639 |
| Pan, no effect / 3 | 59.776 → 59.624 | 18.902 → 18.821 | 4.093 → 4.162 |
| Pan, Shadows/Highlights / 1 | 52.854 → 52.639 | 22.777 → 24.011 | 5.680 → 5.652 |
| Pan, Shadows/Highlights / 2 | 59.573 → 59.641 | 19.289 → 19.714 | 4.804 → 4.559 |
| Pan, Shadows/Highlights / 3 | 59.611 → 59.782 | 18.958 → 18.945 | 5.039 → 4.244 |
| Pan, Clarity / 1 | 52.826 → 52.398 | 22.873 → 23.643 | 6.015 → 6.138 |
| Pan, Clarity / 2 | 59.612 → 59.598 | 19.441 → 19.490 | 4.048 → 4.259 |
| Pan, Clarity / 3 | 59.805 → 59.826 | 18.959 → 19.286 | 5.479 → 6.218 |
| Exposure scrub / 1 | 33.465 → 32.686 | 55.151 → 63.347 | 9.770 → 10.161 |
| Exposure scrub / 2 | 6.776 → 12.131 | 246.959 → 281.970 | 14.440 → 12.019 |
| Exposure scrub / 3 | 15.319 → 16.133 | 155.874 → 92.442 | 11.170 → 11.110 |
| Threshold scrub / 1 | 3.759 → 3.789 | 273.627 → 260.867 | 30.780 → 26.621 |
| Threshold scrub / 2 | 3.787 → 3.777 | 274.260 → 279.263 | 26.186 → 28.737 |
| Threshold scrub / 3 | 3.362 → 3.362 | 361.733 → 338.872 | 22.930 → 23.249 |

Bare pan completes near 60/s, but the first Shadows/Highlights and Clarity
contacts complete about 52/s in both builds. Exposure and Threshold miss the
60/s target in both builds; their stalls remain in the table. No matched
completion-throughput loss exceeds 5%. Owner CPU p95 increases by 18.17% and
15.29% in the first two bare-pan contacts, and 5.21% and 13.49% in the last two
Clarity contacts. Threshold has one 9.74% increase. These adverse repetitions
prevent declaring the common CPU gate cleared; they are not averaged away.

Tracked residency after contact is 573.350 MiB for bare pan, 522.788–522.985 MiB
with Shadows/Highlights or Clarity, and 516.985 MiB for Threshold in both builds.
Exposure is 574.350 MiB except the first M3 contact at 574.185 MiB. These matching
boundaries do not establish a continuous peak or an import/source lifetime bound.

These are renderer completion diagnostics. Navigator and retained refreshes
count; actual fresh canvas presentation times and pose identities are absent.
Scheduled presentation deadlines and physical panel refresh do not establish
presentation rates. Submit-to-complete latency includes scheduling and polling;
it cannot establish the common input-response p99 bound. GPU execution samples,
continuous memory peaks and process/driver inventories were not collected.

The exact baseline and candidate are the 4a/BUILD20 APKs identified in the
[G-Pen comparison](#build20-g-pen-comparison). Each contact requests five seconds;
all scenario thermal status samples are zero before and after. Raw reports,
verified instrumentation outcomes, each parameter value, per-contact CPU/input
quantiles and configured admission limits are retained under
`artifacts/format/m3-final-android-performance-20/`, with
`canvas-analysis.local.md`, `canvas-analysis.json` and `canvas-gestures.tsv`.

## BUILD20 G-Pen comparison

Measured on 2026-10-04 UTC with the clean `4a2cf6aa0` baseline and the M3
candidate based on the same revision. Both are benchmark APKs with release Rust.
The tier photo has one drawing layer above it, Paper hidden, Linear blending,
Navigator open, Stats closed and default glass. Pressure is 1 and prediction is
enabled. A priming stroke is undone before three warmed ten-second strokes with
200 Hz OS-injected stylus input. Both before/after thermal samples are zero.
The matched Fit zoom is 15.9703%, with screen semiaxes 240 × 120 px; the
nominal full brush tip remains at least 17.44 px inside the photo.

Rates count completed nonempty updates consuming new paint input inside the
contact, excluding refinement-only completions. Response is the latest consumed
input event to GPU completion; its p99 differs from the intercompletion gap.
Neither metric establishes physical pen latency or screen presentation.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 65.793 | 25.665 | 50.743 | 13.745 |
| Baseline 2 | 65.281 | 26.588 | 52.332 | 13.795 |
| Baseline 3 | 66.138 | 26.681 | 49.989 | 13.455 |
| Candidate 1 | 65.103 | 27.007 | 52.114 | 13.834 |
| Candidate 2 | 64.995 | 27.159 | 52.525 | 13.984 |
| Candidate 3 | 65.270 | 27.904 | 50.531 | 13.886 |

Both builds meet 60 fresh updates/s and the 33.3 ms gap limit in every stroke.
The candidate loses 0.44–1.31% throughput and grows owner CPU p95 by 0.65–3.21%,
within the 5% comparison bound. Fresh gap p99 separately grows by
1.343 / 0.571 / 1.223 ms. Response p99 grows by 1.371 / 0.193 / 0.542 ms;
the first stroke exceeds the common +1 ms response bound. These samples do not
establish that bound for the candidate, despite the passing stroke target.

Accounted renderer residency after the strokes is 805.641 MiB in the candidate
and 805.641–805.656 MiB in the baseline, with 204 decoded source slots in both.

Measured resident boundaries stay within the additional max(16 MiB, 5%)
comparison ceiling. Allocator snapshots are taken after settling, not inside
motion; they do not establish continuous renderer/process/driver peaks or
edit/undo/output lifetime. GPU execution p95 is unmeasured. Logical workload
settings match; dynamic admission budgets are retained separately. One paired
batch does not establish the repeatability of response outliers or complete M3
qualification. Other brushes and earlier effect/transform rows retain their
stated scope.

Baseline APK SHA-256:
`2dd557379b1cac6ac51ce813f8a168a46cd9a460249846efe08a8e79b5801af2`.
Candidate APK SHA-256:
`83aa369e1b7cc2d9eb16eaf4c2dc1f0f40f574ddeaa0a2965317feb75c2ca01e`.
Exact app/test APK and source provenance is under
`artifacts/format/m3-{baseline,candidate}4a-android-build-20/`; raw strokes and
per-repetition analysis are under `artifacts/format/m3-final-android-performance-20/`.

## Layer color modes

Measured on 2026-10-04 at `de1159228`, release benchmark APK SHA-256
`fc0de7ee32ebbb9947803e461ca2bb436e07915019c2f82543cafcce8e5ea645`.
App sources match the implementation at `c92420837`.
The tier photo sits below one empty paint layer at Fit, with Perceptual blending,
pressure 1, 16 ms prediction, default display settings and panel glass, Stats
closed and thermal status 0 before and after each workload. Each mode uses one
priming gesture undone, followed by three five-second 200 Hz stylus ellipses.
The ellipse radii are 240 × 140 surface pixels at 15.97% Fit.

| Brush | Color mode | Canvas updates/s, median (range) | Fresh input updates/s, median (range) | Fresh completion-gap p99, range | Target |
| --- | --- | --- | --- | --- | --- |
| G-Pen 1024 px | Full color | 59.71 (59.45–59.80) | 59.71 (59.45–59.80) | 27.17–29.62 ms | **Not met** |
| G-Pen 1024 px | Grayscale | 59.52 (59.47–59.58) | 59.52 (59.47–59.58) | 28.94–30.02 ms | **Not met** |
| G-Pen 1024 px | Two-tone | 59.03 (58.54–59.18) | 58.98 (58.54–59.03) | 30.10–30.97 ms | **Not met** |
| Paintbrush 1024 px | Full color | 10.75 (10.74–10.96) | 10.75 (10.74–10.96) | 350.01–385.67 ms | **Not met** |
| Paintbrush 1024 px | Grayscale | 10.76 (10.57–10.98) | 10.76 (10.57–10.98) | 346.47–376.28 ms | **Not met** |
| Paintbrush 1024 px | Two-tone | 10.77 (10.75–10.95) | 10.77 (10.75–10.95) | 345.47–389.82 ms | **Not met** |
| Watercolor Wash 512 px | Full color | 0.40 (0.40–0.40) | 0.40 (0.40–0.40) | 2107.06–2310.56 ms | **Not met** |
| Watercolor Wash 512 px | Grayscale | 0.40 (0.40–0.40) | 0.40 (0.40–0.40) | 1714.39–2208.98 ms | **Not met** |
| Watercolor Wash 512 px | Two-tone | 0.40 (0.40–0.40) | 0.40 (0.40–0.40) | 1909.25–2213.02 ms | **Not met** |

These workloads miss the tier rate. Other brushes, layer-mode conversion latency,
and physical input-to-present latency remain unqualified. Raw runs, screenshots
and environment records are under `artifacts/layer-modes/low-rebased/`;
the binary and source snapshot are under `artifacts/layer-modes/rebased-build/`.

## Brushes

Target: **60 completed updates/s** at the guaranteed size, on the 12 MP canvas.

### Physical tilt response

Measured 2026-10-24 on the reference TCL with `9bb23c036`, benchmark APK and
Rust `release` profile. APK SHA-256 is
`c1e238a0199732e8bf88db89af5a1a33b6b5cfbb305c57bdd91b089adeb9a5a2`.
Each row has three warmed six-second native stylus curves over the 4248 × 2832
Sony photo and one empty paint layer. Input is 200 Hz at pressure 1, with
default prediction, 60 × 10 surface-pixel radii and Fit zoom 0.343538.
Stats is closed; the default workspace has five glass regions and Navigator
is absent. Ink uses SharedDemandRefresh. All shaders are ready before motion,
and thermal status is zero before and after every row and after the series.

Writing uses physical tilt 40° and Size 1024. Geometric contacts are
1024 × 665.6 document pixels for Pencil, Charcoal and Pastel Block;
1024 × 568.9 for Pointy Pencil; 1024 × 563.2 for Shading Pencil;
1024 × 819.2 for Chalk; and 1024 × 716.8 for Crayon. The side rows use 60°
and Size 512: Pencil's geometric contact is 2457.6 × 614.4 document pixels;
Charcoal's is 1024 × 409.6. All remain inside the photo throughout these curves.

| Brush | Tilt / Size | Completed updates/s | Fresh input updates/s | Completion-gap p99 | Fresh-input gap p99 | Result |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Pencil | 40° / 1024 | 66.83–67.50 | 66.83–67.50 | 32.14–34.13 ms | 32.14–34.13 ms | Not met |
| Pointy Pencil | 40° / 1024 | 67.06–68.48 | 67.06–68.48 | 50.66–52.11 ms | 50.66–52.11 ms | Not met |
| Shading Pencil | 40° / 1024 | 71.27–71.94 | 71.27–71.94 | 34.90–35.33 ms | 34.90–35.33 ms | Not met |
| Charcoal | 40° / 1024 | 65.58–66.71 | 65.58–66.71 | 33.79–35.12 ms | 33.79–35.12 ms | Not met |
| Chalk | 40° / 1024 | 30.40–30.76 | 30.40–30.76 | 211.77–213.87 ms | 211.77–213.87 ms | Not met |
| Pastel Block | 40° / 1024 | 34.95–35.59 | 34.95–35.59 | 104.43–107.44 ms | 104.43–107.44 ms | Not met |
| Crayon | 40° / 1024 | 63.87–65.41 | 63.87–65.25 | 34.33–35.63 ms | 34.33–35.63 ms | Not met |
| Pencil | 60° / 512 | 29.71–30.07 | 29.71–30.07 | 73.83–75.77 ms | 73.83–75.77 ms | Not met |
| Charcoal | 60° / 512 | 77.65–79.47 | 77.48–79.47 | 29.79–31.61 ms | 29.79–31.91 ms | Met at this pose and size |

Raw records, screenshots, environment data, source hashes and the measured APK
are in `artifacts/tilt-conversion/android/performance/`. These measurements
qualify only the listed motions on the low-tier reference device.

### Dry-media endpoint and turn completion, 2026-10-24

Matched control `6a7d3b1042d0821ee5823ddd7c96acba8188abf7` against localized
completion, with shared radial integrals, separate broad/fine variants and
zero-deficit tile rejection. The measured APK SHA-256 is
`1ca3ab54f3fbb7950222b0955519e41e381f739b706e167d3dd3465cc2027a53`;
its shader generation is `bfcf62144bad4e57`. The control's native library was
frozen while adding the same benchmark paths and capture controls.
The deployed build refreshes the 14 dry-brush preview assets with a byte-identical
native library and the same shader generation; its APK SHA-256 is
`5b1ae164f95c43851e0782afd5ff1ee46acfdba6f4c8781d716fa300792ab44d`.

TCL TAB 11 Gen 2, Mali-G52, 4248 × 2832 photo with an empty paint layer,
SharedDemandRefresh, Stats closed, pressure 1, tilt 45°, thermal status 0.
Each row has three six-second contacts after readiness and priming. Broad
contacts use nominal Size 1024 at Fit 0.343538. Both presets' geometric contacts
are 1024 × 665.6 document pixels at this pose, about 351.8 × 228.7 surface pixels;
material engagement can occupy less. Continuous and compact zigzag paths have
60 × 10 surface-pixel radii; the wide zigzag uses radius 180 in X.
The fine Pencil uses Size 9 at zoom 1, with a 9 × 5.85-pixel geometric contact.
Physical viewport is 1200 × 1920 with
three quarter-turns, logical viewport 1920 × 1200. These matched measurements
do not replace earlier profiles at different Fit scales.

Rates below count completed nonempty canvas updates, not panel presents. Gaps
are the worst per-contact p99 between fresh-input completions. Raw data also
records fresh-input rates, latency and settings. Values under 60/s or gaps over
33.3 ms do not qualify, including cases where the control already missed.

| Workload | Brush | Control updates/s | Completion updates/s | Median change | Fresh gap, control → completion | Completion verdict |
| --- | --- | --- | --- | --- | --- | --- |
| Continuous curve | Pencil | 72.33–73.28 | 68.17–68.72 | −6.36% | 25.44 → 33.08 ms | Meets measured profile |
| Continuous curve | Charcoal | 69.35–70.67 | 65.61–67.67 | −4.67% | 25.70 → 35.45 ms | **New gap miss** |
| Straight | Pencil | 87.50–90.61 | 89.59–89.93 | +0.11% | 17.80 → 16.20 ms | Meets |
| Straight | Charcoal | 87.68–88.52 | 87.61–88.99 | −0.03% | 15.27 → 16.13 ms | Meets |
| Compact zigzag | Pencil | 72.12–72.54 | 70.79–71.37 | −2.00% | 27.78 → 35.94 ms | **New gap miss** |
| Compact zigzag | Charcoal | 71.25–72.62 | 69.41–70.87 | −2.21% | 28.21 → 37.89 ms | **New gap miss** |
| Frequent lifts | Pencil | 49.80–52.39 | 46.71–48.65 | −6.30% | 102.83 → 105.72 ms | **Both builds miss** |
| Frequent lifts | Charcoal | 48.82–49.86 | 43.01–44.57 | −13.16% | 105.81 → 115.69 ms | **Both builds miss** |
| Wide zigzag | Pencil | 54.46–55.39 | 53.31–53.66 | −2.69% | 42.11 → 50.62 ms | **Both builds miss** |
| Fine zigzag | Pencil | 186.62–190.50 | 190.46–192.00 | +0.37% | 9.97 → 8.68 ms | Meets |

| Workload | Brush | Fresh input updates/s, control → completion | Input-to-GPU p99, control → completion |
| --- | --- | --- | --- |
| Continuous curve | Pencil | 72.16–73.28 → 68.17–68.72 | 55.55–58.80 → 58.90–61.54 ms |
| Continuous curve | Charcoal | 69.35–70.67 → 65.44–67.67 | 55.58–59.50 → 64.15–66.87 ms |
| Straight | Pencil | 87.16–90.61 → 89.59–89.93 | 41.11–55.74 → 44.32–56.25 ms |
| Straight | Charcoal | 87.68–88.52 → 87.61–88.99 | 45.00–50.40 → 43.58–52.12 ms |
| Compact zigzag | Pencil | 71.95–72.54 → 70.62–71.37 | 59.42–61.08 → 68.51–69.58 ms |
| Compact zigzag | Charcoal | 71.08–72.62 → 69.41–70.87 | 60.90–63.98 → 68.92–70.61 ms |
| Frequent lifts | Pencil | 35.69–39.08 → 33.37–34.53 | 99.91–104.20 → 104.76–107.29 ms |
| Frequent lifts | Charcoal | 33.88–35.91 → 29.55–33.09 | 103.04–106.88 → 113.62–120.34 ms |
| Wide zigzag | Pencil | 54.46–55.39 → 53.31–53.66 | 85.68–87.01 → 93.42–94.99 ms |
| Fine zigzag | Pencil | 185.13–188.17 → 188.14–190.17 | 16.81–23.22 → 16.96–21.08 ms |

Completion keeps straight/fine throughput near control, but is **not fully
performance-qualified**. Broad Charcoal curves and compact zigzags introduce
gap misses; frequent Charcoal lifts have the largest measured throughput cost.
Other dry presets, mid/top tiers and physical pen-to-scanout latency remain
unqualified by this comparison. This is the fastest tested implementation that
passed the pixel requirements, not evidence of zero overhead.

The selected-brush readiness test passes for all seven dry presets. The measured
build's first observed selections were 241.69 ms for Pencil and 273.11 ms for
Charcoal. After final packaging, Pointy Pencil takes 13.35 s and Chalk 4.50 s on
first observed selection, despite retaining app data; repeat selections range
from 222 to 273 ms. Cache coverage therefore varies, and these are not
cold-driver compilation costs or a controlled compile-time comparison. The
debug host test separately requires all pressure/tilt/size variants to be
prepared before painting. Ordinary shader modules omit completion, and broad
completion modules omit fine filtering.

A controlled application-cold comparison remains unmeasured. Android's
`pm clear --cache-only` did not return within the bounded attempt. Full app-data
clearing was rejected before execution by automatic approval review to preserve
settings/documents; the safer shader-cache-only fallback failed because the
benchmark package is not debuggable by `run-as`. The final APK was restored.
These tooling failures provide no evidence about the relative compilation cost.

Eight cursor-free TCL captures cover straight/zigzag paths, Size 9/1024, and
light/dark themes. They show firmer endpoints and outside turns with retained
grain. Independent GPU tests check tiny pixel-area coverage, nonflat chord
exposure, cross-tile turns, retracing, shrinking excursions, batching and replay.
Evidence: `artifacts/dry-media-completion/` and
`/mnt/localssd/capycanvas3-dry-completion-20261024/candidate5/`, including
`final-comparison.{md,json}` and the per-contact records.

### Dry-media contact model

Measured on 2026-10-17 UTC with dry kernels specialized to normal blending and
zoomed-out dry relief previews evaluated at display resolution, applied to the
control `c674ee521`. The builds alternate for two passes each with the contained
45° profile described below; the Smoothing 100% row sets Curve fit to its
maximum, which lengthens the predicted tail. Optimized APK SHA-256
`1c8474b097ad06cff2f84aff7eee75be6025f09a739187344eddfc823c097a1f`, control
`4159a2cb7a267538ee3a0ac46d5bc8830686e04e70c07e69629e7b12a755f3ed`. Thermal
status was zero in every record. Ranges cover all six contacts; gaps are the
worst per-contact p99 between fresh completions.

| Brush and Size | Before completed updates/s | After completed updates/s | After fresh input updates/s | After worst fresh p99 gap | After input-to-GPU p99 | 60/s, 33.3 ms gate |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 1024 px | 43.37–44.25 | 86.92–90.00 | 86.92–90.00 | 21.89 ms | 47.21–60.11 ms | Meets |
| Pointy Pencil 1024 px | 49.32–50.20 | 90.69–94.95 | 90.52–94.95 | 20.63 ms | 39.06–59.12 ms | Meets |
| Charcoal 1024 px | 42.19–43.40 | 87.50–90.37 | 87.34–90.37 | 22.59 ms | 52.59–62.37 ms | Meets |
| Pastel Block 1024 px | 33.36–33.95 | 72.83–74.58 | 72.69–74.58 | 31.83 ms | 57.45–69.61 ms | Meets |
| Pencil 1024 px, Smoothing 100% | 35.41–36.09 | 82.99–85.92 | 82.82–85.92 | 23.37 ms | 43.91–61.54 ms | Meets |

First selection after clearing app data (`--probe-presets 2,25,6`, three runs)
takes Pencil 2.52 → 1.81 s, Pointy Pencil 1.96 → 1.32 s and Chalk 0.95 → 0.72 s;
Chalk's background compile order varies between runs. Raw runs are under
`artifacts/dry-preview-speed/tcl/`. The earlier 2026-10-17 rows below measure
the previous build.

Measured on 2026-10-17 UTC with the one-kernel dry evaluator, two fine-tip
nodes from 4 px minor radius, Curve fit vertices bounded by 1% of the contact
radius and five-point Gauss–Legendre for short segments, applied to the control
`1300ba9f2`. The builds alternate for
two passes each with the profile described below, and ranges cover all six
contacts. Optimized APK SHA-256
`c8db6e1f2a909dea4f5d2b1266066d5c59bc6eb99c1ec9785adb4c24685c56c9`, control
`3a7340bdd1fdf0fc9d78f50c5cca810bd5c9226d9d066e6edb1abe5263439b4d`. Thermal
status was zero at every install. Gaps are the worst per-contact p99 between
fresh completions.

| Brush and Size | Before completed updates/s | After completed updates/s | After fresh input updates/s | After worst fresh p99 gap | After input-to-GPU p99 | 60/s, 33.3 ms gate |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 1024 px | 30.75–31.72 | 44.02–44.76 | 44.02–44.76 | 32.85 ms | 82.33–84.65 ms | **Not met** |
| Pointy Pencil 1024 px | 40.51–41.43 | 49.33–50.29 | 49.33–50.29 | 28.89 ms | 72.52–84.18 ms | **Not met** |
| Charcoal 1024 px | 28.88–29.56 | 42.55–43.55 | 42.55–43.55 | 33.48 ms | 84.19–87.99 ms | **Not met** |
| Pastel Block 1024 px | 21.13–24.63 | 29.27–30.07 | 29.27–30.07 | 56.81 ms | 136.48–147.81 ms | **Not met** |

Selecting a brush after clearing app data (`--probe-presets 2,25,6`, three runs)
takes Pencil 9.00 → 2.49 s, Pointy Pencil 8.45 → 1.96 s and Chalk 4.06 → 0.99 s;
a 36 px size change then takes 3.57 s → 79 ms. Raw runs are under
`artifacts/dry-kernel-merge/tcl/`, beside the summary script and the benchmark
wrapper that passes tilt and opacity. The 2026-10-13 rows below measure the
earlier build.

Measured on 2026-10-13 UTC, based on `cbc74b304`, with calibrated material
pressure, restored Material hardness and compile-time aggregate specialization.
Optimized APK SHA-256
`66f517b7c867a83caee591780226ff36591bf246bb3b3fd6ba04ccd7239d6020`,
JNI `7de0e1d01ae832bcb4bd91b0ed7157052f24d3668439edbdc299c50d54c7e49b`;
frozen source `04adb5a414d5fee9148980b56487e9d211e1dcb7b497185ee8faab955f287eda`.
Web Wasm `d5021a7bdcf1e784b28c3935fc75d372ed3a75a56a036b6083fe2e71838cd0cf`,
source `0952df064126eb924d5c2cc9d502645e6c3b53358d35cc9c2a051e2e7d1035b1`.
Production matches between these builds. Final source changes only trailing
WGSL whitespace and tests/docs after measurement.

The 4248 × 2832 tier photo has one empty Full color paint layer above it at Fit,
with the default workspace, Navigator, closed Stats and 16 ms prediction. Each
warmed profile uses three six-second 200 Hz pen contacts at physical pressure
one and an explicitly verified contained 60 × 10 surface-pixel ellipse. Each
contact starts on fresh paint after Undo. Thermal status is zero before/after.
Size is 1024 px, the writing-pose full-pressure long axis. Strength, Opacity and
Texture strength are 100%; Material hardness is Pencil 90%, Pointy 95%, Charcoal
55% and Pastel 50%. Smoothing retains its default 6%.

Native counts nonempty GPU-completed canvas updates inside each input window;
fresh input rates equal these counts in every measured contact. Web counts the
first asynchronous GPU completion consuming a new paint timestamp. Browser
scheduling is included; physical scanout is unmeasured. Gap values are the worst
per-contact p99, excluding idle intervals between contacts. Native and Web runs
do not overlap.

| Host and profile, Size 1024 px | Current updates/s across three contacts | Worst contact p99 gap | 60/s, 33.3 ms gate |
| --- | --- | --- | --- |
| Android Pencil, 45° | 30.46–30.94 | 43.26 ms | **Not met** |
| Android Pastel Block, 45° | 21.27–23.43 | 59.95 ms | **Not met** |
| Android Pointy Pencil, 45° | 39.91–40.41 | 33.05 ms | **Not met** |
| Android Charcoal, 45° | 28.92–29.12 | 45.10 ms | **Not met** |
| Android Pastel Block, 77° | 3.32–3.44 | 483.20 ms | **Not met** |
| Web Pencil, 45° | 21.77–22.30 | 112.60 ms | **Not met** |
| Web Pastel Block, 45° | 17.67–17.95 | 98.20 ms | **Not met** |
| Web Pointy Pencil, 45° | 23.45–24.43 | 109.50 ms | **Not met** |
| Web Charcoal, 45° | 21.79–22.62 | 102.60 ms | **Not met** |
| Web Pastel Block, 77° | 2.95–6.40 | 451.60 ms | **Not met** |

All current profiles miss the rate; native Pointy's gap meets 33.3 ms.
Native input-to-GPU p99 maxima are Pencil 113.16 ms, Pointy 91.57 ms,
Charcoal 120.30 ms, Pastel writing 161.78 ms and Pastel side 1230.37 ms.
Side Pastel spans 3276.8 × 563.2 document pixels and fits fully inside the photo
on this contained path. Web side captures retain three pending tail callbacks
in two contacts; these fall outside the input window and do not increase its rate.
Earlier broad-path probes use different trajectories and are diagnostic only.

Separate same-APK native diagnostics set Smoothing to 0% using the visible
control and verify zero in all post-contact states. Pencil reaches 54.80–55.52/s,
with worst gap 26.88 ms and input-to-GPU p99 78.56 ms: the rate still misses.
Pastel writing reaches 45.02–45.35/s, gap 36.69 ms and input-to-GPU p99 90.24 ms;
both rate and gap miss. These isolate substantial Curve fit work; the current
qualification rows keep default 6%. Other tilts, partial Opacity, Shading Pencil,
Chalk and Crayon at guaranteed sizes remain unmeasured.
Raw contacts, actual settings, thermal records, corrections and source/binary
proof are under `artifacts/dry-pressure-balance/tcl/final-qualified/`.

#### Dry shader preparation

Actual foreground Web first-selection readiness improves from Pencil 38.55 to
14.80 s, Pointy 28.71 to 11.43 s, and Chalk 12.49 to 4.73 s. Other dry families
reuse prepared variants in 181–274 ms; repeated selections take 164–254 ms
without new pipelines. Traces retain actual shader sources and asynchronous
pipeline callbacks. Device driver caches remain intact, so these are fresh
application pipelines, not cold driver-cache measurements. The before source
and identities are under `artifacts/dry-pressure-balance/tcl/shader-latency/`.

Native selected-brush probes, after photo/global startup is primed, measure
Pencil 35.53 s, Pointy 35.04 s and Chalk 13.82 s; other first probes take
30–106 ms, repeats 28–140 ms. All fourteen probes finish within their 60 s
fixture gate. First-use waits remain substantial; this does not qualify an
instant-selection or startup latency target.

All seven intended-size native dry strokes have nonempty pixels and exact
Undo/Redo in both themes. The canonical Strength popup also passes both themes
on Web and native. These functional checks do not qualify nominal-size FPS.

Earlier G-Pen contained-path measurements use APK
`e1e4ccab8f48a05c8291b9d450944a897fc36da95b00347fd29b376f25b4739c`
and Wasm `80a2c1245d7c536a48718a4d3e8c36143190e6f736c476e83165188bf676f7a4`.
Android reaches 82.95–83.91 fresh/s at Opacity 100% and 71.48–71.99 at 50%,
with p99 gaps at most 20.44 and 25.55 ms. Web reaches 19.46–23.07 and
21.72–23.39/s, with gaps at most 137.20 and 130.70 ms. These earlier builds
predate Curve fit and do not qualify the current binary.

### Retained-photo brush measurements

Except for G-Pen, Eraser and the current dry-media rows, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets.

Multiply Glaze and the very complex material rows were measured on 2026-10-19 with the same profile, using a release benchmark build of `66c1142ee`, which runs every material brush as compute kernels; their gap is the completion-gap p99.

Smudge, Natural Blender, Soft, Blur, Bristle, Grainy, the Blending Knife, Blending Stump and Tissue were measured on 2026-10-20 at `68012f20a`, after [paper-aware blending](../history/brushes/paper-aware-blenders.md): six runs from two candidate sets interleaved with a control of the previous `main` (three runs for Blending Stump). Soft Blender and Tissue were measured again at `1136a47c9`, which compiles lifting only for the Tissue. Updates per second are quantized at these rates, so the comparison used each run's median queue-present time: Soft Blender's stayed 4–6% above the control's (681 → 720 ms), inside the 630–812 ms spread between runs of one build. The control completed Smudge 1.6, Natural Blender 1.1, Soft 1.6, Blur 1.9, Bristle 1.5 and Grainy 1.5 updates/s; it crashed during the second Blending Knife stroke in both control sets, before Wet Round, and completed a lone Blending Knife run without crashing. The other blenders (presets 49–55 and 57–62) are unmeasured on this tier, and the wet rows predate the blender suite. Liquify uses the separate Photo-layer profile below.

Marker, Paintbrush, Textured Flat, Dry Scumble and Transparent Glaze were measured on 2026-10-21 after Uniform contacts stopped counting the pose they share twice ([paint state](../reference/painterly-paint-state.md)): six strokes from two candidate sets interleaved with a control of the previous `main` (`c12ec63d2`), which completed Marker 13.96, Paintbrush 7.98, Textured Flat 9.37, Dry Scumble 0.80 and Transparent Glaze 1.60 updates/s. The control's strokes sped up after a lap or two, once the doubled coats brought the ellipse to full coverage and its pixels skipped their contacts. Each pass now lays one coat, so a pixel the ellipse crosses again costs as much as on its first pass. In the first second of a 512 px stroke, both builds completed the same number of updates.

- The older fixtures fit their 520 × 299 px ellipse to 339 × 299 px in the
  TCL's 754 px work area at 16.0% zoom. The current G-Pen row uses the 240 × 120 px semiaxes documented in
  the current comparison above.
- Simple brushes are measured at their guaranteed 1024 px.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1024 px | 42.15 (41.89–42.72) fresh input-attributed updates/s; completion-gap p99 35.28–37.97 ms | **Not met**; [current controls](#brush-and-pending-filter-controls) |
| G-Pen (1), brush controls | Simple | 1024 px | 82.95–83.91 fresh updates/s; fresh gap p99 ≤20.44 ms, contained 60 × 10 path | Earlier measured Android profile; [earlier controls](#dry-media-contact-model) |
| G-Pen (1), M3 BUILD32 | Simple | 1024 px | Both orders: 64.295–65.713 fresh updates/s; fresh gap p99 24.960–27.643 ms, Linear | Meets this stroke; common response excess repeats: forward **+1.007 ms**, reverse **+1.192/+1.778 ms**; presentation unmeasured; [BUILD32](#build32-g-pen-comparison) |
| G-Pen (1), M3 BUILD28 | Simple | 1024 px | BUILD28: 64.926–65.173 fresh updates/s; fresh gap p99 25.959–27.596 ms, Linear | Meets this stroke and matched CPU/response bounds; presentation unmeasured; [BUILD28](#build28-g-pen-comparison) |
| Rough G-Pen (28) | Simple | 1024 px | 25.6 updates/s (25.5–25.7); gap p99 69.5 ms | **Not met** |
| Calligraphy Pen (29) | Simple | 1024 px | 91.0 updates/s (90.7–91.5); gap p99 37.1 ms | **Not met** |
| Antique Pen (30) | Simple | 1024 px | 37.5 updates/s (37.1–38.0); gap p99 79.9 ms | **Not met** |
| Realistic Pen (31) | Simple | 1024 px | 28.3 updates/s (28.2–28.3); gap p99 63.4 ms | **Not met** |
| Wet Ink (32) | Simple | 1024 px | 23.0 updates/s (22.9–23.1); gap p99 73.7 ms | **Not met** |
| Pencil (2) | Simple | 1024 px | 40° writing pose: 66.83–67.50 fresh updates/s; fresh gap p99 ≤34.13 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Pointy Pencil (25) | Simple | 1024 px | 40° writing pose: 67.06–68.48 fresh updates/s; fresh gap p99 ≤52.11 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Shading Pencil (26) | Simple | 1024 px | 40° writing pose: 71.27–71.94 fresh updates/s; fresh gap p99 ≤35.33 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Charcoal (27) | Simple | 1024 px | 40° writing pose: 65.58–66.71 fresh updates/s; fresh gap p99 ≤35.12 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Chalk (6) | Simple | 1024 px | 40° writing pose: 30.40–30.76 fresh updates/s; fresh gap p99 ≤213.87 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Crayon (43) | Simple | 1024 px | 40° writing pose: 63.87–65.25 fresh updates/s; fresh gap p99 ≤35.63 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Eraser (3) | Simple | 1024 px | 29.9 fresh updates/s (29.71–29.95); completion gap p99 44.67–45.06 ms | **Not met** |
| Airbrush (5) | Simple | 1024 px | 32.7 updates/s (32.4–33.1); gap p99 55.6 ms | **Not met** |
| Marker (7) | Complex | 1024 px | 2.05 updates/s (1.90–2.20); gap p99 706.2 ms | **Not met** |
| Blotty Ink (33) | Complex | 1024 px | 15.1 updates/s (15.0–15.1); gap p99 105.5 ms | **Not met** |
| Realistic Brushed Ink (34) | Complex | 1024 px | 14.3 updates/s (14.2–14.4); gap p99 178.0 ms | **Not met** |
| Pastel Block (17) | Complex | 1024 px | 40° writing pose: 34.95–35.59 fresh updates/s; fresh gap p99 ≤107.44 ms | **Not met**; [physical tilt](#physical-tilt-response) |
| Paintbrush (4) | Complex | 1024 px | 0.70 updates/s (0.70–0.70); completion-gap p99 2054.0 ms | **Not met**; [layer color modes](#layer-color-modes) |
| Textured Flat (15) | Complex | 1024 px | 0.80 updates/s (0.80–0.90); gap p99 1691.0 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 0.70 updates/s (0.70–0.70); gap p99 2431.2 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 1.20 updates/s (1.20–1.20); gap p99 1058.0 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 16.6 updates/s (16.3–16.6); input 16.6/s; completion-gap p99 86.0 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 2.6 updates/s (2.5–2.7); gap p99 559.5 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 7.4 updates/s (7.3–7.7); gap p99 217.2 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | 1.2 updates/s (1.2–1.2); input 1.2/s; completion-gap p99 1130.5 ms | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | 1.1 updates/s (1.1–1.2); input 1.1/s; completion-gap p99 1858.7 ms | **Not met**; [layer color modes](#layer-color-modes) |
| Wet Watercolor (21) | Very complex | 512 px | 1.0 updates/s (1.0–1.0); input 1.0/s; completion-gap p99 2049.0 ms | **Not met** |
| Loaded Oil (22) | Very complex | 512 px | 1.2 updates/s (1.2–1.2); input 1.2/s; completion-gap p99 1122.9 ms | **Not met** |
| Palette Knife (23) | Very complex | 512 px | 1.2 updates/s (1.2–1.2); input 1.2/s; completion-gap p99 1613.5 ms | **Not met** |
| Wet Round (11) | Very complex | 512 px | 1.6 updates/s (1.6–1.6); input 1.6/s; completion-gap p99 913.0 ms | **Not met** |
| Natural Blender (24) | Very complex | 512 px | 1.2 updates/s (1.1–1.2); input 1.2/s; completion-gap p99 1083.3 ms | **Not met** |
| Smudge (10) | Very complex | 512 px | 1.7 updates/s (1.6–1.7); input 1.7/s; completion-gap p99 917.0 ms | **Not met** |
| Soft Blender (45) | Very complex | 512 px | 1.5 updates/s (1.5–1.6); input 1.5/s; completion-gap p99 1004.3 ms | **Not met** |
| Blur (46) | Very complex | 512 px | 2.2 updates/s (2.2–2.5); input 2.2/s; completion-gap p99 727.9 ms | **Not met** |
| Bristle Blender (47) | Very complex | 512 px | 2.2 updates/s (2.2–2.3); input 2.2/s; completion-gap p99 768.0 ms | **Not met** |
| Grainy Blender (48) | Very complex | 512 px | 1.6 updates/s (1.6–1.7); input 1.6/s; completion-gap p99 947.1 ms | **Not met** |
| Blending Knife (56) | Very complex | 512 px | 0.7 updates/s (0.7–0.7); input 0.7/s; completion-gap p99 2212.5 ms | **Not met** |
| Blending Stump (65) | Very complex | 512 px | 1.8 updates/s (1.7–1.8); input 1.8/s; completion-gap p99 917.2 ms | **Not met** |
| Tissue (66) | Very complex | 512 px | 1.6 updates/s (1.6–1.6); input 1.6/s; completion-gap p99 960.3 ms | **Not met** |
| Liquify Push (12) | Very complex | 512 px | 22.49 fresh updates/s (22.09–23.75); fresh completion-gap p99 ≤96.27 ms | **Not met** |
| Liquify Twirl Clockwise (36) | Very complex | 512 px | 18.36 fresh updates/s (16.57–18.73); fresh completion-gap p99 ≤170.14 ms | **Not met** |
| Liquify Twirl Counterclockwise (13) | Very complex | 512 px | 18.11 fresh updates/s (17.08–18.31); fresh completion-gap p99 ≤158.49 ms | **Not met** |
| Liquify Pinch (37) | Very complex | 512 px | 29.71 fresh updates/s (29.27–32.24); fresh completion-gap p99 ≤112.90 ms | **Not met** |
| Liquify Expand (38) | Very complex | 512 px | 31.10 fresh updates/s (30.73–31.65); fresh completion-gap p99 ≤101.72 ms | **Not met** |
| Liquify Crystals (39) | Very complex | 512 px | 21.28 fresh updates/s (20.90–21.34); fresh completion-gap p99 ≤84.49 ms | **Not met** |
| Liquify Edge (63) | Very complex | 512 px | 24.50 fresh updates/s (23.93–24.94); fresh completion-gap p99 ≤125.31 ms | **Not met** |
| Liquify Reconstruct (64) | Very complex | 512 px | 34.84 fresh updates/s (34.66–36.60); fresh completion-gap p99 ≤65.94 ms | **Not met** |

### Blenders on populated photographic paint

Measured on 2026-10-21 on the TCL TAB 11 Gen 2, with the blender correctness
changes on `e16049138`: `material_blend.wgsl` SHA-256 starts `d4aab206a8b2`,
`material_transport.wgsl` starts `0477a69071a1`, and the optimized benchmark APK
starts `a3025b3a7a43`. Rust 1.99.1 uses the release profile and R8 optimization.
These measurements select the populated `Photo` paint layer itself at 100%
opacity, with hidden Paper, rather than an empty paint layer above it. They do
not establish the ordinary empty-layer guarantee or a comparison with older rows.

The 4248 × 2832 sRGB8 Sony downscale uses Perceptual blending, Fit zoom 0.343538,
the default Sketch workspace and glass, Stats closed, and no tracing. Each
512 px preset keeps its default settings, including Grainy's paper strength 1,
with 16 ms prediction and a contained 310 × 150 px ellipse at pressure 1 and
tilt 0. Pipelines are warm and a priming stroke is undone before three 10 s
OS-injected 200 Hz strokes. Thermal status is 0 before and after each preset.

| Preset | Completed updates/s, median (range) | Fresh input updates/s, median (range) | Completion gap p99 range | Fresh input gap p99 range | 60/s and 33.3 ms limits |
| --- | --- | --- | --- | --- | --- |
| Grainy Blender (48) | 3.335 (3.333–3.493) | 3.335 (3.333–3.493) | 455.606–517.276 ms | 455.606–517.276 ms | **Not met** |
| Color Mixer (58) | 1.497 (1.397–1.499) | 1.497 (1.397–1.499) | 1133.566–1146.939 ms | 1133.566–1146.939 ms | **Not met** |
| Blending Knife (56) | 0.599 (0.599–0.699) | 0.599 (0.599–0.699) | 1732.741–1739.907 ms | 1732.741–1739.907 ms | **Not met** |

Raw inputs, completion records, setup state, screenshots, source hashes and the
measured APK are retained in `artifacts/blender-audit-current/tablets/low/` and
its parent. These are GPU-completed canvas updates, not scanout measurements.
The photographic paint is opaque; transparent-edge and HDR correctness are
separate renderer fixtures.

### Retained-map Liquify

Measured on 2026-10-20 on the TCL TAB 11 Gen 2, Mali-G52 MC2, with the
retained-map implementation based on `b7fb3654a`. Benchmark APK SHA-256:
`7a5a27c61b93890c744413a0f31a4d6c54c074377884af3ac8e7c3bb2f115c05`.
The eight rows above use the 4248 × 2832 photo, Size 512, full pressure,
16 ms prediction and three five-second contacts after shader warm-up.
`--paint-layer-name Photo` selects the photo itself. Reconstruct first seeds
its map with a five-second Push stroke; each measured contact is undone before
the next. Every mode changes visible canvas pixels.

The 300 × 180 screen-pixel semiaxes run at Fit zoom 0.343538 on the
1920 × 1200 surface. Sketch is collapsed and Stats is closed; thermal status
is zero. A post-run display dump lists only a 60 Hz mode; hardware refresh
was not captured per trial. Values are median completed updates consuming fresh
input per second with the three-run range, and the worst run's fresh completion-gap p99.
Completed nonempty and fresh-input rates are equal in these runs. They do not measure
physical scanout or pen-to-display latency. Every mode misses both 60/s and
the 33.3 ms gap limit. The previous empty-layer measurements exercise map
geometry but do not qualify deformation of photo pixels.

The diagnostics and broad-coverage check below use APK
`933d671b62210326655307c6a85e7601c51ba5f670e073757c7a047e6aee79fe`,
with the same Push/Twirl map arithmetic, twelve-source chunks and allocation
rules. Separate Stats-enabled diagnostics attribute Push's median 48.25 ms GPU
interval to map updates at 22.30 ms, reconstruction at 8.15 ms, main composition
at 4.11 ms and display mips at 0.89 ms. Counterclockwise Twirl takes 45.56 ms,
with 28.92 ms in map updates and 6.99 ms in reconstruction. Phase timings nest
and must not be summed. Unchanged preview pages remain visible without
regenerating their mips or composition; deformation is now the largest measured
cost. These instrumented runs are separate from the qualification rates above.

Guarded broad-coverage Push completes all six planned full-width stripes
without GPU loss or out-of-memory failure. Tracked GPU allocation grows from
846 to 1317 MiB and MemAvailable falls from 2203 to 1677 MiB. The geometric
support upper bound reaches all 204 tile cells; it does not count unique
deformed pixels. Undo takes 231 ms and continued moving Push 507 ms.
History-budget eviction removes the older map, making Reset unavailable after
Undo; drawing remains usable. Undoing the coverage and trimming redo reduces
tracked GPU allocation to 884 MiB. Ordinary-stroke pen-up settlement reaches
856 ms for Push, including momentum and publication.

## Integrated compositor measurement

Measured on 2026-09-30 against `ba8835fec`: Perceptual 12 MP photo,
240 × 140 px trajectory, 16 ms prediction, three five-second strokes after
warm-up. Stats is closed, Navigator is visible and thermal status is zero.
Fresh updates count completed frames that consumed new real pen samples.
This historical candidate separates the front layer from the balanced lower composite and
skips color reads for fully covered in-place uniform Normal pixels. It keeps
the same formats, memory allowance and existing pipelines.

| Workload | Main fresh updates/s | Candidate fresh updates/s (range) | Candidate completion gap p99, range | Target |
| --- | ---: | ---: | ---: | --- |
| G-Pen 1024 px, one photo at Fit | 54.81 | 65.70 (65.59–66.10) | 26.12–28.57 ms | Meets 60/s for this stroke |
| G-Pen 1024 px, eight photos at 50% | 29.88 | 35.67 (35.65–35.83) | 39.05–39.78 ms | Not met |
| G-Pen 1024 px, eight photos at 100% | 19.50 | 27.05 (27.02–27.05) | 44.06–44.75 ms | Not met |
| Pencil 1024 px, one photo at Fit | 11.14 | 11.14 (11.08–11.36) | 107.46–113.63 ms | Not met |
| Eraser 1024 px, one photo at Fit | 30.06 | 29.91 (29.71–29.95) | 44.67–45.06 ms | Not met |

The stacked rows have nine visible layers: one opaque photo, seven translucent copies and one
paint layer. They do not qualify the separate eight-paint-layer target. Fit is
15.97%. Candidate allocator residency is 812 MiB at Fit, 1,069 MiB at 50% and
1,008 MiB at 100%, within 2 MiB of the matched controls. G-Pen owner CPU p50 is
10.37 / 14.64 / 13.15 ms; these are CPU times, not frame or GPU intervals.
Settling is 590–613 / 930–961 / 700–736 ms. The size-class guarantee remains
open because other brushes and trajectories are not qualified.

Candidate: `c452a0642` (production source matches `fc5d00fd5` after the
test-fixture rebase), optimized benchmark APK SHA-256
`865b0dd05b8253b3b22eac806136023a5befeac5b642e5fddb2f3cd24194d047`.
Raw records are `artifacts/optimization-roi/{current-main,final}-low-fit`,
`{current-main,final}-stack{50,100}` and `main-other-simple`. Pencil and Eraser
controls use `829f1e223`; their rates remain within 1% of the candidate.
A neighboring seven-photo check at 100% reaches 27.12/s versus 25.94/s on
`829f1e223`; raw records are `{main,candidate1}-stack7-100`.

The preceding two-page idle-refinement comparison (2026-09-29) passes the
one-frame added-submission p95 budget in all three runs; added GPU-completion
p95 still fails in one run. See
[responsiveness](responsiveness.md#resuming-during-refinement). Its raw records
are `artifacts/latency-investigation/two-25-stack-{50,100}-{constant,pauses}`.
This does not qualify interruption after the current shader and graph changes.
In that comparison, smaller idle batches improve interruption while increasing total settling time;
Fit settling is 590–613 ms here versus 478–544 ms on the current main control.

### Painting below the front layer

Measured on 2026-09-30 on `c82970142` with a benchmark-only paint-position
control. Both optimized release builds keep the covered-pixel shader guard,
transparent-paper removal, memory budget and two-page refinement. The control
changes only the root from `combine(front, over(back))` to `over(all)`.
These are three warmed five-second strokes per position: 12 MP Perceptual
photo, G-Pen 1024 px, 100% zoom, 240 × 140 px path, Stats closed, default
workspace and thermal status zero. Paint stays above the opaque bottom photo;
the other photos have 35% opacity.

| Visible layers | Paint position | Balanced root, fresh updates/s | Front-biased root, fresh updates/s | Front-biased completion gap p99, range |
| --- | --- | ---: | ---: | ---: |
| Eight: seven photos and paint | Top | 16.50 | 26.90 | 44.07–46.94 ms |
| Eight: seven photos and paint | Middle | 16.48 | 13.54 | 81.76–88.15 ms |
| Eight: seven photos and paint | Lowest above the opaque photo | 16.52 | 13.74 | 81.54–86.91 ms |
| Nine: eight photos and paint | Lowest above the opaque photo | 13.72 | 13.71 | 82.20–85.18 ms |

None meets the 60/s target. The isolated root bias costs 17–18% throughput
on the middle and lower eight-layer cases. It reduces the top dependency path
from three blends to one, while six other paths grow from three to four.
Uniform edits across the eight operands therefore average 3.5 rather than 3
blends. Nine operands already have the same root shape with either construction.
The earlier front-only rates do not establish a benefit for drawing on all
layers. Pixel-correctness and logarithmic-work tests do not establish that
tradeoff either. These results compare root shapes with identical other
optimizations, not lower-layer performance against the older main APK.

APK SHA-256: front-biased
`882a013a1209f542caf0442f056ab14143fac58722dfdc0de027ec0f8240d50e`;
balanced `9e3e455bc1446a4e414582791ed288e755c984736e197f6e39c19e962a3d3f88`.
Raw records and source provenance:
`artifacts/optimization-roi/all-layer-followup/tree-comparison.json` and
`{biased,balanced}-{7,8}-{top,middle,lower}` for the listed cases.

### Layer-neutral composition and refinement

The fixed front-operand preference is removed. The selected four-page build
retains balanced whole-run grouping, the covered-pixel guard and transparent-paper
exclusion. A regression checks bounded blend paths and exact pixels for top,
middle and lower edits across layer counts and blend spaces.

Measured on 2026-09-30: the same eight-visible-layer, 100% zoom conditions above,
with three warmed five-second strokes per position. Fresh updates/s are
16.56 at the top, 16.50 in the middle and 16.54 above the opaque bottom photo.
Completion-gap p99 ranges are 68.42–70.34, 69.30–70.66 and 67.75–71.65 ms,
respectively. All remain below the 60/s target. This verifies the selected
implementation across positions; the preceding matched root comparison
establishes the cost of the removed preference.

At Fit, one photo and G-Pen 1024 px now reach 63.69 fresh updates/s
(63.53–63.90), with completion-gap p99 27.07–28.31 ms. These three five-second
strokes use a 250 × 140 px path, Perceptual blending, 16 ms prediction, default
workspace, Stats closed and thermal status zero. This stroke meets the moving
target; it does not qualify the brush class or the eight-paint-layer target.

Four-page batches restore median settling to 496 ms versus 596 ms with two
pages, without a continuous-stroke rate change. Typical resumed GPU completion
increases from 56.6 to 59.0 ms. The two-page cap is removed; fresh-input queueing
behind one unfinished batch remains. See the controlled
[refinement tradeoff](responsiveness.md#refinement-batch-tradeoff).

Source base: `1557688aa` plus the layer-neutral root and four-page cap changes.
Optimized release benchmark APK SHA-256:
`05d747e5f3e6417e9ea3a96a0d1b5bfe38a3aca00744891bbdd01c9a37334a8b`.
Raw records, immutable APKs and source patches:
`artifacts/refinement-tradeoff/{four-overlap-low-constant,final-layer-{top,middle,lower}}`
and `provenance.json`.

### GPU attribution with Stats closed

A separate five-second ATrace diagnostic of the candidate's eight-photo 100%
case enables bounded GPU timestamps without opening Stats. Its 26.54 updates/s
is diagnostic, not a rate qualification. Observations match renderer frame IDs
inside the input window; independently aggregated medians do not add up, and
GPU intervals include scheduling gaps rather than measuring hardware occupancy.

| Phase | Median GPU interval |
| --- | ---: |
| Drawing, 134 observations | 26.77 ms |
| Paint | 3.47 ms |
| Prediction and uncommitted-contact preview | 7.71 ms |
| Main composition, excluding paired mip interval | 10.10 ms |
| Main mips | 6.50 ms |
| Separate overview composition | 0.005 ms |

The main mip rectangle contains 258.3 million base-level pixels across this
stroke; changed output regions contain 213.3 million, or 82.6% of that area.
There are typically 24 tiled output regions per update. Sparse mips could avoid
17.4% of this base-level area, but their dispatch and merge cost is unmeasured.
That area ratio neither establishes a GPU-time saving nor closes the 60/s gap.
Main composition, preview and viewport work remain material costs. None of these
measurements proves the target impossible or establishes a complete path to it.
This diagnostic predates the rebase and uses the candidate APK SHA-256
`f0b87ae5094b33d4383bfb3f9d0ab8d8699d1aa508ee7f6c1fe11edb6d261ad2`.
Raw records: `artifacts/optimization-roi/candidate1-stack100-diagnostic`.

## Input during Healing finalization

TCL navigation during large Healing/Spot Healing finalization has input-queue
p95 of 15.8–19.4 ms and a maximum of 45.0 ms. The largest settle callback is
58.7 ms. The one-second pinch probes present at 54.9–57.0/s; these do not qualify
the sustained navigation row. System available memory stays above 2,198 MiB.

Measured on 2026-09-29, three runs per brush on the tier photo, 512 px,
Perceptual, Fit and Stats closed. Dependent painting queues until the healed
raster publishes. The [responsiveness record](responsiveness.md#healing-finalization)
contains the build, workload, tool-action limits and raw records.

## Pinned localization comparison

Measured on 2026-10-01 PDT / 2026-10-02 UTC on the TCL reference tablet.
Baseline `271918681` is compared with source tree
`e56742a57787e6bf8dd6da3f4fecadcc718657a6`. Release, unminified APK SHA-256:
baseline `616b5b10d19eb4732d9631db122cbce49be7cf139fcdd3f61d24208e93ec36c8`,
candidate `33e88926fd66491d437f8397af08755c2ae5e4832ac8aaf4b0d06ab38e124e48`.
These observations do not qualify the later GPU-bounds successor.

The 4248 × 2832 Sony photo has one empty paint layer, Perceptual blending,
Fit zoom 15.97%, default Navigator and glass, Stats closed and 16 ms prediction.
Three warmed ten-second OS stylus strokes use G-Pen 1024 px and a contained
220 × 100 px trajectory. Camera, settings and visible layers match; painting
thermal status is zero before and after each run.

| Source sequence | Fresh completed updates/s | Maximum fresh gap p99 | Stroke criteria |
| --- | --- | --- | --- |
| Baseline | 66.89–67.32 | 26.56 ms | Met |
| Candidate | 66.27–67.26 | 26.57 ms | Met |
| Repeated baseline | 64.77–66.49 | 27.62 ms | Met |

Candidate painting ranges overlap the baseline drift; no regression is
demonstrated for this stroke. This does not qualify other brushes or scanout.

Three warmed ten-second slider drags use the same photo, empty paint layer and
Navigator, with Exposure on the photo or a Levels/Vibrance/Exposure chain.
The identical test fixture uses the photo workload on both builds.

| Numeric motion | Baseline UI Hz / maximum p99 | Candidate UI Hz / maximum p99 | Raw canvas completions/s, baseline → candidate |
| --- | --- | --- | --- |
| Exposure | 48.93–52.07 / 49.99 ms | 53.45–54.36 / 33.35 ms | 12.27–13.17 → 10.49–10.98 |
| Chain Exposure | 59.11–59.62 / 33.32 ms | 59.32–59.62 / 16.73 ms | 8.49–10.28 → 7.28–9.18 |

Direct Exposure misses the native UI floor on both builds. The chain meets
native UI cadence, but fresh photo preview remains **unqualified**. Completion
counts include Navigator, have no effect input/revision pairing and cannot
establish preview freshness. All three direct candidate runs have fewer
completions; sampled values and dynamic memory budgets also differ. Neither
causal attribution nor harmlessness is established. Numeric thermal status was
zero before each gesture; the final after-snapshot was not captured.

Raw records and complete APK/test-fixture provenance are retained under
`artifacts/reference-localization/`. The initial overflowing paint footprint
and hidden-Navigator numeric attempts are excluded and retained separately.

## Session checkpoints

TCL release-profile diagnostic, 2026-10-04, working changes based on
`a9180575e60670cdd61d73771674ece7f2096d9f`; APK SHA-256
`ea04f5478d3e28795a7bc1269d498e9dfbcb99d781377d490ddc4274b8cda13a`.
The reference 4248 × 2832 photo and a paint layer use Fit zoom and Navigator,
with the default workspace. Each workload warms up and runs three five-second
motions while the two-second session poll is enabled. The G-Pen uses 1024 px,
with a contained 100 × 80 surface-pixel ellipse.

Pan reaches 60.00–60.12 renderer submissions/s with p99 intervals of
18.70–18.92 ms. G-Pen reaches 77.6–79.0 fresh completed updates/s with completion
gap p99 23.36–24.07 ms. The process writes 262–418 KiB per pan gesture and
172–1,664 KiB per continuous stroke. Pan's session storage grows 14,604 bytes in
the first run, then stays nearly constant. Painting grows it by 0–275,422 bytes
per run. The newest checkpoint age is 2.05–2.07 seconds in steady navigation,
with a four-second initial maximum. Continuous contact defers capture until
release and reaches 5.03–6.51 seconds.

These runs collect process-wide kernel write counters and sample session files;
the counters include all process disk writes. Pan records renderer submissions,
not screen presentation. Thermal status was not recorded before these runs, so
neither workload qualifies its tier target. There is no matched baseline APK.
[Raw timing and checkpoint samples](../../artifacts/seamless-restart/android/viewport-benchmark/)
and summaries are under `artifacts/seamless-restart/android/`. Mid and top tiers
remain unmeasured for this change because their reference tablets were reserved
by other work.

The repeated-contact diagnostic uses the same photo, brush and layout with
50 contacts of 100 ms separated by 100 ms pauses in each of three intervals.
Actual intervals are 9.827–9.834 seconds. APK SHA-256 is
`5938506b8c553a5b451e2215254843301c2cb163167f5ba65e39857ed27a08b5`,
with working changes based on `0516be628`; thermal status before the run is 0.
Each interval writes 11.02–14.27 MiB process-wide and grows session storage by
2.08–2.36 MiB. Observed head age reaches 3.52–4.37 seconds. The sampler targets
100 ms; actual median gaps are 125–187 ms and maximum gaps are 149–222 ms.
Head modification time measures observed publication age rather than the fsync
completion instant. The paused workload and 136–144 GPU timing samples do not
qualify a moving-frame rate. [The short-contact summary](../../artifacts/seamless-restart/android/short-checkpoint-summary.json)
retains each interval and its raw source.

## Filter attachment feedback

Measured 2026-10-04 on TCL TAB 11 Gen 2, thermal status 0 before and after,
with the 4248 × 2832 reference photo at Fit, default workspace and panel glass,
one clipped empty paint layer, and attached Gaussian Blur and Curves.
`AndroidTitleBarTest#layerSwipeFrameTiming` with `layerReorderBenchmark` and
`layerRelationshipBenchmark` drags Curves through the neighboring filter gaps.
One priming gesture precedes three five-second mouse gestures; every drop is
canceled.

| Run | 1 | 2 | 3 |
| --- | --- | --- | --- |
| Native UI frames/s | 55.89 | 55.64 | 59.72 |
| Moving-frame interval p99, ms | 33.33 | 33.34 | 16.75 |

These `FrameMetrics` observations do not meet the tier target. They measure the
attached-filter workload, separately from ordinary row reorder. No matched
baseline was measured. The benchmark APK uses release Rust and unminified Kotlin
from `7f69a9356` plus the clipping-filter changes. APK/source hashes, raw frames,
display and thermal records, and the summary are in
`artifacts/clipping-filter/build-provenance.json`,
`artifacts/clipping-filter/low/` and
`artifacts/clipping-filter/motion-summary.json`.

## Image objects

Measured 2026-10-06 on the reference TCL with the frozen optimized arm64-v8a
build M2J (integration `110763374`), 4248 × 2832 canvas at Fit (15.97% zoom),
Navigator open, Stats closed and prediction enabled. Painting uses the 1024 px
G-Pen inside the canvas. Object rows open authored fixtures with one object
layer of four or eight images at 0.55 scale and 35% opacity above an opaque Photo
layer; shared fixtures use one image for every object and Photo, distinct
fixtures one per object. Each motion row keeps three ten-second contacts or
gestures after a priming one, and thermal status was 0 before and after every
case. The pre-M1 references are `d7f10b59e` with the same benchmark harness,
measured in the same session, alternating with M2J. Their placed-photo fixtures
put four or eight top-level photos at the same scale and opacity above Photo,
and their moved layer is the same photo copy translated by (37, 101).

The paint-only control, the unmoved and moved photo-copy rows and their
openings are from build M2K (`52b2da47e` on integration `c104948be`, which
reduces a moved layer's display levels from one prediction texel per texel while
it paints),
measured later the same day against pre-M1 in a session of its own. In that
session M2K painted above four distinct objects at 61.58 / 63.07 / 63.10 fresh
updates/s and moved four shared objects at 48.70 / 50.97 / 48.88 poses/s, as M2J
did.

| Painting workload | Fresh updates/s, contacts 1 / 2 / 3 | Gap p99, ms | Pre-M1 reference, fresh updates/s (gap p99, ms) | Result |
| --- | --- | --- | --- | --- |
| Paint-only control | 62.49 / 62.51 / 62.61 | 27.89 / 28.16 / 28.07 | 61.60 / 61.57 / 60.26 (27.68 / 28.34 / 28.76) | Meets the target; no regression |
| Paint above four shared objects | 62.48 / 63.26 / 62.49 | 26.16 / 26.67 / 25.97 | Four shared placed photos: 51.49 / 52.52 / 52.39 (30.40 / 28.58 / 27.92) | Meets the target; faster than pre-M1 |
| Paint above four distinct objects | 62.28 / 63.16 / 62.09 | 27.51 / 24.99 / 28.10 | Four distinct placed photos: 50.97 / 52.30 / 52.61 (29.86 / 29.82 / 29.34) | Meets the target; faster than pre-M1 |
| Paint below four shared objects, above Photo | 58.81 / 58.69 / 58.77 | 29.20 / 30.39 / 27.26 | Below two photo layers: 58.18 / 58.13 / 57.61 (26.25 / 29.95 / 28.68) | Rate below 60, as before M1 |
| Paint below four distinct objects, above Photo | 57.94 / 58.00 / 57.68 | 28.40 / 26.53 / 27.25 | As above | Rate below 60, as before M1 |
| Eight distinct objects after zoom and Fit | 57.79 / 57.64 / 56.55 | 30.31 / 29.69 / 30.77 | Not measured in this session | Rate below 60 |
| Paint on an unmoved photo copy | 52.95 / 53.42 / 52.96 | 34.37 / 31.29 / 33.48 | 51.16 / 51.82 / 51.40 (34.70 / 32.24 / 34.47) | Below 60; faster than pre-M1 |
| Paint on the copy moved to (37, 101), Fit | 53.01 / 53.49 / 53.87 | 33.26 / 33.32 / 31.11 | 41.70 / 41.68 / 41.51 (40.56 / 39.24 / 38.96) | Below 60; about 28% faster than pre-M1 |
| Paint on the copy moved to (37, 101), 100% zoom | 14.87 / 15.17 / 14.57 | 102.86 / 103.41 / 105.99 | 3.39 / 3.39 / 3.49 (333.90 / 321.12 / 335.70) | Below 60; about 4.4 times pre-M1 |

The eight-source row paints below the objects. In M2J's session the pre-M1
harness stopped at a check after opening eight placed photos, twice, so only its
opening settle is recorded; in M2F's session the same pre-M1 row painted at
49.68–50.40 fresh updates/s. With memory sampling, the eight-source row completes
all five contacts at 51.02–52.63 fresh updates/s (gap p99 30.69–36.85 ms),
peaking at 1,460,157 KiB process PSS and 900,086,772 B GPU allocation.

A second pass of the moved rows in the same session measured 53.07–53.64 fresh
updates/s at Fit and 14.87–15.17 at 100%, against pre-M1's 41.89–42.11 and
3.29–3.49. A moved and an unmoved layer record the same GPU passes per brush
frame; pre-M1 did not derive a display level for a placed layer and resampled
its pixels during composition, which blurred edges at offsets that are not a
multiple of the texel size. With Stats GPU timing, each phase averages over the
frames that ran it:

| GPU phase, ms per frame | M2K at Fit | Pre-M1 at Fit | M2K at 100% | Pre-M1 at 100% |
| --- | --- | --- | --- | --- |
| Whole frame | 26.3 | 25.4 | 48.7 | 95.2 |
| Composition | 16.3 | 16.9 | 40.1 | 88.8 |
| Display sources | 4.3 | 0.6 | 6.1 | 5.5 |
| Main composition | 12.5 | 11.4 | 35.3 | 196.6 |
| Main mips | 0.5 | 1.0 | 3.5 | 13.9 |
| Paint | 3.6 | 3.3 | 3.1 | 3.2 |
| Prediction | 5.3 | 4.4 | 5.1 | 2.4 |

With Stats open the moved layer paints at 40.47 fresh updates/s at Fit (pre-M1
35.65) and 12.72 at 100% (pre-M1 2.79).

Moving one object edits its affine through the shared object-motion gesture and
counts only newly evaluated poses that complete on the GPU during the gesture.
Pose rates are renderer completions, not presented frames. Before M1 nothing
moved image objects; the closest rows are the composed two-photo Free resize
(40.78 updates/s) and the small-radius Gaussian Blur slider (23.5 renderer
updates/s), which are not matched workloads.

| Moving one object | Fresh poses/s, gestures 1 / 2 / 3 | Pose-gap p99, ms | Result |
| --- | --- | --- | --- |
| Four shared objects | 48.58 / 50.39 / 48.69 | 29.2 / 26.6 / 29.2 | Below 60; faster than the closest pre-M1 drag |
| Four shared Nearest objects | 48.74 / 47.90 / 49.03 | 27.1 / 26.7 / 27.3 | Below 60 |
| Four shared objects, attached Gaussian Blur | 21.35 / 21.40 / 21.34 | 58.1 / 57.6 / 55.3 | Below 60; bounded by re-blurring the layer each pose |

Poses show previews from prefiltered image levels, or level zero for Nearest;
the exact result replaces them after the gesture.

Settling is measured from the end of input, or from the opened photo becoming
ready, until composition and edits are idle, including exact object results. It
is not presentation latency.

| Settle | Image objects | Pre-M1 reference |
| --- | --- | --- |
| Opening, paint-only control | 2.43 s | 2.31 s |
| Opening, four shared objects | 2.56–2.60 s | Four shared placed photos: 5.86 s |
| Opening, four shared Nearest objects | 3.00 s | |
| Opening, four distinct objects | 4.65–4.81 s | Four distinct placed photos: 7.14 s |
| Opening, eight distinct objects | 11.81–12.06 s | Eight distinct placed photos: 12.58–13.04 s |
| Opening, four shared objects with Gaussian Blur | 11.27 s | One photo with Gaussian Blur: 11.78 s |
| Opening, layer moved to (37, 101) at Fit / 100% | 2.45–2.47 s / 2.58–2.65 s | 2.41–2.71 s / 3.34–4.14 s |
| After the first painting contact after opening | 1.68–1.83 s | |
| After later painting contacts | 0.20–0.29 s | |
| After moving shared objects | 2.11–2.23 s | |
| After moving shared Nearest objects | 2.39–2.45 s | |
| After moving shared objects with Gaussian Blur | 2.39–2.47 s | |

In a trace of a blurred opening on build G, compiling the document's blur
shaders took 7.7 s; the pre-M1 blurred photo also takes about 12 s to open.

On M2K's debug build, the shared-object, attached-filter preview, and cancel and
Paste Into conversion journeys each pass in light and in dark on the TCL, and the
image-row picking journey passes through both themes.

Build M2J's APK SHA-256 is
`88f57ffb518a269ac1d1ae462d8bb4ad90e3095f74c0ea66230c1acc6fbd4b6f`; its native
library SHA-256 is
`1621d8cbe7068bd461d4d360c2bce4642a4db57c2f66e3bad1e92e7d47fb8a67`. Build M2K's
APK SHA-256 is
`0a9cb2ae4370ab425cfeb1813f17f8b96d36192d19e8f300cc684d2596695793`, native
library `ff09f2b2d5071226ca93dcb20c8331c8f7ff0a906bfe84bc54ebe91c994548e9`. The pre-M1
reference APK with the same harness is
`6e53d01834a4fbb61a90b2e5bd0056038afaedf0c382e768a8bb721e1186380e`, native
library `a05c6edf0de8ec4db3e1e40789caa14dc508274becbb9b3139db75c49e798055`.
Provenance, fixtures and their hashes, per-contact records, Stats traces and the
pre-M1 harness are under `artifacts/object-ga-m2/perf/`. Earlier builds' rows
remain there. The early gate's failing build and records remain under
`artifacts/object-ga-early-gate/`.

## GPU preparation

Measured on TCL TAB 11 Gen 2 on 2026-10-08 with asynchronous WebGPU preparation
based on `067f5cbb7`, optimized `dev-perf` Wasm SHA256
`658380486f41e11637fbfea891978b0cb7b3d3c697afd7837eedc64300d30af9`.
The fixture uses the Perceptual 4248 × 2832 reference photo at Fit, G-Pen 1024 px,
normal Navigator and default glass. Three five-second CDP strokes follow a
1.5-second priming stroke and Undo. Optional compilation is still pending;
`--startup-pending` waits for the selected brush instead of the full catalog.

CPU frame-call rates are 16.63, 21.77 and 20.58/s, with frame-interval p99
132.7, 96.4 and 96.6 ms. Input-to-submission p95 is 46.7, 29.1 and 31.4 ms.
These proxies do not establish fresh GPU completion or presentation, and miss
the 60 Hz target. Shader/driver cache state and thermal status are unmeasured.
The observations are retained under
`artifacts/transform-root-cause/startup-after/pen/`.

The startup/recovery check on 2026-10-09 uses `a742be2a7` plus asynchronous
preparation changes, Wasm SHA256
`38fa3ff549ccfaef8813c69b809dcd1034ef84672ea586ff8732362150c11b02`, on a
2048 × 1536 painted drawing. Initial paper appears at 9.33 s and the selected
brush becomes ready at 42.26 s. A subsequent automatic reload restores usable
paint and history at 44.70 s. Cold worker Apply takes about 39 s. All 281
observed page/worker pipeline calls are asynchronous, with no runtime errors.
Thermal status is zero before and after; browser/driver caches are uncontrolled.
This establishes completion and preparation ordering, not a controlled startup
comparison or input-to-display latency. Shader compilation remains a substantial
first-use delay. Records are under
`artifacts/transform-root-cause/native-restore-fixed/`.

## Zoom settings navigation comparison

Current 1.0.12 APK `ef20cbb3`, base `ee4805dfe`, is unmeasured on TCL because
the reference tablet is reserved by another session. The earlier 1.0.11 APK
`10e734af` measurements retained above do not qualify this build.

The earlier run uses the 12 MP photo, an empty paint layer and the Navigator,
warm shaders, thermal status 0 and three five-second gestures, with
SurfaceFlinger actual-present timestamps. Baseline `f02d74518` horizontal Zoom
measured 41.48–43.59 fps, p99 99.97–116.55 ms. Both earlier builds miss the
Zoom target. The earlier candidate pinch had a 50.05 ms gap run against
baseline p99 33.35–33.55 ms; the comparison does not establish no regression.
Evidence: `artifacts/navigation-controls/zoom-audit-20261009/comparison.json`;
current build provenance: `artifacts/navigation-controls/zoom-audit-20261009/post-rebase-source-provenance.json`.

## XP-Pen Web stabilization comparison

Measured 2026-10-13 UTC on XP-Pen Magic Note Pad (MNP1095), Chrome WebGPU,
with a 4248 × 2832 photo, an empty paint layer, Fit and the default Navigator.
Both builds use `dev-perf`, which enables release optimization. The baseline is
`9e6a17e4357bac0154bf3a96f87c5b1953b01ec2`, Wasm SHA-256
`cb9a433d8dd854878f55b5a5d2f049df1743e421b89b0616438f7ea41411aba1`.
The Curve fit build is identified by Wasm SHA-256
`26b0084b6f3f4e546d94f03c8645cb498a49b2e63005367a3165aea7a1aa2573`.

Each condition has three warmed five-second CDP pen contacts at nominal 200 Hz,
with varying pressure and an 8 ms prediction horizon. Platform prediction is off.
Values are medians across contacts. Each paired cell lists stroke preview
**on / off**. Speed adjustment is off except at strength 100.

| G-Pen diameter | Stabilization | Moving frame calls/s | GPU completion proxy/s | Frame CPU p99, ms |
| --- | --- | --- | --- | --- |
| 18 px | Baseline | 53.99 / 59.25 | 48.22 / 56.53 | 8.60 / 7.50 |
| 18 px | 0 | 59.07 / 59.56 | 54.92 / 56.39 | 8.20 / 7.50 |
| 18 px | 6 | 58.33 / 58.81 | 55.56 / 55.64 | 8.10 / 7.10 |
| 18 px | 50 | 57.73 / 58.69 | 52.13 / 55.33 | 9.60 / 7.80 |
| 18 px | 100 | 58.24 / 58.25 | 50.12 / 55.08 | 9.80 / 7.40 |
| 1024 px | Baseline | 32.28 / 37.79 | 25.63 / 28.24 | 25.10 / 18.70 |
| 1024 px | 0 | 38.83 / 40.45 | 26.87 / 29.21 | 18.50 / 15.30 |
| 1024 px | 6 | 36.60 / 38.08 | 23.99 / 28.60 | 17.40 / 16.70 |
| 1024 px | 100 | 34.62 / 38.12 | 28.36 / 27.26 | 20.90 / 16.40 |

All 42 candidate contacts advance the committed paint revision, with no browser
or measurement-hook errors and no new effect pipelines during drawing. HAL
thermal status stays at zero; candidate CPU/GPU temperature rises from about
37 °C to 45 °C. The 1024 px baseline loses its private USB port mappings after
four contacts; its final two contacts run after reconnecting and reloading.
Runs vary the trajectory phase and do not replay identical raw positions.

The default strength remains close to disabled smoothing in these comparisons.
Strong smoothing redraws a longer preview and generates more contacts. Caching
slope lengths and restricting derivative-bound scans removes repeated work
without changing the bounds or geometry tolerance. The earlier strength-100
preview profile's frame CPU p99 is 36.2 ms; the optimized repeated comparison
has a median of 9.8 ms at 18 px.

Frame calls include frames waiting for GPU work. Completion callbacks measure
asynchronous queue progress and include browser scheduling delay; they are not
physical presentation or photon latency. These supplemental XP-Pen measurements
**do not qualify the TCL 60 fps target**. Raw results and build provenance remain
in `artifacts/curve-fit-production/xppen-candidate-optimized-summary.json` and its
referenced reports.

## Vector layers

Measured 2026-10-20 on the reference TCL at `2567874a7`, built from `git archive`
(benchmark APK SHA-256 `5ca3906e…`, native library `b177594c…`), with
`android-vector-benchmark.py` and, for inking, `android-brush-benchmark.py`
([measuring](measuring.md#vector-layers-android)). The document is a
`crowded_vector_fixture` generated at that commit, 4248 × 2832, with 10,000
hatching paths on the edited Ink vector layer below a translucent Tones vector
layer, an empty Base paint layer and Paper hidden, at Fit (zoom 0.344) in the
default Sketch workspace with Stats and Navigator closed.
Rows marked *round 3* were re-measured on 2026-10-20 on the round 2 and 3 tree
(`git archive e7cdd23412d7`, the same fixture generator) in alternating runs
against `2567874a7` where noted; the other rows are from `2567874a7`.
Each row has one 1.5 s priming gesture and three 5 s gestures (10 s for
inking), each undone; thermal status stayed 0. Presented rates join the canvas
SurfaceView's buffers to SurfaceFlinger presents; drag start is pen-down to the
present of the first frame submitted after the first move. Responses give the
first changed present and, where later, the present that completes the work.
Causes and proposed fixes for the misses are in [known gaps](known-gaps.md);
summaries and symbolized profiles are in `artifacts/vector-tablets-20261020/`.

| Row | Target | Measured | Result |
| --- | --- | --- | --- |
| G-Pen 24 px on the crowded layer | 60 | 83.8–84.1 completed updates/s, gap p99 27.1 ms | **Met** |
| Pen-down 5 ms after committing a 195 ms line | 33 ms | First GPU completion p50 28.6–33.1, p95 40.8–51.0, max 53.8–55.9 ms | **Not met** |
| Move drag, 1 / 100 / 5,000 paths | 60, 100 ms start | 59.8 / 60.0 / 59.6–59.8 presented/s, p99 ≤16.9 ms; start 63–77 / 59–68 / 53–85 ms | **Met** |
| Scale with the Move frame, 100 / 5,000 paths | 60 | 60.0 / 59.6–59.8/s, p99 ≤16.9 ms; 5,000 settles 0.96–1.00 s after release | **Met** |
| Rotate with the Move frame, 100 / 5,000 paths | 60 | 60.0 / 55.4–55.6/s, p99 16.8 / 33.4 ms | 100 **met**; 5,000 **not met** |
| Edit Points anchor drag, 1 / 100 paths selected, *round 3* | 60 | 60.0 / 59.8–60.0/s, p99 ≤16.9 ms | **Met**; 51.8–51.9 / 48.3/s at `2567874a7` |
| Eraser: touched part / whole paths / up to intersection (64 px) | 60 | 59.2–59.6 / 60.0 / 59.8–60.0/s, p99 ≤33.3 ms | **Met** |
| Properties Outline width scrub, 100 / 5,000 paths, *round 3* | 60 | 60.0 / 10.4–11.2 updates/s, p99 ≤17.0 / 133–150 ms | 100 **met**; 5,000 **not met** |
| Pan / pinch / rotate, no paths selected | 60 | 43.1–43.2 / 33.4–33.9 / 41.8–41.9/s; pinch p99 217–233 ms | **Not met** |
| Same, 5,000 paths selected | 60 | 43.1–43.3 / 5.8–28.6 / 41.5–42.1/s; one pinch in three has a 1.25 s gap (*round 3*) | **Not met** |
| Fill inside a closed path | 100 ms | First change 66–70 ms, complete 100–103 ms | **Met** |
| Fill outside the paths (new layer below) | 250 ms | First change 0.60–0.64 s, complete 3.15–3.22 s | **Not met** |
| Commit after a 3 s line | No separate limit | Committed path presented 157–170 ms after pen-up | See the pen-down row |
| Undo of that line | 250 ms | 147–175 ms | **Met** |
| Merge Down onto a vector layer | 100 ms response | First change 310–320 ms, complete 446–527 ms | **Not met** |
| Merge Down onto a paint layer, from the notice | 100 ms response | First change 78–90 ms, complete 1.16–1.22 s | **Met** for the response |
| Rasterize Layer | 100 ms response | First change 112–132 ms, complete 1.03–1.10 s | **Not met** |
| Frame that shows the finished Rasterize | Motion only | 95–110 ms CPU and 157 ms GPU, after about 1 s with no canvas frame | Not a motion row |
| Open the document | 3 s | Parsed 571–625 ms, first frame 876–931 ms, settled 1.21–1.26 s | **Met** |
| Paint-layer thumbnail after filling the whole Base layer | No limit | 4.04–4.10 s, polling every 120 ms like the Layers panel | Unqualified |
| Pan / G-Pen stroke while that thumbnail is pending | 60 | 43.1–43.5 / 60.0/s; 43.3–43.5 / 60.0/s without polling | No extra drops |

### V1 geometry correction measurements (2026-10-22)

Re-measured the affected rows on the reference TCL, at 4248 × 2832 with 10,000
paths, using the same authored crowded fixture for all builds. Baseline source
is `3ac660dc`. Edit Points and ink use `47c53335` plus the geometry snapshot
with manifest SHA-256 `bb5bd518…` (benchmark APK `cc27a4b8…`); erasing uses the
measured cap-aware eraser snapshot `dbe23b92…` (APK `b05e9fbc…`). The earlier
point and ink measurements retain their own source provenance. The final
projection pruning for Butt and Square caps (`erase.rs` SHA-256 `09703e62…`) is
outside the measured Round branch. All builds use the same optimized benchmark
profile, private app ID, Fit view, Sketch workspace, closed Stats and
Navigator, and default 60 Hz panel setting. Each row has a priming gesture and
three 5 s gestures, each undone; thermal status stayed 0. The edited hatches
have authored pressure profiles and Round caps and joins. These measurements
supersede the corresponding older rows above.

| Row | Baseline presents/s | Corrected presents/s | Corrected worst run p99, ms | 60 fps target |
| --- | --- | --- | --- | --- |
| Edit Points, 1 path | 60.0–60.0 | 59.8–60.0 | 16.86 | **Met** |
| Edit Points, 100 paths | 59.8–60.0 | 59.8–60.0 | 16.73 | **Met** |
| Eraser, touched part | 59.6–59.6 | 59.8–59.8 | 16.88 | **Met** |
| Eraser, whole paths | 59.8–60.0 | 60.0–60.0 | 16.85 | **Met** |
| Eraser, up to intersection | 59.6–60.0 | 59.4–59.4 | 33.30 | **Met** |

Intersection erasing starts its first moving presentation in 94.5–100.9 ms,
compared with 90.1–104.5 ms in the baseline. One of three current runs exceeds
the separate 100 ms response limit (one of three in the baseline); that
response target remains **not met**. The motion-rate qualification above does
not include start latency.

The ink runs below count completed canvas activity. Both builds report zero
input-attributed updates/s because vector samples do not advance the recorded
consumed-paint timestamp. These rates therefore do not establish fresh ink
throughput, and the current inking target remains unqualified.

| Row | Baseline canvas updates/s | Corrected canvas updates/s | Corrected worst run p99, ms | Result |
| --- | --- | --- | --- | --- |
| G-Pen 24 px, constant pressure | 110.8–119.5 | 114.9–119.2 | 16.81 | **Unqualified** |
| G-Pen 24 px, variable pressure | 116.7–119.9 | 113.1–118.9 | 16.23 | **Unqualified** |

Build manifests, source diffs, fixed fixtures, per-run reports and presentation
traces are retained under `artifacts/vector-v1-fixes-20261022/performance/`, in
`baseline/low/`, `candidate-batch2-fast/low/` and
`candidate-batch2-cap-aware/low/`. The middle tier was reserved by another
session and was not measured.

## Native source preparation qualification

| Change | Affected work | Reference-device qualification |
| --- | --- | --- |
| Shared bounded preparation workers | Cold source/mip preparation, raster restoration and simultaneous windows | Not measured; existing motion results do not qualify this pool |

The [architecture](../architecture.md) defines the worker and queue bounds.
Warm canvas motion keeps its existing preparation and GPU paths.

## Native column bridges

The low-tier resize gap predates this port. These recorded checkpoints improve
on the original rates; **the 60 Hz target remains unqualified**. Presented
window frames and fresh native bounds are separate measurements.

| Light gap resize checkpoint | Actual window presents/s | Fresh bounds updates/s |
| --- | --- | --- |
| Original `ab6a16b7`, APK `fb5519cc` | 22.91 / 27.79 / 30.86 | 21.81 / 26.07 / 29.22 |
| Shared paths and mask shadow, APK `1e7504b2` | 46.57 / 49.16 / 49.88 | 33.07 / 34.22 / 32.98 |
| Matched FIFO control, APK `d8c080e3` | 48.64 / 50.74 / 49.64 | 36.13 / 34.82 / 35.36 |
| Retained presentation, APK `ccee00b5` | 53.64 / 57.79 / 58.47 | 41.71 / 42.97 / 41.74 |

Captured on the reference TCL at factory density 320, font scale 1, 60 Hz and
thermal status 0, using the 4248 × 2832 photo plus an empty paint layer at Fit,
Low transparency, one primer and three six-second moving contacts. The original
benchmark is unminified and app-traced; later builds use release Rust and R8
without app tracing. The original-to-port rows are descriptive checkpoints,
not an isolated comparison. The FIFO/retained pair has identical DEX, resources,
manifest and ART profile, with only its source-built native library changed.
SurfaceFlinger window latches join actual display presentation times; they do
not identify a fresh geometry revision on every presented buffer.

The merged native path retains authored allocation bounds, original control
clips and a quarter-size GPU shadow mask. Strict ten-keyframe bridge-face and
selected-source checks pass in both themes. Sampled shadow edges differ by at
most one RGB level from the full-size shadow. Retained presentation cuts
successful acquisition from 1,488 / 2,135 / 2,010 ms to 28 / 36 / 29 ms per
contact in the clean matched pair. Its first contact remains below the nominal
rate floor; native bounds updates remain below window cadence.

The navigation comparison completes eighteen paired cases with camera, palette,
settings, Navigator and glass parity, preserving every previously passing row.
Photo/Navigator-open pan and rotation retain approximately 60.02 presented fps
in both builds. Existing pinch stalls also occur in the control. Navigation,
ink, overlap and surface-lifecycle functional checks pass. These checks protect
the retained-presentation change; they do not qualify panel motion.

Recorded builds and raw measurements are under
`artifacts/column-bridge-android/baseline/`, `candidate-fill-only-shadow/` and
`candidate-workspace-shared-readonly/`. The current implementation excludes the
unproductive command-validation cache. Further cold-contact investigation is
deferred.

## Web SVG column bridges

The Web resize gap also predates this port. **These rates are unqualified**;
the original presented rate is unavailable, and Chrome content presents do not
prove fresh SVG geometry on every displayed frame.

| Recorded checkpoint | Chrome content presents/s | Fresh geometry updates/s |
| --- | --- | --- |
| Original left-column resize, `a46600c8d` | Unknown | 8.46 / 8.43 / 8.63 |
| Shared SVG, Wasm `b2746aed` | 21.11 / 21.62 / 23.14 | 21.40 / 22.03 / 23.42 |

Both use the 4248 × 2832 photo plus an empty paint layer at Fit with Low glass.
The older baseline uses dark theme, Chrome 154, density 272/DPR 1.7, CDP mouse
and three warmed five-second contacts. Its clock-anchor uncertainty prevents
an accepted presented-FPS measurement. The SVG checkpoint uses light theme,
Chrome 155, factory density 320/DPR 2, OS pen and three six-second contacts,
with thermal status 0. These different protocols establish the pre-existing
gap and recorded port rates, not a matched causal speedup.

The SVG checkpoint predates the completed-pose cache experiment; the current
implementation retains worker coalescing and stale-result guards without that
cache, additional compositing hints, header-band filters or one-pass canvas
experiments. Strict 80-position bridge checks pass. The unchanged Layers
sharpness assertion fails identically at DPR 1 on pristine `ab6a16b7` and the
cleaned port: 58 changed channels out of 168,000, maximum difference 29. The
remaining rendering, Navigator and docking assertions pass in a separately
reported continuation; this is not a full-suite pass.

Baseline evidence is in
`artifacts/navigator-layout-fix/tcl/baseline-sf-performance/`; the SVG checkpoint
is in `artifacts/column-bridge-web-shipping-final/`. Cleanup verification is in
`artifacts/column-bridge-web-rollback-final/`, with pristine comparison evidence
in `artifacts/column-bridge-web-pristine-baseline/`. Drag-rate root-cause work is
deferred.
