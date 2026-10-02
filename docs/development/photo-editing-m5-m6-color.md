# M5–M6: tone, color and shared controls

[Specification](photo-editing-m5-m6.md)

Status: **proposed for review**, against `origin/main` at `da5b399ad`. This file owns numerical effect behavior, Properties metadata and gradients. [Workflows](photo-editing-m5-m6-workflows.md) owns query lifetimes, panels, presets and delivery. [Execution](photo-editing-m5-m6-execution.md) owns assignment order and checks. All new types and algorithms below are proposed; existing integration points are named explicitly.

## Verified integration points

| Area | Existing code and decision |
| --- | --- |
| Definitions and validation | `crates/layer-core/src/effects.rs`, `effect_catalog.rs`, `assets/filters/manifest.json`: retain runtime filter programs, parameter schemas and exact embedded programs. |
| Properties | `crates/layer-ui/src/effects.rs`, `numeric.rs`: extend `PropertyControl`, `NumericControl` and `EffectAction::Gesture`; replace host channel navigation and per-motion Set actions. Color controls already have Use Current Color. |
| Shader preparation | `crates/layer-render-wgpu/src/effect_preparation.rs`: parameter-only tables, at most eight lookups of 4096 vec4 records. Image statistics and a 33³ LUT do not fit this contract. |
| Color math | `assets/filters/effects.wgsl`, `working_color.wgsl`, `effects_color.wgsl`: processing is Float32, native effects retain finite negative/extended RGB and tiny positive alpha at every storage depth. Preserve neutral bypasses. |
| Spatial work | `scene/windows.rs`, `local_tone.rs`, `snapshot/tone.rs` in the wgpu crate: use budgeted region capture and the existing 768-edge guide; no whole-document texture requirement or CPU pixel analysis. |
| Resource transport | `crates/layer-core/src/project_storage.rs` and existing indexed binary payloads; coordinate ABI, project and preset changes through the integration owner. |
| Tests | Existing `native_effects`, `filter_spaces`, `filter_library`, `local_tone` and `scene::scale` harnesses are the starting points. Keep independent numerical references. |

No new effect-library browser, built-in filter enum, expression language, node editor or general resource graph is needed. Ordinary pointwise effects keep fusion. UI pages and control metadata do not enter shader values or undo history.

## Shared parameter and Properties model

Proposed metadata, alongside existing `section`:

- `EffectProgram.pages: [{id,label}]`, optional, bounded to 16 unique pages. `EffectParameter.page: Option<id>` selects one; null means common controls. Keep selected page in `UiSession` keyed by layer and page set, never in `EffectValue`, document history or shader parameters. Remove host-owned Curves channel selection when this shared selection is available.
- `EffectParameter.visible_when: Option<{key, value}>`, limited to equality against a Toggle or Choice. Validate the referenced key/type/value and forbid references to itself. No expression language, recursive conditions or arbitrary script callbacks.
- Optional numeric presentation metadata: mapping, soft minimum and soft maximum. Move/re-export the existing `NumericMapping` definition so core schema validation and `NumericControl` use the same enum; do not introduce parallel mapping implementations. Keep hard bounds, step, precision and unit where they are. Validate finite ordered soft bounds inside hard bounds and the existing Power exponent interval `[0.125,8]`.
- Optional shared picker/calibration metadata identifies a supported role and ordinary parameter keys; hosts receive typed actions. A picker is not a GPU parameter kind. The [workflow specification](photo-editing-m5-m6-workflows.md) must freeze its source, sample averaging domain, gesture lifetime and refusal behavior.
- Parameter values hidden on another page remain active. Switching pages creates no undo step and invalidates no canvas pixels. A parameter drag creates one undo entry; Escape restores its starting value. Import/Auto/picker multi-value updates use one atomic edit, not repeated `set()` calls committed separately.

Apply paging to Curves, Levels, Hue/Saturation, Selective Color and Channel Mixer. Convert Color Balance's existing Shadows/Midtones/Highlights sections to the same pages in this delivery; values and shader order stay unchanged.

## Levels: schema, order, Auto and calibration

Keep the current master keys `black`, `white`, `gamma`, `output_black`, `output_white`. Add `red_`, `green_`, `blue_` copies of those five fields. Keep global `clamp_input` and `clamp_output`: 22 values total. Pages are RGB, Red, Green, Blue. Defaults are `(0,1,1,0,1)` on every page and both clamps off.

For scalar input `x`, one stage is:

```text
u = (x - black) / (white - black)
u = clamp(u,0,1) if clamp_input
v = sign(u) * abs(u)^(1/gamma), with gamma==1 bypass
y = output_black + (output_white-output_black)*v
y = clamp(y,0,1) if clamp_output
```

Apply the selected channel's stage, then the RGB/master stage, matching current Curves order. Factor this scalar WGSL helper once; render and the histogram/Auto analysis entry call that helper. With every stage neutral and both clamps off, return the original premultiplied pixel directly.

Ranges: gamma `[0.1,10]`, step `.05`, two decimals; anchors step `.01`, at least three displayed decimals, soft range `[0,1]`. For integer documents keep hard anchor range `[0,1]`; for float documents use finite encoded anchor hard bounds `[-65504,65504]`. This is a deliberate editable-anchor limit, not a claim to address the complete Float32 exponent range. Extended pixels outside those anchors continue through the signed formula. Input white must exceed black by `.001`; output endpoints may cross (inversion). Fix ordered-number validation to test a finite positive difference and the required gap, rather than assuming `black + .001` remains distinct at large magnitudes. Parameter-setting constraints and renderer validation must agree. No silently equal input endpoints after numeric rounding.

**Auto:** the action belongs to the selected page. It is a one-shot edit of ordinary parameters and never becomes a permanently auto-adjusting effect.

1. Capture the exact effect input at its stack insertion point, before its mask, opacity and blend. Histogram semantics match existing `Histogram`: every pixel with `alpha > 0` counts once; unassociate RGB; alpha zero contributes nothing. Do not weight counts by alpha or drop tiny positive coverage. Non-finite input makes the operation fail visibly without a partial edit.
2. For Red/Green/Blue, analyze that channel's encoded input before its Levels stage. For RGB, analyze all three channels **after the per-channel Levels stages and before master**. Use a narrow Levels analysis entry calling the same extracted channel-stage helper, supplied with the frozen effect values. This is not a new arbitrary shader graph or a second Levels implementation.
3. Pass one reduces finite minima/maxima and covered count. Pass two builds 4096 uniform bins per channel over each nonconstant channel's actual min/max. Portable u32 atomics are sufficient under `MAX_EXTENT=32768` (at most 2^30 source pixels per channel). Counts from chunks can be added in u64 on the CPU; read back summaries only. Quantiles are nearest ranks `ceil(p*N)` for `p=.001,.999`; choose the selected bin's midpoint, clipped to observed min/max. Normalize in scaled coordinates `a=x/s`, `lo=min/s`, `hi=max/s`, with `s=max(abs(min),abs(max),1)`, so finite Float32 extrema do not overflow `max-min` in the GPU. Combine extrema and compute bin edges/midpoints in f64 on the CPU. The value-error bound is no more than `(max-min)/4096`; state this bound rather than claiming exact real-valued percentiles.
4. For an individual channel, use its two quantiles as black/white and set gamma to 1. For RGB use the minimum of the three low quantiles as master black and the maximum of the three high quantiles as master white, and set master gamma to 1. This is one common stretch; it does not silently introduce an Auto Color mode. Preserve output anchors, other pages and both clipping controls.
5. Reject empty input or a resulting span below `.001` without an undo entry. A constant individual RGB channel contributes its observed constant to the RGB extrema; reject only if the final shared interval is degenerate. Validate representability against the current layer's schema. Do not silently clamp an HDR percentile into an SDR field.
6. Publish only if document epoch, effect identity, insertion-point dependency key, color definition, selected page and frozen Levels values still match. Cancel or recompute when they change. Auto never consumes the live stratified Preview histogram.

**Calibration formulas:** preserve nonneutral master settings and output anchors. Let a channel's values be `(l,h,g,a,b)`, its source sample `x`, and master scalar function `M`. For desired final value `y`, compute the signed algebraic inverse `z=M^-1(y)`, `v=(z-a)/(b-a)` and `q=sign(v)*abs(v)^g`. Black solves `l'=(x-q*h)/(1-q)`; White solves `h'=l+(x-l)/q`. Gray solves `g'=ln(abs((x-l)/(h-l)))/ln(abs((z-a)/(b-a)))`, with matching signs and finite, nondegenerate terms. Black targets final encoded 0, White targets 1. Gray targets the weighted encoded-domain luminance of the current processed sample, so removing a cast retains its brightness. RGB solves all channels; an individual page changes only that channel. After solving in f64, validate all Float32 values and run the candidate through the complete scalar formula including clamps. Refuse unreachable/degenerate targets atomically, with no reset of unrelated controls. These are scalar parameter calculations in Rust, not CPU image processing.

Oracles: distinct nonneutral channel/master stages (catches order reversal); signed input and gamma; crossed output endpoints; all four clipping combinations; neutral chains at alpha `8e-8`; known quantile ramps and repeated distributions; constant one-channel vs fully constant image; empty/transparent input; Auto arriving after a page/source change; gray/black/white solvers with nonneutral master, reversed outputs and unreachable clipping.

## Exact Curves axis and numeric contract

Use one shared scalar encode `E` and decode `D` in Rust, matching existing WGSL.
For Encoded RGB, `E(v)=document_space.encode(v)` and `D(x)=decode(x)`. Stored
curve points remain normalized encoded coordinates. The numeric fields display
encoded values `255*x` for both integer and float documents using this axis;
they do not claim Float32 values are integer byte codes.

For Log HDR, let `F=-8`, `s=hdr_stops-F`, `toe=exp2(F)*e`,
`knee=1/(ln(2)*s)`. Freeze the existing branch equations:

```text
E(v) = v/(toe*ln(2)*s)                  when v <= toe
       (log2(v)-F)/s                   otherwise
D(x) = x*toe*ln(2)*s                   when x <= knee
       exp2(x*s+F)                     otherwise
```

Thus zero remains zero and numeric values near black are not EV interpolation.
Log HDR numeric Input/Output are physical linear document values `[0,2^stops]`.
White is `E(1)`, not normalized 1. The baseline manifest has only Encoded RGB
and Log HDR; do not introduce Linear HDR just because `hdr_curve_white` has an
older branch for that label.

For Encoded numeric controls use existing expression parsing, scale 255, three
display decimals, step `1/255`, resolution `1/(255*1000)`, bounds `[0,1]` in
stored encoded coordinates. For Log physical fields use Number kind, scale 1,
step `.01`, resolution `2^-149`, bounds `[0,2^stops]`; resolve in f64, encode in
f64, then round the resulting graph coordinate once to f32. The power-of-two
resolution does not erase small positive physical values, and dividing the
largest admitted physical value by it is finite in f64. Use a shared Curves
view formatter for shortest round-trip physical edit text, scientific notation
for nonzero magnitudes below `1e-4` or at least `1e6`; do not send fixed-decimal
zero as the editable text of a positive node. This is a small curve formatter,
not a new numeric parser, nonlinear range framework, or host formatting policy.
Readonly EV text is `log2(value)` with two decimals; zero shows `Black`.

Endpoints keep exact graph x=0/1; their Input fields are readonly. Clamp finite
interior numeric input after encoding through `point_between`; clamp output to
graph `[0,1]`. Invalid expressions/nonfinite results retain the original point
and show the existing numeric error. Formatting does not mutate points. If a
pair of neighbors has no representable f32 interior coordinate, horizontal
motion is unavailable rather than creating equal knots; strengthen the shared
neighbor helper with `next_up/next_down` representability checks. Keep the
existing adaptive gap and drag-detach margin, rather than imposing a new global
minimum knot separation on saved curves.

Graph nudge: arrows move the selected coordinate by
`1/255`, Shift by `10/255`; no modifier-dependent host math. X nudges obey shared
neighbor limits, endpoints cannot move horizontally, Y clamps. Key-repeat stays
one gesture until matching key-up; changing key/axis ends that gesture and starts
the next. Delete/Backspace only removes interior points. Escape restores an
active gesture before exiting its surrounding mode; typed Enter commits once.

Targeted sensitivity is `dy/255` in normalized output for logical surface
distance: `new_y=clamp(start_y+(start_surface_y-current_surface_y)/255,0,1)`.
One logical unit therefore equals one keyboard step; upward raises the selected curve output. A decreasing master can reverse the final visual response to a channel edit. Device
pixel ratio, canvas zoom and canvas rotation never enter this expression.
Freeze the starting logical scale for the contact. No hidden acceleration,
pressure multiplier, Shift behavior, or tool setting is necessary.

Targeted x is frozen from the captured pre-effect linear sample. On Red/Green/Blue, use `x=E(sample_i)` before that channel curve. On RGB, first compute `v_i=D(C_i(E(sample_i)))` through the three channel curves, then document-primary linear luminance `Y=v_g+yr*(v_r-v_g)+yb*(v_b-v_g)`, and use `x=E(Y)` before master. This is a luminance-targeted master point, not three hidden channel points. Reject empty/nonfinite input or x outside [0,1]; never silently clamp the sampled tone into another point. The selected page and frozen channel values are part of contact identity.

Plot marker hit radius is 8 logical surface units, Euclidean distance in the
actual displayed plot rectangle; closest marker wins, lower point index on an
exact tie. This is separate from `.002` normalized x knot reuse/insertion
tolerance, which never changes with graph dimensions. A targeted contact uses
that same normalized x reuse rule, keeps its accepted sampled x fixed, and
starts output at the original curve value at that x. If an existing knot moves
to that x, give it this sampled output before applying dy, so initial contact
does not jump the sampled result. Endpoints can only be targeted at exact x0/1;
near-endpoint insertion refusal follows the calibration constraints.

## Curves black, white and gray calibration

Selected point/page live in UiSession keyed by epoch, layer, curve key and validated index. Clear/revalidate them after undo, deletion, domain/page or program change; no point-selection history. Native controls own focus/text drafts, not curve math.

RGB calibration changes the three channel curves and preserves master. A Red,
Green or Blue page changes only that channel. Calibration must not silently
edit master on the RGB page; targeted RGB dragging still edits master as already
specified and creates only one visible master point.

For the sampled straight linear color, let `x_i=E(sample_i)`, channel curve
`C_i`, master `M`, and complete output `D(M(C_i(x_i)))`. Reject source coordinates
outside graph `[0,1]`, transparent/nonfinite samples and nonfinite chain outputs.
Black targets final linear 0; White targets final linear 1. Gray uses the
document-primary luminance Y of the **currently processed** sample, calculated
with `g+yr*(r-g)+yb*(b-g)`; all corrected channels target that same linear Y.
Require Gray Y>0 and `E(Y)` within `[0,1]`. This retains photographic brightness
on Log HDR as well as Encoded RGB. Levels intentionally keeps its separately
specified encoded-domain gray target; do not substitute this Curves target into
the Levels solver.

For each changed channel invert `M(z)=target`, where target is `E(0)`, `E(1)` or
`E(Y)`, and restrict `z` to `[0,1]` because editable knot outputs are bounded.
Master is not required to be globally monotonic. Each existing Hermite segment
is shape preserving between its endpoints, so enumerate every segment whose
closed endpoint-y interval contains the target. Reuse the existing tangent
construction in f64; do not invert sampled plot vertices or a 256-entry LUT.

- A nonconstant segment supplies one root by 48 bounded bisection iterations;
  handle exact endpoint targets without iteration.
- A constant segment equal to the target supplies a root interval. Its candidate
  is the projection of current `C_i(x_i)` onto that interval.
- Rank candidates by `abs(z-C_i(x_i))`, lower z on an exact tie. This explicitly
  chooses the least change to the current channel output, not the first crossing
  of a potentially nonmonotonic master. No roots means unreachable.
- Round the chosen root to f32 and verify `curve_value(master,z)`; f32 packing
  is authoritative. Require normalized error at most `8*f32::EPSILON`. A root
  that cannot meet this requirement is refused, not silently approximated by
  the nearest graph pixel. Endpoint duplicates do not change ranking.

Place one channel knot at exact f32 sampled x with the solved f32 y. If x is
exactly 0/1, update that endpoint's y. Otherwise select the nearest interior knot
within `.002` normalized x (lower-x tie), and move it to sampled x only when
the existing shared neighbor constraints admit that exact x. Never clamp a
calibration sample into another x. If no interior match exists, insert only if
all existing x distances exceed `.002` and the table has fewer than 32 points.
A near-endpoint sample cannot move an endpoint horizontally; if it cannot be
inserted/reused, refuse with unchanged effect. Do not delete another knot to
make space. The same reusable-knot search belongs to plot/targeted insertion;
targeted dragging also fixes the actual accepted x for the whole contact.

After all candidate edits, validate the entire EffectInstance and evaluate the
complete f32 channels-then-master chain at the sampled color. Require normalized
target error at most `8*f32::EPSILON` and linear error
`abs(actual-target_linear) <= max(2e-6,2e-4*abs(target_linear))`. Both are explicit:
a normalized check alone can hide an HDR error after decoding. Any failed
channel cancels the whole proposed edit. Commit one ReplaceLayer history item;
no tolerance rounding of unrelated knots and no master reset.

Required scalar/GPU cases: master identity, increasing, decreasing, W/M-shaped
multiple roots, exact tie, target plateau, no root, source at each endpoint,
sample within insertion tolerance of an endpoint/interior, adjacent f32 knots,
full 32-point table, existing nonneutral channel curves, Log toe/knee/white and
range127, representability refusal, all-or-nothing RGB and one undo. A Gray
contact with nonidentity master must reach the brightness target without erasing
that master. Tests must fail if a single global binary search replaces the
segment enumeration.

## White Balance bounds

The existing shader uses `t=temperature/100`, `q=tint/100` and linear gains `2^(0.8*t+0.25*q)`, `2^(-0.5*q)`, `2^(-0.8*t+0.25*q)` for R/G/B. Preserve its luminance-preservation behavior.

Freeze hard Temperature `[-1000,1000]` and Tint `[-800,800]`; both keep soft
`[-100,100]`, integer step 1 and numeric precision permitting at least three
decimal digits for solved values. Picker solutions are not rounded to integers.
Preserve the current gain formula, units and Preserve luminosity toggle.

These bounds allow red/blue cast ratios spanning 16 stops and green versus the
red/blue geometric mean spanning 6 stops. They bound every gain exponent to
±10 stops, so gains stay within `[1/1024,1024]`. This is broad correction scope,
not a claim that every mathematically possible Float32 ratio is calibratable.
Use the following solve in f64:

```text
temperature = 62.5 * (log2(b) - log2(r))
tint = (100/1.5) * (2*log2(g) - log2(r) - log2(b))
```

Reject nonpositive channels or a solution outside the hard bounds, round once to f32,
then evaluate the actual gain/luminance-preserving shader formula on the sample.
Require finite output and neutrality `max(rgb)-min(rgb) <=
max(2e-6,2e-4*max(abs(rgb)))`. Refusal is atomic; never clamp a solved temperature
or tint and claim neutral. Parameter-independent gain bounds do not guarantee
finite output for source values already near Float32 maximum; retain normal
renderer/nonfinite validation rather than adding a silent highlight clamp.


## Hue/Saturation by range

Proposed parameter schema: existing master Hue `[-180,180]`, Saturation/Lightness `[-100,100]`; six pages Reds/Yellows/Greens/Cyans/Blues/Magentas, each with Hue/Saturation/Lightness in those ranges and Center `[0,360)`, Width `[0,180]`, Feather `[0,90]` degrees; common Colorize toggle, Colorize Hue `[0,360)` and Colorize Saturation `[0,100]`. Total 42 parameters. Width is the full-strength angular width and Feather is the falloff on each side. These independent bounds guarantee `width/2 + feather <= 180`, so no new joint-constraint kind is needed.

Default centers in Oklab hue degrees: 30, 110, 145, 195, 265, 330. Width 30, Feather 30. These are Capy ranges, not a claim of Photoshop's exact numerical sectors. All corrections default to zero. Range membership is computed from the **original unassociated linear input** using `working_to_oklab`, with signed cube roots and document-primary conversion. It must not be computed from encoded ProPhoto channels or from an already hue-shifted intermediate.

```text
distance = min(abs(h-center), 360-abs(h-center))
weight = 1-smoothstep(width/2, width/2+feather, distance)
weight *= smoothstep(.005,.02,Oklab_chroma)
```

Define zero feather as the closed hard interval, avoiding `smoothstep(a,a,x)`. Support may reach 180 degrees on either side. Compute each correction as master plus the sum of weighted range corrections, clamp combined Saturation/Lightness to `[-100,100]`, and wrap summed Hue. Then apply the current extended-domain HSL adjustment once. This keeps existing control semantics and avoids order-dependent cascades when ranges overlap. Colorize uses the chosen hue/saturation with the input's HSL lightness; while enabled, range controls are hidden and inactive, with stored values retained. Master Lightness still applies. A complete neutral adjustment returns the original pixel exactly.

Oracles: identical physical colors represented in sRGB/P3/AdobeRGB/ProPhoto get equal membership; red wraps through zero; zero/full-width/hard-edge cases; overlapping ranges have no evaluation-order dependence; zero chroma receives no range edits; neutral extended/tiny-alpha identity; Colorize affects gray, unlike a hue-range edit.

## Missing pointwise adjustments

Keep these as runtime filter definitions plus original WGSL, with independent numerical tests. Do not infer correctness from the old forty-filter reference PNG. Use native resolution initially; `resolution: display` is earned by the existing reduced-graph qualification suite.

| Filter | Parameter contract and formula |
| --- | --- |
| Invert | No numeric controls. Encoded document `out=1-rgb`; preserve alpha, including extended continuation. |
| Threshold | Threshold soft `[0,1]`, default `.5`; float hard range as Levels. `v = select(0,1,fx_luma(encoded_rgb) >= threshold)`. It is luminance thresholding, not per-channel Posterize. |
| Desaturate | No numeric controls. Reuse the current extended-HSL lightness calculation and return equal encoded RGB. It is the same visual result as master Hue/Saturation at Saturation -100, not a second weighted B&W algorithm. |
| Photo Filter | Tagged Color, Density `[0,100]` default 25, Preserve luminosity default true. Mix encoded RGB toward the tagged filter color by effective strength `(Density/100)*color_alpha`, then use existing `fx_preserve_luma` if enabled. Zero effective strength bypasses all conversion; source alpha is preserved. The default Color is tagged sRGB `[1,.72,.45,1]`. |
| Channel Mixer | Red/Green/Blue output pages, four values per page: R/G/B coefficient and Constant. Coefficients `[-200,200]%`, constants `[-100,100]%`, identity diagonal 100. `out = matrix*encoded_rgb + offset`. Monochrome toggle plus one four-value Gray row (fixed coefficient defaults `[21.26,71.52,7.22]%` and Constant zero) makes 17 parameters. No automatic coefficient normalization. Keep live Total as view-only information if design needs it. |

Selective Color has nine pages (six hues plus Whites/Neutrals/Blacks), four CMYK sliders `[-100,100]%` per page and one Relative/Absolute choice: 37 values. Defaults zero and Relative. Adobe documents those nine families and the distinction between percentage-of-existing-ink and absolute shifts; reproducing undocumented proprietary pixel math is not a requirement. [Adobe Selective Color](https://helpx.adobe.com/photoshop/using/mix-colors.html)

Use the following Capy formula: membership hue is the same Oklab hue helper as Hue/Saturation; neighboring centers interpolate cyclically, multiplied by encoded RGB chroma `max(u)-min(u)` for `u=clamp(encoded_rgb,0,1)`. Remaining weight is partitioned by existing Color Balance shadow/midtone/highlight smoothsteps of `fx_luma(u)`. Accumulate each page's weighted `(C,M,Y,K)` before applying it once. For CMY, base ink is `1-u`; Absolute adds the accumulated CMY amount, Relative adds that amount times base ink. Clamp ink to `[0,1]` and convert to RGB. Apply Black as a common additional reduction: Absolute amount directly; Relative amount times `(1-max(u))`; clamp the resulting bounded RGB to `[0,1]`. Add only the difference from `u` back onto the original extended encoded RGB. This makes zero controls exact identity, preserves out-of-range residuals, and makes Relative unable to darken pure specular white. It is intentionally simple and explicitly differs from Adobe's unpublished implementation. The quality gate below must pass before host integration.


For Selective Color, the two adjacent circular hue-center weights sum to chroma c; multiply existing Color Balance shadow/midtone/highlight weights by (1-c), so all nine memberships sum to one. Skip hue at c=0. With normalized weighted CMY and K, freeze the operation order:

```text
ink = 1-u
new_ink = clamp(ink + CMY * (ink if Relative else 1), 0, 1)
v = clamp(1-new_ink - K*((1-max(u)) if Relative else 1), 0, 1)
out = original_encoded + (v-u)
```

K uses the original u, never the CMY-adjusted result; zero controls bypass conversion exactly.

Selective Color oracles must lock the formula for primary/secondary colors, 50% gray, white, black, half-saturated colors, every single CMYK slider, Relative/Absolute, overlapping hue and tone weights, extended residuals and all-zero identity. **Quality gate:** accept a photographic comparison sheet before host integration; this proposed formula has not been validated visually.

## Color Lookup: bounded imported resource

Proposed scope is creative 3D `.cube` LUTs. Support `LUT_3D_SIZE` 2–65 inclusive, `DOMAIN_MIN`/`DOMAIN_MAX` with default 0/1, optional TITLE and comments, exactly N³ triples with R varying fastest. Limit source text to 16 MiB, title to 256 characters, lines to 4096 bytes, all numbers finite with absolute value at most `1e37`, each domain maximum strictly above minimum. Parse in f64, convert once to stored Float32, then revalidate distinct finite endpoints and samples; reject domains that collapse after conversion. Normalize domain coordinates with scaled differences to avoid finite endpoint subtraction overflow. Accept LF/CRLF and leading/trailing horizontal whitespace; recognize whole-line comments after leading whitespace, not arbitrary trailing tokens. Only ASCII header/data syntax is admitted. Validate selected-space output sample conversion into every supported working RGB space before publishing a resource; changing Color space repeats that validation. Reject 1D/combined shapers, unknown directives, duplicate declarations, malformed/extra/missing samples and out-of-order data. Do not silently approximate unsupported camera-log transforms. The grammar and ordering were checked against Adobe’s original specification via an accessible mirror. Capy’s 65-edge bound is deliberately narrower than the format’s 256-edge maximum; accepting CRLF and longer bounded lines is deliberate reader tolerance. [Adobe Cube LUT Specification 1.0](https://kono.phpage.fr/images/a/a1/Adobe-cube-lut-specification-1.0.pdf)

The effect stores one explicit RGB space for **both input and output** (sRGB default, P3/AdobeRGB/ProPhoto available), independent of the document's working space. `.cube` supplies no reliable profile metadata. The UI shows this Color space choice; default sRGB is not inferred from the current ProPhoto document. No Rec.709 video-range/log/camera interpretation is claimed. Values are encoded in that chosen space; convert document linear RGB to this space, perform tetrahedral lookup, decode/convert back to document linear RGB, and preserve alpha.

Intensity `[0,100]%`, default 100. For out-of-domain input `e`, let `b=clamp(e,domain_min,domain_max)` and define extended result `e + (T(b)-b)`. This preserves the out-of-range residual rather than silently clipping highlights/negative values; an identity LUT stays identity outside its domain. Mix original and mapped encoded coordinates by Intensity; Intensity zero returns original pixels directly. The domain bounds define cube coordinates, not an extra input transfer function. Output triples may be finite and extended.

Storage/render design:

1. A parsed immutable `Arc<Lut3d>` owns samples, dimensions, declared domain and digest. An effect value references this resource; cloning/history/snapshots share it. Import/parse/hash occurs off input threads. Empty resource renders identity and an Import… property; replacing it is one undoable action.
2. Use the existing indexed binary-payload pattern for project/preset/cross-document transport. Serialize a small descriptor plus digest/reference, deduplicate payload bytes and validate count/length/digest/finiteness before publication. Include bytes in `ProjectLimits.asset_bytes`; count unique retained resources and GPU residency. A 65³ RGBA GPU table is 4,394,000 bytes, separate from small parameter uploads.
3. Introduce one host-owned read-only auxiliary storage binding for resource-consuming effects, with typed declarations `lut3d`, `local_illumination`, `dehaze_guide`. No authored resource declarations or arbitrary compute IO. These effects create image boundaries, so a stage has only one auxiliary resource; this avoids an atlas or a per-chain resource packing framework. Ordinary pointwise filters keep fusion. The binding has a harmless empty buffer when unused.
4. Reuse the proof sampler's six explicit tetrahedra and tie ordering. A dedicated three-component accessor reads cube values; keep dynamic-vector-lvalue and Dawn performance workarounds. Update ABI to 4 in one coordinated milestone, catalog/shader validation, generated pipeline cache keys, embedded-program validation and every host bridge build. Add no ABI-3 adapter. A new project/preset format must reject unsupported versions explicitly.
5. Cache resource GPU buffers by digest and device; parameter/opacity/intensity edits and display-resolution variants share them. Upload once per new resource/device. Preview, live composition and exact export execute the same resource semantics. Runtime package loading cannot smuggle new bindings or oversized assets through metadata.

No separate LUT-browser/settings subsystem is necessary in M6. Import/Replace in Color Lookup and self-contained user effect presets provide reuse; use `profile_library`'s bounded/content-addressed storage pattern without copying its whole UI.

Oracles: N=2 identity, affine matrix cube, nonlinear off-grid cube with all six tetrahedron orders and ties; nonunit/negative domain; extended samples; identity outside domain; 0/50/100 intensity; every RGB space/depth/alpha; same output after copy/paste and reopen after deleting the original file; rejection paths; 100 intensity edits do not upload/reallocate cube storage; resource release after layer/history ownership ends.

## Local adjustments and source-aware analysis

Add a narrow typed `EffectAnalysisKind` for `LocalIllumination` and `Dehaze`; the manifest declares a supported analysis, not arbitrary image-reading preparation WGSL. Shared Rust owns its dependency key, memory admission, cancellation and publication. Hosts only schedule/poll/wake, using their existing worker/async integration. This can share the request/cancellation machinery with histogram analysis, but should not become a general job framework.

Input is the actual composite at the effect's insertion point, including group/clipping rules and lower effects, before this effect's own mask/blend/opacity. Use the `compose` function's `stop_before: Option<(usize,bool)>` argument in `scene/stack.rs`, the corresponding Scene field, and `scene_images::input_scope`; a subset of visible layers is insufficient. Cache key includes document epoch, source graph/value revisions (including provisional gestures), source geometry/color/blending, analysis kind/version, relevant analysis parameters, animation snapshot and device. Adjustment amounts that only consume the guide do not rebuild it. Layer moves/reordering, masks on lower layers and M5 retained-transform edits do invalidate it.

Factor `snapshot/tone.rs`'s bounded region reduction so effect analysis can feed it an insertion-point snapshot. Stream regions in document coordinates; reserve all guide/intermediate bytes first; use its split-on-budget behavior, per-region/per-anchor yielding, cancellation and generation rejection. Analyze lower source-aware effects in stack order before upper ones; the stack is already acyclic. Derived guides are never persistent artwork and are not exported as baked approximations. Exact export constructs/awaits the matching guide asynchronously. Live rendering may retain the last compatible guide while source analysis is pending, following existing SDR guide behavior, but must expose pending state to the session and must not substitute it for save/export pixels. Initial missing guide displays unchanged input until ready; errors remain visible and retry only after relevant state changes.

Reuse the current 768-edge document-space approximation for LocalIllumination, including coverage-aware reduction and bilateral four-point illumination lookup. This yields the same guide at every zoom and export resolution. It is not a promise of pixel-exact full-resolution local analysis. New amount sliders are cheap pointwise consumers after preparation.

Proposed Shadows/Highlights parameters: Shadows `[0,100]%`, Highlights `[0,100]%`, both default 0. Use linear luminance `Y`, log floor `-24 EV`, `l=log2(max(Y,2^-24))`, illumination `b`. Define `S=1-smoothstep(log2(.18)-4,log2(.18),b)` and `H=smoothstep(log2(.18),log2(.18)+4,b)`. These 4-EV transitions keep the smooth-field output-log slope at least .25 even at maximum correction. `delta=2*(Shadows/100)*S - 2*(Highlights/100)*H`; output `rgb*2^delta`, preserving hue ratios and alpha. Leave nonpositive luminance unchanged; neutral bypass is exact. This gives bounded ±2 EV recovery with no new radius/tonal-width settings.

Clarity has Amount `[-100,100]%`, default 0. Let `d=l-b`; `delta=clamp(Amount/100*d,-2,2)` and output `rgb*2^delta`, preserving alpha and nonpositive luminance. This reuses the existing Tone×Detail decomposition without turning the SDR viewing controls into document state or imposing the SDR rendition's tone compression/gamut mapping. A separate Texture adjustment and arbitrary radius control are outside M6.

Dehaze requires a new bounded RGB/transmission analysis; it cannot reuse the scalar illumination guide as if it contained atmospheric color. Proposed original implementation: on the fixed 768-edge alpha-aware linear-sRGB guide, compute a separable 15×15 dark-channel minimum; select airlight from the brightest source among the highest .1% of dark-channel values, deterministic coordinate tie-break; clamp its components to a positive epsilon. Compute dark channel of RGB/airlight and refine with the scalar guided filter. [Dark-channel model](https://mmlab.ie.cuhk.edu.hk/archive/2011/Haze.pdf)

For refinement use coverage-weighted guide luma normalized to `[0,1]`, radius 8, epsilon `.001`; `a=cov(I,p)/(var(I)+epsilon)`, `b=mean(p)-a*mean(I)`, output `mean(a)*I+mean(b)` clamped `[0,1]`. Bounded separable box passes avoid float atomics. Retain guide and coverage for the existing edge-aware four-point upsample pattern. Implement equations independently; do not copy research demo code. [Guided-filter equations](https://people.csail.mit.edu/kaiming/publications/eccv10guidedfilter.pdf)

Dehaze Amount `[-100,100]%`, default 0. With `s=abs(amount)/100` and refined darkness `d`, `t=max(.1,1-.95*s*d)`. Positive amount returns `(I-A)/t+A`; negative amount adds haze with `I*t+A*(1-t)`; operate in linear sRGB and convert back, with exact zero bypass. Preserve source alpha, finite extended output and layer mask behavior. All-transparent/zero-airlight input yields unchanged input. Freeze analysis parameters to these values in M6; no atmospheric-picker panel or general auxiliary mask input.


The Dehaze analysis choices are fixed as follows:

- Clamp source analysis RGB to [0,1] in linear sRGB **before** alpha-weighted area reduction to the guide. Full-resolution output still uses the original extended input.
- Ignore zero-coverage cells in both dark-channel minima; clamp spatial edge coordinates. An all-empty window stays empty. Preserve coverage independently from darkness.
- Let N be covered guide cells and K=max(1,ceil(.001*N)). Select the K largest dark values, breaking ties by lowest row-major coordinate. Among these choose the source cell with highest linear-sRGB Y, again lowest coordinate on ties. Estimate airlight on GPU using bounded radix rank selection/reductions, not a CPU download/sort; yield between rank passes.
- If there is no covered cell or selected airlight luminance is nonpositive, render identity. Otherwise floor each airlight component at 2^-16.
- Guided-filter I is bounded guide linear-sRGB Y, with no image-dependent range normalization. Every box mean/moment is alpha-weighted; zero-weight boxes remain empty. Use valid covered a/b moments in the second box pass; radius 8 and epsilon .001 are fixed.
- Store refined darkness clamped [0,1], original guide log-luminance with floor -24, and coverage. Upsample using the current four-neighbor local-guide spatial/coverage weighting and range weight `1/(1+((guide_logY-source_logY)/1.5)^4)`; derive source_logY from the bounded analysis RGB. If total weight <=1e-12, darkness is zero. This reuses the existing edge-aware gather with a defined zero-darkness fallback.
- Amount only consumes this guide; it does not rebuild it. Changing lower-source artwork does. Finite-input/intermediate validation and resource admission are shared with the other analyses.

**Required quality gate:** Shadows/Highlights, Clarity and especially this Dehaze recipe are proposed algorithms, not code-proven photographic results. Before building host UI, produce independent GPU/CPU-oracle fixtures and a compact visual sheet of portrait, landscape, sky, snow/white wall, high-contrast edge, transparent edge and HDR highlight cases. Require exact neutral identity, unchanged constant fields for Clarity and unchanged atmospheric input `I=A` for Dehaze; Shadows/Highlights intentionally changes constant dark/bright fields. Reject halos, unintended hue shifts and gradient reversals, including a smooth-ramp oracle at maximum Shadows/Highlights. Keep this as an explicit quality checkpoint; any algorithm change requires a reviewed specification revision.

## T-3 Gaussian controls and moderate range

This milestone covers moderate Gaussian ranges, not M7's pyramid/lens/tilt-shift blurs. Set sigma hard `[0,85]`, soft `[0,21]`, step `.1 px`, default unchanged, Power exponent `.5` (the existing numeric convention maps value^exponent, so `.5` gives finer low-end travel). Keep the visible label Radius to match the current UI; document that stored sigma uses a three-sigma support. Do not derive defaults from document size through a new policy mechanism.

Replace the current 64-lane/63px cap with a 256-lane preparation, support `ceil(3*sigma)` up to 255, 129 records (header + 128 paired taps), normalized exactly as today. For reduced composition divide sigma by texel side before preparation and multiply tap positions back into document pixels, as today. Update Gaussian Blur, Unsharp Mask, High Pass, Bloom, Soft Focus and Pencil together. Keep per-pass declared `3*sigma` footprints and check reduced-grid interpolation padding through existing `pass_radius`.

Tests: sigma 0 exact identity, .1, 1, 21, 21.1, 64, 85 against independent normalized discrete Gaussian; constant field, impulse, transparent boundary, tile edge and document edge; repeated intensity-only edit does not prepare taps; sigma change prepares once; native vs reduced variants cannot overwrite one another; windowed output matches full rebuild. Exercise Frequency Separation at its admitted range too, since it consumes Gaussian/High Pass. Measure large halos on 24MP plus tier canvases before asserting interactive targets; a raised hard range is not proof that every device meets its rate.

## T-14 shared gradient definition

Replace the duplicate recipe models with core `GradientDefinition { stops, interpolation, dither }`, using existing `GradientStop` and `ColorMixSpace` (`Oklab`, `LinearRgb`, `Classic`) and retaining tagged endpoint definitions. Shape is `Linear`, `Radial`, `Reflected`; geometry remains owner-specific. `EffectValue::Gradient` owns this definition; the transient layer operation uses it too. Selection/mask gradients use validated scalar stops `{position,value,opacity}` derived from the same editor session, not colored stops interpreted independently on each host.

Preserve the existing 2–32-stop bound, exact endpoints 0 and 1, strict increasing positions, finite values and alpha `[0,1]`. A duplicate-position hard edge is outside this milestone. New gradients default to Oklab and Dither on. Interpolation happens in the selected coordinates, with alpha-weighted coordinates: interpolate `(coordinate*alpha,alpha)`, unassociate if alpha>0, then convert to document linear RGB. Classic means encoded **document** RGB; Linear light means linear document RGB; Oklab uses the existing working-primary transforms and signed roots. Endpoint colors remain exactly their tagged values at t=0/1. Gradient Map keeps stop alpha as mapping strength; Gradient Fill/tool use it as coverage.

For tool geometry, `u=dot(p-start,end-start)/|end-start|²`; Linear uses `clamp(u,0,1)`, Radial uses clamped normalized distance and Reflected uses `clamp(abs(u),0,1)`. A zero-length drag makes no edit. Reverse is `1-t`, applied after shape. Gradient Fill's existing angle/center/scale geometry maps to the same shape evaluator, with Reflected centered on its declared center. No independent geometry implementation per host.

Put the bounded stop lookup/interpolation helper in shared WGSL included by effects, paint operations and mask rendering; do not copy it into three shader files. Keep exact stop data in 65 records, carrying interpolation/dither flags in currently reserved header fields. CPU stop insertion uses the same declared interpolation and alpha rule. Delete the old two-color/radial/transparent packing and assertions instead of retaining adapters. Reuse host stop editors with a shared action destination for effect vs tool settings; the host must not fake a document effect layer to edit a tool gradient.

Dither is deterministic in document pixel coordinates and stable across frames, zoom, tile boundaries and export regions. Add zero-mean noise of at most half an integer code step in the final encoded RGB quantization domain; do not perturb alpha for color gradients. At float document depth it is a no-op. Mask gradients may dither their scalar coverage at its actual coverage quantization step. Preserve exact endpoints, constant gradients and fully transparent spans; do not generate coverage outside the gradient/selection. Use the same coordinate hash everywhere and test its mean/range; no animated noise or reusable random texture is necessary.

Tests: all three shapes/mixing spaces; tagged cross-profile endpoints; nonuniform narrow knots; opaque→clear without dark fringe; mismatched endpoint alpha; reverse and off-canvas/transformed layer coordinates; insertion samples exactly the displayed color; mask gray/opacity cannot inherit Oklab lightness; one undo per drag and cancellation; stable dithering across tiles/pan/zoom/redo/export; 8/16-bit banding fixture plus unchanged float results; project save stores raster results and editable fill definitions correctly.
