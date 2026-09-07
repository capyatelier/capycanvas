# Instant stabilized stroke feedback

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

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
| prediction horizon | 8 ms | Default and maximum future interval |
| maximum prediction distance | 96 physical px | Zoom-independent runaway clamp |
| tip lock | 1.0 | Endpoint correction strength |
| correction easing | 1.5 | Distribution of correction behind the endpoint |
| minimum prediction speed | 12 physical px/s | Suppresses stationary noise |
| corner suppression | 1.0 | Stops extrapolation at right-angle turns and reversals |

`layer_canvas_draw_frame_for(now, presentation)` is preferred when a platform
knows its expected presentation timestamp. Both values share the pen-event
monotonic timebase. `now` alone advances time-driven paint; `presentation`
selects the speculative endpoint. The older timed entry point uses the
configured horizon.

## Platform input mapping

All adapters submit historical/coalesced real samples first in chronological
order, followed by temporary samples tagged `LAYER_SAMPLE_PREDICTED`. A new real
sample atomically replaces the prior predicted suffix. If a capability is
absent, no placeholder records are synthesized by the adapter.

| Platform | Real history | Preferred prediction | Graceful path |
| --- | --- | --- | --- |
| Linux Wayland | every tablet-v2 motion/frame group | none in tablet-v2 | shared confidence-limited predictor |
| Windows | reverse `GetPointerPenInfoHistory` to chronological order | shared predictor for the custom canvas | current point only at a corner/low confidence |
| macOS | AppKit tablet/mouse events with pressure, tilt, and rotation | none documented for tablet points | shared predictor |
| iPadOS | `coalescedTouches(for:)` using precise locations | `predictedTouches(for:)` | shared predictor if UIKit returns none |
| Android | `MotionEvent` history and nanosecond timestamps | AndroidX `MotionEventPredictor.predict()` at frame time | AndroidX fallback model, then shared predictor |
| Web/Wasm | `pointerrawupdate`/`getCoalescedEvents()` | `getPredictedEvents()` | shared predictor when the list is empty |

UIKit estimated force/altitude/azimuth updates are a separate sensor-correction
concern. Adapters must preserve their stable sample identity; support for
amending an unfinalized sample can be added without changing prediction flags
or committed document semantics.

## Latency gate and lower bound

The reproducible on/off harness is:

```bash
cargo run --release -p layer-bench -- \
  --feedback-comparison --scenario all \
  --report artifacts/benchmarks/instant-feedback.md
```

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
- [Android MotionEvent history](https://developer.android.com/reference/android/view/MotionEvent.html)
- [AndroidX MotionEventPredictor](https://developer.android.com/reference/androidx/input/motionprediction/MotionEventPredictor)
- [Wayland tablet-v2](https://wayland.app/protocols/wayland-protocols/480)
- [W3C coalesced and predicted pointer events](https://www.w3.org/TR/pointerevents/#coalesced-and-predicted-events)
- [Krita freehand brush smoothing](https://docs.krita.org/en/reference_manual/tools/freehand_brush.html)
