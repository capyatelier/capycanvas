# Independent filter comparison on Windows

The current renderer matches the independent pre-migration filter algorithms
within the existing one-byte tolerance on the tested Windows GPU. This comparison
qualifies the sampled migration cases on that machine. The checked-in Linux
reference still fails on Windows; it is unchanged.

## Evidence

Compared renderer source: `50e3acdfc584ee5d6da8c5b1e67973fe4f5efe03`.
Independent source: `7719e6b0ffa69e9aca1bf19acfedef9584d2fdad`.
Both used Rust 1.98.1, wgpu/Naga 30.0.1 and Intel Iris Xe on Windows,
with D3D12 driver 32.0.101.6737 and Vulkan driver 101.6737.

Each backend renders the same original 384 × 256 artwork, forty filter preview
presets and four clipping/mask/animation scopes. The existing sampling positions
produce 160 tiles in a 512 × 960 sheet: 491,520 pixels, or 1,966,080 RGBA bytes.
The verifier decodes the PNG bytes directly, including RGB beneath transparent
alpha. It requires identical imported inputs and at most one byte of difference
in every output channel. It does not composite, resize, crop or ignore channels.

| Backend | Maximum channel difference | Different pixels | Pixels above one byte | Maximum alpha difference |
| --- | ---: | ---: | ---: | ---: |
| Hardware D3D12 | 1 | 20 | 0 | 0 |
| Hardware Vulkan | 0 | 0 | 0 | 0 |

The independent and current import images match exactly on both backends.
Against the Linux reference, both independent and current Windows implementations
have maximum channel error 255 and the same 2,368 pixels above one byte.
This establishes that the sampled Linux/Windows discrepancy is also present in
the independent algorithms. It does not identify which driver, conversion or
host-environment difference caused it, or establish perceptual equivalence.

The comparison covers these fixed presets and sampled positions. It does not
qualify arbitrary parameters, other GPUs/drivers, browser rendering, physical
input or painting cadence. The ordinary
`runtime_filter_pixel_reference` test remains unchanged and still fails against
the Linux fixture on this machine. No production renderer change follows from
this comparison.

## Reproduce

Apply the [oracle patch](../../tools/visual/windows-filter-oracle.patch) only to a
separate checkout of the pinned independent commit. Its production changes are
exactly the three import-contract changes recorded in the
[reference provenance](../../crates/layer-render-wgpu/tests/fixtures/README.md):
sRGB color attachments, encoded immutable input textures, and texel-exact
explicit sRGB decoding followed by premultiplication. Filter algorithms,
preprocessing and presets are untouched. The added test ports the current capture
loop to the old `BuiltinEffect::ALL` API and compares the resulting raw pixels.

From the current checkout in PowerShell:

~~~powershell
$repo = (Get-Location).Path
$oracle = Join-Path $repo 'artifacts/windows/independent-filter-source'
git worktree add --detach $oracle 7719e6b0ffa69e9aca1bf19acfedef9584d2fdad
git -C $oracle apply --unidiff-zero (Join-Path $repo 'tools/visual/windows-filter-oracle.patch')

# Adapter indices belong to this machine. Inspect the reported adapter;
# the independent test rejects the wrong backend and CPU fallback.
$env:LAYER_GPU_INDEX = '1'
$env:CAPY_ORACLE_BACKEND = 'dx12'
$env:CARGO_TARGET_DIR = Join-Path $repo 'target'
cargo test --locked -p layer-render-wgpu --lib tests::filter_library::runtime_filter_pixel_reference -- --exact --nocapture
~~~

On the recorded reference failure, the current test writes `actual.png`,
`input.png`, `reference.png` and `differences.tsv` under
`artifacts/performance/filter-reference`. Preserve those files for each backend
and confirm they came from this invocation before comparing them. A passing
reference test does not emit fresh failure captures; do not reuse older files.
Then run the independent comparison:

~~~powershell
$env:CAPY_ORACLE_CANDIDATE = Join-Path $repo 'artifacts/performance/filter-reference'
$env:CAPY_ORACLE_OUTPUT = Join-Path $oracle ('artifacts/filter-oracle/' + $env:CAPY_ORACLE_BACKEND)
$env:CARGO_TARGET_DIR = Join-Path $repo 'artifacts/windows/independent-filter-target'
cargo test --manifest-path (Join-Path $oracle 'Cargo.toml') --locked -p layer-render-wgpu --lib windows_independent_filter_capture -- --nocapture
~~~

Repeat the current capture and independent comparison for Vulkan, selecting its
actual adapter index and setting `CAPY_ORACLE_BACKEND=vulkan`. On the reviewed
machine Vulkan was index 0 and D3D12 index 1. Keep outputs separate. The test logs
its adapter and raw error counts, saves independent images locally, and fails if
import pixels differ or any output channel exceeds the one-byte bound.
