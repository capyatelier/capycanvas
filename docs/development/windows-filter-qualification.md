# Independent filter comparison on Windows

The current renderer matches the independent pre-migration filter algorithms
within the existing one-byte tolerance on the tested Windows GPU. This comparison
qualifies the sampled migration cases on that machine. The checked-in Linux
reference failed on Windows; it was not changed.

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
`runtime_filter_pixel_reference` test was not changed and failed against the
Linux fixture on this machine. No production renderer change follows from this
comparison.

## Reproduce

The sRGB8 golden test, its Linux fixture and the oracle patches were removed
with the legacy renderer; the results recorded above stand.
