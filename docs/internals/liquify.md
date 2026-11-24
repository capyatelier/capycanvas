# Liquify sessions and deformation math

[Brushes](brushes.md)

Liquify replaces raster feedback with a deformation session. All
Liquify brushes share one coordinate map over an immutable starting image, and
every result is reconstructed from that image once. Each completed stroke writes
the deformed result into the layer, so leaving Liquify needs no bake. The session
continues across tool switches until something else changes the layer, and each
undo step retains the map with the pixels. This document describes the shared
implementation and its qualification criteria. Hardware results belong in the
[performance tier tables](../PERFORMANCE_TARGETS.md).

## Goals and scope

- **No repeated color interpolation.** Repeated dabs, pen lifts, changes of
  Liquify brush or settings, tool switches and Undo keep deforming the same
  starting pixels.
- **Tier frame rates.** Every mode targets the very complex brush rate at 512 px
  on each tier's canvas. Qualification and remaining gaps are recorded in the
  [performance tier tables](../PERFORMANCE_TARGETS.md).
- **Familiar tools.** Artists coming from Procreate find its Liquify modes and
  controls. As in GIMP and Krita, a session keeps deforming the original pixels;
  unlike them, every stroke stays individually undoable after the artist moves
  on.
- **Shared ownership.** Shared Rust owns the lifecycle, targeting, input ordering
  and history. The shared GPU renderer evaluates the map and the pixels on every
  host.
- **Nothing new to manage.** There is no Apply button, extra layer, saved filter
  or mode setting.
- **One path.** The per-dab color feedback path is removed, not kept as a
  fallback.

Persistent, re-editable Liquify filters are outside this implementation.

### Procreate coverage

Every option in Procreate's [Liquify](https://help.procreate.com/procreate/handbook/adjustments/adjustments-liquify)
has a counterpart:

| Procreate | Capy Canvas |
| --- | --- |
| Push | Push |
| Twirl Left, Twirl Right | Twirl Counterclockwise, Twirl Clockwise |
| Pinch | Pinch |
| Expand | Expand |
| Crystals | Crystals |
| Edge | Edge |
| Reconstruct | Reconstruct brush |
| Size | Size |
| Pressure: the maximum effect at full pen pressure; a finger always applies it | Strength, scaled by the pressure mapping; touch input that paints applies full Strength |
| Distortion | Integer scattering for Crystals; contact displacement for smooth modes |
| Momentum: the effect continues after lift-off | Momentum, given Procreate's meaning |
| Adjust, with its Amount slider | Adjust |
| Reset | Reset |
| Adjust and Reset appear once there are changes | Both are unavailable, with a reason, until the session has changes |
| Apple Pencil Pro barrel roll sets Twirl's amount | Twirl presets map Twist to Strength |
| Commit by leaving Adjustments | Nothing to commit; the layer is already current |

All modes share one implementation. Stationary application, absolute
barrel-roll mapping and the particular Distortion response below are Capy Canvas
choices, not claims of an exact match to undocumented Procreate behavior.

## Sessions

A session belongs to one paint source. It holds $I_0$, the source's raw pixels
when the session started, as an `Arc<PaintSource>` retaining that source's base
image, operations and raster storage. Unrelated layers can release their
historical resources independently. Sharing the immutable source directly also
avoids the artwork ownership that survives a `SceneSnapshot` scope change.
It also holds the current map and the `PaintSource` record its last stroke
produced. The session keeps $I_0$ alive itself, independently of history.
Selecting a Liquify brush creates nothing; the first effective dab starts a
session.

Each completed chunk updates the map where its dabs act, and the unfinished
chunk draws as a replaceable preview
([update points](#evaluation-and-fixed-update-points)). At Fit zoom, the preview
shows unresolved map changes while full-resolution color waits until the
preview ends. Native-resolution frames reconstruct their updated tiles
directly. Those color tiles are output only: the next dab reads $I_0$ and the
map, never the previous result. Completing the stroke makes the layer current;
leaving Liquify adds no separate bake.

**Continuation rule.** At a Liquify dab, shared Rust compares the target's
current `PaintSource` record, meaning its pixel version, base image, color mode,
domain and operations, with the records that carry a session map: the live
session's last record and every undo step that stores map tiles. The pixel
version alone is not enough: replacing a photo layer's base image keeps its
pixel version (`rasterized_source` in `crates/layer-ui/src/source_edit.rs`). A
match continues that session from its map. Any other record starts a new
session from the current pixels. One check covers every tool and command,
including ones added later. Compatibility also includes the occurrence's alpha
lock. After an alpha-lock transition, the next effective dab starts from the
current pixels, preserving opacity created by earlier unlocked deformation.

| Artist action | Behavior |
| --- | --- |
| Draw with Liquify, lift the pen, draw again | Both strokes deform the same starting pixels. Pen-up completes one undo step. |
| Change Liquify mode, size, strength or pressure response | The session continues. New dabs use the new settings; earlier dabs keep theirs. |
| Change the working selection | The session continues. Later dabs use the new selection; earlier deformation is unchanged. |
| Pan, zoom, rotate or sample a color | The session continues. Sampling sees the deformed artwork. |
| Select another tool, then return to Liquify | Nothing happens on the switch; the layer already shows the result. Liquify continues the session. |
| Paint or edit another layer | The session continues. |
| Paint, fill, filter, transform or paste on the target | The edit applies to the current pixels. The next Liquify dab starts a new session from them, because the new paint must deform too. |
| Undo or Redo to a Liquify step | The next Liquify dab continues from that step's map. |
| Draw with another tool right after a Liquify stroke | The contact queues behind the pending Liquify stroke under the existing ordered operation rules. There is no bake to wait for. |
| Switch drawings | Each drawing keeps its own sessions in its history. |
| Restart | Pixels and undo history are restored without map tiles. The next Liquify dab starts a new session. |
| Liquify without an effective change | No undo step and no new pixel version. |

A new session starts from pixels that were already reconstructed once, so each
session boundary adds one color interpolation. Boundaries come only from edits
that change the target some other way, from map tiles the history budget
dropped and from restart.

## History

Each effective Liquify stroke is one undo step. The step stores the pixel tiles
and the map tiles the stroke changed, and refers to the session's $I_0$. Versions
share unchanged tiles, and history charges each tile once.

- **Undo and Redo restore pixels at once** through ordinary raster history. The
  next Liquify dab shares that revision's immutable GPU map tiles. Missing
  tiles mean identity, so an older revision cannot inherit newer deformation.
- **Undo runs in order.** A middle stroke cannot be removed while later strokes
  are kept. Reconstruct takes back deformation in one area.
- **Maps stay on the GPU.** Pixel history uses ordinary asynchronous readback.
  Maps have no CPU copy, compression, disk format or GPU wait. Renderer
  replacement drops their auxiliary ownership before painting resumes; the
  next effective dab starts from the preserved current pixels.
- **The budget drops map tiles before steps.** History charges shared map and
  original-cache resources once. When history exceeds its byte budget, it
  releases the oldest noncurrent map first. The step still restores its pixels;
  Liquify from it starts a new session. Current revisions keep their maps.
  Publication rechecks the budget when a pending stroke supplies its map.
- **The session and its steps keep the starting image.** The live session and
  every step with map tiles keep $I_0$ alive; it is released with the last of
  them.
- **Pen corrections replay one stroke.** A late correction replays the corrected
  stroke from the previous step's map, as `CanvasEngine::replay_completed` does
  for other brushes, and amends that step's pixel and map tiles.
- **Cancelling restores pixels and map together.** Cancelling a contact returns
  both to the stroke's start, including chunks it already committed; the stroke
  keeps its starting tiles until pen-up.
- **Recovery and files hold pixels only.** Session recovery keeps pixels and undo
  history but not map tiles. Save, export and copy read the layer's tiles, and
  portable artwork never contains the map. Follow the
  [document](../internals/documents.md) and
  [recovery](../internals/session-recovery.md) contracts.

## Coordinate composition

Use paint-source coordinates, with pixel centers at half-integer positions.
Let $I_0$ be the starting image and $F_n(x)$ the backward map from output
position $x$ to its position in $I_0$ after $n$ dabs. Initially:

$$
F_0(x)=x,\qquad u_0(x)=0,\qquad F_n(x)=x+u_n(x).
$$

The visible image is reconstructed from the starting image:

$$
I_n(x)=\mathcal S[I_0]\bigl(F_n(x)\bigr).
$$

Here $\mathcal S$ includes color reconstruction and the appropriate sampling
footprint. A new dab supplies a backward map $g_n(x)=x+d_n(x)$ from its output
to the previous appearance. Its exact update is:

$$
F_{n+1}(x)=F_n\bigl(g_n(x)\bigr),
$$

or, for the stored displacement field:

$$
u_{n+1}(x)=d_n(x)+u_n\bigl(x+d_n(x)\bigr).
$$

The old field is evaluated at the displaced position. Adding $d_n(x)$ to
$u_n(x)$ is generally wrong. Tracing an output position visits the newest dab
first, then the earlier dabs; changing their order changes the deformation.
Each dab retains its resolved center, motion, pressure response, footprint,
selection influence, mode and deterministic randomness. SimpleITK's
[transform and resampling reference](https://insightsoftwareconsortium.github.io/SimpleITK-Notebooks/Python_html/21_Transforms_and_Resampling.html)
demonstrates the interpolation loss and the importance of composition order.

### Why this preserves detail

For a one-dimensional signal, bilinearly resampling a half-pixel translation
twice gives:

$$
I_1[k]=\tfrac12 I_0[k]+\tfrac12 I_0[k-1],
$$

$$
I_2[k]=\tfrac14 I_0[k]+\tfrac12 I_0[k-1]+\tfrac14 I_0[k-2].
$$

Composing those translations instead gives $F_2(k)=k-1$: a whole-pixel shift,
which reproduces $I_0[k-1]$ exactly away from source boundaries.

In general, one bilinear interpolation at subpixel offset $f$ spreads each
sample with variance $f(1-f)$ along that axis, and repeated interpolations add
their variances. At its default spacing, Push re-samples carried paint about 12
times per brush width of travel. That adds no blur if every step happens to be a
whole pixel, a standard deviation of about 1.7 px if every step is half a pixel
and about 1.4 px if the fractions are spread evenly. These are estimates, not
measured preset results. Composing interpolates once.

Every result is reconstructed from $I_0$ through the current map, and a previous
result is never a dab's color source. “One resampling” means one reconstruction
from $I_0$ for each evaluated result, not one texture fetch. Implementing the
same pixel feedback in a shader would retain the blur.

### Evaluation and fixed update points

The map is stored at pixel centers, so reconstruction reads it without
interpolation. An update for dabs $n$ to $n+K-1$ evaluates, at every pixel
center $x$ in their footprint,

$$
u_{n+K}(x)=G(x)-x+\tilde u_n\bigl(G(x)\bigr),\qquad G=g_n\circ g_{n+1}\circ\dots\circ g_{n+K-1},
$$

tracing $G$ analytically from the newest dab and reading the stored field
$\tilde u_n$ bilinearly at $G(x)$. That read is the only interpolation. It
slightly smooths the deformation but, unlike color interpolation, does not
soften pixels.

Both passes below also need the Jacobian $J_F$ for the filter footprint. The
trace carries it by the chain rule: starting from $J=I$ and $y=x$, each dab or
sub-step $j$, newest first, sets $J\leftarrow J_{g_j}(y)\,J$ and then
$y\leftarrow g_j(y)$, one 2×2 product each, with $J_{g_j}$ from
[Jacobians and folding](#jacobians-and-folding). At the end,

$$
J_F(x)=\tilde J_n(G(x))\,J_G(x).
$$

The stored footprint travels separately from displacement. Four footprint
components are packed as BF16 values alongside the two full-precision
displacement components in `rgba32uint`: 16 bytes per pixel. BF16 retains the
float32 exponent range; half-float footprints overflow under repeated Pinch.
Packing rounds to nearest even and bounds exceptional values. Bilinear map
reads interpolate displacement and the footprint correction $J-I$ together;
missing tiles contribute zero to both.

Finite differences cannot distinguish compression from consecutive discrete
jumps: integer displacements $(-4,-2,0)$ give a minmod slope of 2 and a spurious
three-pixel filter. A Crystals dab instead transports the stored footprint
unchanged. A chain of integer offsets retains identity exactly; mixed
operations carry the smooth footprint through discrete jumps. Three Pinch
dabs at strength 0.5 give $1.175^3\approx1.62$ at their center, including when
all three belong to the unfinished chunk.

The result depends on where updates fall, so:

- **Updates follow the stroke, not the frame.** The stored map is updated after
  every $K$ dabs and at pen-up. The same dabs and pen lifts give the same result
  at any frame rate, batching or prediction.
- **Writes happen only where dabs act.** A pass writes only pixels at which some
  dab of its chunk has $s>0$ at its traced position. Other pixels keep their
  values, so unselected and untouched pixels never change.
- **One preview per frame.** A frame evaluates the unfinished chunk as a
  replaceable preview: each pixel traces the chunk's dabs, reads the stored map
  once and reconstructs color once. Predicted dabs appear only in this preview
  and are replaced when real samples arrive, under the
  [input correction contract](../internals/input.md).
- **One update per completed chunk.** A frame that reaches an update point
  also runs the update above over the completed chunk's footprint, writing its
  map tiles. Consecutive chunks of the same stroke accumulate their changed
  regions. At Fit zoom the full-resolution map continues to update, while the
  reduced preview shows every unresolved region of the stroke. The full
  color result is reconstructed on pen-up, including a pen-up with no new dabs.
  At native zoom, or without a reduced preview, color is reconstructed once
  after the last chunk in the frame.
  Each chunk reads only coordinates and the original, so this produces the
  same map and pixels as reconstructing after every chunk.
- **$K=8$.** Live input, corrections, replay and prediction use the same fixed
  chunk boundary. A larger chunk reduces map interpolations while lengthening
  the pixel loop.

A pen lift is an update point, so one stroke and the same dabs split across two
strokes can differ slightly.

## Dab transformations

For a dab, let $c$ be its center, $v$ its resolved motion in source pixels,
$w(x)$ its brush coverage and $M(x)$ its selection influence. Define
$s(x)=\operatorname{clamp}(k\,w(x)\,M(x),0,1)$, where $k$ includes strength
and the resolved input dynamics. A zero influence gives the identity map.
Selection attenuates deformation rather than crossfading the original and
warped colors, which would create double edges.

These mappings keep the current modes' response scales and replace their
accumulation rule:

| Mode | Backward map $g(x)$ | Visible effect |
| --- | --- |
| Push | $x-s(x)v$ | Move content with the stroke. Momentum generates dabs after lift-off. |
| Twirl Counterclockwise | $c+R(0.75s(x))(x-c)$ | Turn content counterclockwise. |
| Twirl Clockwise | $c+R(-0.75s(x))(x-c)$ | Turn content clockwise. |
| Pinch | $c+(1+0.35s(x))(x-c)$ | Pull content inward by sampling farther from the center. |
| Expand | $c+(1-0.35s(x))(x-c)$ | Push content outward by sampling nearer the center. |
| Crystals | $x+d(x)$ | Scatter content by whole pixels, one random direction per 4 px cell; see below. |
| Edge | $c+\bigl(I+0.35s(x)\,nn^{T}\bigr)(x-c)$ | Pull content toward the stroke line from both sides; $n$ is the unit normal of $v$. |

$R(\theta)$ is the usual matrix
$\left[\begin{smallmatrix}\cos\theta&-\sin\theta\\\sin\theta&\cos\theta\end{smallmatrix}\right]$,
with radians and a downward-positive canvas $y$ axis. A backward sampling
rotation turns visible content in the opposite direction, explaining the Twirl
signs. Rust mode names describe the visible turn; existing stored IDs stay
unchanged. Edge takes $n$ from the dab's motion. A dab without
motion keeps the previous dab's normal, and a stroke's first dab, which has
none, leaves content unchanged.

The coefficients describe brush response, not research-derived optimal values.

### Crystals

Each 4 px cell has one random direction, and every pixel moves by a whole
number of pixels:

$$
d(x)=\bigl\lfloor 24D\,s(x)\,(N_{\text{cell}}-\tfrac12)+\tfrac12\bigr\rfloor,
$$

per component, with $D$ the distortion and $s(x)$ the per-pixel influence every
mode uses. The two components of $N_{\text{cell}}\in[0,1)^2$ come independently
from an integer hash of the cell $\lfloor x/4\rfloor$, the stroke seed and the
dab index. CPU and GPU both round with $\lfloor\cdot+\tfrac12\rfloor$ in 32-bit
floats, because WGSL's `round` rounds halves to even and Rust's rounds them away
from zero. Coverage involves a square root and a division that GPUs need not
round exactly, so an offset within float error of a half pixel can still round
differently; such pixels differ by one pixel. Crystals' Jacobian is $I$ between
its jumps, so its trace leaves $J$ unchanged.

- **Selection and coverage apply per pixel.** An unselected pixel never moves,
  even in a partly selected cell, and nothing moves outside the tip. Where $s$ is
  constant, as in the tip's hard core, a cell moves rigidly; in the soft edge,
  offsets inside a cell step by whole pixels.
- **Whole pixels keep updates exact.** Within a chunk of Crystals dabs, offsets
  land map reads on grid points, so updates never blend neighboring cells;
  interpolating a map that jumps at every cell edge would smear them. Over
  undeformed content, reconstruction copies source pixels exactly. Where earlier
  dabs left a fractional displacement, it stays fractional, and reconstruction
  interpolates once, as everywhere else.
- **Integer randomness.** Wrapping 32-bit multiplication, XOR and shifts replace
  the sine-based hash. Independent component salts replace correlated random
  components; neither trigonometric precision nor dab-center repetition sets
  the pattern.
- **Smooth dabs over Crystals still interpolate across cell edges.** Measure how
  visible that is.

### Jacobians and folding

Each dab's Jacobian serves twice: for folding and for the filter footprint. With
$a=0.35$, $\theta=\pm0.75s$, $q=n\cdot(x-c)$ and
$J_{90}=\left[\begin{smallmatrix}0&-1\\1&0\end{smallmatrix}\right]$:

| Mode | $J_g$ | $\det J_g$ |
| --- | --- |
| Push | $I-v\,\nabla s^{T}$ | $1-\nabla s\cdot v$ |
| Twirl | $R(\theta)\bigl(I+J_{90}(x-c)\,\nabla\theta^{T}\bigr)$ | $1+\nabla\theta\cdot J_{90}(x-c)$ |
| Pinch | $(1+as)I+a(x-c)\,\nabla s^{T}$ | $(1+as)\bigl(1+as+a\,\nabla s\cdot(x-c)\bigr)$ |
| Expand | $(1-as)I-a(x-c)\,\nabla s^{T}$ | $(1-as)\bigl(1-as-a\,\nabla s\cdot(x-c)\bigr)$ |
| Edge | $I+as\,nn^{T}+aq\,n\,\nabla s^{T}$ | $1+as+aq\,(n\cdot\nabla s)$ |
| Crystals | $I$ between jumps | Not applicable |

A map folds where $\det J\le 0$, and content is then repeated or mirrored.
Composing maps that do not fold gives a deformation that does not fold, so where
the limits below apply, limiting each dab keeps the exact deformation
fold-free. Crystals jumps by design, and Reconstruct and Adjust can collapse
content ([map edits](#map-edits)).

Liquify tips have a radius of at least 1 px and a hardness of at most
$1-e_{\min}$. The round analytic tip then falls off linearly over a relative
edge width $e=\max(1-h,1/r)\le1$ and is continuous. Below 1 px,
`analytic_coverage` jumps at the center, to 1 from $1/e$ beside it, and two
points can map to one even for a tiny Push. Without a selection,
$s(x)=\sigma(\rho)$ with $\rho=|x-c|$, $\nabla s=\sigma'(\rho)(x-c)/\rho$ and
$-k/(e\,r)\le\sigma'\le0$, so:

| Mode | Folds exactly when |
| --- | --- |
| Push | $k\lvert v\rvert\ge e\,r$ |
| Pinch, Edge | $ak\ge e$ |
| Expand, Twirl | Never |

At one spacing step, $\lvert v\rvert=2r\cdot\text{spacing}$, so Push folds once
spacing reaches $e/(2k)$. With a selection, or a textured or non-round tip, $s$ is not
radial and only the general determinant applies; Twirl can then fold too.

- **Sub-steps.** Split a dab that would fold into $n$ sub-steps, each applying
  $1/n$ of its motion (Push) or of $a$ (Pinch, Edge), so every sub-step keeps
  $\det J\ge\delta$: $n=\max(1,\lceil k\lvert v\rvert/(e\,r\,(1-\delta))\rceil)$ for
  Push and $n=\lceil ak/(e(1-\delta))\rceil$ for Pinch and Edge. The sub-steps
  approximate the unsplit dab. Counts are rounded up and bounded below by one.
- **The minimum edge bounds the work at ordinary spacing.** With
  $A=2k/(e(1-\delta))$ and spacing $p$, Push needs about
  $\max(1,\lceil Ap\rceil)/p$ sub-steps per brush width. At fine spacing this
  grows as $1/p$. The implementation uses $e_{\min}=0.25$ and
  $\delta=\tfrac12$. Pinch and Edge need at most 3 sub-steps per dab.
- **The tip sets the count.** $n$ is computed once per dab from the tip, so
  frames and batches cannot change it. A selection edge is much steeper, about
  $k$ per pixel when antialiased, and would need tens of sub-steps per dab
  (depending on size and spacing), so it does
  not count. A Push dab that moves content away from a hard selection edge
  therefore copies a strip of the unselected content beside the edge, up to
  $k\lvert v\rvert$ wide, into the selection.
- **The stored map can still fold.** It samples the exact deformation at pixel
  centers, and updates read it bilinearly. Where the deformation turns or
  squeezes faster than one pixel can represent, the bilinear interpolant can
  fold between pixel centers although no dab and no exact composition does. For
  example, three Twirl taps of radius 2 px, hardness 0.5 and strength 0.72, each
  followed by an update, give a negative determinant inside one cell.
  Reconstruction reads only pixel centers, so such a fold acts through later
  updates that read inside the cell and through the filter's footprint. The
  interpolant's determinant is affine within a cell, so a cell is fold-free
  exactly when its four corner determinants, cross products of the cell's edge
  differences, are positive. A single central difference can miss a fold
  ([Liu et al.](https://arxiv.org/abs/2212.06060) analyze the same problem for
  pixel grids). Use the corner test in the acceptance checks, and measure how
  often and how visibly stored maps fold at ordinary brush sizes.

### Map edits

These operations edit the displacement $u$ directly instead of composing another
backward map. Their scope is the session: everything since the target was last
changed some other way.

| Operation | Map edit | Behavior |
| --- | --- | --- |
| Reset | $u\leftarrow 0$ | The layer returns to $I_0$ exactly, in one undo step. |
| Reconstruct | $u(x)\leftarrow(1-s(x))\,u(x)$ per dab | Paint restores the original under the brush. |
| Adjust | $u\leftarrow t\,u_A$ for $t\in[0,1]$, with $u_A$ the map when Adjust opened | Reduces everything applied in the session. While the slider moves, preview at screen resolution, as full-screen filters may; render full resolution on release. One undo step. |

Numeric controls forward their native contact phases when the shared setting
requests a preview. Amount movement changes neither authored pixels nor history;
release commits the last value, and cancel restores the value at contact start.
Discrete numeric edits commit directly. Closing Adjust or beginning another edit
finishes an active contact. Interrupted or stale contact events cannot alter a
different target or raster revision.

A Reconstruct dab fits the newest-first trace: it blends the remaining trace's
result with the position the trace reached it at, $F'(y)=y+(1-s(y))(F(y)-y)$, so
a pixel still reads the stored map once. Its Jacobian is
$J_{F'}=s\,I+(1-s)\,J_F-(F(y)-y)\,\nabla s^{T}$, and Adjust's is
$(1-t)I+t\,J_A$, with $J_A$ the Jacobian of $x+u_A$. Scaling a displacement can
collapse content: halfway between the identity and a 180-degree rotation maps
every point to the center, so Reconstruct and Adjust are not fold-free. GIMP's Erase scales
its field the same way; accept it rather than restrict these tools.

### Input behavior

- **Momentum.** After pen-up, dabs continue along the final direction with
  quadratic strength decay. Momentum $m$ generates $\lceil36m\rceil$ contacts
  at 60 Hz. Travel is bounded by $m$ times the brush diameter and by the final
  measured speed. The continuation is fixed at pen-up and belongs to the
  stroke. New input or an edit first applies its remainder, so the result
  never depends on timing. Reconstruct has no momentum.
- **Distortion.** Crystals uses integer scattering. Other modes except
  Reconstruct use existing deterministic contact jitter, up to 10% of the
  diameter along each stroke axis. This makes an irregular stroke without
  per-pixel noise evaluation or changing the smooth dab's folding bound.
- **Barrel roll.** Twirl presets multiply Strength by the absolute normalized
  `Twist` sensor. Devices without barrel-twist capability retain plain Strength.
  Host input remains responsible for hardware sensor values.
- **Holding the pen still.** Twirl, Pinch, Expand, Edge and Reconstruct emit
  contacts at 20 Hz from input timestamps. Push needs motion, Crystals needs
  distance, and Edge needs a previously established stroke normal.

## Precision and sampling

- **32-bit displacement.** Store $u$ as two 32-bit floats per pixel; the identity
  is exactly zero, and steps stay below 0.001 px for displacements under
  16384 px. Half precision steps by 0.5 px past 512 px and by 2 px past 2048 px.
  Filtering 32-bit float textures is optional in WebGPU, so map reads
  interpolate manually, like `canvas_sample`.
- **Sparse tiles.** Map tiles are 256 px, aligned with paint tiles. Only touched
  tiles exist. Global Amount zero releases the current map; local Reconstruct
  retains tiles so it does not require a GPU reduction or CPU readback.
- **Linear premultiplied color.** Reconstruct premultiplied color and alpha in
  the linear working space, as Transform does; resampling stays linear in both
  blend spaces ([documents](../internals/documents.md)). Identity leaves pixels
  unchanged, constant color regions remain constant and transparent edges
  acquire no colored fringe. In a Perceptual document a squeezed dark line
  therefore comes out lighter than resampling encoded values would make it,
  exactly as with Transform; change both together if that changes.
- **Alpha lock.** An alpha-locked layer keeps $I_0$'s alpha at $x$ and takes the
  straight color of the sample at $F(x)$. Where that sample is fully
  transparent, the pixel keeps $I_0(x)$.
  Changing alpha lock ends compatibility with that session on the next
  effective dab; the new $I_0$ contains the current opacity.
- **True source extent.** Respect the source's sparse extent and off-frame
  authored content. Cache edges, missing residency and tile borders are not
  image boundaries. Map interpolation and image reconstruction need their own
  halos and source dependencies.
- **Filtering where content is squeezed.** Push piles content at its leading
  edge and Twirl compresses as it winds. Reconstruction averages bilinear taps
  over the parallelogram given by the stored footprint's columns. Its total
  budget is 4 taps for prediction and 16 for persistent tiles, rather than 16
  along each axis. Selecting an earlier mip for larger footprints trades a
  little sharpness for bounded work. Whole-pixel identity footprints take a
  single exact texel load.
- **Bound the local approximation.** Each map tile retains an unclipped,
  conservative source rectangle for its full output pixel area. Reconstruction
  uniformly shortens the footprint's two columns when their parallelogram
  would leave that rectangle. The stored Jacobian remains unchanged. A local
  derivative can greatly overestimate the finite area of a strongly nonlinear
  warp: repeated Pinch within a 64 px radius can have a center derivative above
  1,000 while its mapped pixel still stays inside that radius.
  The common scale incorporates preview pixel size after clipping; even a
  saturated finite footprint cannot overflow before it reaches that bound.
  Source capture includes the rectangle and the bilinear halo of its largest
  possible mip level, up to 256 px. Only cache enumeration clips to the source
  extent; the filtering rectangle retains negative and off-frame coordinates.
  This preserves real transparent boundaries without treating uncached tiles as
  transparent image content.
- **Display-sized prediction.** Smooth modes reuse the renderer's existing
  reduced preview pages at Fit zoom, up to one sample per 4 × 4 document pixels.
  Reconstruction scales the transported footprint by that pixel area. Unchanged
  regions average the current pixels; cancellation restores them exactly.
  Crystals and native-resolution views keep full-resolution prediction.
  Persistent map updates and committed pixels always use the document grid.
- **Reduced levels beyond the cap.** Sampling 32:1 stripes with 16 level-zero
  taps can return only one stripe color. First-touch source tiles therefore
  build nine box-filtered levels in linear premultiplied color, through the
  existing display-mip kernels. Choose a level at which the footprint's
  per-axis counts and their product fit the tap budget. At most eight reductions
  fit a 256 px tile. Adjacent tiles are looked up separately at every tap;
  their edges do not become image boundaries. Sharpness steps when the selected
  mip changes. Where deformation varies faster than the map grid resolves,
  its transported footprint remains an approximation.
  [Heckbert's image-warping research](https://www2.eecs.berkeley.edu/Pubs/TechRpts/1989/5504.html)
  describes mapping-dependent filtering. Compare with a single-resampling
  reference, not an unfiltered, aliased image.
- **Stretching stays soft.** Stretching cannot recover detail absent from
  $I_0$. Compare Transform's clamped Catmull-Rom filter for magnified areas, and
  never compensate with sharpening.
- **Error budget.** Storage precision and filtering follow a measured error
  budget against direct reverse composition of the dab maps.

## Performance and memory

- **Map updates and reconstruction.** The map pass evaluates the newest-first trace
  and reads the stored map bilinearly. Reconstruction reads the resulting map
  at pixel centers. A reduced moving preview covers all unresolved stroke tiles;
  full-resolution color waits until that preview ends. A native-resolution frame
  reconstructs after its consecutive chunks finish.
  Preview tiles outside the unfinished chunk reuse their pixels while their
  committed map and output texture are unchanged. Reused pages remain visible
  without invalidating source mips or composition. A retired preview page,
  changed map or replacement texture still invalidates its region.
  An empty unfinished chunk
  retains the preview until pen-up. Untouched pixels compare against the map
  last reconstructed into the layer, so earlier compression cannot change
  merely because a later dab shares its tile.
  All output tiles share each pass. Pixels outside the changed region copy
  their old map directly and keep their current color.
  First-touch source mip reductions share one additional pass for the batch.
  Count sub-steps, source chunks and reconstruction taps as well as dabs.
- **Compile the selected mode.** Each mode has its own map recipe. Analytic
  brushes without grain or selection omit texture coverage and selection
  sampling; other brushes retain those inputs. Prepare the selected recipe
  before pen-down through the existing shader queue.
- **Portable bindings.** Push, Twirl and Edge gather twelve integer map textures
  per chunk, with two tile coordinates per uniform vector. The other modes
  gather nine with one coordinate per vector. The same shader and encoder
  specialize this capacity by mode; measured dispatch savings favor twelve for
  the more expensive traces, while the smaller layout reduces lookup cost for
  the other modes. The prior map and three brush textures keep both layouts
  within the portable limit of sixteen sampled textures. Original pixels
  occupy 16-layer texture arrays, with a sparse tile-index buffer.
  Reconstruction binds at most eleven arrays per source chunk. Large sources use
  additional chunks, each in its own pass; every tile accumulates their disjoint
  contributions in its own scratch texture, so no dispatch in a pass waits for
  another. The scratch pool holds two pages' bytes at the preview's block size,
  and tiles beyond it take further passes. A
  tile's chunk is its block of 16 × 11 pages, so the order of those sums never
  depends on which tiles a session copied first or rebuilt later. No
  Float32 filtering, Float32 blending, bindless texture arrays or float atomics
  are required. The same manual sampling path runs on Mali and other GPUs.
- **Lazy originals.** A tile is captured before its first deformation or source
  read. Each 16-layer allocation holds about 21.3 MiB including all mip levels;
  the unused last block is bounded by 15 tiles. Source bounds propagate
  conservatively through chunks. No full-canvas snapshot is allocated at the
  first dab. The stroke's lane admits each allocation with the frame's other
  outputs. An allocation that holds no tile the current frame reads is released
  when another edit needs its memory. Before a later frame reads the tile, its
  restore decodes it again from $I_0$ straight into an allocation and rebuilds
  its levels, holding no page copy. A frame still reads every original under the
  connected field of its contact. `liquify::memory_tests` checks the release,
  the rebuilt pixels and the pass counts of a stroke that fits.
- **Peak memory.** Map storage costs 16 bytes per touched pixel, plus unchanged
  tiles shared across versions. An update temporarily owns both old and new
  tiles. Original caches cost 16 bytes per pixel plus one third for mip levels,
  in addition to the immutable authored source and ordinary pixel history.
  Shared resource IDs deduplicate this ownership in history and open-drawing
  accounting. Measure total residency, pending pixel readbacks and preview
  storage on the reference tablets, including the 6 GB TCL.
- **Transitions.** Tool switches do no pixel work. Undo selects existing map
  ownership; there is no map upload or readback. A map evicted by the history
  budget, a restart or renderer replacement creates a session boundary.
- **Thread rules.** Never wait for the GPU, copy canvas pixels or block on I/O
  on the UI or input thread. Avoid replaying past dabs per frame and avoid
  full-canvas work for a local stroke. Painting and Smudge keep their own color
  accumulation. Measure every affected row under the
  [measuring rules](../performance/measuring.md).

## Acceptance criteria

The replacement is ready when shared tests without a window, independent GPU
references and real host journeys establish all of the following:

- Identity, zero influence and whole-pixel translations preserve source pixels;
  composed fractional translations avoid the repeated-resampling blur.
- The same dabs and pen lifts give identical results at any frame rate, batching
  or prediction. Mixed-mode order matches direct map composition within the
  interpolation of its update points.
- Crystals moves pixels by whole pixels and moves no unselected pixel. It
  agrees with a 32-bit CPU reference except where an offset lies within float
  error of a half pixel, where it differs by one pixel.
- Consecutive Crystals steps, including the $(-4,-2,0)$ displacement pattern,
  preserve exact source pixels instead of deriving a continuous filter
  footprint from integer jumps.
- No dab of a round tip folds after sub-stepping, at any allowed spacing,
  hardness or pressure, away from selection edges. The corner test reports how
  often stored maps fold for each preset across brush sizes, and the visible
  effect is compared with the exact composition.
- Filtering copies whole-pixel Crystals offsets exactly, leaves unselected
  pixels unchanged and averages one-pixel stripes to gray at 32:1 and 64:1.
- Fine lines, checkerboards, transparent edges, strong compression, large
  coordinates, sparse sources and tile crossings meet the sampling and geometry
  error budgets.
- Repeated local Pinch with a stored center derivative above 1,024 preserves
  opaque constant color, including across published strokes. Its bounded
  footprint and mip halo never read an uncached valid source tile.
  Sustained Pinch that saturates the finite footprint also preserves opacity
  when a shifted Fit preview samples and expands that footprint.
- Undoing any number of Liquify strokes and drawing again equals a session in
  which those strokes never happened. This holds across tool switches and edits
  to other layers; after Liquify, painting on the target, undoing the paint and
  liquifying again; while the last stroke is still pending on the GPU; and after
  a late pen correction.
- A step whose map tiles were dropped restores exact pixels and starts a new
  session, and so does restart. A live session keeps working after history
  drops every step's map tiles.
- Cancelling a stroke restores its pixels and map, including chunks it already
  committed.
- Continuation stops when anything in the target's `PaintSource` record
  changes, including a replaced base image with the same pixel version.
- Switching alpha lock after moving paint into transparency preserves that
  paint's current opacity on the next effective dab.
- Releasing history frees unrelated layers' historical resources while a
  Liquify session on another source remains live.
- Every journey in the session table works on each affected host, with native
  input preserved and navigation responsive.
- Measurements on each tier's reference hardware establish the stroke rate of
  every mode at 512 px, the pixel-readback cost per stroke, the peak memory of a
  whole-canvas session and the delay from Undo to the first continued Liquify
  dab, both with a retained map and after map eviction.

No GPU layout, filter kernel or latency claim is accepted solely from this
design; those choices must satisfy these criteria.

## Relevant precedents

- **GIMP.** [GEGL's warp operation](https://github.com/GNOME/gegl/blob/master/operations/common-cxx/warp.cc)
  updates its field as $D_{\text{new}}(p)=v(p)+D_{\text{old}}(p+v(p))$, reading
  $D_{\text{old}}$ bilinearly: true composition. Its `map-relative` operation
  gives the sampler the field's finite-difference Jacobian. GIMP's
  [Warp tool](https://github.com/GNOME/gimp/blob/master/app/tools/gimpwarptool.c)
  commits on tool switch, and the session becomes one undo step.
- **Krita.** Its [Liquify worker](https://invent.kde.org/graphics/krita/-/blob/master/libs/image/kis_liquify_transform_worker.cpp)
  moves a grid of nodes spaced 8 px apart and re-renders from the original
  pixels. Strokes undo individually during the transform and as one step after
  it is applied. Returning to the tool resumes the transform when it is the last
  undo step ([continuous transform](https://docs.krita.org/en/reference_manual/tools/transform.html)).
- **Procreate.** [Liquify](https://help.procreate.com/procreate/handbook/adjustments/adjustments-liquify)
  offers Push, Twirl Left and Right, Pinch, Expand, Crystals, Edge and
  Reconstruct, with Size, Pressure, Distortion, Momentum, Adjust and Reset, and
  barrel roll for Twirl on Apple Pencil Pro. It commits when the artist leaves
  Adjustments.
- **Photoshop.** [Liquify](https://helpx.adobe.com/photoshop/using/liquify-filter.html)
  keeps applying Twirl, Pucker and Bloat while the tool is held still, at the
  speed set by Rate, and offers Reconstruct and Restore All.
