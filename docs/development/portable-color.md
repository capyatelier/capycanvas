# Portable photo codecs and color management

`layer-color` uses `libjpeg-turbo-rs` 0.8.0, `moxcms` 0.9.1 and patched `rav1d`
1.1.0. JPEG, AVIF import/export, encoded previews, ICC and source-tile LZ4
processing use Rust. AVIF encoding uses rav1e 0.8.1 without native build features
and oxideav-av1 0.1.16 for exact 12-bit alpha/gain samples. Native
HEIC import remains until its replacement is qualified;
GTK/GPU platform APIs retain their build requirements. See the
[migration record](portable-photo-core.md) for current capability and validation.

JPEG import/export supports RGB, gray and explicitly profiled Adobe CMYK/YCCK
input. Original ICC bytes, integer source samples, alpha, orientation and print
density retain their existing interpretation. The JPEG decoder buffers a whole
image; its scanline facade does not provide bounded-memory streaming. PNG/TIFF
delivery continues to consume rows.

Camera JPEGs with an MPF directory containing a baseline primary photograph and
only large thumbnails open the primary photograph. Unknown auxiliary image
types and multi-frame JPEG images remain unsupported. JPEG and AVIF support
SDR-base HDR gain maps through the shared Rust reconstruction path.
The Sony ILCE-7RM5 sample `sony_a7r_v_29.jpg` was checked at 9504×6336 pixels.

## Available-memory budgets

The shared policy uses **remaining memory**, not device model or OS categories:

| Operation | Fraction of available memory |
| --- | ---: |
| Retained source data during import | 1/8 |
| Decoder working set | 1/3 |
| JPEG encoder working set | 1/2 |

Native defaults refresh available memory for each operation. iOS queries
`os_proc_available_memory`, which reflects the app's allowance. Other native
targets use `sysinfo` available memory and its reported cgroup limit where
available. Hosts with additional process limits should supply the smaller
remaining allowance explicitly. Zero headroom stays zero.

Browsers have no reliable cross-browser query for remaining process memory.
When no measurement is available, the fallback assumes 512 MiB of headroom:
64 MiB source, approximately 171 MiB decode, 256 MiB encode. This fallback applies to any
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
The 61 MP Sony fixture opens at full size on the attached 12 GB MovinkPad's
Chrome (which reports the capped 8 GiB hint). Other device/workload pairs require
qualification. See [Device Memory](https://www.w3.org/TR/device-memory/) and the
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

JPEG preflight accounts for retained input capacity and the backend's estimated
pixel/coefficient storage before allocating decoded pixels. Encoding estimates
eight times the raw pixel buffer plus scratch and retained metadata. These are admission estimates,
not allocator-enforced caps or reservations against concurrent jobs. The 32768
pixel dimension limit and address-space overflow checks also remain in force.

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
Linear RGB is shaped before compiling device LUTs to avoid coarse sampling of
dark colors. Absolute intent applies media-white scaling at matrix endpoints;
absolute conversions between two non-matrix profiles with different media
whites fail explicitly.

Black point compensation is unavailable in the new adapter. Export and document
conversion offer no toggle, and explicit requests return an error rather than
silently ignoring the setting. The serialized option is retained for document
and recipe compatibility; hosts send it as off.

Different CMMs do not produce identical LUT/perceptual results. The independent
CMYK fixture test uses a 2% ink/extended-linear-RGB tolerance across four intents;
this is interoperability coverage, not print-proof equivalence or certification
of arbitrary ICC profiles. Built-in RGB precision and integer round-trip tests
retain their stricter tolerances.

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

Validation on the development Linux host: all 63 color tests (including external
fixtures) and 17 core color tests pass; the Linux client compiles; the Android
AArch64 crate check and the executing WASM example pass. CMYK/JPEG handoffs checked
by Pillow differ by at most one 8-bit ink code. Apple and Windows SDK builds and
physical-device memory qualification were not run here. Strict Clippy has
pre-existing core/TIFF warnings; the color crate passes with `--no-deps` and the
existing TIFF `drop_non_drop`/`single_element_loop` warnings allowed.

## Interactive display residency

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
