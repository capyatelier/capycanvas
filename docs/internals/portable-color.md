# Portable photo codecs and color management

[Editor internals](README.md)

The shared ICC transform dependency disables architecture-specific unsafe SIMD
features and uses its scalar Rust LUT/options paths. Cargo feature checks must
retain that choice; changing only a direct dependency cannot disable features
enabled elsewhere in the graph.

`layer-color` reads and writes photos and converts colours in Rust, for every
client. It uses `libjpeg-turbo-rs` without its `simd` feature, so decoding runs
the scalar path instead of unchecked vector loads, `moxcms`, patched `rav1d` for AVIF decoding,
`rav1e` and `oxideav-av1` for AVIF encoding, and `rust_h265` for HEIC import.
The Linux, WebAssembly and Android production graphs contain no C codec, C build
tool, dynamic loader or Zstd dependency; independent native codecs appear only as
explicitly selected test references. The remaining codec patches are listed in
[vendor/README.md](../../vendor/README.md). HEIC export and direct PQ/HLG HEIC or
AVIF import are not supported. Large AVIF export is slow, especially on Android.

JPEG import/export supports RGB, gray and explicitly profiled Adobe CMYK/YCCK
input. Original ICC bytes, integer source samples, alpha, orientation and print
density retain their existing interpretation. The JPEG decoder buffers a whole
image; its scanline facade does not provide bounded-memory streaming. PNG/TIFF
delivery continues to consume rows. JPEG-compressed TIFF with gray and alpha is
refused because the TIFF decoder cannot read two-component JPEG.

Camera JPEGs with an MPF directory containing a baseline primary photograph and
only large thumbnails open the primary photograph. Unknown auxiliary image
types and multi-frame JPEG images remain unsupported. JPEG and AVIF support
SDR-base HDR gain maps through the shared Rust reconstruction path.
`GainMapPreview` carries the preview size, HDR pixels, SDR pixels and delivery
statistics. Photo admission and working codecs share channel/profile checks;
built-in gray output becomes a gray profile before validation.

## Available-memory budgets

The shared policy uses **remaining memory**, not device model or OS categories:

| Operation | Fraction of available memory |
| --- | ---: |
| SDR JPEG import: input, codec buffers and source together | 1 |
| Other decoders, gain-map JPEG included: each codec buffer | 1/3 |
| Other decoders, gain-map JPEG included: retained source | 1/8 |
| JPEG encoder working set | 1/2 |

Native defaults refresh available memory for each operation. Physical iOS queries
`os_proc_available_memory`, which reflects the app's allowance. Simulator uses
the Mac host's memory query because it does not report an iOS app allowance.
Other native targets use `sysinfo` available memory and its reported cgroup limit
where available. Apple host queries count compressed pages once through total
minus used memory. Hosts with additional process limits should supply the smaller
remaining allowance explicitly. Measured zero headroom stays zero.
Lossless WebP row output takes the budget's `encode_bytes` directly.

Browsers have no reliable cross-browser query for remaining process memory.
When no measurement is available, the fallback assumes 512 MiB of headroom:
512 MiB import, 256 MiB encode. This fallback applies to any
unmeasurable host, rather than categorizing all mobile devices as small.
Reported device RAM or a JS heap limit must not be passed as free WASM memory.

The Web host supplies a separate **capacity-based admission policy** when the
browser exposes `navigator.deviceMemory`. This is an approximate, capped physical
RAM hint, not free memory. With a hint capped at 8 GiB, source/decode/JPEG-encode
allowances are respectively capacity / 32, / 8, and 3 / 16 (maximum 256 MiB,
1 GiB, and 1.5 GiB). One file worker serializes these jobs. These are ceilings,
not upfront allocations, and are kept distinct from `from_available_memory`.
Browsers without that hint retain the fallback above. Allocation/device losses
still need recovery; this policy cannot reserve RAM against other tabs/apps.
See [Device Memory](https://www.w3.org/TR/device-memory/) and the
[page-memory proposal](https://wicg.github.io/performance-measure-memory/): neither
provides remaining system RAM to this host.


Hosts can provide a current budget without changing the codec:

```rust
use layer_color::photo::{PhotoMemoryBudget, DecodeLimits, JpegEncodeOptions};

let budget = PhotoMemoryBudget::from_available_memory(available_bytes);
let decode = DecodeLimits::from_memory_budget(budget);
let encode = JpegEncodeOptions::from_memory_budget(95, budget);
// read_photo(reader, decode)
// write_jpeg_rows(writer, extent, interpretation, resolution, encode, rows)
```

SDR JPEG preflight charges the input capacity, three copies of its APPn payloads
and the source row band, then gives the backend's pixel/coefficient estimate what
remains. The source tiles get what is left beside the decoded image buffer.
Encoding estimates eight times the raw pixel buffer plus scratch and retained metadata. These are admission estimates,
not allocator-enforced caps. Before decoding, the app reads the JPEG header and
reserves the decode's need in the memory ledger: input, marker copies, scratch,
row band and the larger of the codec estimate and the worst-case source tiles.
The decoder gets the same number as its limit. Other formats reserve the sum of
their per-buffer limits. A concurrent import sees the reduced headroom; the
reservation shrinks to the decoded source, or a decoder process's is released,
when the decode finishes. The `MAX_EXTENT` dimension limit and address-space
overflow checks also remain in force.

The policy leaves headroom for the editor, GPU, other apps and allocation
overhead. It does not certify every 100 MP editing session: one 100 MP RGBA f32
surface occupies about 1.49 GiB, and multiple surfaces/history can exceed a
device's allowance. Over-budget files currently return a clear error. Automatic
reduced-resolution import and a shared editor-wide memory reservation manager
are separate work. Hosts should serialize large photo operations or divide the
available budget among simultaneous operations.

## ICC compatibility

RGB matrix/TRC, grayscale and supported CMYK/LUT profiles use portable code.
Matrix transforms retain extended RGB and independent alpha. Generated profile
headers are deterministic; imported profile bytes are retained exactly.
Integer RGB sources with matrix profiles decode through per-channel tables
evaluated at every possible source code, followed by the profile's Float64
matrix into linear working RGB. Tables use at most 768 KiB per decoder and are
reused across tiles. This preserves the floating transform's results without
evaluating tone-curve powers for every pixel.
Linear RGB is shaped before compiling device LUTs to avoid coarse sampling of
dark colors. Absolute intent applies media-white scaling at matrix endpoints;
absolute conversions between two non-matrix profiles with different media
whites fail explicitly.
PCS Lab normalization follows the selected table's encoding: `mft2` uses legacy
16-bit Lab in both v2 and v4 profiles, while `mft1` uses full-range 8-bit Lab.
Missing intent tables fall back to the perceptual table. The
[backend correction](../../vendor/README.md#icc-lab-table-encoding) applies this
rule to sampled and direct transforms.

Black point compensation is unavailable in the new adapter. Export and document
conversion offer no toggle, and explicit requests return an error rather than
silently ignoring the setting. The serialized option is retained for document
and recipe compatibility; hosts send it as off.

Different CMMs do not produce identical LUT/perceptual results. The independent
CMYK fixture test uses a 2% ink/extended-linear-RGB tolerance across four intents;
this is interoperability coverage, not print-proof equivalence or certification
of arbitrary ICC profiles. Built-in RGB precision and integer round-trip tests
retain their stricter tolerances.

## Editable artwork color

Color preparation borrows the authored `Document` and builds a candidate with
replacements in its paint and coverage stores. Each source retains its own local
pixel domain. `Document::color_edit` checks source identity, completed backing,
raster coverage and retained originals before publishing the composition color
and all converted backing in one edit. Imported originals and tagged effect
colors keep their own defining color spaces.

`validate_document_color` prepares every distinct retained source's transform
into the composition's working space before editor adoption. Unsupported source
profiles and HDR sources in SDR documents fail admission without changing the
live artwork. Photo opening and flattened conversion create typed authored
records directly; a flattened copy contains one paint occurrence.

## Validation

```bash
cargo test --locked --release -p layer-color
cargo check --locked -p layer-linux --tests
cargo build --locked --release -p layer-color --example portable_smoke --target wasm32-unknown-unknown
node -e 'WebAssembly.instantiate(require("fs").readFileSync("target/wasm32-unknown-unknown/release/examples/portable_smoke.wasm"), {}).then(({instance}) => { if (instance.exports.portable_smoke() !== 1) process.exit(1); console.log("WASM color and JPEG checks passed"); })'
```

The WASM smoke example executes RGB/gray ICC conversion and a profiled JPEG
round trip, including source-tile storage. The renderer's existing native-only
file-worker scheduling is unchanged; this does not wire browser photo dialogs.

Optional independent CMYK references are generated outside the app, using a
local system LittleCMS library only as a validation oracle:

```bash
python3 tools/validation/icc_reference.py /path/to/cmyk.icc artifacts/color-reference
LAYER_TEST_CMYK_PROFILE=/path/to/cmyk.icc \
LAYER_TEST_CMYK_REFERENCE="$PWD/artifacts/color-reference" \
cargo test --locked --release -p layer-color cmyk_output_matches_independent_reference_samples -- --ignored --nocapture
```

The reference directory records the CMM version and profile/sample hashes.
`tools/validation/jpeg_interchange.py` continues to generate and independently
verify the optional CMYK/YCCK, progressive, orientation and 60 MP JPEG fixtures.
Generated fixtures remain local under `artifacts/`.

## Interactive display residency

Native raster publication scans every dirty Float32 page before writing native
samples or promoting canonical working pixels. Internal FullColor encoding reads
that completed scan's status without repeating validation; in-place and scratch
destinations use the same rule. Standalone candidate encoding, Gray/TwoTone mode
projection and scalar coverage retain their own validity checks. Native rounding,
alpha association and transfer boundaries remain the publication contract.
Untracked color batches omit writable change flags; tracked batches bind only
their admitted source buffers. Batches contain at most sixteen pages, with at
most two color pages per dispatch and format-specific storage-buffer limits.

Large unchanged photos use completed Float32 display mips. Full-resolution
filters run before reduction; these display textures never feed edits or exports.
Contiguous mip levels use hardware bilinear/trilinear sampling, with a bounded
page-atlas fallback when a complete pyramid is not admitted.

GTK admits a complete pyramid within one quarter of measured Vulkan driver
headroom. Android uses one half of the smaller driver/system headroom; without
the driver budget extension, only a verified integrated GPU with host-visible
local memory can use measured system headroom alone. This follows the
[Vulkan shared-memory model](https://docs.vulkan.org/guide/latest/memory_allocation.html).
Web uses its separate capacity-based admission ceiling (the same maximum 1.5 GiB
as delivery). These are per-admission ceilings, not memory reservations or total
process bounds. Only required textures are allocated; unsupported dimensions and
unknown native headroom use the bounded path.

After large file jobs, an idle Web file worker whose Wasm heap exceeds 256 MiB
retires after five seconds, provided no output lease or pending job exists. The
next operation recreates it. This releases an otherwise non-shrinking Wasm arena
without interfering with the separate interactive tile-compression worker.
