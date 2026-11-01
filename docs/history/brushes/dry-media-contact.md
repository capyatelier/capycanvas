# Dry media: analytic relief and paper contact

[Brush design records](README.md)

This record fixes the intended dry-media model and explains its decisions. It
covers pencil, charcoal, chalk, pastel and crayon through one contact evaluator.
The equations describe the accepted design, not a claim that the editor or every
analytic integral is implemented. The
[contact engine guide](../../internals/contact-brush-engine.md) describes current
code. The [earlier redesign](../dry-media-brush-design.md) supplies the preceding
stroke-engine context.

## Accepted design

Dry media deposit pigment where an analytic contact surface engages a stationary
paper height field. Pressure recruits contacts; pigment loading strengthens
contacts that already engage. Relief varies with tilt through three authored
poses. The edge taper comes primarily from contact loss, including empty paper
contacts near the boundary, rather than a radial opacity fade.

**Authoring contract:**

- **One analytic relief field per pose.** Predefined shapes and constrained
  profiles supply efficient evaluation and integrals. There is no arbitrary
  author code, separately authored support mask or pressure distribution map.
- **Three poses.** Fixed upright, writing and side keys have authored dimensions
  and relief profiles within one brush-wide supported shape family.
- **Two shared material controls.** Texture strength `S` and Material hardness `Lambda`
  apply across all poses. Engagement width `W` is fixed at `0.05`.
- **Existing pigment and dynamics controls.** Brush size, pressure response,
  pressure-size response, Strength, Opacity and color complement material contact.
  Strength scales material pressure; Opacity caps one stroke over its starting paint.
- **No separate relief strength or bite control.** The relief field contains its
  values directly. A fixed paper-relief coupling makes selectivity evolve with
  that field.
- **Paper can be a bitmap.** A procedural paper recipe can also be baked into a
  cached height texture. Bitmap contact/relief fields are outside this design.
- **No forced maximum-pressure contact.** Some valleys and recessed edge areas
  may remain empty at maximum pressure.

### Contact and loading

At one canvas point inside the resolved analytic shape:

```text
p_pen = clamp(input_pressure(stylus_pressure), 0, 1)
p = Strength * clamp(pressure_gain * p_pen, 0, 1)

h = clamp(0.5 + S * (paper_height - 0.5), 0, 1)
d = 0.5 - h

T = R + (1 + kappa * R) * d              kappa = 0.25
x = clamp((0.5 * p - T) / W, 0, 1)

C = x*x * (3 - 2*x)
H = 1 - (1 - Lambda)^2
L = (1 - H) + H*p*p
local_deposition = C * L
```

Outside the shape, `C = 0`. These equations are conditioned on an active painting
contact; pen-up and zero-force endpoint handling remain explicit input/stroke
rules. The nonzero floor in `L` does not establish a pen contact or independently
guarantee zero deposition at zero pressure.

| Quantity | Meaning and source | Authoring location |
| --- | --- | --- |
| `stylus_pressure` | Normalized device pressure. | Device input. |
| `input_pressure` | Existing device calibration and input smoothing. | Input processing. |
| `pressure_gain` | Material approach gain, clamped before Strength multiplication; one in built-in dry presets. | Brush material definition; no per-pose map. |
| `p_pen` | Calibrated physical pressure in `[0,1]`; controls pressure-driven dimensions. | Calculated. |
| `Strength` | Material-pressure multiplier in `[0,1]`, after existing Strength mappings. | Painting control; stable `flow` ID. |
| `p` | Strength-scaled material pressure in `[0,1]`; drives contact and loading. | Calculated on the CPU per contact. |
| `paper_height` | Paper height in `[0,1]`; larger values are peaks. Coordinates are stationary in document space. | Selected paper asset and its feature scale/orientation. |
| `S` | Strength of paper-height variation; `S >= 0`. | Brush-wide Texture strength, relative to the preset's paper response. |
| `h`, `d` | Adjusted height and centered paper depth. | Calculated. |
| `R` | Nonnegative local recess in the model's normalized height units; smaller values engage sooner. | Analytic relief profile in each pose. |
| `kappa` | Relief-dependent paper-selectivity coupling. | Engine constant `0.25`. |
| `T` | Engagement threshold. | Calculated. |
| `W` | Positive engagement interval from first contact to full transfer. | Fixed `0.05`; absent from the painting controls. |
| `x` | Clamped local engagement, not radial position or pen pressure itself. | Calculated. |
| `C` | Contact response in `[0,1]`. | Calculated; cubic engine transfer for this design. |
| `Lambda` | Loading pressure response in `[0,1]`. | Brush-wide Material hardness; stable `material_hardness` setting ID and authored `loading` field. |
| `L` | Pigment loading conditional on contact. | Calculated. |
| `0.5`, `3`, `2` | Height/approach normalization and cubic endpoint coefficients. | Engine constants. |

### Built-in material defaults

These are recipe choices in the shared brush definitions. All seven use approach
gain one; their Strength defaults preserve their preceding material pressure.

| Preset | Material hardness `Lambda` | Strength |
| --- | --- | --- |
| Pencil | 0.70 | 70% |
| Pointy Pencil | 0.75 | 70% |
| Shading Pencil | 0.65 | 63% |
| Charcoal | 0.55 | 85% |
| Chalk | 0.15 | 75% |
| Pastel Block | 0.60 | 85% |
| Crayon | 0.90 | 70% |

Higher pencil/crayon hardness makes light contact release less pigment. Powdery
charcoal/pastel retain more pigment at light contact; Chalk retains the strongest
loading floor. Relief and paper still determine which locations can engage.

### Tilt and contact coordinates

Let `theta` be tilt away from upright, expressed in the same angle units as the
three ordered pose keys `theta_0 < theta_1 < theta_2`. Between keys `i` and `i+1`:

```text
t = clamp((theta - theta_i) / (theta_(i+1) - theta_i), 0, 1)
w = t*t * (3 - 2*t)
resolved_parameter = (1 - w)*parameter_i + w*parameter_(i+1)
```

Use the nearest endpoint pose outside the authored angle range. Resolve
orientation along the shortest angular path. Shape transitions use predefined
morph rules within a supported family; an enum identifying a shape is not a
quantity to interpolate.

For resolved dimension ratios `(a,b)` and orientation `psi`:

```text
radii = 0.5 * brush_size * pressure_size_response(p_pen) * (a,b)
(u,v) = rotate(canvas_point - stroke_center, -psi) / radii
```

The division is component-wise. `brush_size` is the long writing-pose axis at
full pressure; `a` and `b` are positive pose dimensions relative to that axis.
`psi` combines the transformed stylus direction with the authored orientation
offset. The upright/absent-direction case uses a stable engine orientation
policy. Neither zoom nor canvas rotation changes the material's tilt magnitude.

### Analytic relief

The implemented family uses an ellipse or a beveled block:

```text
ellipse support: q0 = u*u + v*v
ellipse relief: q = (u + delta)^2 + v*v
block: edge = max(abs((u,v)) - face, 0) / rim
       q = (edge.x + edge.y)^2
support: q0 <= 1 for ellipses; q <= 1 for blocks

R(u,v) = r0 + c2*q + c4*q*q
```

Use nonnegative `r0`, `c2` and `c4`. `r0` sets the central recess, `c2` gives a
rounded rise, and `c4` gives a flatter center with a steeper rim. These are field
values, without a separate relief-depth multiplier. The block's flat face has
constant recess; its bevel approximates a small rounded corner. The corner width
is brush-wide and scales with the smaller semiaxis. Three fixed tilt keys at 0°,
45° and 76.5° interpolate dimensions and coefficients; the pose editor is deferred.
Pointy Pencil uses `delta=0.65*smoothstep(0,1,clamp(theta/theta_1,0,1))`,
placing its shallowest relief toward the stylus-tip
side, local negative X. Other built-ins use zero. The physical outline remains
centered; the offset changes engagement within that outline. The renderer
transforms this vector when aligning its integration coordinates with motion,
so reversing stroke direction does not reverse the tip-side bias. Changes
between different shape families remain deferred.

A future constrained relief widget can produce these coefficients from presets
and curve controls; built-in definitions currently provide them directly. They are the field definition itself. There is no additional
`relief_strength * R` multiplier. The polynomial family is fixed in code; a
freehand curve or expression does not become arbitrary executable shader math.

Resolve geometry and profile coefficients with tilt, then evaluate the resulting
field. Do not blend separately rendered marks or independently normalized maps.
Circle and ellipse share one family naturally. Rectangle and rounded-block
families need their own predefined support, profile and morph/integral rules.
The ellipse equation does not promise that all other shapes use the same `q`.

| Authoring quantity | Current definition |
| --- | --- |
| Tilt key | Engine-fixed 0°, 45° and 76.5°. |
| Contact shape | Brush-wide ellipse or beveled block. |
| Width and length | Positive relative dimensions in each pose. |
| Corner width | Brush-wide block rim fraction. |
| Relief profile | Nonnegative `r0`, `c2`, `c4` in each pose. |
| Relief offset | Pointy Pencil ramps from zero upright to `0.65` by the writing pose; others zero. |
| Alignment | Transformed stylus azimuth; no additional per-pose offset. |

The pose editor is deferred. Key editing, further directional skew and morphing across
different shape families require their own authoring and integral decisions.

Brush size is the long axis of the writing pose at full pressure. That pose's
largest relative semiaxis is one; upright and side dimensions scale around it.
The maximum pose extent remains a separate reference for path retention and
taper distance. Size is geometric support, not the width of fully deposited
pigment. Stamp spacing and size/rotation variation are absent from this model.

`S` and `Lambda` are shared brush controls, not additional controls on these
pose keys. `W` is a fixed engine choice. Shape-specific curve handles are not
fixed by this mathematical contract.

### Swept exposure and pigment accumulation

For a fixed resolved pose and a canvas point, let `I_segment` be the swept
segment's portion of the contact chord, and `ell_full` the full geometric chord
length through that point:

```text
E = (1 / ell_full) * integral_over_I_segment(C * L) d_ell
F = (1 / ell_full) * integral_over_full_chord(C * L) d_ell
Delta_D = K * E
B = K * F
p = contact.x = Strength * clamp(pen * pressure_gain, 0, 1)
phi = mix(0.12, 1, p^3)
room = max(0, B-D_old)
D_new = D_old + mix(min(Delta_D,room), Delta_D, phi)
A = 1 - exp(-D_new)
alpha_stroke = opacity * A
```

The chord is normalized using geometric support, not the fragment length or the
number of engaged paper contacts. Empty/outside support contributes nothing.
The stationary-tap limit evaluates `C*L` only for a confirmed tap, using the
[engine's tap exposure rule](../../internals/contact-brush-engine.md#swept-deposition),
without dividing by a zero chord.

`E` is renderer exposure. `K` folds in the fixed optical normalization six,
color alpha and directional feed; it is not another editor control. Strength
enters only through material pressure, not through `K`. Dry contacts carry unit
pigment feed; their material pressure is already resolved on the CPU. `D` is a
conceptual optical dose, not a requirement to store a new physical
pigment layer in the artwork. `A` describes ordinary same-color flow
accumulation. Existing blending, selection and other accumulation modes retain
their own renderer contracts.

Coarse full-pass exposure uses exact full-transfer length and three-node
Gauss–Legendre integration of the ramps. Fine contacts reuse their filtered
full-pass dose; a tap is its own pass. A completion uses
`B=K*max(max F, sum E + E_completion)`, so it fills the rest of the same pass.
Fine elliptical completions share the body filter’s transverse nodes and support
clipping for incident, joint and full-pass exposure.
Incident contacts compose in stroke order. Self-overlap beyond a full pass is
dwell and follows the same grip law, including inner corners and hairpins.

Resolve pose parameters on the CPU per retained input/contact, and send compact
resolved data to the GPU. Use predefined fixed-pose integrals and bounded
segmentation/interpolation for changing pressure and tilt. The design does not
claim a closed-form integral for an arbitrary continuously deforming tip.

## Decision history and mathematical rationale

The choices below separate accepted approximations from physical laws. A rejected
default can still be useful for another brush family. In particular, linear
contact remains an open alternative for a later visual/performance comparison;
the current cubic is not declared universally optimal.

### 1. Pressure to approach: `A = 0.5p`

**Purpose.** Convert Strength-scaled material pressure into a normalized approach distance
that recruits paper peaks before valleys. The pressure response is brush-wide;
there is no authored two-dimensional pressure map inside the tip.

**Choice and constant.** Linear approach keeps the response monotone and cheaply
integrable. The factor `0.5` matches the chosen centered paper-height units. It
is a calibration convention, not a measured force/displacement coefficient.
Increasing it makes a given pressure reach more recessed contacts; changing it
requires coordinated recalibration of relief and response widths. Existing
pressure mapping already handles sensitivity without changing this constant for
every pose.

**Alternatives.** A mechanical force-balance solve relates applied force to
indentation more faithfully but requires material properties, contact-area
feedback and potentially iterative work. It is outside this inexpensive model.
An additional power curve for approach is unnecessary while the existing
pressure response supplies that authoring behavior. A local multiplier
`p * relief_weight` is rejected as the default: a small weight can keep an edge
weak at every pressure, rather than letting it engage once approach exceeds a
geometric threshold. Separately authoring that multiplier also restores the
pressure map the design removes.

**Research basis.** [Greenwood and Williamson][rough-contact] motivate recruiting
asperities by separation and distinguish load from indentation. The
[crayon model][crayon] solves contact displacement from force. Our linear
conversion intentionally approximates that mechanical step; neither source
derives the coefficient `0.5`.

### 2. Paper response: `h = clamp(0.5 + S(H - 0.5), 0, 1)`

**Purpose.** Keep one stationary surface responsible for both interior tooth and
edge contact loss. `S=0` removes height variation, `S=1` preserves the asset's
height values, and larger `S` increases contrast until heights clip.

**Choice and constants.** Centering around `0.5` changes variation around a fixed
reference instead of translating the whole surface. The `[0,1]` bound keeps the
threshold response well behaved. Clipping at high strength can create plateaus;
it is intentional bounded contrast, not a physically simulated deformation.
Paper feature size and orientation come from texture coordinates, not `S`.

**Alternatives.** Multiplying final opacity by noise produces pale pigment even
where contact should be firm, and does not give geometry-based empty contacts.
Randomly rotating rough dabs ties grain to stamp placement and adds another
changing field; it is not needed for this stationary-paper model. A nonlinear
paper remapping can be baked into the asset if useful, rather than adding
another response function to every deposited pixel. Automatic histogram
normalization is rejected at runtime because changing an asset's level or tooth
should retain its authored meaning. Paper strength per pose duplicates controls
that the relief profile is intended to supply.

**Research basis.** [Sousa and Buchanan][graphite] describe height-dependent pencil
deposition. [Rudolf, Mould and Neufeld][crayon] represent paper as a height field.
The centered affine strength adjustment is our inexpensive authoring convention,
not an equation taken from either paper.

### 3. Tilt interpolation: three keys with cubic weights

**Purpose.** Represent the recognizable upright, writing and side postures while
allowing tilt to change continuously within a stroke. The same key weights
resolve dimensions, supported shape parameters and relief coefficients.

**Choice and constants.** The cubic weight has zero slope at each key. Adjacent
intervals meet smoothly, including at the writing pose. Authored key angles
determine transition locations and widths. There are no universal pencil-angle
constants: device normalization and brush design are separate concerns.

**Alternatives.** Hard three-state selection introduces visible size/profile
jumps and is rejected. Linear key weights are inexpensive but introduce slope
changes at keys; they remain a possible simplification if those changes prove
irrelevant. A single universal tilt-to-shape formula is elegant for one material
but constrains authors to its particular upright/writing/side relationship.
Arbitrary author functions need validation and lose the bounded predefined
integral contract. Dense lookup tables add storage and interpolation error
without being necessary for these short formulas. The selected three-key model
is one continuous evaluation rule with two interpolation intervals, not three
disconnected rendering modes.

**Research basis.** Pencil position and tip shape are inputs to the
[graphite model][graphite]. The easing polynomial is
[standard Hermite smoothstep][smoothstep]. Neither source prescribes three poses;
the key structure is an authoring decision.

### 4. Contact coordinates and analytic relief

**Purpose.** Describe where the tip lies and how recessed each point is, using
one field that controls contact onset, cross-sectional taper and paper selection.
Geometry accounts for the long side of a pencil or the face of a pastel stick.

**Choice and constants.** Translation, rotation and division by resolved radii
put each shape in normalized coordinates, with the support field equal to one
at its boundary. The
`r0 + c2*q + c4*q*q` basis provides onset and curvature without arbitrary
exponents. A central recess is useful for a lifted shading face; it is part of
the authored field rather than an additional strength parameter. Nonnegative
coefficients keep recess monotone in `q` and permit its inverse through one
quadratic root. No universal coefficient values are prescribed.

Along a fixed-pose straight sweep, `q` is quadratic within each admitted piece.
Relief is at most quartic, and cubic contact is at most degree twelve. This bounds
integration and clipping work. A stick uses the square of the sum of normalized
excursions beyond its flat face. Its support is an octagon; four slab intersections
provide the chord without circular corner roots. Only the small corners differ
from a rounded rectangle. The flat face, outer dimensions and monotone rim retain
the useful stick controls.

A shifted ellipse field supplies Pointy Pencil's directional engagement.
`q=(u+delta)^2+v^2` places its shallow point toward negative X; the centered
`q0=u^2+v^2` still bounds physical support. The built-in maximum `delta=0.65` moves the
minimum by 65% of the long semiaxis without moving the cursor outline. The shift
ramps with normalized tilt using the same cubic smoothstep, reaching its maximum
by the writing pose. Upright contact stays symmetric. A smaller `0.35` shift
left too little separation between tip-side and heel-side engagement for the
intended directional pencil. A constant shift would make even upright contact
asymmetric. It is a
recipe choice rather than a universal pencil constant. Increasing its magnitude
strengthens asymmetry and can leave more of the opposite side empty. The shift
retains a quadratic `q`, so existing clipped polynomial integrals still apply.
Changing dose by a directional opacity gradient would weaken grains independently
of engagement; a separate two-dimensional pressure map adds another authored
field. Both are unnecessary for this directional contact effect.

**Alternatives.** Circular corner arcs were considered for sticks; they require
quadratic boundary intersections without adding a necessary media control.
Superellipse exponents introduce higher-degree pieces or numerical roots. A
multiplicative directional skew `(c2*q+c4*q*q)*(1+b_skew*u)` keeps recess
nonnegative when `abs(b_skew)<=1`, but can lose outward monotonicity and raises
relief to degree five. Inverting that field for engagement needs more complex
roots; it is deferred while elongated tilt poses provide the required shading
behavior.

A separately authored mask duplicates shape support. A separate
pressure map supplies independent local transfer but adds a second field and
can restore a spatial opacity gradient; the intended dry-media marks do not
require it. An additional relief-strength multiplier duplicates the vertical
scale already contained in the authored field. Arbitrary analytic expressions
cannot guarantee cheap swept integrals. Bitmap relief may be inexpensive for
sampled evaluation, but has no general closed-form swept integral and is
deferred, not declared inherently too slow. Irregular or wedge shapes are not
forbidden in principle; they need predefined geometry and justified artist value
rather than unrestricted mask editing.

**Research basis.** The [crayon model][crayon] separates tool surface geometry from
material transfer. The [graphite model][graphite] considers tip position and
shape. Our polynomial basis and absence of a separate pressure field are
computational and interface approximations.

### 5. Threshold: `T = R + (1 + kappa R)d`

**Purpose.** Compare approach with a paper-and-tip threshold. Centered depth
`d=0.5-h` is negative at peaks and positive in valleys; it is a signed offset,
not a claim that physical valley depth is negative. Larger recess delays
engagement. The interaction makes paper selectivity change naturally with the
relief field and therefore with tilt.

**Choice and constant.** The additive baseline `T=R+d` is a valid geometric
approximation. The selected weak coupling uses `kappa=0.25` to add some
relief-dependent selectivity without another authored parameter. It is an engine
calibration choice, not a physical material constant. For `R>=0`, `d` in
`[-0.5,0.5]` and `0<=kappa<2`:

```text
dT/dR = 1 + kappa*d > 0
dT/dh = -(1 + kappa*R) < 0
```

Thus recess always reduces contact, peaks always engage more easily, and raising
pressure increases contact for a fixed pose. At `kappa=0.25`, the first derivative
lies between `0.875` and `1.125`. For fixed paper, the threshold remains affine in
relief: `T=d+(1+kappa*d)R`. The interaction therefore preserves the predefined
relief's polynomial degree. Increasing `kappa` strengthens coupling; approaching
or exceeding `2` risks losing strict monotonicity in recess.

**Alternatives.** `kappa=0` remains the simpler additive baseline, not a physically
invalid alternative. Stronger coupling, including `kappa=1`, is not the default
because it changes onset/selectivity more strongly than needed. A raw valley
depth product `R+(1+kappa*R)(1-h)` needs a consistent approach offset and relief
calibration; otherwise centering differences get mistaken for a different
material response. A bounded interaction such as `1+R/(1+R)` adds nonlinear
relief dependence and complicates the integral without a current requirement.
Independent per-pose paper response/bite adds a control the shared field is
intended to replace.

Clamping the upper threshold to ensure every point engages at maximum pressure
is rejected: permanently empty valleys or edge areas are permitted. Pure
`pressure*relief_weight-paper_depth` is rejected for the onset/ceiling coupling
described in section 1. A reference-relative interaction
`1+kappa*(R-R_reference)` is a possible refinement if arbitrary height-reference
changes must be invariant. It is not part of this contract: the predefined
field's height reference is fixed, and it must not be silently renormalized per
pose.

**Research basis.** [Rough-surface contact theory][rough-contact] motivates
separation thresholds. Drawing-specific height engagement is described by the
[graphite][graphite] and [crayon][crayon] models. The weak bilinear interaction is
our approximation; those sources do not derive `kappa=0.25`.

### 6. Engagement width: `x = clamp((A-T)/W, 0, 1)`

**Purpose.** Give eligible contacts a finite interval over which transfer becomes
strong. With fixed `T`, first contact is at `A=T`, half response at `A=T+W/2`, and
full response at `A=T+W`. Since `A=0.5p`, the unclipped pressure interval is `2W`.

**Choice and controls.** `W>0` prevents division by zero. Small `W` makes eligible
contacts strengthen quickly; large `W` leaves more partial transfer. It does not
change the empty set `A<=T`. `W` is artistically distinct from relief, which
changes onset. The initial design exposed it as Contact softness. The painting
interface now fixes it at `0.05` for every dry preset: eligible grains strengthen
over an effective-pressure interval of `0.1`, while paper and relief govern
contact loss. This favors distinct marks over a broad partial-transfer gradient.
It is not another pose field or a pixel blur radius. The internal field remains
part of the contact evaluator, allowing independent reference tests of its math.

**Alternatives.** Independent engagement softness has a meaningful material
authoring role, but exposing that subtle distinction in the painting panel adds
a control alongside the more useful Texture strength. Broader widths increase
partial transfer without recruiting or removing contacts. Automatically choosing `W`
from paper variance can counteract intentional paper-strength changes.
Automatically choosing it from relief range/slope can counteract an intentional
contact profile. Per-pixel or pressure-dependent widths add another varying
field and may complicate integration. Per-pose widths expand the editor before
there is a demonstrated design need. None is the selected default.

Map analysis can recommend a starting value or identify a large partial-contact
fraction across pressure and tilt. It cannot uniquely infer material behavior
from geometry. Antialiasing must account for pixel coverage separately; widening
`W` with zoom or brush size would change the material response.

**Research basis.** [Smoothstep][smoothstep] supplies a finite transition interval.
The [crayon model][crayon] includes material compression separately from surface
geometry. Our width is a phenomenological engagement control, not its compression
constant or a solved elastic modulus.

### 7. Contact transfer: `C = x*x*(3-2*x)`

**Purpose.** Suppress barely engaged transfer and approach full transfer smoothly.
The function can approximate unresolved engagement within a sampled paper grain;
it is not a literal microscopic contact simulation or final opacity.

**Choice and constants.** Requiring `C(0)=0`, `C(1)=1`, `C'(0)=0` and `C'(1)=0`
uniquely gives `3x*x-2x*x*x` as the lowest-degree polynomial. Its derivative
`6x(1-x)` is nonnegative. Near first touch it behaves as `3x*x`; near full
engagement the shortfall behaves as `3(1-x)*(1-x)`. The coefficients `3` and `2`
are fixed by endpoint conditions, not independent artistic settings.

**Alternatives.** A hard step provides binary grains and can simplify clipped
chord coverage, but needs appropriate spatial/swept averaging to avoid abrupt
changes. Linear `C=x` is the cheapest continuous candidate and can materially
simplify swept integrals; it remains open for the later comparison. Quadratic
`C=x*x` is inexpensive but asymmetric and has a slope discontinuity at its upper
clamp. Quintic `6x^5-15x^4+10x^3` smooths the second derivative at the endpoints
but adds polynomial work without a requirement for that continuity. General
sigmoids, powers and lookup curves add calibration or evaluation work without a
current authoring need.

The cubic is optimal only within the stated lowest-degree endpoint constraints.
It is not proven fastest or visually best for every dry material. For quadratic
engagement along a fixed sweep, the unclipped linear, cubic and quintic
integrands have degrees two, six and ten respectively. Clipping and integration
strategy still determine actual runtime.

All these positive continuous curves retain the same exactly empty set. The
desired missing contacts near edges come from the threshold and relief. A broad
`W` can still produce a gradient with cubic contact; changing its coefficients
is not a substitute for appropriate contact recruitment.

**Research basis.** The exact cubic is [WGSL's Hermite smoothstep][smoothstep].
Its use as contact transfer is our approximation, not a transfer law established
by the drawing-material papers.

### 8. Loading: `L = (1-H) + H*p*p`

**Purpose.** Control pigment amount at an engaged contact separately from
recruiting contacts. A paper peak already at `C=1` can continue depositing more
as pressure rises. Relief alone cannot supply that behavior while keeping the
contact saturated.

**Choice and constants.** Displayed hardness `Lambda` resolves to
`H=1-(1-Lambda)^2`. `L` blends constant loading and material pressure squared.
`Lambda=0` makes pressure act through engagement and existing geometry;
`Lambda=1` makes per-contact loading equal to pressure squared. Intermediate values
retain strong individual grains during light shading while allowing additional
pressure buildup. The material-pressure endpoint is normalized to one; full loading is reached
only when calibrated pressure and Strength both reach one, unless `Lambda=0`
makes loading constant. Displayed 50% resolves to effective hardness 75%; both endpoints stay fixed.

The painting control is **Material hardness**, with `Lambda` from zero to one.
Contact stays pressure-sensitive throughout. Higher hardness makes pigment loading
more dependent on pressure; lower hardness retains strong individual grains at
light contact. Pencils and crayon default high, charcoal and pastel medium, and
chalk low. These are artistic material responses, not a measured hardness scale.

A two-way Pressure balance control was considered and rejected. Its second half
froze contact pressure toward a full-pressure reference while retaining pigment
sensitivity. It added a second response regime without a useful painting control.
A separate density control would duplicate the pressure effect expected from
Strength. Scaling pigment dose by Strength independently was rejected: it
weakens existing grains without recruiting or releasing paper contacts.

**Alternatives.** Constant loading and pure `p*p` are retained as endpoints of the
family. Linear pressure loading releases more pigment at light pressure. A smoothstep or general exponent
of `p` provides more shaping but lacks a current requirement. Such loading can
often be resolved per contact on the CPU, so it is not inherently expensive per
pixel; nonlinearity can still increase integration complexity when pressure
varies within a sweep. Tying pigment amount to radial position or separately
authored local pressure restores a spatial dose fade that this design avoids.

`Lambda` is separate from bite: it changes pigment amount, not the contact
threshold. No universal material values are prescribed by this record.

**Research basis.** The [graphite model][graphite] distinguishes pigment transfer
from paper damage. The [crayon model][crayon] relates transfer to contact and
friction. They motivate a separate material response, not this particular
loading curve. Their material coefficients must not be confused with our `Lambda`.

### 9. Swept exposure: normalized contact integral

**Purpose.** Integrate the material response along the traveled contact rather
than inserting many randomly rotated stamps. Normalize by the full geometric
chord so partitioning a steady, fixed-pose passage does not add pigment.

**Choice and constants.** A split segment integrates over disjoint chord
intervals with the same denominator; its exposures add to the unsplit result.
For a full chord with `C=L=1`, `E=1`. The geometric denominator does not expand
when paper loses contact, so sparse grain remains a smaller dose rather than
being normalized back to solid coverage. Feed and world-travel scaling remain
part of the existing stroke-engine calibration.

**Ends and turns.** Flow deposition supplements this integral with localized
completion: `delta=g*C*max(0,max(F)-sum(e))`, where F is each incident line's full
passage and e its incident half-line passage. Endpoints use g=1. The shared
generator selects and coalesces turn candidates using contact-scale geometry.
Ordinary segments retain their original exposure and repeated-pass buildup.
The [contact engine](../../internals/contact-brush-engine.md#swept-deposition)
defines the gate, scheduling, and shader specialization. This is a geometric
boundary rule; the material literature does not establish it as a physical law.
At a symmetric endpoint, exposure becomes `F*(1+C)/2`, reaching F at full
engagement while preserving softer completion on weak paper contact.

**Alternatives.** Normalizing each fragment by its own length lets splitting
increase pigment and is rejected. Normalizing by only engaged contacts restores
dose lost through grain and defeats the paper response. An unnormalized length
integral changes dose with contact size unless feed is recalibrated; it is not
the chosen continuation of this engine. Dense stamp subdivision approximates
the field but increases work with path length. An exact integral through an
arbitrarily moving/deforming shape is outside the fixed analytic-family
contract. Resolve poses and bound changes with shared stroke segmentation.

**Pixel footprint.** Point sampling aliases fine pencil contacts as they move
between pixel rows. Filtering geometric exposure alone and then applying an
exponential can overfill a partially covered pixel, because optical transfer is
nonlinear. The fine-contact path averages optical alpha over the pixel footprint
and converts it back to equivalent dose. A moving ellipse uses the pixel
square's normal projection with the same four- or two-node full-pass filter as its
completions; a stationary tap uses two dimensions. Dense stamp
subdivision is rejected because its cost grows with distance and can change dose
with partitioning. Filtering is fully enabled below four pixels in minor radius,
blends out by eight pixels, and is specialized away for large contacts. These are
fixed approximation thresholds, not new brush controls.

**Research basis.** The [existing contact engine](../../internals/contact-brush-engine.md)
provides normalized swept exposure. The drawing-material papers motivate local
deposition, not this particular chord normalization. Segment additivity is a
mathematical requirement of the chosen integral, not an empirical claim. The
clipped seven-node Gauss–Legendre identity integrates the degree-twelve contact
polynomial exactly; the [NIST quadrature reference](https://dlmf.nist.gov/3.5#v)
gives its degree-thirteen exactness bound. The pixel-footprint approximation
follows area coverage and optical averaging rather than a new physical law.

### 10. Dose and opacity: `alpha = 1-exp(-D)`

**Purpose.** Preserve a first passage on blank paper, then reduce deposition on
existing marks. Each contact lays its dose in full until the grain holds its
full-pass optical dose `B`; beyond that only `phi` sticks. Grip rises with the
cube of material pressure, from 0.12 to one at material pressure one.
At Strength 100%, full pressure covers what is beneath; below 100%, full pressure
slides over part of it.

**Choice and constants.** Minimum grip `0.12` is a fixed engine choice.
At fixed `B` and material pressure, unmasked, unlocked contacts compose independently of their partition.
The exponential converts total optical dose to coverage; ordered contacts use:

```text
remaining = exp(D_before-D_after)
q = 1-remaining
```

A second identical pass adds `phi*B`; after `k` such passes the dose is
`B*(1+(k-1)*phi)`. Layering keeps building without a finite cap. No pigment-mass
image, paper deformation or new artwork storage is required.

**Strength as material pressure.** Strength multiplies calibrated pen pressure
before contact and loading: `p=Strength*clamp(pressure_gain*p_pen,0,1)`. Reducing Strength lowers approach,
recruits fewer paper locations and reduces loading through the same response as
lower pressure. Physical pressure still drives contact dimensions and tilt still
selects relief. At fixed geometry and effective pressure, Strength and pen
pressure therefore produce the same contact, loading and grip. Pressure gain is clamped
before Strength multiplication; the GPU style uses unit gain for these resolved
contacts. This avoids applying calibration twice.

The former direct pigment-feed multiplier, including `g(s)=s/(8-7s)`, is rejected
for dry media: it lightens existing contacts without changing their eligibility.
Applying both pressure scaling and that feed reduction adds a second intensity
response. A separate Strength-to-contact coupling control is unnecessary because
Strength shares the material-pressure response. Balancing contact loss against
pigment loading is selected by Material hardness within that response.
The present multiplier is linear, bounded and resolved once
per contact, with no additional per-pixel operation or integral degree.

Strength sets overall material pressure; at 100%, full pressure saturates every grain it reaches.
Material hardness sets light-touch loading; at full pressure loading is one at every hardness.
Texture strength sets how many deep valleys stay white at full pressure.

**Stroke ceiling.** Strength controls material pressure; Opacity limits the contribution of one
stroke over its starting paint. For raw coverage `A`, the stroke contribution is
`opacity*A`. Repeated passes can approach that ceiling; pen-up starts another
budget, allowing subsequent strokes to build further. Painting with partial
Opacity preserves existing coverage instead of lowering the layer's alpha.
For a fixed pigment, incremental source alpha can be expressed as:

```text
A_new = A_old + (1-A_old)*request
alpha_increment = opacity*(A_new-A_old)/(1-opacity*A_old)
```

That scalar-only implementation is rejected for pointwise painting: after
coverage saturates, it prevents later pigment colors from replacing earlier
colors. Retaining the stroke's starting paint `B` gives the general composite
`F=(1-opacity)*B+opacity*G`, where `G` is the full-opacity stroke result over `B`.
Density starts from `G.a=(F.a-(1-opacity)*B.a)/opacity`, or from stored alpha
without grouped Opacity. Zero Opacity leaves the pixel unchanged; full-grip contacts can replace opaque pigment.
Selection multiplies the resulting increment `q` as an alpha mask; fractional
selection retains its existing partition dependence.
For Normal paint, a contact request `q` updates it directly:

```text
F_new = (1-q)*F_old + q*((1-opacity)*B + opacity*(color,1))
```

Erase uses zero in place of `(color,1)`. This form avoids division by Opacity and
preserves color changes at the ceiling. Only touched GPU pages retain `B`;
full-opacity Flow strokes need no extra state. More general brush blend modes
recover `G`, apply the blend, then composite the stroke over `B` again.

Applying Opacity to every deposit's dose is rejected because repeated passes
then overcome it, making the control equivalent to Strength. Multiplying every
segment's alpha independently also loses the intended stroke ceiling. A separate
full-canvas stroke image is unnecessary: only touched pages need retained
starting paint. Photoshop's
[Opacity and Flow distinction](https://helpx.adobe.com/photoshop/using/painting-tools.html)
motivates this digital control. It is separate from the dry-material contact law.

**Alternatives.** Independent tone curves on segment exposures lose partition
invariance.
Full Kubelka-Munk scattering, material transport, crayon smearing and persistent
paper damage represent additional phenomena beyond this contact model. They are deferred,
not claimed to be reproduced by tuning `Lambda` or relief.

**Research basis.** [Sousa and Buchanan][graphite] model local graphite capacity;
the friction blend in [Rudolf, Mould and Neufeld][crayon] motivates fading grip
as material accumulates. The grip law and its constants are engine choices.
[Transmittance][transmittance] motivates exponential coverage, without adopting
the crayon model's scattering or transport.

## Intended artistic behavior and remaining choices

| Scenario | Intended behavior | Primary authoring decisions |
| --- | --- | --- |
| Heavy pencil outlines | Concentrated contact, dark buildup, with optional surviving tooth and edge gaps. | Upright/writing geometry and relief; loading and Strength. |
| Normal pencil drawing | Pressure recruits a wider set of contacts and strengthens engaged peaks. | Writing profile, pressure-size response and `Lambda`. |
| Light pencil shading | Sparse, individually visible grain instead of a uniformly pale radial fade. | Broad side profile, paper strength and modest loading pressure response. |
| Charcoal line | Strong engaged grains with a distinct contact boundary. | Narrower geometry, paper asset and relief taper. |
| Broad charcoal shading | Broad contact with interior tooth and increasing empty contacts toward its edges. | Side dimensions, rim relief and paper strength; fixed `W` keeps transfer narrow. |
| Chalk, pastel or crayon face | A block or rounded face whose coverage and grain can range from broken to more uniform. | Shape family, flatter profile and paper strength. |

These are design goals, not empirical results or claims of a complete material
simulation. Hard-pencil paper flattening, tip wear, powder transport and wax
redistribution need history/state beyond a static paper field.

The contract fixes the equations and ownership of controls. Built-in recipes
author `S`, Material hardness, pressure response and relief coefficients in the
shared [preset definitions](../../../crates/layer-core/src/contact_presets.rs).
The pose editor and curve widget remain deferred. Linear versus cubic contact
remains open for the later benchmark; no change of editor interface is required
to compare them. Bitmap relief and arbitrary contact math remain outside scope.

## External references

- [Greenwood and Williamson, *Contact of Nominally Flat Surfaces* (1966)][rough-contact],
  Proceedings of the Royal Society A, 295, 300–319; DOI
  `10.1098/rspa.1966.0242`. Basis for rough-surface separation/contact reasoning.
- [Sousa and Buchanan, *Observational Models of Graphite Pencil Materials* (2000)][graphite],
  Computer Graphics Forum, 19(1), 27–49. Drawing-specific height, pressure,
  transfer and paper-damage model.
- [Rudolf, Mould and Neufeld, *A Bidirectional Deposition Model of Wax Crayons*][crayon].
  Contact geometry, force, friction, wax redistribution and scattering model.
- [W3C, WebGPU Shading Language: `smoothstep`][smoothstep]. Normative definition
  of the cubic Hermite transfer used for contact and pose interpolation.
- [Pharr, Jakob and Humphreys, *Physically Based Rendering*, fourth edition:
  Transmittance][transmittance]. Exponential attenuation and optical thickness.

[rough-contact]: https://ks3-cn-beijing.ksyun.com/attachment/919c460af08014143cb9478ff11fbab7
[graphite]: https://visgraf.impa.br/Courses/npr07/materials/2%20lapis/Observational%20Models%20of%20Graphite%20Pencil%20Materialspdf.pdf
[crayon]: https://people.scs.carleton.ca/~mould/papers/crayon-cgf.pdf
[smoothstep]: https://www.w3.org/TR/WGSL/#smoothstep-builtin
[transmittance]: https://www.pbr-book.org/4ed/Volume_Scattering/Transmittance
