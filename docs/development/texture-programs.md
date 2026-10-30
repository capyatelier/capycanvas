# Texture programs

[Developer guide](README.md) · [Paper](paper.md) · [Runtime filters](../reference/runtime-filters.md)

A texture program describes a paper or canvas surface as one WGSL function:
height in micrometers at a position in millimeters on a repeating tile. The app
bakes the function at the drawing's pixel density into a single-channel texture
that brushes read one texel per pixel. Built-in programs live in their own
catalog, [`textures.json`](../../assets/filters/textures.json), written in the
filter package format with `kind: "texture"`, so they reuse its parameter
declarations, WGSL modules, naga validation and saved-value contract without
appearing among filters.

This guide defines the program contract, the `tx_` library, the bake and the
programs behind the built-in papers. [Paper](paper.md) covers the layer,
brushes, resolution model and library that use the result.

## Principles

- **One function.** A program maps a position to a height. Resolution, tiling,
  antialiasing, normalization and upload belong to the bake, so a program never
  sees pixels.
- **Physical units.** Positions and lengths are millimeters and the result is
  micrometers of tooth: the depth a drawing tip feels, not the sheet's full
  profile. The bake keeps the absolute relief, which is what makes a rough
  sheet rougher than a hot-pressed one under the same brush.
- **Periodic by construction.** Every library operator rounds its feature size
  to a whole number of repeats across the tile, so any composition of operators
  tiles without a seam.
- **Describe what the tip touches.** A drawing tip follows slow undulation, so
  features longer than about 5 mm get no more than a few micrometers; formation
  clouds and cockling are not tooth. A large-scale term makes whole patches of
  the sheet unreachable and shows as blotches in every stroke.
- **Give each surface its own fine tooth.** A contact breaks up only where
  heights vary across the tip. Relief made of plateaus, sparse fibers or
  features near the tip's size engages the whole contact at once and fills it
  solid. Every program models what its sheet is made of below about 0.3 mm:
  cotton fibers and felt imprints, calendered pores, laid grain, grit, kozo
  fibers or pulp clumps. Its depth sets how soon the paper fills under
  pressure; smooth papers have less of it, not none. At 300 ppi a texel is
  85 µm, so fibers are drawn as bundles about a texel wide.
- **Only what the surfaces need.** There are no stages, buffers, image inputs or
  user programs. Each was checked against the surfaces below; see
  [decisions](#decisions).

## Program contract

### Catalog entry

```json
{
  "category": "paper",
  "icon": "paper",
  "program": {
    "abi": 5,
    "id": "watercolor",
    "kind": "texture",
    "label": "Watercolor",
    "period": 128,
    "entry": "capy_watercolor",
    "wgsl": ["texture_library.wgsl", "texture_watercolor.wgsl"],
    "parameters": [
      {"key": "felt", "label": "Felt", "default": {"kind": "number", "value": 1.0},
       "kind": {"kind": "number", "min": 0.4, "max": 5, "step": 0.1, "decimals": 1, "unit": "mm"}},
      {"key": "pressing", "label": "Pressing", "default": {"kind": "number", "value": 45},
       "kind": {"kind": "number", "min": 0, "max": 100, "step": 1, "decimals": 0, "unit": "%"}},
      {"key": "seed", "label": "Variation", "dimension": "count", "default": {"kind": "number", "value": 1},
       "kind": {"kind": "number", "min": 0, "max": 16777215, "step": 1, "decimals": 0, "unit": ""}}
    ]
  },
  "presets": [
    {"id": "hot_press", "label": {"message": "resources-paper-hot-press"},
     "values": {"felt": {"kind": "number", "value": 2.0}, "pressing": {"kind": "number", "value": 85}}},
    {"id": "cold_press", "label": {"message": "resources-paper-cold-press"}, "values": {}},
    {"id": "rough", "label": {"message": "resources-paper-rough"},
     "values": {"felt": {"kind": "number", "value": 3.0}, "pressing": {"kind": "number", "value": 5}}}
  ]
}
```

| Field | Rule |
| --- | --- |
| `kind` | `"texture"`. Only the texture catalog may hold texture programs; nothing lists it among filters. |
| `period` | Tile edge in millimeters, 8 to 400. Choose at least five to ten repeats of the largest feature. |
| `entry`, `wgsl` | As for filters. Programs list `texture_library.wgsl` before their own module. |
| `parameters` | Number, Toggle and Choice, with `scalar` or `count` dimension. Units are display labels such as `mm`, `%` or `/cm`. A seed is a count from 0 to 16777215 labelled Variation. Program and parameter labels are literal: nothing shows them yet. |
| `presets` | Entry-level `{id, label, values}` items with typed values and translated labels. Omitted keys take their defaults. Each preset is one built-in paper; drawings store program values, not preset IDs. |
| Rejected | `passes`, `lookups`, `auxiliary`, `time`, `resolution`, `constant_color`, pixel dimensions and image parameters. |

### Entry point

```wgsl
fn capy_watercolor(p: vec2f, b: u32) -> f32
```

- `p` is a position in millimeters inside `[0, period)²`.
- `b` addresses the parameter block. `fx_parameter(b, i).x` reads parameter `i`
  in declaration order, exactly as filters do.
- The result is height in micrometers. Larger values are peaks; the offset is
  arbitrary.
- A program is pure and deterministic. Every loop bound is a constant or
  `min(n, constant)`.
- A program may use WGSL built-ins, `fx_parameter` and the `tx_` library. It
  declares no bindings and calls no filter helper that reads an image or time.

## The `tx_` library

`assets/filters/texture_library.wgsl` holds the whole library. Sizes are
`vec2f` in millimeters so any operator can be anisotropic; write `vec2f(s)` for
an isotropic size. Seeds and salts are `u32`; operators called with different
seeds are independent.

| Function | Result |
| --- | --- |
| `tx_period() -> f32` | Tile edge in millimeters, supplied by the bake. |
| `tx_texel() -> f32` | Texel edge in millimeters for the current bake. Only thin-feature operators use it. |
| `tx_grid(size: vec2f) -> TxGrid` | A lattice with `count = max(1, round(period / size))` cells per axis and `cell = period / count`. |
| `tx_cell(g: TxGrid, p: vec2f) -> vec2i` | The unwrapped cell containing `p`. Neighbors are `c + vec2i(x, y)`. |
| `tx_random(g: TxGrid, c: vec2i, salt: u32) -> vec4f` | Four uniform numbers in `[0, 1)` for a cell, wrapped into the tile. |
| `tx_noise(p: vec2f, size: vec2f, seed: u32) -> f32` | Periodic gradient noise, roughly `[-1, 1]`, with features of `size`. |
| `tx_fbm(p: vec2f, size: vec2f, octaves: u32, seed: u32) -> f32` | Up to eight octaves of `tx_noise`, each half the previous size and amplitude, normalized to the same range. |
| `tx_warp(p: vec2f, amount: f32, size: vec2f, seed: u32) -> vec2f` | `p` plus a periodic offset of up to about `amount` millimeters. |
| `tx_cells(p: vec2f, size: vec2f, jitter: f32, seed: u32) -> vec4f` | Cellular noise: distance to the nearest and second-nearest site in cell units, and two random numbers of the nearest site. `jitter` from 0 (grid) to 1 moves the sites. |
| `tx_lines(p: vec2f, spacing: f32, spread: f32, width: f32, wobble: f32, seed: u32) -> vec2f` | Lines of constant `x`: a ridge profile from 0 to 1 and the distance in millimeters to the nearest line. `spread` places each line within a band of that fraction of the spacing; `wobble` bends lines by up to that many millimeters. Pass `p.yx` for horizontal lines. |
| `tx_weave(p: vec2f, pitch: vec2f, irregularity: f32, seed: u32) -> f32` | Plain weave from 0 to 1 with warp threads `pitch.x` apart and weft threads `pitch.y` apart. Over and under alternate at each crossing. `irregularity` from 0 to 1 varies thread position, thickness and slubs. |
| `tx_fibers(p: vec2f, len: f32, width: f32, density: f32, angle: f32, alignment: f32, curl: f32, seed: u32) -> f32` | Scattered fibers from 0 to 1. Lengths vary from 0.5 to 1.5 times `len`; `density` is fibers per square millimeter, at most 64 per cell; `alignment` runs from 0 (any direction) to 1 (all along `angle` in radians); `curl` bows each fiber by up to that fraction of its length. Fibers rise and dip along their length. |
| `tx_ellipse(q: vec2f, radii: vec2f, angle: f32) -> f32` | Approximate signed distance in millimeters from the origin-centered ellipse. |
| `tx_thread(across: f32, radius: f32) -> f32` | Round cross-section: 1 on the center line, 0 at `radius`. |
| `tx_smin(a: f32, b: f32, k: f32) -> f32`, `tx_smax(a: f32, b: f32, k: f32) -> f32` | Smooth minimum and maximum over a blend width `k`. `tx_smin(h, level, k)` is a pressing plateau. |
| `tx_disc(i: u32, n: u32) -> vec3f` | Offset of tap `i` of `n` in the unit disc and its weight `1/n`, on a Vogel spiral. |

The lattice functions are the core of the periodicity guarantee:

```wgsl
struct TxGrid {
    count: vec2i,
    cell: vec2f,
}

fn tx_hash(c: vec2i, salt: u32) -> vec4f {
    var v = vec4u(bitcast<vec2u>(c), salt, 0x9e3779b9u) * 1664525u + 1013904223u;
    v.x += v.y * v.w; v.y += v.z * v.x; v.z += v.x * v.y; v.w += v.y * v.z;
    v ^= v >> vec4u(16u);
    v.x += v.y * v.w; v.y += v.z * v.x; v.z += v.x * v.y; v.w += v.y * v.z;
    return vec4f(v >> vec4u(8u)) / 16777216.0;
}

fn tx_grid(size: vec2f) -> TxGrid {
    let count = max(vec2i(round(tx_period() / max(size, vec2f(1e-4)))), vec2i(1));
    return TxGrid(count, tx_period() / vec2f(count));
}

fn tx_cell(g: TxGrid, p: vec2f) -> vec2i {
    return vec2i(floor(p / g.cell));
}

fn tx_random(g: TxGrid, c: vec2i, salt: u32) -> vec4f {
    return tx_hash(((c % g.count) + g.count) % g.count, salt);
}
```

`tx_hash` is a PCG-style integer hash over a 4D state. Randomness comes only
from integer hashes of wrapped cell coordinates, so every GPU produces the same
layout.

### Rules that keep tiles seamless

- **Position reaches the height only through `tx_` operators**, lattice cells
  and the positions derived from them. Raw trigonometry of `p` does not tile.
- **Put alternating phases in one cell.** A whole-repeat count can be odd, so
  parity between neighboring cells does not survive the wrap. The honeycomb
  program places two staggered sites in each cell; `tx_weave` pairs threads
  internally for over and under.
- **Warp with `tx_warp`.** Its offset is periodic, so warped coordinates stay
  periodic.
- **Expect sizes to snap.** Rounding to whole repeats changes a feature by at
  most half a repeat across the tile, which five or more repeats make
  negligible.

### Neighborhood effects without stages

The surfaces need blur, dilation and slope only of shapes the program
already describes, so the program evaluates them directly:

- **Blur:** average a helper over `tx_disc` taps. The canvas program levels its
  weave this way to model gesso filling the hollows.
- **Dilation and covering:** offset a signed distance before shaping it. The
  handmade program drapes pulp over its inclusions this way.
- **Slope:** take central differences of a helper.

Each tap multiplies the helper's cost, so taps are used on the cheap parts of a
program.

## The bake

1. **Tile size.** The Paper's mapping gives drawing pixels per paper millimeter on
   each axis ([resolution model](paper.md#resolution-and-scale)). For each
   axis, `texels = round(period × pixels per millimeter)`; an imported image
   uses its physical size as the period. The period is stretched to that whole
   number of pixels, by under half a pixel per tile. Texel `i` covers document
   pixels `i + k × texels` and is evaluated at the paper position of its
   document pixel, so the mapping's offset is baked in and a brush reads the
   texel at the document pixel modulo the tile. Above 4096 texels on an axis,
   the bake uses 4096 and brushes magnify bilinearly.
2. **Probe.** The source is evaluated at 65,536 positions chosen by a fixed
   hash. The values are read back asynchronously and their 0.5th and 99.5th
   percentiles become `low` and `high`; `relief = high − low` micrometers.
3. **Bake.** Fragment draws fill an R8Unorm texture in 256 × 256 pages, one page
   group per submission, so no submission runs long. Each texel evaluates the
   source once, at its center plus a Gaussian offset drawn from a hash of the
   texel, and stores `(h − low) / (high − low)` clamped to `[0, 1]`. The offset
   is σ = 0.5 texel for programs; for images it depends on the shrink factor
   ([imported images](#imported-images)). Positions are wrapped into
   `[0, period)` first; evaluating far outside the tile loses f32 precision.
4. **Publish.** The texture and its relief replace the previous paper in the
   brush binding. Brushes scale their paper strength by `relief / 100 µm`.

A bake is a cache, not authored data. Transcendental functions may round
differently on different GPUs; the integer hashes keep the layout identical.

### One jittered sample per texel

Each texel is a true sample of the surface, so the height histogram, which is
what a dry-media threshold acts on, is the same at every resolution. Detail
finer than a texel becomes independent noise instead of moiré. The Gaussian
offset leaves under 1% of a regular pattern's alias at one cycle per texel, 6% at
0.75 and 29% at the Nyquist frequency, and costs one evaluation per texel.

`tx_lines` and `tx_fibers` draw at least one texel wide and scale their height
by true width over drawn width. A 0.03 mm fiber is 0.35 pixels wide at 300 ppi;
without that rule it would break into dots.

| Alternative | Why not |
| --- | --- |
| Point sample at the texel center | Same cost, but laid lines, honeycomb rims and fine canvas show moiré. |
| Supersample and average | Removes the noise but compresses sub-texel variation, so pencil tone changes with resolution. Costs k² evaluations. |
| Band-limit each operator | Crisper near the Nyquist frequency, but every operator and every composition must account for the footprint, and nonlinear shaping still aliases. |

### Imported images

An imported paper is a gray `capy.image/1` with a physical resolution
([library](paper.md#paper-library)). The bake reads it through a built-in
`image` program. With `m` image samples per drawing pixel:

| Shrink factor | Each texel |
| --- | --- |
| `m ≤ 1` | Bilinear interpolation, with no offset. At exactly 1, with the image grid on the pixel grid, each texel is an exact copy of one image sample. |
| `1 < m < 2` | The nearest image sample at an offset with σ = 0.5 × (m − 1) drawing pixel. |
| `m ≥ 2` | The nearest image sample at an offset with σ = 0.5 drawing pixel: the moiré-free, histogram-preserving shrink. |

A program is a continuous surface, so its offset never discards detail. An
image near 1:1 would lose exact samples to the full offset, which is why the
offset grows only as the image shrinks. An image at its own resolution in a
drawing of the same paper resolution, at Scale 100%, is copied exactly; that
includes an image without a resolution in a 300 ppi drawing.

The probe and normalization are the same as for programs. An imported image
has no measured relief, so it uses the 100 µm reference.

## Validation and tests

- **Catalog load:** the same naga validation as filters, with `tx_period`,
  `tx_texel` and `fx_parameter` supplied by the bake wrapper. Rejected fields,
  periods outside 8 to 400 mm and unsupported parameter kinds fail the package,
  and every preset must validate against its program.
- **Built-in limits:** 4096 texels per axis; cellular operators search 3 × 3
  cells; `tx_fibers` draws at most 64 fibers per cell; `tx_fbm` stops at eight
  octaves.
- **Seams and relief:** a headless GPU test bakes every preset, requires its
  wrap to be no rougher than its interior and orders the watercolor and drawing
  presets by relief.
- **Previews:** the [Choose Texture](paper.md#choose-texture) previews of the built-in papers
  are made from their bakes. `write_built_in_paper_previews` in
  [`paper.rs`](../../crates/layer-render-wgpu/src/paper.rs) regenerates
  [`paper_previews.gray`](../../assets/filters/paper_previews.gray), and
  `built_in_paper_previews_match_their_bakes` fails when a program changes
  without them.
- **Brushes:** the dry-media tests paint on a striped image paper, so contact
  is checked against an exact, known height field. The ignored
  [`paper_gallery`](../../crates/layer-render-wgpu/tests/paper_gallery.rs) test
  paints every dry-media brush at three pressures on each paper at real size
  and writes one PNG per paper to `GALLERY_DIR` (`GALLERY_PAPERS` picks papers by
  ID), with `coverage.tsv` giving each shading box's coverage and its solid and
  open shares; tune programs against it. The ignored
  `built_in_paper_relief_statistics` test prints each paper's relief, height
  spread by feature size, skew and bake cost per texel (`PAPERS` picks papers).
- **Cost:** bake time per preset on each tier's reference hardware belongs in
  the [performance targets](../PERFORMANCE_TARGETS.md).

## Saved data

A Paper stores its texture as
`{"builtin": "watercolor", "version": 1, "values": {...}}` with every key, like a
built-in effect. Library operators, seeds and noise belong to each program's
data version, as `fx_noise` belongs to its filters. Texture programs are
artistic surfaces: improving a built-in's appearance keeps its ID and version,
while a change of parameter meaning needs a new version.

Texture programs are built in. Installing custom filters refuses a package
with a texture program, a texture preset or a texture parameter, so no drawing
can depend on a texture another installation lacks.

## Decisions

Each line weighs a capability against the simpler option. A capability stays
only when it improves performance, has a meaningful artistic effect, or makes
the app simpler to use.

| Question | Choice | Simpler option and why it loses, or wins |
| --- | --- | --- |
| Language | WGSL programs in the filter catalog | An embedded interpreter or a new language. Krita's SeExpr fork is GPL-3; JIT runtimes cannot run on iOS; CPU interpreters bake for seconds to minutes and would block Web's single thread; no CPU interpreter for WGSL exists. A new language adds a parser and an interpreter for a subset of what the catalog already validates. |
| Stages: fiber splatting, blur buffers, histogram passes | None | A stage graph like Material Maker's. The simpler option wins. Gathering fibers from neighboring cells with a reach test costs a few thousand operations per texel for dense kozo, under 4 ms per 2048² bake on a desktop GPU, and the surfaces' blur and dilation are expressible in the program. Revisit only if a measured Low-tier bake exceeds two seconds. |
| Normalization | Probe percentiles in the bake | Each program normalizes itself. Every parameter change would then shift the paper's contrast, and Texture strength would mean something different on each paper. The probe adds about 2% to a 2048² bake. |
| Relief | Programs return micrometers | Normalized height only, plus a separate relief setting. Hot press, cold press and rough would then feel the same under a pencil unless the artist adjusted each brush. Returning physical units costs nothing and needs no extra control. |
| Sampling | One jittered sample per texel | See [one jittered sample per texel](#one-jittered-sample-per-texel). |
| User programs | Built-in only | Development loading through custom filters. A drawing that names a custom texture would then lose its paper anywhere else; portable user programs need a contract for custom code shared with custom filters, which is separate from Paper. |
| Catalog | A texture catalog beside the filter manifest | Texture entries in the filter manifest. Every filter list, search and installer would then have to skip them. |

## Reference surfaces

Eighteen programs make 38 papers; presets are listed with each. Their
amplitudes are tooth depths tuned by eye, not measurements
([measurements](paper.md#measurements)). Fine tooth paper has the height spread of
the paper the app had before Paper, so each dry-media brush keeps its tooth
contrast; the other papers were tuned against it in the
[paper gallery](#validation-and-tests). Bristol and hot press fill sooner than
Fine tooth paper; rough, handmade and canvas leave more paper white. Each 300 ppi
tile bakes in under 4 ms on an NVIDIA RTX PRO 6000; mobile bake times are not
measured.

Every paper keeps its baked heights within the spread of the first thirteen,
which `built_in_paper_relief_statistics` prints: relief of 30–150 µm, at least
5 µm of variation finer than 0.3 mm, skew within ±0.8 and a few micrometers
above 5 mm. Deeper relief clips the contact: brushes scale tooth by relief, so
everything above the paper's middle fills at once. Surfaces whose real relief
is millimeters deep, such as asphalt, burlap and dry mud, keep it in
narrow gaps and give their tops the fine tooth.

### Hot-press, cold-press and rough watercolor

All three come off a cylinder mould and take their relief from woolen felts.
Rough is never rolled, cold press passes cold rollers and hot press passes hot
rollers, which flatten the felt peaks
([Jackson's Art](https://www.jacksonsart.com/a-guide-to-paper)). One program
models the felt as warped cell mounds that pressing flattens to a plateau, with
cotton fibers and the curly imprints of the felt's wool on top. Pressing
flattens the fibers with the felt, so hot press keeps faint, flush fibers.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `felt` | mm | 0.4–5 | 1.0 |
| `pressing` | % | 0–100 | 45 |
| `seed` | count | 0–16777215 | 1 |

Presets: Hot press (`felt` 2.0, `pressing` 85), Cold press (1.0, 45), Rough
(3.0, 5), Extra rough (4.0, 0), Hahnemühle's torchon. Period 128 mm.

```wgsl
fn capy_watercolor(p: vec2f, b: u32) -> f32 {
    let felt = fx_parameter(b, 0u).x;
    let pressing = fx_parameter(b, 1u).x / 100.0;
    let seed = u32(fx_parameter(b, 2u).x);
    let q = tx_warp(p, 0.6 * felt, vec2f(3.0 * felt), seed);
    let cells = tx_cells(q, vec2f(felt), 0.9, seed + 1u);
    let mounds = 1.0 - cells.x * cells.x;
    let tooth = tx_fbm(p, vec2f(0.3 * felt), 3u, seed + 2u);
    let pressed = tx_smin(mounds, mix(1.2, 0.4, pressing), mix(0.25, 0.6, pressing)) + mix(0.6, 0.35, pressing) * tooth;
    let cotton = tx_smin(tx_fibers(p, 2.0, 0.08, 2.0, 0.0, 0.0, 0.5, seed + 3u), mix(1.0, 0.2, pressing), 0.15);
    let wool = tx_fibers(q, 3.0, 0.1, 1.0, 0.0, 0.0, 1.0, seed + 4u);
    return mix(120.0, 10.0, pressing) * pressed + mix(40.0, 44.0, pressing) * cotton - mix(30.0, 16.0, pressing) * wool;
}
```

### Bristol and cartridge

Bristol is a machine-finished board of pasted plies, calendered to a toothy
vellum or a smooth plate finish
([Bristol board](https://en.wikipedia.org/wiki/Bristol_board)). Machine-made
sheets align more fibers along the machine direction. The program uses a
slightly stretched fine tooth, partly aligned fibers and a calender plateau.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `tooth` | mm | 0.1–1 | 0.35 |
| `finish` | % | 0–100 | 35 |
| `seed` | count | 0–16777215 | 1 |

Presets: Fine tooth (`tooth` 0.35, `finish` 35), Bristol (0.2, 55). Period
64 mm.

```wgsl
fn capy_drawing(p: vec2f, b: u32) -> f32 {
    let tooth = fx_parameter(b, 0u).x;
    let finish = fx_parameter(b, 1u).x / 100.0;
    let seed = u32(fx_parameter(b, 2u).x);
    let grain = tx_fbm(p, vec2f(1.3 * tooth, tooth), 4u, seed);
    let fibers = tx_fibers(p, 2.0, 0.03, 3.0, 0.0, 0.5, 0.2, seed + 1u);
    let calendered = tx_smin(grain + 0.6 * fibers, mix(0.8, -0.1, finish), 0.3);
    return mix(83.0, 20.0, finish) * calendered;
}
```

### Laid Ingres

Laid paper records its mould: fine laid wires, held by chain wires that leave
thicker pulp beside them. Across 205 Rembrandt sheets the mean chain spacing is
24.8 mm, with a spread of 0.86 mm within a sheet
([Johnson et al.](https://people.ece.cornell.edu/johnson/JSEH.pdf)). Laid lines
run at roughly one per millimeter. Jittered line positions give the irregular
spacing; a whole number of chain intervals fits the 175 mm period. The pulp
grain is streaked along the laid wires, so the laid lines stay the dominant
pattern.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `laid` | mm | 0.5–2 | 1.0 |
| `chain` | mm | 10–50 | 25 |
| `seed` | count | 0–16777215 | 1 |

Preset: Ingres. Period 175 mm.

```wgsl
fn capy_laid(p: vec2f, b: u32) -> f32 {
    let laid = fx_parameter(b, 0u).x;
    let chain = fx_parameter(b, 1u).x;
    let seed = u32(fx_parameter(b, 2u).x);
    let wires = tx_lines(p, laid, 0.04, laid, 0.04, seed);
    let chains = tx_lines(p.yx, chain, 0.06, 0.6, 0.15, seed + 1u);
    let shadow = exp(-pow(chains.y / 3.0, 2.0));
    let felt = tx_fbm(tx_warp(p, 0.3, vec2f(1.5), seed + 2u), vec2f(0.7), 3u, seed + 3u);
    let grain = tx_fbm(p, vec2f(0.15, 0.6), 2u, seed + 4u);
    return 22.0 * wires.x * (1.0 - chains.x) - 8.0 * chains.x + 5.0 * shadow + 14.0 * felt + 26.0 * grain;
}
```

### Honeycomb pastel paper

Canson describes the textured side of Mi-Teintes as "a texture of crisscrossing
lines" that accepts heavy layering
([Canson](https://en.canson.com/expert-advice/selecting-your-pastel-paper));
its geometry is unpublished. The program places two staggered oval dimples in
each cell, so the stagger survives any whole number of rows, and leaves rims
of felted fibers between them.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `pitch` | mm | 0.5–3 | 1.1 |
| `seed` | count | 0–16777215 | 1 |

Preset: Pastel. Period 64 mm.

```wgsl
fn capy_honeycomb(p: vec2f, b: u32) -> f32 {
    let pitch = fx_parameter(b, 0u).x;
    let seed = u32(fx_parameter(b, 1u).x);
    let g = tx_grid(vec2f(pitch, 1.7 * pitch));
    let q = tx_warp(p, 0.1 * pitch, vec2f(5.0 * pitch), seed);
    let c = tx_cell(g, q);
    var dimple = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let n = c + vec2i(x, y);
            let r = tx_random(g, n, seed + 1u);
            for (var k = 0; k < 2; k += 1) {
                let site = vec2f(0.25 + 0.5 * f32(k)) + 0.08 * (select(r.xy, r.zw, k == 1) - 0.5);
                let d = tx_ellipse(q - (vec2f(n) + site) * g.cell, pitch * vec2f(0.56, 0.44), 0.15 * (r.z - 0.5));
                dimple = max(dimple, smoothstep(0.0, -0.3 * pitch, d));
            }
        }
    }
    let felt = tx_fibers(p, pitch, 0.05, 6.0, 0.0, 0.0, 0.6, seed + 2u);
    return 40.0 * (1.0 - dimple) + 25.0 * felt;
}
```

### Sanded pastel paper

Sanded papers carry grit, fiber or pumice in a binder. Grades from 240 to 800
correspond to particles from about 59 to 12 µm
([grit table](https://steeldata.info/semi/demo/help/others/grit_size_table.html)),
under a fifth of a pixel to two thirds of a pixel at 300 ppi. Each grain is a
round particle of random size; the jittered bake turns them into per-pixel
statistics. Coarser grit stands taller, as on Pastelbord and Colourfix
Rough.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `grit` | mm | 0.012–0.1 | 0.035 |
| `seed` | count | 0–16777215 | 1 |

Presets: Sanded pastel, Coarse grit (`grit` 0.09). Period 64 mm.

```wgsl
fn capy_sanded(p: vec2f, b: u32) -> f32 {
    let grit = fx_parameter(b, 0u).x;
    let seed = u32(fx_parameter(b, 1u).x);
    let grains = tx_cells(p, vec2f(1.5 * grit), 1.0, seed);
    let size = 0.6 + 0.8 * grains.z;
    let grain = size * tx_thread(grains.x, 0.6 * size);
    let binder = tx_fbm(p, vec2f(0.5), 3u, seed + 1u);
    return 40.0 * sqrt(max(grit / 0.035, 1.0)) * tx_smin(1.3 * grain, 0.9, 0.2) + 8.0 * binder;
}
```

### Kozo and unryu washi

Kozo fibers average 10 mm long and 25–35 µm wide
([University of Melbourne](https://cultural-conservation.unimelb.edu.au/papermulberry.html)).
Unryu adds long fibers "swirled onto the wet kozo"
([Legion Paper](https://legionpaper.com/unryu)). Fibers are gathered from
neighboring cells; the swirled strands are ordinary fibers drawn in warped
coordinates, which bends them along the warp field. The surface is a web of
fiber bundles over soft pulp, partly aligned in one direction, as fibers settle
when the sheet is formed by rocking the mould.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `fiber` | mm | 2–25 | 10 |
| `density` | /mm² | 0–2 | 0.4 |
| `strands` | % | 0–100 | 0 |
| `seed` | count | 0–16777215 | 1 |

Presets: Washi (`fiber` 10, `density` 0.4, `strands` 0), Unryu (10, 0.3, 100).
Period 175 mm.

```wgsl
fn capy_washi(p: vec2f, b: u32) -> f32 {
    let fiber = fx_parameter(b, 0u).x;
    let density = fx_parameter(b, 1u).x;
    let strands = fx_parameter(b, 2u).x / 100.0;
    let seed = u32(fx_parameter(b, 3u).x);
    let pulp = tx_fbm(p, vec2f(0.4), 3u, seed + 2u);
    let fibers = tx_fibers(p, 0.6 * fiber, 0.1, 1.5 * density, 0.0, 0.5, 0.4, seed + 3u);
    let swirl = tx_fibers(tx_warp(p, 6.0, vec2f(30.0), seed + 4u), 4.0 * fiber, 0.3, 0.01 * strands, 0.0, 0.0, 1.0, seed + 5u);
    return 22.0 * pulp + 18.0 * fibers + 16.0 * swirl;
}
```

### Handmade paper with inclusions

Inclusions are mixed into the vat or sprinkled on and covered with a thin
second layer of pulp before the sheet air-dries
([Atlas of Making](https://atlasofmaking.kenjiliu.com/papers/leaf-paper)).
Irregular, warped pulp gives the uneven surface, with pulp clumps of many sizes
and coarse plant fibers; each sparse cell may hold one petal or seed, and
offsetting its signed distance drapes pulp past its edge.
Inclusions stay low so that pencil reaches the pulp around them.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `amount` | % | 0–100 | 50 |
| `size` | mm | 2–20 | 6 |
| `seed` | count | 0–16777215 | 1 |

Preset: Handmade paper. Period 175 mm.

```wgsl
fn capy_handmade(p: vec2f, b: u32) -> f32 {
    let amount = fx_parameter(b, 0u).x / 100.0;
    let size = fx_parameter(b, 1u).x;
    let seed = u32(fx_parameter(b, 2u).x);
    let q = tx_warp(p, 1.2, vec2f(2.0), seed);
    let fine = tx_fbm(q, vec2f(0.7), 4u, seed + 7u);
    let felt = (tx_noise(q, vec2f(1.6), seed + 2u) + 0.9375 * fine) / 1.9375;
    let g = tx_grid(vec2f(3.0 * size));
    let c = tx_cell(g, p);
    var lump = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let n = c + vec2i(x, y);
            let r = tx_random(g, n, seed + 3u);
            if r.w > amount {
                continue;
            }
            let s = tx_random(g, n, seed + 4u);
            let shape = tx_ellipse(p - (vec2f(n) + r.xy) * g.cell, size * vec2f(0.5 + 0.6 * s.x, 0.25 + 0.2 * s.y), 6.283185 * r.z);
            lump = max(lump, (0.4 + 0.6 * s.z) * smoothstep(0.5 * size, -0.2 * size, shape));
        }
    }
    let fibers = tx_fibers(p, 2.5, 0.08, 3.0, 0.0, 0.0, 0.6, seed + 5u);
    let flocs = abs(fine - 0.2);
    return 40.0 * felt + 20.0 * lump + 20.0 * fibers + 92.0 * flocs;
}
```

### Linen and cotton canvas

Measured canvases run from 10 to about 27 threads per centimeter; the weft
varies more than the warp
([Johnson et al.](https://sethares.engr.wisc.edu/paperspdf/dsp2009.pdf)). Thin
gesso coats follow the weave and later coats level it
([Golden](https://goldenartistcolors.com/resources/black-and-white-gesso)). The
program levels the weave toward its own disc average, which fills the hollows
first. Finer threads make a shallower weave.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `threads` | /cm | 8–30 | 13 |
| `irregularity` | % | 0–100 | 50 |
| `priming` | % | 0–100 | 70 |
| `seed` | count | 0–16777215 | 1 |

Presets: Canvas (`threads` 13, `irregularity` 50, `priming` 70), Linen (20, 40,
60), Canvas paper (10, 0, 90), the embossed weave of oil sketching pads.
Period 64 mm.

```wgsl
fn capy_canvas(p: vec2f, b: u32) -> f32 {
    let threads = fx_parameter(b, 0u).x;
    let irregularity = fx_parameter(b, 1u).x / 100.0;
    let priming = fx_parameter(b, 2u).x / 100.0;
    let seed = u32(fx_parameter(b, 3u).x);
    let pitch = vec2f(10.0 / threads, 10.0 / (0.9 * threads));
    let weave = tx_weave(p, pitch, irregularity, seed);
    var level = 0.0;
    for (var i = 0u; i < 12u; i += 1u) {
        let t = tx_disc(i, 12u);
        level += t.z * tx_weave(p + 0.7 * pitch.x * t.xy, pitch, irregularity, seed);
    }
    let ground = mix(weave, max(weave, level + 0.2), priming);
    let skin = tx_fbm(p, vec2f(0.4), 3u, seed + 1u);
    return mix(500.0, 120.0, priming) * 13.0 / threads * ground + 8.0 * priming * skin;
}
```

### Newsprint

Newsprint is groundwood: about 45% fines under 0.5 mm, short broken fibers and
a few percent shives, with a length-weighted fiber length near 0.75 mm against
2.5 mm for chemical pulp. Its formation is cloudy, with blotches stretched along
the machine direction, and its surface is fuzzy. The program has no calender
plateau and no long fibers: a fine pebbly grain over cloudy flocs, short hairs
and sparse raised shives, so charcoal lays down a soft, mottled gray.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `flocs` | mm | 0.3–6 | 1.5 |
| `fuzz` | % | 0–100 | 60 |
| `seed` | count | 0–16777215 | 1 |

Preset: Newsprint. Period 64 mm. Source:
[`texture_newsprint.wgsl`](../../assets/filters/texture_newsprint.wgsl).

### Fabric: rough linen and burlap

Linen reads through its slubs: yarns swell to about twice their width for 7–30
mm, warp threads stay thin and straight, weft threads lump at each crossing, and
rows bunch and loosen. Burlap is a loose jute weave of about 3 threads per cm
with square holes, flattened twisted yarns, swollen yarns and pairs pulled
together. The program weaves paired threads, so over and under survive the tile
wrap, through a slowly warped grid. Each thread has its own position, width and
slubs, and the thread on top carries fine grain and twist ridges along its own
direction; priming fills the hollows.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `threads` | /cm | 3–30 | 9 |
| `irregularity` | % | 0–100 | 60 |
| `openness` | % | 0–100 | 0 |
| `priming` | % | 0–100 | 0 |
| `seed` | count | 0–16777215 | 1 |

Presets: Rough linen (`threads` 11, `irregularity` 80, `openness` 10, `priming`
30), Burlap (3.8, 70, 72, 0). Period 64 mm. Source:
[`texture_fabric.wgsl`](../../assets/filters/texture_fabric.wgsl).

### Gesso

Gessobord has a "slight, consistent texture": sanding makes it smoother and
extra coats rougher ([Ampersand](https://ampersandart.com/full/introduction-to-ampersand-gessobord));
unsanded gesso keeps its brush marks. Two coats of bristle grooves, each warped
and loaded unevenly along the stroke, lie horizontally and vertically;
`down` sets how much of the panel the vertical coat covers, from all horizontal
to all vertical with crossing patches between. Sanding levels the ridges to a
plateau over chalky grit.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `brush` | mm | 0.2–1.5 | 0.4 |
| `sanding` | % | 0–100 | 70 |
| `down` | % | 0–100 | 50 |
| `seed` | count | 0–16777215 | 1 |

Presets: Gesso panel, Gesso across and Gesso down (`sanding` 75, `down` 50, 0
and 100), Rough gesso, Rough across and Rough down (`brush` 0.6, `sanding` 0,
`down` 50, 0 and 100). Period 128 mm. Source:
[`texture_gesso.wgsl`](../../assets/filters/texture_gesso.wgsl).

### Concrete: matte film, chalkboard and concrete

Formed concrete is a smooth skin with sparse, isolated pits, mostly 1–4 mm and
rarely 8–16 mm, covering about 0.4–1.1% of a wall. A painted chalkboard is a
dense, uniform, pebbly grain with hairline scratches and no pits; drafting film
is a fine, even matte. One program makes all three: a cement-paste grain at
`grain`, large pores at three times `pore_size` with smaller ones between, and
long straight scratches.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `grain` | mm | 0.02–1 | 0.12 |
| `pores` | % | 0–100 | 15 |
| `pore_size` | mm | 0.2–6 | 0.9 |
| `scratches` | % | 0–100 | 0 |
| `seed` | count | 0–16777215 | 1 |

Presets: Matte film (`grain` 0.03, `pores` 0), Chalkboard (0.15, 0, `scratches`
60), Concrete. Period 128 mm. Source:
[`texture_concrete.wgsl`](../../assets/filters/texture_concrete.wgsl).

### Aggregate: sand and asphalt

Medium sand is 0.25–0.5 mm; asphalt surfaces use 9.5–12.5 mm aggregate with
1–1.5 mm of profile depth ([CT DOT](https://portal.ct.gov/-/media/dot/documents/dresearch/ct22432103pdf.pdf)),
which the program keeps in narrow gaps. Grains below 1 mm are round beads of
two sizes scattered at random; above it they turn chipped and rough-topped and
mix with two smaller grades of stone. `embed` raises the binder around them.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `grain` | mm | 0.1–12 | 0.4 |
| `embed` | % | 0–100 | 30 |
| `seed` | count | 0–16777215 | 1 |

Presets: Sand, Asphalt (`grain` 6, `embed` 25). Period 128 mm. Source:
[`texture_aggregate.wgsl`](../../assets/filters/texture_aggregate.wgsl).

### Weathered wood

Weathering erodes soft earlywood faster than latewood
([Williams et al.](https://wfs.swst.org/index.php/wfs/article/view/884)), so
the rings, 4–10 mm apart in softwood, stand as ridges, the surface Ernst rubbed
for his first frottages ([Tate](https://www.tate.org.uk/art/art-terms/f/frottage)).
Growth rings are warped lines along the grain that flow around knots; aligned
fibers, streaks and checks give the valleys their tooth.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `rings` | mm | 2–12 | 5 |
| `weathering` | % | 0–100 | 70 |
| `knots` | % | 0–100 | 40 |
| `seed` | count | 0–16777215 | 1 |

Preset: Barn wood. Period 175 mm. Source:
[`texture_wood.wgsl`](../../assets/filters/texture_wood.wgsl).

### Crackle: dry mud and crumpled paper

Drying mud splits into plates whose crack spacing follows the layer thickness
([Ma et al.](https://arxiv.org/pdf/1807.06126)); a flattened crumpled sheet
keeps a web of creases. Both are networks of cell edges at two sizes. `folds`
raises that share of the edges into creases and sinks the rest into cracks;
`gap` sets their width. Plates curl up at their rims and keep silty grain;
facets sit at slightly different heights.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `size` | mm | 3–40 | 22 |
| `gap` | mm | 0–3 | 1.2 |
| `folds` | % | 0–100 | 0 |
| `seed` | count | 0–16777215 | 1 |

Presets: Dry mud, Crumpled (`size` 9, `gap` 0, `folds` 50). Period 175 mm.
Source: [`texture_crackle.wgsl`](../../assets/filters/texture_crackle.wgsl).

### Papyrus and reeds

Papyrus is two crossed layers of pith strips, 15–50 mm wide with a few narrow
slivers, laid edge to edge with 1–2 mm overlaps and pounded together. The
strips are the main pattern: each is a plateau at its own height with its own
fiber strength, overlaps rise as narrow ridges and a few strips leave thin
slits. Within a strip, fiber bundles run parallel about 1.2 mm apart, wander
gently and sit offset from the neighboring strip's, with finer fibers and grain
along them. `across` and `down` set the strength of the horizontal and vertical
layers; Reeds are a single layer.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `width` | mm | 5–60 | 25 |
| `across` | % | 0–100 | 100 |
| `down` | % | 0–100 | 85 |
| `seed` | count | 0–16777215 | 1 |

Presets: Papyrus, Reeds across (`down` 0), Reeds down (`across` 0, `down` 100).
Period 175 mm. Source:
[`texture_papyrus.wgsl`](../../assets/filters/texture_papyrus.wgsl).

### Leaf

Fallen leaves overlap as smooth plates 2–4 deep, in mixed shapes and random
orientations, and a rubbing picks up their raised veins and outlines. Each cell
places three leaves of random size, angle and shape, ovate, lanceolate, lobed
or toothed; the top leaf at each point contributes its midrib, angled secondary
veins and a fine net of veinlets, and every outline steps the surface.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `size` | mm | 10–80 | 45 |
| `veins` | mm | 1–10 | 3 |
| `seed` | count | 0–16777215 | 1 |

Preset: Leaf. Period 128 mm. Source:
[`texture_leaf.wgsl`](../../assets/filters/texture_leaf.wgsl).

### Quilted tissue

Bathroom tissue is embossed with discrete dots, not ridges: an all-over
staggered lattice about 1.3 mm apart, chains of single dots tracing outlines,
and medallions of concentric dot rings. Dots are about 0.45 mm across. The quilt
is a diamond lattice on diagonals whose spacing is snapped to the tile, with
dotted outlines, a ring medallion in each diamond and a finer lattice between,
over a soft, fibrous base.

| Parameter | Unit | Range | Default |
| --- | --- | --- | --- |
| `cell` | mm | 6–40 | 22 |
| `seed` | count | 0–16777215 | 1 |

Preset: Toilet. Period 64 mm. Source:
[`texture_quilt.wgsl`](../../assets/filters/texture_quilt.wgsl).
