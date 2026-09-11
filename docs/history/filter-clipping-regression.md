# Incremental clipping composition

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

The clipping regression came from rebuilding unchanged scene layers beneath a
cached, animated clipping stack. The shader was not the source of the extra work.

The image-stage scheduler now retains both inputs to the final blend:

```
cached isolated clipping input → filter ─┐
cached composition below the base ─────┴→ cached composition → canvas
```

Backdrop dependencies follow the clipping base's sibling scope, including isolated
groups. Filters in the same clipping stack share its backdrop cache. A terminal
clipped image stage retains its blend operation's textures, bindings and uniforms;
ordinary animated frames do not reconstruct that operation. The base's opacity
and blend mode apply after filtering, while masks preserve their existing scope.
A nonterminal image stage exposes both its raw clipping stack and cached backdrop
to the subsequent clipped operations. Unclipped filters can consume completed
composition images directly.

There is no separate constant-background shortcut and no Heat Haze-specific code.
The final cached-image path is shared by clipped and unclipped adjustments.

## Active drawing and damage

- Backdrop edits refresh only tiles intersecting their conservative dirty region.
  Untouched cached tiles remain valid.
- A frozen clipped filter does not rerun when only its backdrop is painted.
  Only the damaged backdrop tiles and final blend region update.
- Painting the clipping base updates its filter input and the filter's required
  neighborhood/halo; its unchanged backdrop is not rebuilt.
- With animation enabled, the time-dependent filter updates as required, but an
  accompanying backdrop stroke still refreshes only the damaged backdrop tiles.
- Reparenting, clipping-scope changes, extent changes and other structural edits
  invalidate affected dependencies; full invalidation is not used for each dab.

## Physical GPU benchmark

Release build, NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan. Each case has 320
updates, with 64 CPU warm-up updates excluded. The multilayer scene has a
65%-opacity clipping base over three paint layers using Multiply blending.
CPU is submission time, GPU uses timestamps, and completion includes waiting for
the submitted GPU work. None of these measures establish display/input latency.

Animated multilayer scene before and after backdrop caching:

| Canvas | Path | CPU median / p95 / p99 (ms) | GPU median / p95 / p99 (ms) |
| --- | --- | --- | --- |
| 2048×1536 | Before, unclipped | .027 / .041 / .127 | .045 / .046 / .047 |
| 2048×1536 | Before, clipped | 2.302 / 3.935 / 4.144 | 1.444 / 1.742 / 1.880 |
| 2048×1536 | After, unclipped | .035 / .041 / .045 | .045 / .046 / .046 |
| 2048×1536 | After, clipped | .043 / .065 / .183 | .060 / .061 / .061 |
| 4096×4096 | Before, unclipped | .046 / .083 / .197 | .220 / .221 / .221 |
| 4096×4096 | Before, clipped | 22.485 / 27.754 / 29.573 | 8.567 / 8.914 / 9.016 |
| 4096×4096 | After, unclipped | .037 / .063 / .203 | .217 / .219 / .219 |
| 4096×4096 | After, clipped | .060 / .146 / .159 | .310 / .311 / .311 |

Active drawing into the same 4096×4096 multilayer scene with a clipped filter:

| Update | CPU median / p95 / p99 (ms) | GPU median / p95 / p99 (ms) | Completion p99 (ms) |
| --- | --- | --- | --- |
| Base stroke, frozen filter | .375 / .501 / .610 | .117 / .124 / .125 | .974 |
| Backdrop stroke, frozen filter | .225 / .366 / .835 | .098 / .105 / .105 | 1.332 |
| Backdrop stroke + animation | .243 / .381 / .620 | .410 / .414 / .416 | 1.221 |

The stroke crosses a tile corner: four 256×256 backdrop tiles update, totaling
262,144 pixels (1.56% of the 4K canvas). Frozen-backdrop painting runs zero filter
pixels. Base painting runs the 1,936-pixel expanded filter region and refreshes
zero backdrop pixels. All measured completion tails remain below the 8.33 ms
120 Hz budget; CPU scheduling tails vary between runs.

```
cargo test --release -p layer-render-wgpu clipped_animation_latency --offline -- --ignored --nocapture --test-threads=1
```

CSVs live under ignored `artifacts/benchmarks/`: `clipped-animation.csv` and
`clipped-animation-multilayer-before.csv`.

## Storage and validation

A terminal clipping stack adds one RGBA8 backdrop image and one RGBA8 composition
result, plus a 96-byte uniform record. This is about 24 MiB at 2048×1536 and
128 MiB at 4096×4096. The tested one-pass filter's total image-cache storage is
48 MiB and 256 MiB respectively. Backdrop images are shared by the stack's filters,
resources persist across frames, and obsolete stages/backdrops are released.
This trades explicit GPU storage for avoiding repeated scene reconstruction.

`cached_clipping_matches_tiled_composition` compares against the original tiled
path at two animation times across eighteen scenes: opacity, Multiply blending,
base/filter masks, hidden bases, mask overlays, multiple clipped image filters,
unrelated lower paint, isolated groups, pointwise clips above an image stage, and
an unclipped image filter above the completed clipping stack. Maximum error is
one output byte.

`painting_backdrop_updates_only_dirty_tiles_without_rerunning_frozen_filter`
checks tile edges/document edges, frozen and animated filters, exact backdrop tile
work, retained bindings/storage, and equality with a genuinely uncached rebuild.

## Layer UI follow-ups

Filter insertion and picker previews share `Document::clipping_stack_top`:
a new ordinary filter goes above the selected layer's entire clipping stack,
including hidden clips, without changing its parent or clipping relationships.
The Layers footer offers Delete selected layers, governed by lightweight shared
document validation, not by cloning the document on each UI refresh.
Animated filter rows carry a subdued shared sparkle SVG, driven by the program's
time-input capability rather than its current playback setting.
