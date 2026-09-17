# GTK print proofing implementation record

2026-09-17. **GTK app review accepted by the user.** The original
[handoff](../development/color-management-m3-gtk-handoff.md) and the pre-implementation
[design/tolerances](../development/color-management-m3-gtk-design.md) define the
scope. Other hosts, HDR and the deferred regeneration redesign are outside
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
reported by the wrapper is 455,376 KiB. Additional fresh 24/45/60 MP baseline runs are retained alongside this run.
Final proof measurements are recorded below once qualification finishes.

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

## Numerical and storage qualification

Production uses portable Rust and the original matrix/shaper and
mft1/mft2/mAB/mBA profile stages. The independent oracle is system LittleCMS 2.16
(`2160`), no optimization/cache, full adaptation and an explicitly bounded physical
device connection. The [design](../development/color-management-m3-gtk-design.md)
records the failed prototypes, interpolation choices, v4 CMYK black correction,
gamut-boundary reporting correction and refined shadow grid. Failed runs remain
in the ignored artifact directory. No independent CMM is linked into production.

All **1,512 reference cases** pass: nine targets × four working spaces × two input
depths × seven intent/BPC combinations × three simulation states. Each has 2,010
cube/dark-neutral/dark-random/random samples. Maximum XYZ component difference:
**0.001123**; largest case p99 CIE76: **0.1087**; maximum CIE76: **0.1601**.
Subset maximum CIE76: cube 0.1482, dark-neutral 0.1360, dark-random 0.1601,
random 0.1208. Black endpoints also pass their separate 0.0002 XYZ gate.

Gamut classification agrees outside the ±1 band around each decision threshold.
Deduplicating the intent/simulation repetitions leaves 144,720 samples across
72 working-space/target/depth combinations: **17,301 boundary samples**, including
**45 raw classification disagreements**. Those exclusions are not an overall
accuracy claim. Maximum difference between individual round-trip distances is
0.16182. Raw distances and boundaries are retained in the reference fixtures.

The corpus contains built-in v4 matrices, a v2 matrix written by LittleCMS,
v2 CMYK mft2, two real v4 RGB mAB/mBA output profiles, a v4 CMYK mft2 exchange
profile, and independently generated XYZ-PCS mft1/mft2/mAB profiles. The last
three are synthetic encoding coverage, not printer-characterization evidence.
`references/oracle-expanded/reference.json` records exact profile hashes, CMM
version, sample subsets, black endpoints and fixture hashes. No third-party
profile is redistributed. Downloaded target identities:

| Target | SHA-256 |
| --- | --- |
| Krita `cmyk.icm` | `156e7c14f244cfc4ed83a755ca4803d80e15dd249b40fae82cb127d3902e15c7` |
| WhiteWall Fuji glossy | `dbeaef9a7db0d8b1c2e90a9f900be7ba73559461d1a3c5850fccc020c0010a95` |
| WhiteWall William Turner | `ad081178572f4f4edfcf6ad49f6436e43ec27cc6cdf10bf157177007dd2030cf` |
| ICC PRMG CMYK v2.0.1 | `4fefdeba2c2ee7b0ab7d7a8801b4cfdca824c106834d4054eac94ce9e8a0726d` |

Sources: [WhiteWall profile instructions](https://service.whitewall.com/hc/en-us/articles/213813645-Does-WhiteWall-offer-color-management-ICC-color-profiles),
[Fuji glossy](https://static.whitewall.com/ICC/WhiteWall_ICC_Lambda_Fuji_Crystal_DP_II_glossy.icc),
[William Turner](https://static.whitewall.com/ICC/WhiteWall_ICC_Inkjet_Hahnemuehle_William_Turner.icc),
[ICC exchange-profile description](https://registry.color.org/profile-library/exchange-space-profile),
[PRMG profile](https://registry.color.org/profile-library/profiles/PRMG_v2.0.1_MR.icc).

All **420 viewing-cache builds** pass the original off-grid appearance and neutral
limits, covering five real/built-in targets, all four spaces, seven intent/BPC
policies and three simulation states. 357 use 65³, 61 use uniform 129³ and two
ProPhoto/CMYK saturation cases use squared-grid 129³. Each candidate checks 4,096
independent off-grid inputs in both sRGB and P3 viewing. Samples occupy 5,492,500
or 42,933,780 bytes; qualification cold builds range 87–4,166 ms (median 206 ms).
The complete 420-case process peaked at 48,752 KiB RSS. This is transform-only
memory, not GTK/driver memory. Profiles that exceed admission or quality limits
fail explicitly; successful parsing alone is not acceptance for proofing.

GPU parity tests pass for both uniform and refined shadow caches in every working
space, both depths and both managed surfaces, with zero, near-zero, fractional
and full coverage. Preview agrees with CPU within one encoded byte. Export bytes
and exact artwork/composite buffers remain unchanged through proof/warning toggles.
Native recipes preserve exact profile bytes, deduplicate source/proof payloads,
participate in ordinary undo/redo and dirty checkpoints, and round-trip at both
depths without changing raster data. Invalid profile direction/channel/class,
version, recipe, archive reference and memory-admission cases fail.

```sh
cargo run -p layer-color --example proof_profiles --locked --offline -- artifacts/color-m3/references/working
python3 tools/validation/proof_synthetic.py artifacts/color-m3/references/synthetic
python3 tools/validation/proof_reference.py artifacts/color-m3/references/working \
 artifacts/color-m3/references/oracle-expanded \
 --target /usr/share/color/icc/krita/cmyk.icm \
 --target artifacts/color-m3/references/whitewall-fuji-glossy.icc \
 --target artifacts/color-m3/references/whitewall-william-turner.icc \
 --target artifacts/color-m3/references/prmg-cmyk.icc \
 --target artifacts/color-m3/references/working/srgb.icc \
 --target artifacts/color-m3/references/synthetic/matrix-v2.icc \
 --target artifacts/color-m3/references/synthetic/mft1-xyz.icc \
 --target artifacts/color-m3/references/synthetic/mft2-xyz.icc \
 --target artifacts/color-m3/references/synthetic/mab-xyz.icc
LAYER_PROOF_REFERENCE="$PWD/artifacts/color-m3/references/oracle-expanded" \
 cargo test -p layer-color --locked --offline --lib proof_matches_independent_cmm -- --ignored --nocapture
cargo run -p layer-color --release --locked --offline --example proof_lut -- \
 /usr/share/color/icc/krita/cmyk.icm \
 artifacts/color-m3/references/whitewall-fuji-glossy.icc \
 artifacts/color-m3/references/whitewall-william-turner.icc \
 artifacts/color-m3/references/prmg-cmyk.icc artifacts/color-m3/references/working/srgb.icc
cargo test -p layer-render-wgpu --locked --offline proof_view_matches_cpu_and_never_changes_artwork_or_export -- --nocapture
LAYER_GPU_PROOF_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
 cargo test -p layer-render-wgpu --locked --offline proof_shadow_grid_matches_cpu_and_never_changes_artwork_or_export -- --ignored --nocapture
cargo test -p layer-core -p layer-color -p layer-ui -p layer-engine --locked --offline --lib
cargo check --workspace --all-targets --locked --offline
```

Final shared results: 80 core, 67 color (five optional fixtures ignored), 431 UI
and 63 engine tests pass. Workspace check passes with existing platform warnings.
Reference and hardware GPU tests were run explicitly, separately from those
ordinary suites. Logs are under `artifacts/color-m3/references/`.

## GTK workflow qualification

The final release GTK test executable SHA-256 is
`52b798b90d988e402d6ef15b7a8ffa9db1d0faefd008fd58236c13c666cd6a50`, preserved
at `artifacts/color-m3/final-performance/gtk-tests`. Cargo artifact records are
`references/gtk-final-build.{jsonl,log}`. Native tests each run in their own
process because GTK initialization is confined to the initial test thread.

The actual setup dialog, file chooser, pointer button signals and keyboard
controller pass setup → compare → paint → exact undo/redo → Save As → reopen →
explicit RGB U16 TIFF export. A CMYK proof is preserved alongside independent
sRGB delivery; exported files exactly match a normal-view export. Temporary view
toggles do not dirty the document, change its archive or compile another cache.
Screenshots and exact files are in `artifacts/color-m3/gtk-journey/`.

Separate tests pass setup/preparation cancellation, rapid target supersession,
invalid saved-profile failure and recovery by undo. The old target is retired on
failure and never relabelled as the requested target. Diagnostics and deliberately
injected GPU failure/recovery pass with proof enabled in P3 U8 and ProPhoto U16;
recovery reuses the validated CPU cache and republishes it to the replacement GPU.
Expected injected GPU panic messages are retained in the recovery log.

```sh
# Build as above, preserve executable, then run each filter separately:
GSK_RENDERER=vulkan bash tools/performance/gtk-raster.sh \
 artifacts/color-m3/final-performance/gtk-tests \
 native_proof_setup_compare_history_save_reopen_and_rgb_export \
 artifacts/color-m3/final-performance/journey
GSK_RENDERER=vulkan bash tools/performance/gtk-raster.sh \
 artifacts/color-m3/final-performance/gtk-tests \
 native_proof_cancellation_supersession_and_failed_profile \
 artifacts/color-m3/final-performance/cancellation
LAYER_BENCH_PROOF=/usr/share/color/icc/krita/cmyk.icm GSK_RENDERER=vulkan \
 bash tools/performance/gtk-raster.sh artifacts/color-m3/final-performance/gtk-tests \
 native_wide_color_gpu_failure_recovery artifacts/color-m3/final-performance/recovery
```

## Final performance and delivery

The [performance record](color-management-gtk-m3-performance.md) preserves raw
distributions, repeated baseline comparisons, first-use latency and memory.
Proof-enabled 24/45/60/61 MP navigation presents 3,840/3,840 poses with zero missed
refresh slots at 4K/120 Hz; three 61 MP repeats also present every pose. The largest
129³ shadow cache sustains 120 Hz with zero missed slots and one initial fit pose
omitted. Cold readiness is 345–360 ms for the ordinary target, 3.70 s for the
refined shadow case. Warm cached completion is observed within 22 ms at 20 ms
polling. Drawing CPU/GPU changes stay within the baseline regression trigger.

Limits remain explicit: the latency investigation retains normal-view outliers
and does not claim their cause is resolved. Concurrent native save/reopen and
full-resolution TIFF export miss two/four refresh slots over eight seconds with
proof off/on. Physical print matching, calibrated-display accuracy, constrained
hardware and other hosts have not been qualified here.

Production source: **`53daf782ed36e1edb4c6a476af6dcf5eaf9f869a`**. Standalone
release executable: `artifacts/color-m3/review/capycanvas-gtk-m3`; SHA-256
**`8f7d0ac7de54937ec9325ee15039c01f655d059e50b3b8b9765fbc241980c4f6`**.
`review/build.json` records Cargo command, toolchain and test identities.
The final subsequent source changes are the optional larger-cache benchmark
policy and these handoff documents; they do not change production behavior.

Run `bash artifacts/color-m3/review/launch.sh`. The build has been launched and
verified running on the user's desktop with a managed P3 Vulkan canvas; the
original app process remains running. The launcher isolates D-Bus, settings,
workspace, profile library and recovery state. Private-session accessibility and
portal startup warnings are retained in `review/app.log`; canvas startup succeeds
and file dialogs use the native in-process chooser. No open drawing was discarded.

Follow the [manual review guide](../development/color-management-m3-gtk-review.md).
The handoff stops for the user's app review and confirmation. Automated checks
and a running build do **not** imply that the user has accepted phase 3.


## Profile-picker review revision

The follow-up GTK review removes the two-stage profile selection, optional target
name and separate setup management button. One picker now lists saved profiles
and standard color spaces, with **Add profile…** and **Manage saved profiles…**
always available below the scrolling choices. Adding validates the profile for
its current role, saves the exact validated bytes once and selects it immediately.
The descriptive ICC name (filename fallback) supplies new proof names. Filename
fallbacks persist as bounded display metadata beside unchanged ICC bytes. Removing
a saved entry preserves the currently selected bytes and embedded document data;
an embedded profile outside the library remains available as the current profile.

Proof, source interpretation/repair and export share this picker and keep their
selections independent. Source compatibility filters the visible library. File
reads and validation stay on workers; an export preset selected during loading
supersedes the pending profile result. Profile failures disable the dependent
confirmation action and remain visible; cancellation preserves the prior choice.
A damaged saved proof can still be opened to remove or replace its target.

Soft Proof Setup contains four settings: profile, rendering intent, black point
compensation and **Print simulation** (Colors only / Black ink / Paper and ink).
The simulation choices map to the existing saved semantics without renderer,
transform, artwork, history-format or export-encoding changes. The numerical and
performance qualifications above therefore remain applicable.

Verification artifacts are in `artifacts/color-m3/profile-picker/`. Native GTK
checks cover proof setup/compare/edit/save/reopen/export, all simulation choices,
add-and-select, duplicate import, saved-profile reuse, removal with embedded-data
preservation, cancellation, stale export-preset completion, untagged Open/Place/Paste,
source repair, export presets and resized PNG/TIFF/JPEG delivery. All four profile
unit tests pass, including the unnamed-profile filename/reuse regression. Final
executable identities and launch verification are recorded in
`profile-picker/review/build.json`. Use the updated
[manual guide](../development/color-management-m3-gtk-review.md) for this revision;
manual acceptance is still pending.


## Second GTK profile review

The proof dialog now initializes its string expression before its model, so the
first rendering intent is visible immediately. Native `GtkPopoverMenu` sections
provide the profile menu's spacing and separators. The profile manager uses an
Adwaita header, a + action, a spacious scrolling list and row options like Manage
Workspaces. **Show in Profile Menus** persists independently of profile bytes;
hidden profiles stay usable in existing selections and drawings. Removal remains
limited to the application library.

Soft Proof Setup uses one short conceptual sentence and **Cancel / Apply**.
Applying enables the preview; the existing **View → Soft Proof** toggle turns it
off without deleting settings. There is no separate setup-removal action. Lengthy
export instructions, implementation details and duplicated explanations were
removed from this dialog; brief option explanations remain in tooltips.

The interaction review considered [Photoshop's setup and Proof Colors toggle](https://helpx.adobe.com/photoshop/using/proofing-colors.html)
and [Affinity Photo's Soft Proof adjustment](https://affinity.help/photo2/en-US.lproj/pages/Adjustments/adjustment_softProof.html).
The existing canvas-only proof architecture follows the setup/toggle approach;
these changes do not introduce an adjustment layer or change exported pixels.

File dialogs continue to use `GtkFileDialog`, with remembered folders for artwork,
ICC profiles, native saves and image exports. Open and Import share the artwork
folder; every ICC entry point shares the profile folder. Successful selection
persists the parent directory; cancellation preserves the previous choice.
Missing directories fall back to the normal chooser behavior.

The earlier review launcher forced `no-portals` and isolated desktop settings,
which caused the fallback chooser and unexpected title-bar buttons. The revised
launcher uses the real desktop session and its settings, with `CAPY_NEW_INSTANCE`
to open the review executable alongside any running application. App files and
recovery remain isolated. Window-control policy itself has not changed.

Artifacts and the current launcher are under `artifacts/color-m3/profile-review-2/`.
The native checks cover visible defaults, menu layout, profile hiding/reuse,
remembered artwork/ICC folders across windows, cancellation, settings, proof
save/reopen and RGB delivery. A separate desktop check opens and cancels both
file-picker roles with portal tracing enabled; both issued portal OpenFile requests
and GTK read the desktop’s `appmenu:close` decoration layout. All eight GTK checks
and four profile unit tests pass. The unit tests cover
exact bytes, filename fallbacks, deduplication, visibility and role validation.
Manual acceptance remains pending the user's next review.

## Document-properties review

Document Properties now contains concise size, color-space, resolution and depth
values, with source-image facts in a separate group. Preservation and saving
instructions have been removed; assumed profiles remain identified. Soft Proof
Setup, Soft Proof and Gamut Warning now share one View menu section.

The GTK photo-master workflow and compile check pass. The inspected properties
capture and logs are in `artifacts/color-m3/document-properties-review/`; the
existing `profile-review-2/review/launch.sh` is updated to this build.

Turning off both Soft Proof and Gamut Warning now hides the proof status instead
of displaying a saved target during normal viewing. Turning either option back
on restores the indicator. The target, cached transform and document remain intact.
The existing proof journey checks status visibility alongside canvas comparison,
unchanged saved data and export independence.

## Accepted GTK workflow and profile portability

The user accepted the revised GTK app on 2026-09-17 and authorized integration.
This supersedes the pending review status recorded in earlier revisions above.
The accepted workflow uses **Proof Colors** and **Proof Setup…** together in View;
first use of Proof Colors opens setup, and cancelling leaves the view unchanged.
Proof Colors uses Ctrl+Alt+P; Gamut Warning uses Ctrl+Shift+Y, preserving Ctrl+Y
redo. Native menu rows provide consistent spacing, checkmarks and shortcut columns
throughout the shared GTK menus and preference reset menus.

Each document embeds only its active proof profile. The original remains under
**Document Profile** while choosing a replacement. Applying a replacement first
preserves the original's exact ICC bytes in the local **Saved Profiles** library.
Cancelling does not import the original; failure to preserve it leaves the saved
proof setup unchanged. After saving and reopening, the original can be selected
from the same machine's library. Another machine receives only the new embedded
proof profile. Existing source-image profiles keep their separate storage role.

The release GTK build, four profile library/reader tests, and three native tests
pass: profile portability and one-profile archive persistence; picker import,
visibility, removal and export independence; and proof cancellation, supersession
and invalid-profile recovery. Portability checks also cover retry after a local
library write failure and proof use without an installed profile. Logs and the
preserved test executable are in `artifacts/color-m3/proof-portability/`.
Earlier native menu/preferences checks are in `artifacts/color-m3/view-menu-spacing/`;
first-use checks are in `artifacts/color-m3/proof-first-use/`.

The accepted review build is recorded in
`artifacts/color-m3/profile-review-2/review/build.json`. This acceptance covers GTK
app behavior; the stated limits on other platforms, HDR, calibrated displays and
physical print matching remain unchanged.
