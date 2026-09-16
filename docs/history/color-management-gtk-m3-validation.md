# GTK print proofing implementation record

2026-09-16. **In progress; not ready for manual acceptance.** The original
[handoff](../development/color-management-m3-gtk-handoff.md) and the pre-implementation
[design/tolerances](../development/color-management-m3-gtk-design.md) define the
remaining work. Other hosts, HDR and the deferred regeneration redesign are outside
this task. No calibrated display or physical print comparison has been performed.

## Baseline before rendering changes

Parent `7d2511e5d44e841975f82dec6a42c57b52506e31`, release GTK test executable
SHA-256 `05a683782a3fea01353f0b7b5d03fe37134571a2dcca3c9f1dcc6a63ee345704`.
Build record and raw results: `artifacts/color-m3/baseline/`. Rust 1.96.0
(`ac68faa20`, LLVM 22.1.2), Fedora kernel 7.1.10-200.fc44.x86_64, GTK 4.22.4,
Mutter 50.4. Renderer: NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition,
PCI `f1:00.0`, driver 610.57.04, Vulkan, 250 W limit, isolated 3840×2160 120 Hz
Wayland output at 200% scale. The user's existing app was left running.

Fresh 61 MP ProPhoto U16 native navigation: 957/960 distinct poses presented,
all three omissions in initial fit; zero missed refresh slots. First response
32.998 ms; request-to-present p95/p99 7.838/7.906 ms. This is software request to
Wayland presentation feedback, not physical input-to-photon. The pen-up test uses
12 800 ms, size-720 palette-knife contacts on a 4096² ProPhoto U16 document with
32 paint layers. Every terminal raster commit and host-backed publication passes.
Worker CPU p95/p99 1.030/1.210 ms; GPU p95/p99 1.194/1.409 ms. Process high-water
reported by the wrapper is 455,376 KiB. Final proof measurements and broader
baseline workload coverage remain outstanding.

```sh
cargo test -p layer-linux --release --locked --offline --no-run --message-format=json
# Copy the executable from Cargo's compiler-artifact record to preserve identity.
LAYER_TEST_MONITOR=3840x2160@120 LAYER_TEST_SCALE=2 LAYER_NAVIGATION_PHOTO=61mp \
 LAYER_NAVIGATION_COMPLETE=1 LAYER_NAVIGATION_MAXIMIZE=1 GSK_RENDERER=vulkan \
 bash tools/performance/gtk-raster.sh artifacts/color-m3/baseline/gtk-tests \
 native_large_photo_navigation artifacts/color-m3/baseline/navigation-full
python3 tools/performance/photo-navigation-report.py artifacts/color-m3/baseline/navigation-full.json
LAYER_TEST_MONITOR=3840x2160@120 LAYER_TEST_SCALE=2 GSK_RENDERER=vulkan \
 /usr/bin/time -v -o artifacts/color-m3/baseline/penup-time.txt \
 bash tools/performance/gtk-raster.sh artifacts/color-m3/baseline/gtk-tests \
 native_penup_and_following_strokes artifacts/color-m3/baseline/penup
```

## Numerical proof evaluator checkpoint

`layer-color` now evaluates original matrix/shaper and mft1/mft2/mAB/mBA stages
parsed by moxcms. It applies explicit image-intent/BPC and separate paper/ink
viewing policy in D50 XYZ. Unsupported/conflicting requests fail. Device gamut is
determined by repeated relative PCS/device round trips; a channel-clipping test
alone is not used. The recipe type is independent of delivery. It is not yet
connected to saved documents, cached viewing or GTK controls at this checkpoint.

The original compiled-CMM prototype failed the declared gates. Replacing its
fixed intermediate grid with original profile stages resolves the errors without
relaxing tolerances. A further diagnostic established LittleCMS uses trilinear
interpolation for Lab-indexed reverse tables and tetrahedral device interpolation.
The production evaluator matches those choices. Failed runs and the raw endpoint
diagnostic are retained under `artifacts/color-m3/references/`.

The independent fixture generator uses system LittleCMS 2.16 (`2160`), no cache,
no optimization, full adaptation, and an explicit bounded device step between CMM
legs. The design records why unbounded matrix round trips were an invalid oracle
for physical target gamut. Production retains only portable Rust dependencies.

All **672 cases** pass: four working spaces × two quantized input depths × four
targets × seven image intent/BPC combinations × three simulation states. Each case
contains 2,010 cube/dark-neutral/dark-random/random inputs. Maximum XYZ component
error is **0.000292**, largest case p99 CIE76 is **0.0486**, and maximum CIE76 is
**0.0523**, inside the declared interoperability limits. All gamut classifications
agree outside the ±1 CIE76 boundary band. Source and destination black detection
is compared independently for every target/working profile and applicable intent.

The actual corpus currently covers built-in v4 RGB matrices, a v2 CMYK mft2
profile installed with Krita, and two real WhiteWall v4 RGB mAB/mBA output
profiles. More synthetic encoding/failure cases and v4 CMYK coverage remain to
be added; these 672 cases are not the full phase-3 acceptance claim. No third-party
profile is redistributed. `oracle-bounded/reference.json` contains exact bytes'
SHA-256, source paths, CMM version, black endpoints, sample subsets and per-file
fixture hashes.

```sh
cargo run -p layer-color --example proof_profiles --locked --offline -- artifacts/color-m3/references/working
python3 tools/validation/proof_reference.py artifacts/color-m3/references/working \
 artifacts/color-m3/references/oracle-bounded \
 --target /usr/share/color/icc/krita/cmyk.icm \
 --target artifacts/color-m3/references/whitewall-fuji-glossy.icc \
 --target artifacts/color-m3/references/whitewall-william-turner.icc \
 --target artifacts/color-m3/references/working/srgb.icc
LAYER_PROOF_REFERENCE="$PWD/artifacts/color-m3/references/oracle-bounded" \
 cargo test -p layer-color --locked --offline --lib proof_matches_independent_cmm -- --ignored --nocapture
cargo test -p layer-core -p layer-color --locked --offline --lib
```

Lab download URLs are linked by the current
[WhiteWall profile instructions](https://service.whitewall.com/hc/en-us/articles/213813645-Does-WhiteWall-offer-color-management-ICC-color-profiles):
[Fuji glossy](https://static.whitewall.com/ICC/WhiteWall_ICC_Lambda_Fuji_Crystal_DP_II_glossy.icc),
[William Turner](https://static.whitewall.com/ICC/WhiteWall_ICC_Inkjet_Hahnemuehle_William_Turner.icc).
The shared run passed 78 core and 64 color tests; optional independent/device
fixtures remain explicitly ignored in the ordinary suite. The proof reference
test was run separately and passed. No render-path or UI performance acceptance
is implied by this numerical checkpoint.
