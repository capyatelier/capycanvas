# Vendored code audit — 2026-09-20

The initial audit found `rav1d` accounted for 61% of seven committed crate
snapshots, mostly assembly this application never builds. The safe cleanup
removed that assembly and the redundant `heif-oxide` runtime wrapper. The best
remaining route to fewer maintained forks is upstreaming the portability,
resource and platform fixes. Replacing the GPU forks through product changes
has substantially higher costs.

The inventory below records the initial research before cleanup. The subsequent
authorized cleanup is recorded immediately below. “No downside” in the inventory
distinguishes preserving user-visible behavior from introducing integration,
maintenance, platform, or build costs. Alternatives beyond the completed cleanup
have not been compiled or qualified by this audit.

**Completed safe cleanup, preserving the portable Rust core.** The application
now calls patched `rust_h265` directly; `heif-oxide` is no longer a Cargo dependency.
Only its attributed [test writer and four streams](../../vendor/heif-test-support/README.md)
remain. rav1d's unused assembly and C/assembly build dependencies are removed,
and enabling assembly fails explicitly. Six production crate snapshots remain.
All vendor material is approximately 8.12 MiB, down from 16.16 MiB (about 50%).
The existing Metal sRGB delta is now captured in a named patch. No system codec
libraries were introduced, and no product behavior was intentionally dropped.

Validation passed 118 shared color tests, native AV1 8/10/12-bit decoding,
production photo decoding/encoding in Chrome Wasm without host codecs, Android
compilation, three-target dependency checks, assembly rejection, and Web package
notice tests. See [the cleanup record](portable-photo-core.md#vendor-cleanup-without-native-dependencies--2026-09-20).
The remaining production forks need portability/resource/cancellation APIs
(`rav1d`, `rust_h265`, `image-webp`) or platform GPU fixes (the three wgpu crates).
GTK is a separately bundled host runtime. The small color/hash adaptations,
Gradle wrapper, test support and font remain for the reasons below; not every
small item is technically irremovable, but changing them has no established
maintenance benefit without added dependencies, reduced testing, or behavior changes.

**Scope and evidence.** Audited Git-tracked files at
`5004f1bead31f9b3359b34b5713919fdedcd73ff`, workspace and isolated-tool manifests,
notices, build/package recipes, dependency call sites, and relevant history.
Compared all seven snapshots directly against cached published `.crate` archives;
all seven archive SHA-256 hashes match [the recorded provenance](../../vendor/README.md).
Searched outside `vendor/` for copied/adapted code, license markers, external
source references, binary assets, patches, and submodules. No Git submodules were
present. Ignored build outputs and Cargo/Maven/tool caches are not repository
vendoring. Downloaded sources that our recipes patch or bundle are listed separately.

Upstream research used project sources, published API documentation, and platform
specifications linked below. Web indexes can lag live repositories; “not found
upstream” means the inspected source/release does not establish a replacement,
not that every branch and open PR has been exhaustively checked.

**Initial committed crate inventory, before cleanup.** Sizes are uncompressed tracked file bytes, including
licenses and fixtures. Source lines include comments, tests, generated bindings,
and assembly; they are not compiled application size. “Changed files” includes
manifest edits and added files, but not omitted upstream package files.

| Snapshot | Version | Files | MiB | Source lines | Changed files versus registry |
| --- | --- | ---: | ---: | ---: | ---: |
| [rav1d](../../vendor/rav1d/) | 1.1.0 | 171 | 9.874 | 290,469 | 18 |
| [wgpu-hal](../../vendor/wgpu-hal/) | 30.0.1 | 84 | 2.084 | 55,430 | 7 |
| [wgpu](../../vendor/wgpu/) | 30.0.1 | 189 | 1.692 | 40,070 | 2 |
| [rust_h265](../../vendor/rust_h265/) | 0.1.0 | 91 | 1.375 | 18,107 | 7 |
| [wgpu-types](../../vendor/wgpu-types/) | 30.0.1 | 35 | 0.621 | 15,593 | 1 |
| [image-webp](../../vendor/image-webp/) | 0.2.4 | 19 | 0.299 | 8,699 | 4 |
| heif-oxide (runtime crate subsequently removed) | 0.1.0 | 23 | 0.133 | 3,046 | 4 |
| **Total** | | **612** | **16.077** | **431,414** | **43** |

The vendor README and eight patch files add 82,194 bytes, making all of `vendor/`
16.156 MiB across 621 tracked files. Upstream-generated WebGPU bindings inside
`wgpu`, and dav1d-derived assembly inside `rav1d`, are included in these totals.
The seven crates are selected by root `[patch.crates-io]`; the isolated
[AV1 qualification tool](../../tools/validation/portable-av1/Cargo.toml) also
patches `rav1d` independently.

**1. `rav1d`: AVIF decoder.**

1. **Why vendored:** to compile the same decoder on native and
   `wasm32-unknown-unknown`, with 8/10/12-bit source planes. The
   [patch](../../vendor/rav1d-portable.patch) makes assembly build dependencies
   optional, replaces several libc integer aliases, supplies Wasm errno/offset
   definitions, and exposes the error enum. It does not change AV1 algorithms.
   [Application integration](../../crates/layer-color/src/photo/avif_io/codec.rs)
   owns container, color, memory admission, and tile boundaries.
2. **Alternatives:** upstream these portability changes and use a registry
   release; qualify another pure Rust decoder; or use native/browser codecs.
   The inspected [upstream manifest](https://raw.githubusercontent.com/memorysafety/rav1d/main/Cargo.toml)
   still has unconditional libc/cc/nasm dependencies, and the
   [published package](https://docs.rs/crate/rav1d/1.1.0) is the same baseline.
   `oxideav-av1` is already used here for encoding; its
   [newer decoder API](https://docs.rs/crate/oxideav-av1/0.1.18) merits a
   compatibility/performance experiment, not an assumption of equivalence.
   [rav1d-safe](https://docs.rs/crate/rav1d-safe/0.6.0) offers planes, metadata,
   limits and cancellation, but its
   [manifest](https://raw.githubusercontent.com/imazen/rav1d-safe/main/Cargo.toml)
   declares `AGPL-3.0-only OR LicenseRef-Imazen-Commercial` and unconditional
   cc/nasm build dependencies. It is not compatible with the current
   [permissive dependency allowlist](../../deny.toml) as configured, and is not
   an established drop-in for the project's build requirements.
3. **No downsides?** Upstreaming has no intended product downside, but depends on
   acceptance/release and native/Wasm regression checks. Other decoders require
   sample, metadata, resource, and performance qualification. Platform codecs
   introduce availability and output differences and reverse the shared
   [Rust-only photo-core decision](portable-photo-core.md).
4. **Product changes that help:** allow platform-dependent AVIF support, or
   accept decoded display RGB rather than preserving source precision/metadata.
   Dropping AVIF decoding entirely also affects AVIF export: gain-map generation
   decodes the compressed base, and previews/reopening need decoding. Removing
   only the AVIF Open menu option would not eliminate this dependency.

**Immediate size reduction within the existing design:** tracked `.asm` and `.S`
files total **8,326,976 bytes / 7.941 MiB / 235,684 lines**. Both application and
qualification manifests disable default features and enable only `bitdepth_8`
and `bitdepth_16`; [build.rs](../../vendor/rav1d/build.rs) gates assembly tooling
behind `asm`. Omitting those files would remove **49.4% of crate-snapshot bytes**
without an expected runtime change in current builds. The tradeoff is losing
the snapshot's assembly-enabled configuration and adding a pruning step to
upgrades. Make that unsupported configuration fail clearly, retain provenance
and notices, and confirm resolved target features before adopting. This reduces
checkout size, not fork count, and does not shrink current binaries.

This audit checked `cargo tree --locked --offline -p layer-color -e features -i
rav1d` for Linux, `wasm32-unknown-unknown`, and `aarch64-linux-android`; all three
resolve only the two bit-depth features. This verifies feature selection, not a
build or runtime test of a pruned snapshot.

**2. `rust_h265`: HEVC decoder.**

1. **Why vendored:** portable HEIC decoding needed source VUI color/chroma
   metadata, expected-dimension/working-memory admission, cancellation inside
   decoding, syntax bounds, and rejection of truncated or incomplete stills.
   These changes live in bitstream, SPS/PPS, slice, decoder and public API files
   in [the combined HEIF patch](../../vendor/heif-portable.patch). They replaced
   the native libheif/libde265 production route.
2. **Alternatives:** upstream the API and hardening changes, then use the
   published decoder directly; qualify a different Rust HEVC implementation;
   or restore libheif/libde265 or host decoding. The
   [published rust_h265 API](https://docs.rs/rust_h265/0.1.0/rust_h265/)
   exposes frames and NAL decoding, but does not provide the added
   `DecoderLimits`/`SequenceInfo` contract. A wrapper around that release cannot
   interrupt internal work or admit internal allocations at the required points.
   [libheif](https://github.com/strukturag/libheif) is a mature container/codec
   route, but adds native build/runtime integration; moving it to a package
   dependency does not preserve the current portable Rust codec architecture.
3. **No downsides?** No confirmed immediate replacement. Using the unpatched
   crate loses protections and metadata. Native/OS alternatives need platform
   availability and precision/color tests. A new Rust decoder requires the
   current corpus and malformed-input/cancellation qualification.
4. **Product changes that help:** stop accepting HEIC; restrict it to platforms
   with a qualified system decoder; or offer conversion to an explicitly
   normalized RGB image instead of source-preserving import. Relaxing the
   Rust-only architecture permits native codecs. Merely restricting HEIC to
   smaller or 8-bit images does not remove malformed-header or internal-allocation
   concerns. Current qualification already excludes some HEIC variants and
   does not establish 12-bit HEVC support; see
   [the supported scope](portable-photo-core.md).

**3. `heif-oxide`: wrapper around HEVC.**

1. **Why vendored:** expose its previously private HEVC module, add a bounded
   first-picture adapter over patched `rust_h265`, bound hvcC parameters, and
   fix its test container writer. The application deliberately bypasses its
   sRGB convenience conversion and threaded grid decoder, using our own BMFF,
   grids, ICC and geometry instead. The
   [upstream helper](https://raw.githubusercontent.com/dan335/heif-oxide/main/src/hevc.rs)
   still shows the simpler unbounded adapter.
2. **Alternatives:** **depend directly on patched `rust_h265`**, putting the small
   hvcC/NAL orchestration into the application's existing
   [HEVC integration](../../crates/layer-color/src/photo/avif_io/hevc.rs).
   Alternatively upstream the wrapper API. Simply selecting unmodified
   [heif-oxide 0.1.0](https://docs.rs/crate/heif-oxide/0.1.0) does not supply the
   current contract.
3. **No downsides?** Direct integration has **no identified necessary product
   regression**, but requires implementation and tests and retains the lower
   decoder fork. Also,
   [HEIC tests](../../crates/layer-color/src/photo/avif_io/hevc_tests.rs)
   import the vendored `test_builder.rs` by path and read its testdata. Removing
   the directory requires replacing that generator or retaining a small,
   explicitly attributed test-only extract. Moving copied code into `crates/`
   does not make it cease to be vendored code.
4. **Product changes that help:** none required for direct integration. Dropping
   HEIC removes both this wrapper and `rust_h265`, at the cost of a common photo
   import format. Dropping grids alone is unnecessary: we already implement them.

**4. `image-webp`: WebP entropy-table memory limits.**

1. **Why vendored:** the upstream allocation limit did not cover retained
   lossless Huffman groups/trees. The
   [four-file patch](../../vendor/image-webp-memory.patch) propagates the budget
   through still, animation, and compressed-alpha paths and accounts table
   capacities. Our [adapter](../../crates/layer-color/src/photo/webp_io.rs)
   separately admits encoded data, frames, transforms, and a temporary-tree margin.
2. **Alternatives:** upstream the patch; qualify another decoder; use
   [libwebp](https://developers.google.com/speed/webp/docs/api) with native/Wasm
   integration; or isolate decoding in a genuinely resource-limited runtime.
   The inspected [upstream API](https://raw.githubusercontent.com/image-rs/image-webp/main/src/decoder.rs)
   still explicitly says some allocations ignore its memory limit, and the
   [published release](https://docs.rs/crate/image-webp/0.2.4) is our baseline.
   A caller-side output buffer limit alone is not equivalent. A Web Worker alone
   is not a hard memory quota; a separate constrained Wasm instance/process is
   a different architecture requiring its own limits and failure handling.
3. **No downsides?** Not by removing the patch today. Upstream adoption could
   preserve behavior. Decoder replacement/isolation adds build, output,
   communication, or platform costs. Dimension limits alone do not bound the
   entropy structures being patched.
4. **Product changes that help:** reject WebP entirely, or restrict input to
   opaque lossy VP8 and reject VP8L plus losslessly compressed alpha. That loses
   lossless/transparent WebP imports. Rejecting animations alone does **not**
   solve the issue: ordinary stills also exercise the patched paths.

**5. `wgpu`: browser asynchronous pipeline creation.**

1. **Why vendored now:** the actual diff contains only two files: public async
   render/compute pipeline methods and shared WebGPU descriptor conversion.
   Although all three wgpu crates were originally copied for platform work,
   `wgpu` itself now differs for [this async extension](../../vendor/wgpu-webgpu-async-pipelines.patch).
   [Tablet traces](../history/web-startup-tablet-2026-09-17.md) found GPU-process
   stalls from synchronous compilation; yielding JavaScript between jobs did
   not fix them.
2. **Alternatives:** upstream typed async creation, use an independently maintained
   compatible release, or implement a separate raw-WebGPU backend. The
   [inspected upstream device API](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu/src/api/device.rs)
   lacks these async methods; [30.0.1](https://docs.rs/crate/wgpu/30.0.1)
   remains the release shown by the registry documentation. Browser WebGPU
   [already specifies async creation](https://www.w3.org/TR/2026/CRD-webgpu-20260810/#dom-gpudevice-createrenderpipelineasync).
   Exposing a raw device is insufficient: the application needs a typed wgpu
   pipeline, and this version has no corresponding public raw-pipeline import.
3. **No downsides?** No immediate equivalent identified. Upstreaming can preserve
   behavior. A raw WebGPU backend duplicates integration; a worker alone does
   not establish that GPU-process stalls disappear.
4. **Product changes that help:** precompile a much smaller fixed set before
   enabling editing, remove speculative catalog warmup, or limit runtime/custom
   shaders and accept a blocking first-use/loading phase. This trades startup
   or first-use responsiveness and possibly filter flexibility. Background
   yielding alone has already been tested and was insufficient.

**6. `wgpu-types`: Wayland pass-through color space.**

1. **Why vendored:** one file adds `SurfaceColorSpace::PassThrough` and its
   capability bit, needed with the Vulkan HAL mapping. This permits the GTK
   host to attach an explicit image description rather than letting the driver
   select the legacy description. Local evidence records Mutter interpreting
   that legacy sRGB path as gamma 2.2.
2. **Alternatives:** upstream the enum/capability and backend support; rely on a
   compositor/driver path proven to describe the intended transfer accurately;
   or change Linux presentation to GTK-managed textures with explicit color
   state. [Vulkan defines pass-through for application-owned description](https://docs.vulkan.org/refpages/latest/refpages/source/VkColorSpaceKHR.html).
   The [inspected wgpu surface types](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu-types/src/surface.rs)
   do not contain the variant. Backend replacement or an application-owned
   Vulkan swapchain creates much more integration work than this patch.
3. **No downsides?** Not for all currently supported environments. Removing it
   risks incorrect color. GTK-managed texture presentation needs performance
   qualification: [the presentation investigation](../history/wayland-subsurface-feasibility.md)
   records why the direct child-surface path was pursued.
4. **Product changes that help:** require a proven compositor/driver version;
   accept a slower GTK-managed presentation path; or weaken the display-color
   guarantee. Restricting documents to sRGB does not fix a transfer-curve mismatch.
   A compositor-specific gamma adjustment is not an equivalent color contract.

**7. `wgpu-hal`: five independent platform changes.**

The whole crate can leave `vendor/` only when every still-required local change
has an alternative. The actual archive comparison found these five reasons:

| Local change and reason | Alternatives to the fork | No downsides? | Product changes that help |
| --- | --- | --- | --- |
| Vulkan pass-through mapping, round-trip test, and unsupported-backend match arms | Upstream support or the presentation alternatives above | No immediate equivalent established; paired with `wgpu-types` | Same Linux color/presentation tradeoffs as above |
| Metal Float32 format capabilities: physical M4 iPad rejected the composition pipeline because RGBA32Float blending was missing; filtering/resolve used macOS-only gating | Upstream capability fix; use FP16 intermediates; or implement explicit shader blending/filtering | Upstream fix can preserve behavior. FP16 reduces precision/range; explicit blending adds renderer complexity and memory/traffic | Relax Float32 intermediate precision, accept FP16 on iPad, or narrow supported devices. Merely dropping HDR is insufficient: the recorded failure was an SDR pipeline |
| Metal sRGB surface explicitly tagged instead of `nil` | Keep the current explicit P3 viewing path; or set the owned CAMetalLayer's sRGB color space in host code after every configure | Potentially removable without a product change: current Apple canvas configuration requests P3. Confirm all surface callers/tests; a host workaround adds lifecycle responsibility | None needed if P3 remains the sole Apple surface contract; dropping wide gamut is unnecessary |
| Android Vulkan command buffers freed at every completed reset, allocated on demand | Upstream fix; supported-driver requirement; or materially redesign command recording/submission | Not established. Reset-only and periodic-buffer-free strategies already failed the recorded long workload | Limit large photo/canvas work, or exclude affected Adreno configurations. Smaller jobs reduce pressure but do not prove sustained mapping growth is fixed |
| Android bounded framebuffer reuse and periodic pool-storage reclamation | Upstream policy; redesign rendering to create fewer framebuffers; accept more driver allocation work | Removal can regress drawing latency; reverting both Android patches can restore resource exhaustion | Lower performance targets, brush/canvas limits, or narrower device support, with renewed sustained-workload tests |

Evidence: [Float32 patch](../../vendor/wgpu-metal-float32.patch),
[Android command-memory patch](../../vendor/wgpu-android-command-memory.patch),
[resource-reuse patch](../../vendor/wgpu-android-resource-reuse.patch),
[Android measurements](android-wide-brush-performance.md), and
[Apple adoption record](apple-handoff.md#managed-sdr-canvas-and-controls).
Apple documents that a [nil CAMetalLayer color space disables color matching](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace)
and publishes [device format capabilities](https://developer.apple.com/metal/capabilities/).
The inspected upstream [Metal format table](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu-hal/src/metal/adapter.rs),
[Metal surface](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu-hal/src/metal/surface.rs),
and [Vulkan reset code](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu-hal/src/vulkan/command.rs)
still show the relevant older policies. An upgrade alone is therefore not a
demonstrated solution.

**Provenance gap found and subsequently documented:** explicit Metal sRGB tagging was present in the actual source
diff, but absent from the named patch files and the vendor README's explanation.
It is now recorded in [wgpu-metal-srgb.patch](../../vendor/wgpu-metal-srgb.patch).
The Apple development record documents it. Also, historical references to a local
wgpu Float32-atomic extension do not describe a current diff: only `surface.rs`
differs in `wgpu-types`. Future removal decisions should use archive diffs, not
historical patch descriptions alone.

**8. GTK 4.22.4: downloaded, patched, and shipped with Linux packages.**

1. **Why bundled:** a tablet-pad mode notification can carry a null surface before
   keyboard focus and crash GDK before application handlers run. The
   [one-condition guard](../../tools/build/gtk-runtime/pad-event-surface.patch)
   preserves tablet support. Normal packaging downloads verified upstream source,
   builds the patched shared library, and includes corresponding source and
   recipe; see [the runtime recipe](../../tools/build/gtk-runtime/README.md).
   This is product vendoring despite there being no committed GTK source tree.
2. **Alternatives:** use an upstream or distro release containing a qualified
   fix; require such a runtime as a system dependency; or distribute through a
   shared runtime containing the fix. Simply moving the same patch to our own
   runtime repository relocates its maintenance.
3. **No downsides?** A fixed system/runtime package could preserve behavior, but
   raises the minimum supported environment and shifts installation requirements.
   The inspected upstream [pad handler](https://raw.githubusercontent.com/GNOME/gtk/main/gdk/wayland/gdkseat-wayland.c)
   still constructs the event from `seat->keyboard_focus`, and
   [surface dispatch](https://raw.githubusercontent.com/GNOME/gtk/main/gdk/gdksurface.c)
   lacks our guard. No verified upstream replacement was found.
4. **Product changes that help:** support only distributions/runtimes with the
   fix, or abandon Wayland tablet input. The tested
   `GDK_WAYLAND_DISABLE=zwp_tablet_manager_v2` workaround disables stylus support
   as well as pads, which is particularly costly for a drawing product.
   Removing pad bindings in our UI does not stop the earlier GDK crash.
   [Failure and workaround evidence](../history/settings-implementation-plan.md).

**Copied implementation outside `vendor/`.** These are small, attributed algorithm
adaptations rather than whole dependency forks. File lengths include first-party
integration and tests; do not count the entire containing files as copied code.

| Item and locations | Why copied/adapted | Alternatives | No downsides? | Product changes that help |
| --- | --- | --- | --- | --- |
| **9. Oklab**, GPU [working_color.wgsl](../../crates/layer-render-wgpu/src/working_color.wgsl), CPU primitives in [okhsv.rs](../../crates/layer-ui/src/color/okhsv.rs) | Perceptual color operations with WGSL vectors, signed cube roots, and document-primary conversion | Use a registry color crate for CPU operations; obtain shader routines from an upstream shader package; independently implement the published equations | CPU replacement is plausible after numerical checks. A CPU library does not eliminate the GPU shader implementation; replacing tiny stable math can add more infrastructure than it removes | Use RGB interpolation/operations instead, changing perceptual appearance and possibly existing brush/gradient results |
| **10. Okhsv**, [okhsv.rs](../../crates/layer-ui/src/color/okhsv.rs) with project-specific [gamut.rs](../../crates/layer-ui/src/color/gamut.rs) | Smooth perceptual picker, stable double-precision edge handling, cached hue terms, and document-gamut boundaries | `palette::Okhsv` from crates.io; retain only the application-specific integration | Not equivalent for current wide-gamut picking: Palette documents its Okhsv as sRGB-based. Our gamut code derives boundaries for the selected RGB space and handles the blue reentry boundary | Make the perceptual picker explicitly sRGB-only, or replace it with RGB/HSV/native pickers. This changes gamut coverage or picking behavior |
| **11. Skia RWTMO**, [sdr.rs](../../crates/layer-core/src/color/hdr/sdr.rs), [hdr_mapping.wgsl](../../crates/layer-render-wgpu/src/hdr_mapping.wgsl), and copied [C++ oracle](../../tools/validation/rwtmo_reference.cpp) | Consistent reference-white HDR-to-SDR rendering/export/proof behavior across CPU/GPU, with an independent numerical reference | Depend on a suitable upstream implementation; execute full Skia only in reference tooling; use an alternate tone map; or implement the standard independently | No smaller equivalent drop-in established. Full Skia is a much larger dependency and does not directly replace WGSL. Keeping only generated oracle values removes C++ maintenance but weakens convenient regeneration | Change the default tone-mapping appearance, drop browser/Skia matching, simplify to clipping/exposure, or remove HDR-to-SDR behavior. Existing documents/exports can look different |
| **12. SplitMix64 finalizer**, [quantize.rs](../../crates/layer-color/src/icc/output/quantize.rs) | Repeatable coordinate-based output dithering, independent of strip processing order | Use `rand_xoshiro::SplitMix64`, seed from the packed pixel coordinate, take one 64-bit output, and retain the existing threshold conversion | **Algorithmically equivalent route identified**, provided seed/first-output semantics match. It adds dependency surface to replace a handful of arithmetic lines; no migration/performance test was performed | No product change needed for exact replacement. Removing dithering can restore visible banding; changing its pattern changes exported samples |

Research: [Ottosson's Oklab reference](https://bottosson.github.io/posts/oklab/),
[Okhsv reference](https://bottosson.github.io/posts/colorpicker/),
[Palette's Okhsv API](https://docs.rs/palette/latest/palette/struct.Okhsv.html),
[upstream Skia implementation](https://github.com/google/skia/blob/main/src/codec/SkHdrAgtm.cpp),
[local RWTMO rationale](../history/color-management-sdr-proof-update.md),
[Vigna's finalizer](https://prng.di.unimi.it/splitmix64.c), and
[rand_xoshiro's implementation](https://docs.rs/rand_xoshiro/latest/src/rand_xoshiro/splitmix64.rs.html).
The exact historical Skia revision is recorded in
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md); the web fetch of that
historical revision failed, so upstream inspection used current main plus the
committed pinned oracle, not a verified historical-source comparison.

**13. Gradle wrapper: build-time vendored code.**

1. **Why included:** reproducible Android build entry point without requiring
   contributors to preinstall the matching Gradle. The committed wrapper JAR is
   48,462 bytes; shell/batch launchers add 11,500 bytes. Properties pin the
   downloaded distribution to Gradle 9.5.0 and a checksum; that distribution is
   not itself committed.
2. **Alternatives:** require a preinstalled pinned Gradle, provide a controlled
   development image, or generate/download the wrapper during environment setup.
   [Gradle recommends committing wrapper files, including the JAR](https://docs.gradle.org/current/userguide/gradle_wrapper.html).
3. **No downsides?** Application behavior is unaffected, but contributor/CI
   bootstrapping becomes more demanding or network-dependent. There is no local
   behavioral fork to retire here.
4. **Product changes that help:** none; this is a development workflow decision.
   Dropping Android would remove it, but would be disproportionate to about 59 KiB.

**Adjacent third-party assets and validation-only sources.** These are included
so “all vendored code” does not hide a font or a downloaded patched dependency.

| Item | Why retained | Alternatives | No downsides? | Product/workflow changes that help |
| --- | --- | --- | --- | --- |
| **Roboto-derived battery font**, [CapyBatteryNumerals-Bold.ttf](../../apps/layer-linux/fonts/CapyBatteryNumerals-Bold.ttf), 4,916 bytes plus notice | GTK battery digits match Android; local font is static bold, digits-only, renamed | Use the system UI font or require an externally installed font | Runtime remains functional, but exact glyph metrics/appearance vary and layout needs checking | Accept platform-native battery typography. This is an easy cosmetic tradeoff if exact visual parity is unimportant |
| **HEVC/HEIF testdata** in both codec snapshots, plus [derived HEIC fixtures](../../crates/layer-color/tests/fixtures/heif/README.md) | Independent encoded inputs and regression coverage | Download pinned upstream fixtures on demand or regenerate with independent encoders | No production behavior change, but offline tests/reproducibility become harder; deleting tests loses assurance | Keep committed small fixtures and make a larger conformance corpus an explicit optional validation step |
| **Validation-only libheif 1.23.4 plus local source-profile patch** | Independent HEIC reference must retain source NCLX after RGB passthrough, including bitstream-only metadata | Use an upstream release with equivalent behavior; compare raw YUV plus metadata separately against unmodified libheif; or isolate profile checks to committed independent vectors | No production runtime downside, but changing the oracle can lose exactly the source-color coverage it was introduced to provide | Change validation strategy, not the product. Do not restore these native libraries to production merely to remove this test patch |
| **Validation-only libde265 1.1.3** | Independent HEVC decoding behind libheif | Compatible system development package or a pinned test image | No product change; version/build reproducibility differs | Accept a qualified system reference environment |
| **Validation-only libavif 1.4.2** | Independent AVIF container/metadata/precision and gain-map validation | Compatible system package or test image | No product change; available API/version must match the reference tools | Accept a qualified system reference environment |
| **Validation-only dav1d 1.5.3** | Independent AV1 decoder for libavif | System dav1d or another independently qualified libavif backend | No product change; changing decoder changes the reference being used | Standardize the test environment/backend |
| **Validation-only libaom 3.14.1** | Independent AV1 encoding and known-sample fixtures | System libaom or pregenerated independently verified fixtures | No product change; fixture regeneration/version reproducibility changes | Run fixture generation in a dedicated pinned environment |
| **Validation-only libjpeg-turbo 3.1.4.1** | JPEG component of the independent HDR reference stack | Compatible system library/test image | No product change; reference output/build settings may differ | Accept a qualified system reference environment |
| **Validation-only libultrahdr 2.0.0** | Independent JPEG gain-map interchange oracle | Compatible system package/test image or committed independent reference results | No product change; availability and regeneration coverage differ | Make reference regeneration a dedicated optional workflow |

The seven native libraries above are downloaded only by the opt-in
[reference recipe](../../tools/validation/photo-codecs/photo-codecs.py), which
pins [upstream archive URLs, versions and hashes](../../tools/validation/photo-codecs/photo-codecs.json).
Only libheif has a local source patch; the other six are unmodified source builds.
They are excluded from application packages. These are test-environment
dependencies, not seven additional committed production forks. Upstream source
research for the exceptional patch: [libheif's conversion implementation](https://raw.githubusercontent.com/strukturag/libheif/v1.23.4/libheif/context.cc)
and [its decoding options](https://github.com/strukturag/libheif/blob/v1.23.4/libheif/api/libheif/heif_decoding.h).
The local [patch](../../tools/validation/photo-codecs/libheif-source-profile.patch)
is the precise behavior being preserved.

The font's [local provenance](../../apps/layer-linux/fonts/LICENSE.txt) identifies
Roboto 3.005; [Roboto's upstream](https://github.com/googlefonts/roboto-3-classic)
confirms the external font family. No font migration was tested. Mathematical
AVIF fixtures and first-party validation bridges are not imported codec code.
External photo/HDR samples fetched by validation scripts are test assets, not
vendored implementations. Project icons are
[original assets](../../apps/layer-web/icons/README.md), not a copied icon library.
Notices for ordinary registry dependencies such as `fasteval`, `exr`, `rav1e`,
`oxideav-av1`, and `lz4_flex` do not mean their source is vendored. Zrip/Zstd
vendoring mentioned in history has already been removed.

**Recommended order.**

| Priority | Action | Expected benefit | Necessary product concession |
| --- | --- | --- | --- |
| 1 | Prune unused rav1d assembly, with explicit feature restrictions | About 7.94 MiB removed; nearly half the committed crate bytes | None for current configurations; assembly-enabled snapshot builds cease to be supported |
| 2 | Remove `heif-oxide` as a production wrapper, preserving/replacing independent fixture tooling | One less production crate fork and duplicated container library | None identified; some attributed test code may remain |
| 3 | Upstream rav1d portability and image-webp allocation limits | Eliminates two forks if accepted/released; rav1d is the largest snapshot | None intended |
| 4 | Upstream rust_h265 limits/metadata/cancellation, GTK null guard, and each independent wgpu fix | Retires fixes at their natural owners without weakening product guarantees | None intended; release/runtime availability may constrain rollout |
| 5 | Check whether Metal sRGB tagging can leave the patch set under the existing P3-only surface contract | Removes a poorly inventoried, potentially unused local delta | None identified; verify alternate callers and surface tests |
| 6 | Use system battery typography if exact Android parity is expendable | Removes a small custom font and loading code | Cosmetic difference |
| 7 | Leave tiny stable color/hash routines and the standard Gradle wrapper unless dependency policy demands otherwise | Avoids trading a few lines for a larger integration/dependency burden | None |

For each future replacement, reuse the existing relevant qualification: exact
AV1 8/10/12-bit native/Chrome planes; HEIC source metadata, grids, malformed input
and cancellation; WebP resource regressions; physical iPad pipeline creation;
Android sustained large-image/wide-brush work; browser staged startup and shader
rejection; GTK tablet startup and color-managed presentation. Passing compilation
alone cannot establish that a replacement has no downsides.

**Moving a fork is a separate option.** Cargo supports a
[revision-pinned Git override](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html),
and a maintained private/public registry release could replace a local path.
This can preserve code and product behavior while deleting snapshots from this
checkout. It leaves patch maintenance, source availability, caching, release and
provenance work elsewhere; the current `unknown-git = "deny"` source policy
also needs an explicit reviewed configuration change. Fetch-and-apply scripts
similarly move the snapshot into a build cache and add another bootstrap step.
Neither is a reduction in the amount of third-party code we maintain.
