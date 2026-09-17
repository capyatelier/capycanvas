# Independent filter comparison on Metal

The current filter presets are perceptually equivalent to the independent
pre-migration algorithms on the tested Mac. The latest follow-up below retains
small raw differences with no perceptible mismatch in reviewed artwork.
No production renderer or reference tolerance changes follow from these
comparisons. The ordinary Linux-reference test still fails.

## Current renderer follow-up — 2026-09-16

Renderer `0577006d` captures all forty presets and four scopes again on the same
Mac Metal adapter. The retained independent images have their original hashes
verified against the successful `7719e6b0` run. Source and imported RGBA bytes
still match exactly. Apply only the current transparent-black output contract
to independent pixels with exactly zero alpha; current output already satisfies
that contract everywhere. The independent PNGs remain unchanged.

| Comparison | Pixels compared | Different pixels | Maximum channel difference | Pixels above one byte |
| --- | ---: | ---: | ---: | ---: |
| Sampled sheet, canonical RGBA | 491,520 | 27 | 2 | 1 |
| All 160 complete images, canonical RGBA | 15,728,640 | 494 | 2 | 8 |
| Complete images over black, encoded RGB | 15,728,640 | 493 | 2 | 1 |
| Complete images over white, encoded RGB | 15,728,640 | 488 | 2 | 1 |

Four sampled reference pixels need transparent-black canonicalization. Maximum
full-image alpha difference is one level. The current sampled output therefore
does **not** repeat the older exact match or meet the one-byte raw limit. Normal
size, unscaled pairs of Gaussian blur, masked Gaussian blur, Ripple, masked Glass,
Rainy glass, VHS, masked Heat haze and masked Domain warp have no perceptible
mismatch. Under the user's perceptual acceptance rule, these small differences
do not justify a renderer workaround. The Linux atlas still fails with maximum
error 56, and its assertion and fixture remain unchanged.

The broader renderer qualification corrects nine test-assumption failures:

- Scalar canonical values use the specified [WGSL division accuracy](https://www.w3.org/TR/WGSL/#floating-point-accuracy),
  while retaining exact integer-code checks, requiring exact untouched pixels
  and rejecting any decoded value that cannot round-trip to the original code.
  A negative corpus covers every U8/U16 code, adjacent wrong codes and values
  beyond the precision bound. Production shader math is unchanged.
- Source-neighborhood and raw-region fixtures now contain 80 and 70 tiles,
  respectively, and prove eviction from the current 64-tile cache while retaining
  their pixel, prediction, sparse-edit and memory checks. Upload checks use the
  current 16 MiB byte ceiling; asynchronous completion does not imply a fixed
  number of forced drains.
- Snapshot checks distinguish unsupported allocator reports from an observed
  zero-byte allocation. Metal supplies no such report through wgpu. Exact bands,
  histograms, budget rejection/shrinking and cancellation still execute.

The full physical-Mac renderer run reports **259 passed, one failed, 28 ignored**;
the sole failure is the unchanged Linux atlas. The final scalar-only follow-up
also qualifies the added range/precision negative checks. All edits are tests
and documentation. Release binaries, installed review apps, runtime memory
limits and production rendering are unchanged. Evidence is
`artifacts/apple-renderer-qualification-v2/`. Physical iPad filter output,
arbitrary parameters and full cross-platform appearance remain separately scoped.

## Retained-photo integration — 2026-09-16

After integrating shared retained-photo placement from `522db8dc`, all 163 decoded
capture images (source, input, atlas and 160 complete filter outputs) are identical
to the preceding `0577006d` capture. The independent comparison and normal-size
review above therefore remain applicable to these filter cases.

The integrated renderer run reports **275 passed, two failed, 29 ignored**.
Besides the known Linux atlas, a new placed-photo cache fixture assumes a nonzero
host allowance. Metal's default is zero, selecting bounded tiles. Giving this
cache-specific fixture an explicit allowance makes its unchanged painting,
prediction, cancellation, Undo/Redo, detail-retention and memory-pressure checks
pass. Production memory policy is unchanged. The complete Apple bridge suite
passes **68 tests**, with the existing opt-in 61 MP fixture excluded.

Release unit tests also exposed an existing configuration omission: their GPU
recovery helper was available only with debug assertions. It now also compiles
under `cfg(test)`; shipped Release applications still exclude it. Evidence is
`artifacts/apple-placement-integration-v1/`. These GPU-backed checks run on Mac
for both Apple policies and do not replace physical iPad or sustained-performance
acceptance of the newly integrated renderer.

Both integrated Release builds pass with empty build logs. Their exported
symbols confirm that the test fault helper is absent. Installed artist review
apps and drawings are unchanged; the new products are not installed over them.

## Evidence

Compared renderer source: `7a70cf45e147e011c88d66c0e3f79fe15976470e`.
Independent source: `7719e6b0ffa69e9aca1bf19acfedef9584d2fdad`.
Both use Rust 1.98.1, wgpu/Naga 30.0.1 and hardware Metal on Apple M2 Pro.
The independent checkout applies only the three import-contract corrections in
the [reference provenance](../../crates/layer-render-wgpu/tests/fixtures/README.md).
Filter algorithms, preprocessing, presets and composition remain unchanged.

Each renderer uses identical original 384 × 256 artwork, forty filter preview
presets and four clipping/mask/animation scopes at time 2.5. Source and imported
RGBA bytes match exactly. Every full-size output is retained, as well as the
existing 512 × 960 sheet sampled at the original positions.

| Comparison | Pixels compared | Different pixels | Maximum channel difference | Pixels above one byte |
| --- | ---: | ---: | ---: | ---: |
| Sampled sheet, raw RGBA | 491,520 | 0 | 0 | 0 |
| All 160 complete images, raw RGBA | 15,728,640 | 13 | 2 | 2 |
| Complete images over black, encoded RGB | 15,728,640 | 13 | 1 | 0 |
| Complete images over white, encoded RGB | 15,728,640 | 11 | 1 | 0 |

Raw comparisons include RGB beneath transparent alpha; alpha matches everywhere.
The two differences above one byte are red-channel values at translucent
Gaussian-blur edge pixels, with alpha 111/255. The other eleven are one-level
differences in Gaussian blur and High pass. Thus the full raw images do **not**
pass a one-byte limit, even though the existing sampled comparison passes.

For display review, both images are decoded from sRGB, composited in linear
light over the same black or white background, and encoded back to sRGB8.
No pixels are resized or omitted. Full-size side-by-side reviews cover Gaussian
blur, masked Gaussian blur, Halftone, Rainy glass, Painterly and masked Heat haze;
they show no perceptible mismatch. The display comparisons supplement the raw
counts and do not replace or weaken the test's tolerance.

Both sampled Metal sheets differ identically from the checked-in Linux PNG:
maximum channel error 56, with 1,384 pixels above one byte across 43 of 160 cases.
This establishes that those sampled differences also occur in the independent
algorithms. It does not identify their driver/conversion cause or establish
full-size Linux/Web/Android parity. The Linux PNG is unchanged.

This qualifies the fixed presets and scopes on this Mac. Arbitrary parameters,
other GPUs, physical iPad rendering/input and sustained performance remain
separate requirements. Captures, exact changed-pixel locations, source patches,
logs and comparison reports are ignored under
`artifacts/apple-filter-qualification-v1/`.

## Reproduce

The existing reference test optionally exports complete images when
`CAPY_FILTER_CAPTURE` names a directory. Use a fresh directory and inspect its
`adapter.txt`, `filters.txt`, input and source before comparing. Captures are
written before the unchanged Linux PNG assertion, so that assertion may still
fail after all images have been saved.

From the current checkout on the Mac:

```bash
repo=$(pwd)
export CAPY_FILTER_CAPTURE="$repo/artifacts/apple-filter-current"
cargo test --locked -p layer-render-wgpu --lib tests::filter_library::runtime_filter_pixel_reference -- --exact --nocapture
```

Then apply the shared [independent capture patch](../../tools/visual/windows-filter-oracle.patch)
to a separate checkout of the pinned commit. The patch retains its original
Windows test name and also accepts an explicit Metal backend. It rejects CPU
fallback and an unexpected backend; inspect both recorded adapters.

```bash
oracle="$repo/artifacts/apple-filter-independent-source"
git worktree add --detach "$oracle" 7719e6b0ffa69e9aca1bf19acfedef9584d2fdad
git -C "$oracle" apply --unidiff-zero "$repo/tools/visual/windows-filter-oracle.patch"
export CAPY_ORACLE_BACKEND=metal
export CAPY_ORACLE_CANDIDATE="$CAPY_FILTER_CAPTURE"
export CAPY_ORACLE_OUTPUT="$repo/artifacts/apple-filter-independent"
export CARGO_TARGET_DIR="$repo/target"
cargo test --manifest-path "$oracle/Cargo.toml" --locked -p layer-render-wgpu --lib windows_independent_filter_capture -- --nocapture
```

The independent test checks exact imported inputs and the original sampled
one-byte limit. It also exports all complete images as `<filter>-<scope>.png`.
For the separate full-image analysis, decode each matching PNG as RGBA8, require
identical dimensions and filter lists, and count absolute differences across
every channel of all 160 images. Keep this result distinct from the sampled
test result. Apply the same scalar transfer functions and alpha composition to
both images when preparing display comparisons.
