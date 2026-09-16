# Milestone 2: main integration and platform decision

## Decision

Port and qualify SDR milestone 2 on the remaining hosts before starting GTK
milestone 3 (print proofing). The GTK implementation is useful, but its shared
color contracts already require changes in the other applications. Adding proof
profiles, simulations and delivery controls first would increase that integration
work. Following this assessment, the user authorized completing Web and Android,
qualifying performance on the attached Wacom MovinkPad 14 and deploying both for
hands-on confirmation. Apple and Windows integration, and GTK print proofing,
remain outside that authorization.

**This branch is not safe to land on the shared default branch yet.** Existing
color and gradient property controls have incompatible live data contracts on
Web, Android, Apple and Windows. This is a current functionality regression,
separate from the absence of the new photo/color workflows on those hosts.
The remote default branch is `origin/main`; there is no remote `master` target
in this assessment. No remote branch was changed.

## Integrated source

The local merge combines:

- GTK milestone 2: `43153d6dff779aa96c18cbf4dc40ddf59c6e2312`.
- Freshly fetched `origin/main`: `dff06311a8a03a006705a5a118bb73bdbee2e023`
  (`Validate Apple artwork recovery and native navigation`).

Upstream includes contact brushes, persistent ink coverage, effect gesture
transactions, native lifecycle/recovery work and host UI changes. Seven textual
conflicts were resolved. GTK retains managed color textures and adds upstream's
physical-pixel wheel raster sizing and title-bar icon sizing. The renderer keeps
native stroke-edge destination companions alongside upstream's contact/coverage
changes. Effect edits retain no-op detection and upstream gesture preview/history
transactions. Upstream gesture tests now use tagged colors, including intentional
invalid values to exercise rejection; the platform loop also covers GTK.

GPU tests exposed unconditional R8 coverage rounding in the incoming ink shader.
It now applies only to the attachment-based renderer; native Float32 coverage
retains faint and 16-bit paint. An incoming prediction test differed at its final
display comparison by one U8 code in 28 channels, so that display comparison
allows one code. Repeated previews and project save/reopen comparisons remain
exact. The user's accepted visual tolerance does not change backing precision.

Actual-target compilation additionally exposed an unguarded `native_edit` access
in display-cache admission. WASM now explicitly excludes native complete-pyramid
admission, matching the native-only state. This is a build integration fix; it
does not implement browser native SDR storage or photo workflows.

## Current-code blockers

[`RgbColor`](../../crates/layer-core/src/color/value.rs) serializes as
`{"space":"Srgb","rgba":[r,g,b,a]}`. RGB coordinates are encoded in the named
space. [`EffectValue`](../../crates/layer-core/src/effects.rs) carries that object
for colors and gradient stops, and shared
[`PropertyControl`](../../crates/layer-ui/src/effects.rs) publishes it directly.

| Host | Existing control behavior with the current shared model |
| --- | --- |
| Web | `effects.js` calls `.slice()` on color values and gradient stop colors. Executing the actual controls with the tagged schema throws `c.value.value.slice is not a function` and `c.slice is not a function`. Its outgoing color actions still contain arrays. |
| Android | `Effects.kt` casts color values to `JSONArray` and uses `getJSONArray("color")` for gradient stops. Shared values are objects. These paths have incompatible casts/accessors and outgoing arrays. |
| macOS / iPadOS | `PropertyControls.swift` uses `.array`, positional subscripts and `.paintColor`. The JSON wrapper maps an object accessed as an array to an empty array; component editing reports `Invalid color`, while positional readouts fall back to zero. Gradient editing has the same mismatch. |
| Windows | `EffectView.cpp` reads colors through `CapyUi::array`, which returns an empty array for an object, leaving zero readouts and no component updates. `GradientView.cpp` also reads an array and indexes its components. Outgoing actions retain the old representation. |

The Web reproduction is a Node probe of `createEffectPanels` with minimal DOM
objects, not a full browser acceptance test. Both controls pass with the prior
array model and fail with the current tagged model. The other rows are source
findings, not claims of native runtime testing on those systems.

All four non-GTK application trees are unchanged relative to the fetched
`origin/main`. The regression is at the shared/host boundary; absence of host-file
diffs does not establish compatibility. Updating consumers requires preserving
the tag and converting the preview into the host's declared display space,
including linear-versus-encoded semantics. Merely extracting `rgba` is insufficient
for a complete wide-gamut port.

There is also a project interchange gap. Non-GTK hosts still construct the
attachment-based sRGB8 renderer. Canonical GTK paint uses profile-encoded,
straight-alpha 8/16-bit storage; the old renderer expects premultiplied sRGB8
attachments and explicitly rejects other descriptors in
[`restore_raster`](../../crates/layer-render-wgpu/src/raster.rs). This includes
painted GTK sRGB8 documents, not only 16-bit/wide-gamut files. Full native SDR
restoration and capture need host integration, or unsupported documents must be
rejected clearly before replacing the current document. Silent precision loss is
not an acceptable landing fix.

Native Float32 rendering is currently selected by GTK-specific constructors.
Web has additional WASM/async storage work, and mobile/Metal/D3D implementations
need capability, lifecycle and memory qualification. Texture filtering and color
attachment blending are separate GPU capabilities; a successful Linux build does
not establish another backend's support. The
[Vulkan format-feature specification](https://docs.vulkan.org/refpages/latest/refpages/source/VkFormatFeatureFlagBits.html)
defines these separately; device admission must query actual support.

## Why defer print proofing

The current [`ICC adapter`](../../crates/layer-color/src/icc.rs) rejects black
point compensation and absolute-intent conversion between two non-matrix profiles
with different media whites. This is verified in current code, rather than
inferred from an old proposal. Milestone 3 also needs proof-profile ownership,
paper/ink simulation, gamut warnings and independent numerical qualification.
It is not just another GTK dialog over a finished transform path.

The ICC describes BPC as a CMM adjustment, and ICC-absolute conversion involves a
media-white adjustment. These behaviors require explicit implementation and
validation; choosing a rendering-intent enum alone does not supply them.
[ICC connection guidance](https://www.color.org/iccmax/connection2/) and
[ICC White Paper 40](https://www.color.org/WP40-Black_Point_Compensation_2010-07-27.pdf).

Recommended implementation order:

1. Update existing color/gradient consumers and shared action contract tests on
   every host. Verify existing drawing, effects, history, saving and recovery;
   make unsupported document modes explicit at import/open boundaries.
2. Integrate native SDR document/storage/render paths and the milestone-2 host
   workflows. Qualify each backend and its memory limits with real SDK/device
   tests. Reuse the shared color/codec implementation, rather than duplicating it.
3. Start print-proofing transforms and GTK UI once the SDR contracts are stable
   across the supported applications.

The first step can make a narrower main landing safe without claiming full
milestone-2 feature parity. Web/Android work alone does not clear the documented
Apple/Windows landing blockers.

## Validation of this merge

Local raw logs, commands and the Web contract probe are under
`artifacts/color-m2/main-integration/`. `integration-runs.json` records compile
commands; `validation-runs.json` records shared/GPU checks. Release test executable
paths are in `test-executables.json`.

- `cargo check --workspace --all-targets --offline`: pass.
- `cargo check -p layer-web --target wasm32-unknown-unknown --offline`: pass.
- `ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/29.0.14206865" cargo ndk -t arm64-v8a --platform 29 check -p layer-android --offline`: pass.
  An initial direct cross-check failed to locate the NDK compiler; the configured
  NDK check resolves that environment issue.
- Release tests for `layer-core`, `layer-color`, `layer-engine`, `layer-ui`,
  `layer-workspace` and `layer-host`: **652 passed, five optional cases ignored**.
- `cargo build -p layer-linux --release --offline`: pass.
- The initial full renderer run passed 251 cases, with three failures and 28
  optional cases ignored. The three failures identified the coverage rounding
  and preview display comparison described above. All affected material and
  working-precision groups pass after correction; the three contact-brush GPU
  cases also pass. Subsequent project/GTK checks use the corrected release test
  binaries recorded in `final-test-executables.json` and `coverage-fix-runs.json`.

Apple and Windows native SDK builds were not run in this Linux environment.
Android's Rust target check does not compile or exercise its Kotlin controls.
WASM compilation does not exercise browser UI, WebGPU or persistence. Upstream's
recent Apple/Windows acceptance records provide evidence for their upstream
source snapshots, not qualification of this merged shared color implementation.
