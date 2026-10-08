# Runtime cost of brush-driven and continuously flowing paint

[Technical documentation](../README.md)

Research for [physical media modelling, issue #7](https://github.com/capyatelier/capycanvas/issues/7).
Source baseline: [`893a425cc1e2adc1db74bb5dead42f5a6e3e115a`](https://github.com/capyatelier/capycanvas/tree/893a425cc1e2adc1db74bb5dead42f5a6e3e115a),
2026-10-08. This is a static source review and an analytical cost model. No
application, shader, build, test suite or benchmark was executed. Numerical
examples are calculated assumptions, not device measurements or qualified frame
rates. The conclusions concern both computation and the behavior being bought
with it.

## Decision

A sparse, clocked paint simulation is the stronger architecture for paint that
continues to move after the pen lifts. It can also reduce the work on the path
from new input to visible paint. It is **not unconditionally cheaper in total**:
its cost grows with the area of still-moving paint, its lifetime, simulation
resolution and required substeps. The current model avoids that continuing cost
by producing a bounded approximation only when contacts arrive.

The best candidate for the requested behavior is cheap, ordered GPU deposition
plus a clocked simulation over **active material tiles**, with a separately
specified stopping policy. A global clock does not require a full-canvas solve.
For strokes that must stop immediately, a well-batched local approximation can
remain cheaper. Improving batching and avoiding repeated page copies benefits
either model; those improvements cannot be credited exclusively to fluid physics.

Three comparisons must remain separate:

| Comparison | What can be concluded without running an implementation |
| --- | --- |
| Current implementation versus a redesigned clocked implementation | There are identifiable command and copy costs the redesign could remove. The size of a speedup is unknown. |
| Efficient local approximation versus efficient fluid simulation | Neither dominates for every workload; the break-even depends on active area and per-cell work. |
| Equal physical behavior with different scheduling | Spreading work over time can reduce bursts and first-paint latency if deposited or intermediate state is shown promptly; it does not eliminate required total work. |

## What the existing code actually does

The watercolor path is incremental already. It consumes new contacts; it does
not solve or replay the whole stroke on every frame.

| Source fact | Consequence |
| --- | --- |
| Committed wet/watercolor contacts are split into batches of at most three dabs. [Batch construction](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-engine/src/canvas.rs#L2390), constant at line 51. | Fine spacing can create many dependent deposition batches. |
| Three transport stages follow each material update group, after its deposition microbatches. [Grouping and encoding](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/lib.rs#L2483). | Multiplying the three transport stages by every dab would overstate the current cost. |
| Transport damage expands new-contact rectangles by the configured distance and unions them within each touched page. [Damage](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/lib.rs#L4938), [page scissor](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/lib.rs#L2786). | It is local, but may shade dry holes inside rectangles and copies whole touched pages. |
| Deposition reads existing paint, applies stroke coverage and a motion backtrace, then writes color, coverage and wetness. [Material fragment](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/material_brush.wgsl#L361). | A new contact is more expensive than depositing an independent source term. |
| Wetness transport is a nonconserved activation front with local pigment relaxation. [Exchange](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/watercolor_transport.wgsl#L138). | It does not numerically converge to a physical fluid solution; it produces a different approximation. |
| Working color is RGBA32Float; scalar surfaces are R32Float. [Formats](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/pipeline_device.rs#L83). | Color/coverage/wetness ping-pong pairs total 48 bytes per fully provisioned document pixel, before other resources. Older R8 comments do not describe these allocations. |
| Pending frame work has no continuously evolving watercolor condition. [Renderer pending work](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/lib.rs#L3386). | After queued input work finishes, wetness remains but watercolor does not move while idle. |

On an unclipped transport fragment, the source contains eleven filtered
conductance samples, ten color/wetness texel loads, four neighbor exchanges and
orientation math, per stage. Three stages therefore contain 33 conductance
samples and 30 color/wetness loads per processed pixel before selection queries,
writes and composition. These are logical shader operations; caches, filtering
and compiler lowering prevent treating them as GPU timings.
[Transport fragment](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/watercolor_transport.wgsl#L200).

The existing [top-tier record](../performance/top-tier.md#watercolor-prediction-precision)
also identifies substantial command finalization/submission overhead. Its
historical measurements are not measurements of a proposed replacement, and the
linked raw artifacts are unavailable in this checkout. No claimed speedup below
is inferred from the ratio of old paper frame rates to that record.

## A cost model with a testable break-even condition

Measure area in document pixels unless stated otherwise. Use costs from the same
GPU and execution layout when eventually calibrating the model.

| Symbol | Meaning |
| --- | --- |
| `λ` | New dabs per second. |
| `A_d` | Average dab footprint evaluated for deposition; overlap is counted repeatedly. |
| `f_u` | Nonempty material update groups per second in the current path. |
| `A_u` | Average transport area per group: sum of its actual page scissors. |
| `c_m`, `c_p` | Effective GPU cost per dab-footprint pixel for current material deposition and proposed cheap deposition. |
| `c_t` | GPU cost per document pixel of one current transport stage. |
| `f_s` | Simulation ticks per second, independent of input and display rates. |
| `m` | Physical time substeps per tick. |
| `q` | Document pixels along one simulation cell edge; `q = 4` gives one cell per 4×4 document pixels. |
| `A_w` | Current active simulation domain area, including tile rounding, halos and newly activated neighbors. |
| `c_s` | Cost per simulation cell of a complete physical substep, including all its solver stages. |
| `H_D`, `H_F` | Other GPU costs per second: copies, composition, prediction, conversions, synchronization and work omitted by the footprint approximation. |

With fixed effective coefficients, the approximate GPU work rates are:

```text
G_D = λ A_d c_m + 3 f_u A_u c_t + H_D
G_F = λ A_d c_p + f_s m (A_w / q²) c_s + H_F
```

Do not multiply `c_s` by the solver's pass count again: it already includes all
passes in a substep. Pressure iterations inside one substep are not additional
increments of physical time. `A_d c_m` is an effective deposition model, not a
claim that every current fragment lies within a dab; remaining scissor work
belongs in `H_D`.

Let `ΔD = λ A_d (c_m - c_p)` and `ΔH = H_F - H_D`. Rearranging gives the
conditional break-even proof:

```text
G_F < G_D
iff f_s m (A_w / q²) c_s < ΔD + 3 f_u A_u c_t - ΔH
iff A_w < q² (ΔD + 3 f_u A_u c_t - ΔH) / (f_s m c_s).
```

If the numerator is nonpositive, no positive active area wins under these
coefficients. This proves a condition within the model, not an actual frame
rate. It explicitly includes the proposed saving from cheap deposition.

CPU work needs its own equation, for example
`C = command_count × effective_command_cost + input + bookkeeping`. GPU and CPU
times cannot generally be added into a wall-clock frame time: they overlap, and
queue dependencies determine latency. A fluid implementation can win CPU time
through batching while losing GPU time through a larger active area.

### Why neither approach always wins

**Counterexample to “continuous flow is always cheaper.”** After pen-up, with no
other changes, `λ = f_u = 0`. The current path needs no incremental painting work.
Any still-moving fluid with `A_w > 0` needs positive work. Continuous flow is then
strictly more costly and supplies behavior the old model does not have.

**Counterexample to “dab locality is always cheaper.”** Consider repeated
contacts over a bounded wet patch. Active area stays bounded while current
ordered deposition and material-update work can increase with contact density.
If the difference `c_m - c_p` is positive, the fixed-clock fluid term eventually
fits inside the saved contact work. This is conditional: contact processing is
not free, and changed deposition or stronger forcing may increase `m`.

For painting over fresh paper, increased speed is not a free win. It increases
both dab arrivals and wet trail area. For long fresh trails the dominant terms
on both sides grow roughly linearly with brush speed.

## Calculated painting workloads

Assume an approximately round 512 px brush, spacing `s = 0.07`, transport margin
`R = 26`, and constant pressure. Spacing and transport distance follow the Wash
preset; diameter is set to the guaranteed very-complex brush size. Aspect,
jitter, selection, prediction and actual page layout are omitted for transparent
arithmetic. [Preset](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-core/src/presets.rs#L349),
[spacing rule](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-engine/src/brush.rs#L1098).

Use brush speed `v = 8192` document px/s, material update rate `f_u = 120`, and
effective wet width `W = 512 + 2 × 26 = 564`. This is an assumed workload that
can supply enough dabs for 120 updates/s, not the throughput of the existing app.

```text
dab spacing             = s D = 35.84 px
λ                       = v / (s D) = 228.571 dabs/s
square dab evaluations  = λ D² = 59.919 million pixel-contact evaluations/s
A_u rectangle proxy     = W (W + v/f_u) = 356,598.4 pixels/update
current transport work  = 3 f_u A_u = 128.375 million stage-pixels/s
```

For a nonoverlapping fresh path whose paint stays active for `T` seconds, use a
rectangular strip proxy:

```text
A_w,proxy = W² + W v T = 318,096 + 4,620,288 T.
```

A circular swept footprint instead has endcap area `πW²/4`. The rectangle is a
simple proxy, not the exact code scissor or a measured stroke. Long examples
represent fresh curved paths on a sufficiently large canvas, not an arbitrarily
long straight line. Clip area to the canvas and account for overlap. Additional
fluid spread, multiple wet layers, halos and tile rounding can increase the
actual simulation workload relative to this proxy.
Actual update scissors enclose discrete dab centers: with `n` equally spaced
dabs, the center span is `(n-1)sD`, not `v/f_u`. This matters at the example's
1.90 dabs/update. The rigorous break-even equation uses measured `A_u`; the
following values use the stated swept rectangle consistently.

For an illustrative complete solver step costing `c_s = 4 c_t`, `f_s = 60`, and
`m = 1`, the table compares **only fluid work against current transport work**.
Deposition savings and `H` terms are excluded from both sides.

| Active lifetime `T` | Active area proxy | Fluid/current transport, `q=1` | `q=2` | `q=4` |
| --- | ---: | ---: | ---: | ---: |
| 0.25 s | 1.473 MP | 2.75× | 0.69× | 0.17× |
| 1 s | 4.938 MP | 9.23× | 2.31× | 0.58× |
| 2 s | 9.559 MP | 17.87× | 4.47× | 1.12× |
| 5 s | 23.420 MP | 43.78× | 10.95× | 2.74× |

For example, the one-second, `q=4` case is:

```text
60 × (4,938,384 / 16) = 18,518,940 complete cell-steps/s
18,518,940 × 4 / 128,375,424 = 0.5770.
```

The transport-only break-even lifetime is:

```text
T* = [3 f_u A_u q² / (f_s m r) - W²] / (W v), where r = c_s/c_t.
```

It is 0.047 s at `q=1`, 0.394 s at `q=2`, and 1.783 s at `q=4` for the assumed
`r=4`. At `q=4`, `T=1`, changing `r` from 1 to 4 to 8 changes the ratio from
0.144 to 0.577 to 1.154. Two substeps or a 120 Hz simulation doubles it again.
There is no measured justification for choosing `r=4`; this sensitivity is why
the table cannot establish a winner before kernel calibration.

The result nevertheless resolves the proposed mechanism: a lower simulation
resolution and independent tick rate can overcome a larger active area, while
long wet lifetimes can reverse the result. Cheap deposition and fewer copies
shift the threshold in favor of the clocked design. Additional composition and
pigment work shift it back.

The one-second example also quantifies the required deposition saving. Before
overhead, divide `fluid work - current transport work` by the 59.919 million
square-footprint contact evaluations/s:

| Simulation grid | Required `(c_m-c_p)/c_t` for total GPU break-even, ignoring `ΔH` |
| --- | ---: |
| `q=1` | Greater than 17.638 |
| `q=2` | Greater than 2.803 |
| `q=4` | Greater than -0.906 |

Thus the `q=2` solver can still win total GPU work despite costing 2.31× the
current transport term, if each contact-pixel saves more than 2.803 current-stage
pixel costs. The negative `q=4` threshold means its modeled transport saving can
even tolerate a small deposition increase. Neither condition establishes that
the actual deposition kernels have those costs.

### A fixed wet patch differs from an expanding trail

Repeatedly painting within a 1 MP region at `q=4`, 60 Hz needs 3.75 million
cell-steps/s. Under `r=4`, that is 15 million current-stage-equivalent pixel
operations/s, about 0.117 of the example's current transport term. This regime
is favorable for a clocked solver because new contacts reuse the same domain.

In contrast, for fresh long trails and `v/f_u` large compared with `W`:

```text
λ ≈ v/(sD),   3 f_u A_u c_t ≈ 3 W v c_t,
fluid work ≈ f_s m W v T c_s / q².
```

Dividing by `v` shows that higher speed alone does not force a crossover:

```text
fluid wins approximately when
A_d(c_m - c_p)/(sD) + 3 W c_t > f_s m W T c_s/q²,
```

before overhead and canvas saturation. The key controls are lifetime, resolution
and work per step, not just contacts per second.

## Copies, commands and memory can outweigh the equations

Under the partial-page watercolor path, one deposition microbatch copies color
and coverage, the update snapshots wetness, and the first transport stage copies
color and wetness. Counting both a read and a write, these are respectively
40, 8 and 40 bytes per copied pixel with current Float32 formats. That is
**88 bytes per touched page pixel per update** when one deposition microbatch
touches the same page set. More microbatches add more deposition copies.
[Material copies](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/material_tiles.rs#L93),
[wetness snapshot](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/lib.rs#L2561),
[transport copies](https://github.com/capyatelier/capycanvas/blob/893a425cc1e2adc1db74bb5dead42f5a6e3e115a/crates/layer-render-wgpu/src/lib.rs#L2729).

For a rectangle of width `w` and height `h` placed uniformly relative to a grid
of page width `P`, expected intersected pages are `(1+w/P)(1+h/P)`: in each
dimension the expected crossed grid boundaries are length divided by `P`, plus
the starting page. The illustrative 564×632.267 rectangle intersects about
11.114 pages of 256×256. With the stated one-microbatch assumption, that is about
44.46 material-plus-transport page render passes/update, or 5,335/s at 120 Hz,
and 7.69 GB/s of logical copy read/write volume alone. Clears, rendering,
portable attachment work, prediction and other copies are additional.

This is a geometry/copy calculation, not measured DRAM traffic or the exact
page sequence of the implementation. It explains why a lighter equation can
still be expensive. A replacement needs batches spanning tiles, such as compute
over an active tile list in an atlas or buffers, rather than one command-heavy
sequence per tile per few dabs. Removing copies requires a correct replacement
for immutable source/destination staging; simply deleting them creates hazards.

Those changes also apply to an event-driven model. Comparing a batched fluid
solver only against today's page executor would conflate scheduling, storage
layout and physical modeling.

### Bandwidth and state envelope

A D2Q9 distribution has nine scalar values per simulation cell. With Float32
and double buffering, that is `9 × 4 × 2 = 72 bytes/cell` of distribution storage
alone. One ideal streaming read/write of these values is also 72 bytes/cell-step.
Pigment, absorbed material, water supply, temporary state, color rendering and
document history add costs.

Take **160 bytes/cell-step as a hypothetical total traffic coefficient**, not a
measurement or a guaranteed upper bound. At 60 Hz and one substep:

| Active document area | Traffic at `q=1` | Traffic at `q=4` |
| --- | ---: | ---: |
| 0.25 MP | 2.4 GB/s | 0.15 GB/s |
| 1 MP | 9.6 GB/s | 0.60 GB/s |
| 4 MP | 38.4 GB/s | 2.40 GB/s |
| 16 MP | 153.6 GB/s | 9.60 GB/s |
| Entire 9504×6336 canvas, 60.217 MP | 578.1 GB/s | 36.13 GB/s |

At `q=4`, that full canvas still needs about 258.4 MiB just for distribution
buffers. Coarse simulation does not remove full-resolution artwork, pigment
output, prediction or history storage.

The top-tier GPU's listed peak bandwidth is 67.2 GB/s; it is shared with other
work and is not a sustainable painting budget. The `q=4` full-canvas example
would require 72.26 GB/s at 120 Hz, already above that peak under the assumed
traffic model. Even the 72-byte distribution streaming floor at full native
resolution and 60 Hz is about 260.1 GB/s. These streaming estimates rule out
naive dense native-resolution designs under their assumptions; they do not
predict sparse performance. [Hardware and budgets](../PERFORMANCE_TARGETS.md),
[bandwidth qualification](../performance/measuring.md).

If a calibrated implementation can allocate `B` bytes/s to fluid state traffic,
its area envelope is:

```text
A_w,max = B q² / (f_s m b), where b is measured bytes/cell-step.
```

With the illustrative `b=160`, `q=4`, 60 Hz, `m=1`, budgets of 2, 5 and 10 GB/s
cover 3.33, 8.33 and 16.67 MP. In the fresh-trail example these correspond to
roughly 0.65, 1.73 and 3.54 seconds of active lifetime before extra halo or
spreading costs. These budgets are scenarios, not assigned device capacities.

## What spreading the computation over seconds does and does not buy

### First paint can be fast while flow matures slowly

Separate three deadlines: first deposited mark, onset of flow, and mature spread.
If deposition is immediately composited, new ink need not wait for a simulation
tick. If it waits, the clock alone adds up to `1/f_s` delay: 16.67 ms at 60 Hz,
with mean 8.33 ms for uniformly distributed arrival phases, before GPU work.

Suppose an effect needs 120 physical substeps in total. An instantaneous
implementation does all 120 before showing the mature result. A 60 Hz solver
doing one substep per tick presents intermediate states for two seconds. For
the same domain and operations, both perform 120 substeps. The latter reduces
the burst by 120×, not the total work, and may pay for more intermediate
compositions. If the desired result is visible evolution, those intermediate
states are useful output.

That example is not a description of current watercolor: its three long-hop
transport stages are not 120 compressed physical substeps and need not converge
to the same result.

### Local propagation imposes a minimum number of steps

For a synchronous update whose dependencies extend at most `r` simulation cells
in Chebyshev distance, after `n` updates a disturbance can influence only cells
within `n r` of its origin. Proof: the initial dependency radius is zero; each
update expands the preceding radius by at most `r`. Induction gives the bound.

Therefore reaching document distance `L` by elapsed time `τ` requires
`n ≥ ceil(L/(rq))`. For a single-cell streaming step, a 26 px spread needs at
least 26 steps at `q=1`, or seven at `q=4`: 0.433 s or 0.117 s of simulated time
when each step represents 1/60 s. Tick-only wall-clock completion varies by one
tick with arrival phase; execution and presentation add latency.
This is a dependency bound, not a prediction that a visible pigment front moves
that fast. Bulk flow can be much slower. Long-distance gathers or additional
neighborhood stages change the bound and must be counted explicitly.

Ordinary diffusion is slower than this maximum-support bound. For the explicit
2D five-point diffusion update with `a = κ Δt/q²`,

```text
c_next = (1 - 4a)c + a(c_left + c_right + c_up + c_down).
```

Nonnegative weights require `a ≤ 1/4`. For a unit impulse on an unbounded uniform
grid at that limit, one step's mean-square radial displacement is `q²`, so after
`n` steps the RMS distance is `q sqrt(n)`.
An RMS spread of 26 px requires at least 676 steps at `q=1` or 43 at `q=4`,
about 11.27 s or 0.717 s at 60 steps/s. This result is for that diffusion scheme;
it is not a bound on advective ink flow or the current long-hop front.

### Stability and clock progress cannot be deferred arbitrarily

For explicit diffusion and an explicit advection scheme with CFL constant `C`,
the respective necessary step restrictions are:

```text
Δt ≤ q²/(4κ),          Δt ≤ C q/u_max,
Δt = 1/(f_s m).
```

For a solver using both restrictions, choose at least
`m = max(1, ceil(4κ/(f_s q²)), ceil(u_max/(C f_s q)))`. These are scheme-specific
conditions, not a stability proof for MoXi. Its lattice velocity and relaxation
parameters need their own admissible range. Likewise, stable semi-Lagrangian
advection still has accuracy and conservation limits at large steps.

As an illustration, prescribing 300 document px/s fluid velocity, `C=1`,
`q=1`, and 60 Hz requires at least five explicit advection substeps/tick. At
`q=4`, it requires two. A slow watercolor flow of 20 px/s can satisfy that one
condition with one native-resolution step at 60 Hz. Brush speed is not generally
fluid speed: fast brush travel should deposit a swept source, not force every
fluid cell to travel at the tip speed.

Pressure relaxation is another distinction. Iterating an incompressibility
solve improves a residual for the same physical instant. Performing a fraction
of that solve each display frame while continually changing its right-hand
side is not equivalent to integrating a slow fluid. A method with intentional
finite-speed pressure propagation can be chosen, but it changes the model.

Let `V` be the sustained cell-step capacity per second available to simulation.
Keeping physical time aligned with wall time requires:

```text
V ≥ f_s m A_w/q².
```

If the inequality fails, bounded scheduling prevents one long stall but the
simulation falls behind. Dropping elapsed time slows the medium; accumulating
elapsed time builds a backlog; increasing `Δt` may violate stability or change
appearance. A fixed work budget guarantees bounded work, not bounded physical
time lag. Memory limits also require a policy for newly wetted tiles.

Finally, average utilization does not bound the worst burst. A 60 Hz tick that
occupies the GPU for 8 ms can interfere with a 120 Hz presentation schedule.
Interleave bounded submissions with fresh deposition, avoid queuing seconds of
simulation, and account for composition. Different tiles must observe a
consistent simulation epoch and exchange their halos; updating only whichever
tiles fit this frame without correct boundary time handling can leak mass or
produce seams.

The [motion targets](../PERFORMANCE_TARGETS.md) still apply while paint visibly
moves after pen-up. A 30/60 Hz solver with repeated presents does not establish
120 Hz motion, and interpolation is an approximation that needs visual
qualification rather than being counted as fresh simulation work.

## Expressiveness depends on state as well as scheduling

| Behavior | Current local approximation | Clocked material simulation |
| --- | --- | --- |
| Predictable mark that stops at pen-up | Natural and inexpensive after completion. | Freeze after accepted work, or stop after a bounded settling interval. |
| Paint continuing to spread without input | Absent. | Natural if wet regions remain active. |
| Interaction between marks laid down seconds apart | Samples retained wetness, without idle evolution. | Can depend on elapsed absorption, drying and flow. |
| Distant parts of a connected wet region responding | Limited by the new-contact damage domain. | Can propagate through the active region over time. |
| Conserved pigment, puddle movement and accumulation | Current max-front/flattened-color representation does not enforce them. | Possible with suitable state and conservative transfer; a clock alone does not provide them. |
| Thick acrylic, pickup, ridges and pigment mixing | Needs additional material behavior. | Also needs additional material behavior; water flow alone does not supply it. |

An insufficiency proof for RGBA plus a single wetness value is simple. Construct
two physical scenes with identical stored RGBA/wetness throughout the relevant
domain, including neighboring pixels and all other available inputs. In one,
pigment is fixed to paper; in the other it remains suspended in water. They must
respond differently to the same subsequent flow. A deterministic update of
those identical stored values cannot produce both required outcomes. At least
another material distinction is needed for that behavior.

Likewise, two acrylic pixels can have equal color and paint height but different
mobility because their binder has dried differently. Height alone cannot encode
both volume and drying state. A tagged medium can interpret a channel differently
for watercolor and acrylic. Encoding several quantities in a channel would be
a new representation with its own precision limits, not a scalar that directly
means wetness or height. Additional state should follow the selected effects,
not an attempt to model every physical variable.

Color mixing is an independent decision: a clocked RGB solver still mixes RGB.
Pigment concentrations and optical coefficients can improve mixtures under
either scheduling model, with additional state and rendering cost. Those costs
must appear in `c_s`/`H_F`, or in the corresponding local-model terms.

Freezing evolution need not dry paint. Frozen wet state can be disturbed by a
later stroke; fixing pigment changes how later strokes interact. After two
strokes mix, a stopping rule cannot generally stop only one stroke's contribution
without retaining provenance. Define the policy for the affected material domain,
including what happens when a new stroke interrupts settling.

## Which paper contributes the useful mechanisms

**MoXi** provides the strongest starting point for clocked ink dispersion:
D2Q9 local streaming/collision avoids a global pressure solve. Its original
implementation performs six fluid and six pigment/glue texture updates per
timestep; GPU-friendly does not mean one cheap pass. It uses surface, flow and
fixed pigment state, with drying and quick-dry controls. Sparse tiles and modern
pass fusion are proposed adaptations here. Its ink fixture behavior also needs
review for watercolor rewetting. [MoXi, §§4–7, mirrored author text](https://doczz.net/doc/2039446/moxi--real-time-ink-dispersion-in-absorbent-paper).

**IMPaSTo** supplies localized conservative paint transport, height and brush
pickup/deposition. Velocity is derived from brush contact rather than solved
through a general fluid system, and the original method limits brush movement
to one cell per step. It naturally suits contact-driven thick paint, but does
not by itself add post-contact fluid motion. Full pigment/optical rendering is
additional cost, separate from scheduling. [IMPaSTo, §§4–5](https://gamma.cs.unc.edu/IMPASTO/publications/Baxter-IMPaSTo_Web-NPAR04.pdf).

**Curtis** provides a watercolor behavior reference, but its adaptive water
steps and up-to-50 divergence-relaxation iterations make it a less promising
low-latency execution foundation. Spreading those iterations over display
frames does not automatically preserve its physical update semantics.
[Curtis et al., §4](https://grail.cs.washington.edu/projects/watercolor/paper_small.pdf).

## Recommendation for this engine

Prototype the execution architecture and one material model together, with
separate measurements for deposition and flow. The hypothesis worth testing is
**batched, sparse, moderately coarse clocked flow can improve both input
responsiveness and physical expression**, especially for revisited wet regions.
It is not that a full-resolution global fluid solver is intrinsically cheaper.

1. Preserve shared contact placement and ordered GPU deposition. Normalize water
   and pigment injection to the intended stroke/time rule so event rate does not
   change the amount deposited. Simple sources can be accumulated cheaply;
   destination-sensitive pickup and smearing still require ordered exchange.
2. Maintain active tiles independently of latest input damage, with cross-tile
   neighborhoods and retirement after mobility falls below a defined threshold.
   The current permanent wetness floor cannot itself identify moving paint.
   “Active” must include stationary water that can still evolve or affect a
   neighbor, not only cells with nonzero velocity.
3. Batch computation across those tiles. Include page allocation, copies,
   transport, material-to-color conversion and composition in the budget. Keep
   fresh paint visible without requiring completion of the whole settling job.
4. Keep physical substeps independent of presentation rate. Use a shared clock
   policy with bounded work, explicit behavior under overload and suspension,
   and deterministic ordering of deposits relative to ticks.
5. Treat coarse simulation as an appearance tradeoff. `q=4` reduces cell count
   by 16 only when substep needs and halo overhead permit it. Full-resolution
   tips, paper detail and edge rendering can preserve some detail, but cannot
   recover arbitrary fine physical interactions that the simulation never
   represented. Validate export and zoomed-in results, not only Fit zoom.
6. Extend shared material capture/history and prediction isolation. Undo, redo,
   save and reopening need a defined state/time boundary for evolving paint.
   Predicted input must never permanently inject water or momentum. Native hosts
   schedule presentation; simulation rules remain in shared Rust/GPU code.

The first comparison should include an optimized event-driven baseline using
the same batching improvements. Calibrate `c_m-c_p`, `c_t`, `c_s`, copy volume,
command cost and composition cost, then evaluate active-area/lifetime scenarios
against the break-even equation. Required cases include short isolated strokes,
repeated mixing in one patch, fresh long trails, broadly wetted paper, fast
resumed input during flow, and stopping immediately or after a short tail.

Record first-paint latency, flow-onset latency, physical-time lag, completed fresh
updates, presented motion, worst bursts and total work through settling. Device
qualification must follow the existing tier rules; none was performed for this
analytical report.
