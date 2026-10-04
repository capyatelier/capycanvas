# Photo adjustment mathematics

[Runtime filters](runtime-filters.md) · [Properties controls](../ui/numeric-controls.md#properties-and-curves) · [Color sampling](../ui/color-picker.md#levels-and-curves)

These contracts describe the shipped runtime adjustments. Parameter schemas and
WGSL live in [`assets/filters`](../../assets/filters); shared calibration and
numeric editing live in [`layer-ui`](../../crates/layer-ui/src/effects.rs).
Processing preserves source alpha and supports signed/HDR values. Neutral
adjustments return the original premultiplied pixel without conversion when
clipping and other active options permit an identity result.

## Levels and calibration

For one encoded scalar, apply the channel stage before the RGB/master stage:

```text
u = (x-black)/(white-black)
u = clamp(u,0,1) if Clamp input
v = sign(u)*abs(u)^(1/gamma)
y = output_black+(output_white-output_black)*v
y = clamp(y,0,1) if Clamp output
```

Gamma 1 bypasses the power. Input anchors require a representable gap of at
least .001; output anchors may cross. Accepted bounds constrain authored data
independently of slider ranges. Changing document depth does not narrow stored
values; built-in code and controls resolve from the current bundled catalog.

Auto counts each pixel with positive alpha once, after unassociation. Individual
pages analyze their channel before correction; RGB analyzes all three channels
after their stages and before master. A minima/maxima pass precedes 4096 uniform
bins per nonconstant channel. Quantiles use nearest ranks `ceil(p*N)` at
`p=.001,.999`, taking the bin midpoint clipped to observed extrema. The value
error is at most `(max-min)/4096`. GPU coordinates use scaled finite values;
CPU summary calculations use Float64. RGB takes the smallest low and largest
high quantile, including constant channels, for one common stretch. Auto resets
only selected input anchors and gamma. Empty, invalid, degenerate or
unrepresentable results fail atomically. Publication checks the frozen source,
owner, effect values and selected page.

For calibration let `(l,h,g,a,b)` be channel anchors/gamma/output anchors, `M`
the master scalar, `x` the input sample and `y` the desired final encoded value.
With `z=M^-1(y)`, `v=(z-a)/(b-a)` and `q=sign(v)*abs(v)^g`:

```text
Black: l' = (x-q*h)/(1-q)
White: h' = l+(x-l)/q
Gray:  g' = ln(abs((x-l)/(h-l)))/ln(abs((z-a)/(b-a)))
```

Black targets 0, White 1 and Gray the processed sample's encoded-domain
luminance. Gray requires matching signs and nondegenerate terms. Solve in
Float64, pack once, then validate the complete chain including clamps. RGB
changes all channels atomically and preserves master/output settings.

## Curves coordinates and calibration

Encoded RGB stores document-encoded coordinates and displays `255*x`.
For Log HDR, `F=-8`, `s=hdr_stops-F`, `toe=exp2(F)*e` and
`knee=1/(ln(2)*s)` define encoding E and decoding D:

```text
E(v) = v/(toe*ln(2)*s) when v <= toe; otherwise (log2(v)-F)/s
D(x) = x*toe*ln(2)*s  when x <= knee; otherwise exp2(x*s+F)
```

Log numeric fields show physical linear values, with white at `E(1)`. Shared
formatting and neighbor representability preserve small positive values and
strictly ordered knots. Graph interaction is specified in
[numeric controls](../ui/numeric-controls.md#properties-and-curves).

Calibration preserves master and changes channel curves. Black/White target
linear 0/1; Gray targets the processed sample's document-primary linear
luminance. Invert master by enumerating every shape-preserving Hermite segment
whose endpoint-y interval contains the encoded target. Nonconstant segments
use bounded bisection; a matching constant segment projects current channel
output onto its root interval. Choose the root nearest current channel output,
with lower z breaking a tie. A globally nonmonotonic master is valid.

Pack and verify the root, then insert or reuse a knot at the exact sampled x.
Reuse tolerance is .002 normalized x; endpoints move only in y and the maximum
is 32 points. Refuse unrepresentable neighbors, unreachable targets or a full
table atomically. The final Float32 chain must satisfy normalized error
`8*f32::EPSILON` and linear error `max(2e-6,2e-4*abs(target))`.

## Hue ranges and Selective Color

Membership uses original unassociated linear RGB converted to physical Oklab.
Default hue centers are 30, 110, 145, 195, 265 and 330 degrees, with full-strength
Width 30 and Feather 30. For circular angular distance d:

```text
weight = (1-smoothstep(width/2,width/2+feather,d))*smoothstep(.005,.02,chroma)
```

Zero Feather uses the closed hard interval. Sum weighted corrections with
Master, clamp Saturation/Lightness to ±100%, wrap Hue and apply extended HSL
once. Colorize uses its independent hue/saturation and Master Lightness,
retaining inactive range settings.

Selective Color interpolates between adjacent cyclic hue centers. Their total
weight is encoded chroma `c=max(u)-min(u)`, where `u=clamp(encoded_rgb,0,1)`.
Remaining weight `1-c` is partitioned by Color Balance's tonal weights:
Shadows `1-smoothstep(0,.5,Y)`, Highlights `smoothstep(.5,1,Y)` and the residual
Midtones, with `Y` encoded document luminance. Accumulate normalized CMY/K
corrections before applying them once:

```text
ink = 1-u
new_ink = clamp(ink+CMY*(ink if Relative else 1),0,1)
v = clamp(1-new_ink-K*((1-max(u)) if Relative else 1),0,1)
out = original_encoded+(v-u)
```

Black uses original u. This is Capy's formula, not proprietary editor pixel
math; the extended residual remains unchanged.

## Local adjustments

[Runtime filters](runtime-filters.md#shadowshighlights-and-clarity) owns guide
lifetimes, admission and the fixed 768-edge spatial approximation. With linear
luminance Y, `l=log2(max(Y,2^-24))`, guide illumination b and `m=log2(.18)`:

```text
S = 1-smoothstep(m-4,m,b)
H = smoothstep(m,m+4,b)
Shadows/Highlights: delta = 2*(Shadows/100)*S-2*(Highlights/100)*H
Clarity: delta = clamp((Amount/100)*(l-b),-2,2)
out = rgb*2^delta
```

Nonpositive luminance stays unchanged. Hue ratios and alpha remain intact;
overflow preserves the source and subnormal outputs use nearest-even packing.

## Dehaze

Analysis clamps source linear-sRGB to [0,1] before coverage-weighted guide
reduction. A separable 15×15 dark minimum ignores empty cells. From the highest
`max(1,ceil(.001*N))` dark values, choose the source with highest linear-sRGB
luminance; lowest row-major coordinate breaks ties. Nonpositive airlight yields
identity; otherwise floor its components at `2^-16`.

Refine the dark channel of RGB/airlight using a coverage-weighted guided filter:
radius 8, epsilon .001, bounded linear-sRGB luminance I, `a=cov(I,p)/(var(I)+.001)`,
`b=mean(p)-a*mean(I)`, and darkness `clamp(mean(a)*I+mean(b),0,1)`.
Four-neighbor upsampling uses spatial/coverage weights and range weight
`1/(1+((guide_logY-source_logY)/1.5)^4)`. Only zero coverage uses fallback.

For bounded straight source B, atmosphere A, darkness d and amount in percent,
`t=max(.1,1-.95*abs(amount)/100*d)`. Negative amounts add haze:
`C=B*t+A*(1-t)`. Positive amounts reconstruct `J=(B-A)/t+A` with fixed protections:

```text
chroma = max(B)-min(B)
white = smoothstep(.5,.9,Y(B))*(1-smoothstep(.1,.35,chroma))
C = B+(J-B)*(1-white)
stable = (B/Y(B))*max(Y(C),0) if Y(B)>0, otherwise B
weight = (1-smoothstep(.02,.12,chroma))*(1-smoothstep(.05,.18,Y(J)))
C = C+weight*(stable-C)
```

Convert `C-B` to document linear RGB, multiply by original alpha and add to
original premultiplied RGB. This preserves extended residuals. Zero amount,
transparent input, inactive airlight, `t=1` and `B=A` bypass exactly. Guide
construction and consumer use the same frozen insertion source for live,
query and export paths.
