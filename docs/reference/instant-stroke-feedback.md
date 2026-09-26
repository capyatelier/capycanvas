# Instant stabilized stroke feedback

[Technical documentation](../README.md)

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](../development/publication.md#publication-checks).

## Implementation status

The shared engine implements this model for every current brush execution
class. Dry source-over prediction uses a transparent sparse GPU overlay; dry
erase, non-normal blend, smudge, wet paint, and liquify use an exact
copy-on-write branch of the touched sparse pages. Both paths invoke the same
brush shaders as committed drawing.

The committed generator is never rewound. A configurable timestamp-bounded
suffix remains provisional, its generator is cloned from the finalized
frontier, and pen-up flushes the real suffix before removing the preview in the
same GPU submission. Disabling the feature returns at pen-down to the original
incremental path and emits no preview batch.

## Goal

Drawing should feel like a physical pencil on paper: visible ink should reach the
stylus tip immediately, even when high-quality smoothing intentionally delays
final stroke geometry.

Two errors must be minimized together:

1. **Tip gap** — the distance between the displayed mark and the stylus position
   when the frame reaches the screen.
2. **Correction gap** — the difference between the provisional mark and the
   geometry that later becomes finalized. Large corrections make the tail swim,
   snap, or flicker even if it reaches the tip.

Eliminating only one error is insufficient. A raw line reaches the tip but can
look rough and move substantially when corrected; an ordinary delayed smoother
looks clean but visibly trails the pencil.

## Recommendation

Represent an active stroke as an append-only finalized prefix followed by a
short, replaceable, **tip-locked provisional tail**:

```text
real/coalesced input -> stroke modeler -> finalized prefix -> persistent GPU pages
                              |
                              +-> provisional tail -> replaceable GPU preview pages
platform/engine prediction ------------------------^
```

The CPU stroke modeler owns only sequential input, smoothing, prediction,
dynamics, and contact placement. All provisional and persistent pixels are
rasterized by the same GPU brush implementation.

For every display frame:

1. Advance points that the modeler guarantees will never change into the
   finalized prefix.
2. Estimate the stylus position at the expected presentation time, using
   platform prediction when available and a short, confidence-limited engine
   prediction otherwise.
3. Generate the best provisional smoothed tail and smoothly constrain its
   endpoint to that tip estimate.
4. Force terminal brush coverage at the endpoint so contact spacing cannot
   leave a small hole.
5. Restore the previous preview from finalized GPU state and rasterize the new
   tail. Dirty damage is the union of the old and new tail bounds.

The endpoint correction should fade in from zero at the finalized frontier.
This preserves position and tangent continuity while placing the final preview
contact under the tip. Prediction should shorten rapidly at corners, reversals,
deceleration, or low confidence. On pen-up, the modeler flushes to the final real
position and commits that result without an intermediate blank frame.

## Destination-aware brushes

The same model extends to smudge, wet paint, and liquify by treating preview as
a speculative branch of all mutable brush state, not merely an alpha overlay:

```text
stable checkpoint = layer pages + material pages + brush state + dab state
preview state      = replay(provisional tail, fork(stable checkpoint))
```

Each update discards the old branch, commits newly finalized contacts, and
replays the bounded tail from the new checkpoint. Current destination-aware GPU
preview reads committed coverage and, for loaded wet paint, reservoir state,
carries color within the provisional batch, and writes only private color
pages. Watercolor previews sample same-layer committed pigment, use private
coverage/color/wetness page pairs, receive the same three transport stages,
and use the same live composition edge. Only the material-state damage halo is
copied into the recyclable wetness pages.
Preview deliberately does not advance the committed reservoir or deposit
persistent material state. Future time-evolving water/pigment fields must gain
equivalent private preview state.
Long-running diffusion, drying, or dripping should initially operate only on
finalized state.

Start with pencil and inexpensive dry ink, where the short tail touches few
pages and corrections are subtle. Extend to destination-aware brushes after
measuring replay cost and correction visibility. Never substitute an
approximate preview shader: provisional and finalized contacts must use the same
brush semantics.

## Correctness and acceptance

- Finalized output is independent of display-frame cadence and input batching.
- Provisional work never advances document history, random state, reservoirs,
  or time-based paint accumulation.
- Dab spacing, texture randomness, pressure, tilt, and twist remain continuous
  across the finalized/provisional frontier.
- The displayed mark covers the presentation-time tip estimate; tip-gap p95 and
  p99 are measured in pixels rather than judged only by render latency.
- Provisional-to-final position, tangent, opacity, and color corrections are
  measured at p95/p99 and visually tested for swimming and texture flicker.
- Preview plus composition remains inside the 8.33 ms 120 Hz frame budget on
  each target device.

Prediction cannot know the literal future perfectly, particularly during fast
reversals. The product objective is therefore zero *visible* tip gap with the
smallest possible, smoothly distributed correction behind the nib.

## Runtime controls

`InstantFeedbackConfig` is interaction state and is snapshotted at pen-down; it
is not saved into a brush or document. The C ABI exposes the same fields through
`LayerInstantFeedbackSettings`.

| Control | Default | Purpose |
| --- | ---: | --- |
| enabled | true | Zero-work bypass for interaction A/B tests |
| platform prediction | true | Prefer native/browser future samples |
| engine prediction | true | Confidence-limited fallback when native prediction is absent |
| finalization lag | 8 ms | Size of the replaceable real-input suffix |
| prediction horizon | 8 ms | Engine future interval; native samples have a separate 64 ms safety cap |
| maximum prediction distance | 96 physical px | Zoom-independent runaway clamp |
| tip lock | 1.0 | Endpoint correction strength; automatic at full strength in preferences |
| correction easing | 1.5 | Distribution of correction behind the endpoint |
| minimum prediction speed | 12 physical px/s | Suppresses stationary noise |
| corner suppression | 1.0 | Stops extrapolation at right-angle turns and reversals |

`CanvasEngine::render_frame_for(now, presentation)` is preferred when a platform
knows its expected presentation timestamp. Both values share the pen-event
monotonic timebase. `now` alone advances time-driven paint; `presentation`
selects the speculative endpoint. `render_frame_at` uses the configured horizon.

The **Prediction amount** slider defaults to 16 ms for new settings and Reset;
existing saved values are preserved.

**Smooth Motion** is shared across platforms. **Settings → Input → Prediction
algorithm** retains its dropdown for future alternatives, with **Smooth Motion
(Optimized)** as its only supported choice and default. Saved Previous and other
retired experimental choices migrate to Optimized while other preferences remain
intact. The standalone C feedback API retains its ABI and uses Optimized.

The predictor combines a recent acceleration fit with 100 ms of causal drawing
history. Sustained smooth motion supports stable reach; slow/medium detail keeps
prompt response without unnecessarily shortening steady lines. A bounded
correction field smooths the whole preview at matching future times, while stops
and abrupt direction changes release that memory promptly. Measured ink, sensor
values, native precedence and physical distance limits remain authoritative.

Reach continuity uses changes in turning rate across that history, so a
consistent curve can retain length without increasing geometric smoothing.
An isolated moderate speed dip carries less weight than consecutive deceleration.
Strong slowing and abrupt turns still revoke continuity immediately. A projected
stop releases correction memory 8 ms beyond the requested target, or 24 ms when
falling raw pressure corroborates an impending lift. Pressure remaining steady
never prevents a stop; measured braking, distance and stale-input bounds still
apply independently. Falling pressure alone does not cut a steady forecast short.

Optimized filters visible display lead separately from curve geometry. A brief
fit-window confidence collapse may retain reach while recent drawing history
still supports continuation, with a missing fit bridged for at most 24 ms.
The newest observations always refit local geometry. The anchor join has its own
length, so cropping visibility cannot reshape the remaining curve. Frames with
no new input consume the previous forecast rather than advancing its endpoint;
policy changes, view changes, native takeover and stale input clear that memory.

See [recording and evaluation](../development/stroke-recording.md) for the
whole-preview metrics and regression bank. The chosen continuity behavior can
trade a small amount of instantaneous accuracy for smoother corrections.

The native-prediction switch appears directly below **Enable stroke prediction** on
every host. Android reports framework `MotionPredictor` availability for the
connected stylus; Web checks for `getPredictedEvents`; iPadOS uses UIKit predicted
touches; Windows creates a WinUI `PointerPredictor` for the canvas input source and
falls back to Smooth Motion when it is unavailable or fails. Linux and macOS
currently show a disabled switch. Capability
is transient and never overwrites the saved choice. When supported native
prediction is selected, **Prediction amount**, **Prediction algorithm**, and their
reset actions are disabled. Native timing comes from its sample timestamps and presentation time.
Endpoint tracking is always full strength for both native and shared prediction;
the retired `tip_lock` preference still loads but no longer affects rendering.
If native samples are absent,
the engine uses the selected shared algorithm with its automatic 8 ms fallback. Turning native prediction off, or
losing support, restores the saved prediction time.

## Lead stability and impending lift

Native prediction owns a small preview-only history. The predicted distance from
the latest real position is filtered using elapsed presentation time, with 32 ms
extension and 14 ms retreat time constants, and extension limited to 0.8 physical
pixels per millisecond. Stops, strong deceleration, sharp turns and lift handling
bypass that filter so it cannot retain a dangerous old lead. Intermediate native
points obey the same limits. Smooth Motion uses the separate motion-history and
whole-preview correction handling described above.

Up to 16 raw pen-pressure observations over 24 ms are used to fit a falling trend,
before the user's pressure curve. At least three observations spanning 6 ms and
a net drop of 0.04 are required, with pressure still falling. Native lookahead is
capped at half the estimated time to zero pressure; Smooth Motion uses that
estimate to qualify its early-stop alarm. Steady light pressure, rebounds,
mouse/finger input, predicted samples, and late sensor corrections do not qualify.
Pen-up/cancel discards the history; no inferred lift alters document input.

These are conservative heuristics, not a calibrated lift detector. Shared tests
cover varying frame rates/zoom, jitter, stops/reversals, pressure curves and replay
identity. `AndroidPredictionTest` checks device capability and actual Compose
controls and pen rendering; `node apps/layer-web/test.mjs --package --prediction` checks
browser controls and native sample ingress. Physical pen feel still needs device
assessment; synthetic traces cannot establish that subjective result.

## Platform input mapping

All adapters submit historical/coalesced real samples first in chronological
order, followed by temporary samples tagged `LAYER_SAMPLE_PREDICTED`. A new real
sample atomically replaces the prior predicted suffix. If a capability is
absent, no placeholder records are synthesized by the adapter.

| Platform | Real history | Preferred prediction | Graceful path |
| --- | --- | --- | --- |
| Linux Wayland | every tablet-v2 motion/frame group | none in tablet-v2 | shared confidence-limited predictor |
| Windows | reverse `GetPointerPenInfoHistory` to chronological order | WinUI `PointerPredictor.GetPredictedPoints()` on canvas moves | shared Smooth Motion predictor when the predictor is unavailable, fails or is disabled |
| macOS | AppKit tablet/mouse events with pressure, tilt, and rotation | none documented for tablet points | shared predictor |
| iPadOS | `coalescedTouches(for:)` using precise locations | `predictedTouches(for:)` | shared predictor if UIKit returns none |
| Android | `MotionEvent` history and nanosecond timestamps | framework `MotionPredictor.predict()` on pen moves (API 34+) | shared predictor when unavailable/disabled or no samples arrive |
| Web/Wasm | `pointermove`/`getCoalescedEvents()` | `getPredictedEvents()` | shared predictor when the list is empty |

UIKit estimated force/altitude/azimuth updates are a separate sensor-correction
concern. Adapters must preserve their stable sample identity; support for
amending an unfinalized sample can be added without changing prediction flags
or committed document semantics.

## Latency gate and lower bound

The disabled branch performs no tail scan, prediction, contact replay, preview
allocation, or extra GPU command. Enabled prediction necessarily evaluates and
rasterizes the contacts that make the future mark visible. Topmost source-over
dry paint draws predicted contacts directly after composition and allocates no
preview pages. One destination-aware batch reads committed pages and writes only
the damaged preview rectangle. Exact erase and uncommon multi-batch cases copy
only the damaged GPU region needed by their private branch. All cases use the
existing single contact upload and GPU submission, bounded to the union of
old/new tail damage. No canvas pixel is processed in host memory.

The dry-brush milestone result is recorded in
`artifacts/benchmarks/instant-feedback-dry.md`.
The final all-brush on/off matrix is recorded in
`artifacts/benchmarks/instant-feedback.md`.
On the recorded 4K/32-layer run, every enabled p99 is at most 5.025 ms, every
brush passes the 8.33 ms gate, and every tip-gap p95/p99 is 0 px.

## References

- [Google Ink Stroke Modeler](https://github.com/google/ink-stroke-modeler)
- [Apple predicted touches](https://developer.apple.com/documentation/uikit/uievent/predictedtouches%28for%3A%29)
- [Apple coalesced Pencil touches](https://developer.apple.com/documentation/uikit/getting-high-fidelity-input-with-coalesced-touches)
- [Windows pen history](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getpointerpeninfohistory)
- [WinUI pointer prediction](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.input.pointerpredictor)
- [Android MotionEvent history](https://developer.android.com/reference/android/view/MotionEvent.html)
- [Android MotionPredictor](https://developer.android.com/reference/android/view/MotionPredictor)
- [Wayland tablet-v2](https://wayland.app/protocols/wayland-protocols/480)
- [W3C coalesced and predicted pointer events](https://www.w3.org/TR/pointerevents/#coalesced-and-predicted-events)
- [Krita freehand brush smoothing](https://docs.krita.org/en/reference_manual/tools/freehand_brush.html)
