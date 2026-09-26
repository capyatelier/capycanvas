# Implementation audit: adjustments and filters (ADJ)

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only implementation audit made by an agent against baseline `dac76c20`. It checks each item in the first draft of the build list against the code, the platform hosts and the design records. "Plan line N" refers to that superseded first draft; the current [research record](../photo-editing-research.md) incorporates the corrections. Line numbers can drift in later commits; verify before relying on one.

---

Paths below are relative to ``. The absolute paths of the files cited most often are listed at the end.

## Existing infrastructure the plan missed

1. **"Use selected color" hook.** `EffectAction::UseCurrentColor` (`crates/layer-ui/src/effects.rs:174,602-606`) and the `color_action` field (`:300-301`) already exist.
   - All five hosts draw `color_action` generically:
     - GTK: `apps/layer-linux/src/effects.rs:645-656`
     - Web: `apps/layer-web/effects.js:156-157`
     - Android: `Effects.kt:200-203`
     - Apple: `PropertyControls.swift:94-95`
     - Windows: `EffectView.cpp:117-125`
   - It is only set for paper color (`effects.rs:430`) and the selection-mask color (`selection_properties.rs:54-55`).
   - The eyedropper already chooses where a picked color goes: the mask target or the current color (`crates/layer-ui/src/session.rs:3847-3856`).
2. **Sampling from the canvas into a parameter already exists.** The tonal-selection probe does it (`TonalProbe`, `crates/layer-render/src/lib.rs:374-382`).
   - A point probe returns the 5×5 alpha-weighted luminance in stops. An area probe returns the 5%/95% quantiles (`crates/layer-render-wgpu/src/tonal.rs:22-52`).
   - The result is written into the "Custom" range (`crates/layer-ui/src/tonal_selection.rs:191-204,385-398`).
3. **A GPU histogram already exists.** `tonal.wgsl:45-51` keeps 4,434 bins of 1/16 stop × 64 shards using integer atomics, and reads back only the counts.
   - `local_tone.wgsl` has portable reductions: no float atomics, subgroups or filterable float textures (`docs/development/gtk-gpu-tone-guide-milestone.md:44-50`).
4. **Shadows/Highlights plus Clarity already exists as the SDR rendition's Tone × Detail control.** It is a local-Laplacian illumination guide:
   - `crates/layer-core/src/color/hdr/local.rs:1-13`
   - GPU: `crates/layer-render-wgpu/src/local_tone.rs` and `local_tone.wgsl`
   - Formula `output = input − Tone×(illum − log2 .18) + (Detail−1)×(input − illum)`: `docs/history/color-management-local-tone.md:14-21,41-65`
5. **Pyramid code already exists in three places:**
   - Coverage-aware `downsample` and `expanded` in `local_tone.wgsl:76-123`
   - A dual-filter down/up blur in `backdrop_blur.rs` / `backdrop_blur.wgsl` (UI glass; `BackdropBlurStyle` at `:49-58`)
   - The display mips in `display_mips.rs:1-2`
6. **A portable 3D LUT already runs on the GPU.**
   - `ProofLut` (65³ or 129³, storage buffer): `crates/layer-color/src/icc/proof/view_lut.rs:5-12`
   - Tetrahedral sampling with FXC and Dawn workarounds: `crates/layer-render-wgpu/src/proof_view.wgsl:20-45`
   - The content-addressed ICC library is a ready pattern for a LUT library: `crates/layer-ui/src/profile_library.rs:1-9`
7. **Reference layers and composite snapshots already exist, the same kind of miss as your clone example.**
   - `RegionSource::Layers` (`crates/layer-render/src/lib.rs:352-353`) and `doc.reference_snapshot()` (`crates/layer-core/src/layers.rs:635`) are already a region-tool source: Visible, Editing or Reference (`crates/layer-ui/src/region_tools.rs:170-182`).
   - Filter previews already compose "the insertion point in its layer-group scope" (`crates/layer-ui/src/effects.rs:52-68`; `docs/history/filter-library-design.md:97`).
8. **Presets and copy/paste pieces already exist.**
   - Per-filter preview presets, a key→value map: `effect_catalog.rs:43-44,56-63`
   - `EffectInstance::rebind` copies values by key with validation: `crates/layer-core/src/effects.rs:314-329`
   - Programs are self-contained inside documents: `effect_catalog.rs:289-295`; `docs/reference/project-format.md:51-52`
   - Duplicate layer: `art_layers.rs:1125`
   - Copy/Paste mask: `art_layers.rs:878-900`
9. **Much of this was already designed** in `docs/history/non_destructive_filters_wgsl_shader_subsystem.md`:
   - §13 reduced-resolution intermediates (557-582)
   - §15 global-statistics effects such as auto levels (616-645)
   - §18 auxiliary inputs such as a LUT (707-731)
   - §21 and Phase 3 shareable presets (800-828, 1564-1576)
   - Phase 4 multi-pass graph (1578-1588)
   - Phase 5 compute/histogram with a new security (RCE) review (1590-1601)

## Per-item findings

**Inventory, Adjustments row.** Verdict: partly wrong or imprecise.
- **"13 adjustments":** a curated subset. There is one 40-filter catalog (`effect_catalog.rs:273`), and every entry is `kind: adjustment`.
  - Categories: tone 5 (includes Vignette), color 9 (includes Solarize and Iridescence), artistic (includes Posterize).
- **"Hue/Sat master only":** true (`assets/filters/effects.wgsl:82-91`). But "no Colorize" is imprecise: Black & White with Tint already does Colorize, taking hue and saturation from the tint and lightness from the image (`effects.wgsl:152-154`).
- **"No eyedroppers":** imprecise. The "Use selected color" button (paper and mask only) and tonal canvas sampling exist (findings 1 and 2).
- **"No presets or copying":** partly wrong. Duplicate, preview presets and `rebind` exist (finding 8). What is missing is user-facing presets and cross-document copy.
- **Missing adjustments list:** some are already reachable.
  - Invert = Curves `[[0,1],[1,0]]` exactly; validation allows it (`effects.rs:546-555`).
  - Photo Filter ≈ White Balance temperature with preserve luminosity (`filter_library.wgsl:86-95`), or Split Tone.
  - Threshold ≈ Posterize 2, per channel (`effects.wgsl:164-167`).
- **"Histogram is a separate window":** accurate, but it is nonmodal and updates automatically, with RGB/R/G/B/Luminance channels, log counts, an HDR stop axis and clipping counts:
  - `crates/layer-core/src/color/histogram.rs:7-16`
  - `apps/layer-linux/src/histogram.rs:289-313`
  - `crates/layer-ui/src/color_management.rs:228-248`
- **Correction:** rewrite the row along these lines.

**Inventory, Filters row.** Verdict: imprecise.
- **"Gaussian (≤21 px)":** the parameter is σ. Its key is `sigma` but its label is "Radius" (`assets/filters/manifest.json:479-480`).
  - The kernel reaches `min(ceil(3σ),63)` px (`assets/filters/gaussian-prepare.wgsl:9`), so 63 px at the maximum.
  - The conservative halo is 2 passes × 3σ = 126 px (`manifest.json:486,490`; `crates/layer-core/src/effects.rs:270-274`; test `:815-818`).
  - `docs/reference/runtime-filters.md:224` itself says "sigma 0–21".
- **Denoise 1–3 px:** accurate (`manifest.json:640`).
- **Chromatic Aberration:** a uniform shift along one angle (`filter_library.wgsl:132-135`), not radial.
- **Correction:** "Gaussian σ ≤ 21 (kernel ≤ 63 px)".

**ADJ-1 (eyedroppers and targeted adjustment).** Verdict: existing infrastructure overlooked, plus a wrong model choice.
- **Picker lifecycle to reuse:** saving and restoring the previous tool, touch-hold loupe and Escape (`crates/layer-ui/src/color_picker_session.rs:34-92,192-314`). Add a destination to it, beside the existing mask/current-color switch (`session.rs:3847-3856`), rather than building a separate mode.
- **Sample source:** "the composite below the effect" does not exist. `ColorSampleSource` offers only `Composite` and raw `Layer` (`crates/layer-render/src/lib.rs:293-298`). Build it from `RegionSource::Layers` or the filter-preview composition (finding 7).
- **Sample averaging:** circular samples average in Oklab; the square ones average linear RGB (`lib.rs:333-335`). Choose deliberately for black and white points.
- **Targeted Curves:** reuse `EffectAction::CurvePoint` inside `Gesture`, which is one undo step (`crates/layer-ui/src/effects.rs:177-180,198-204,500-588`). Map the sampled value into the curve domain with `effects.wgsl:26-45` and `crates/layer-core/src/effects.rs:645-653`.
- **Model:** a picker changes no GPU data. Make it an optional field on `EffectParameter`, which has no `deny_unknown_fields` (`crates/layer-core/src/effects.rs:194-203`), so old builds simply ignore it. Do not add an `EffectParameterKind` variant: older builds cannot read an unknown variant, and `gpu_parameters` (`:483-506`) would need to handle it.
- **Dependency inversion:** a Levels gray point is impossible with composite-only Levels (`effects.wgsl:47-59`). It depends on ADJ-4, which is P1, while ADJ-1 is P0.
- **White Balance neutral picker:** it can be solved in closed form from the gain model (`filter_library.wgsl:86-95`). But the ±100 sliders cap R/B gain at ±0.8 EV, so strong casts will saturate.
- **Quick win:** set `color_action` for every effect Color parameter (Split Tone, B&W tint, Pencil, Halftone). This is a Rust-only change because all five hosts already draw it.

**ADJ-2 (Hue/Sat by range, Colorize).** Verdict: existing infrastructure overlooked.
- **Code to reuse:**
  - B&W's six-sector hue interpolation (`effects.wgsl:145-150`)
  - Vibrance's skin hue distance (`:138-141`)
  - The extended-range HSL domain (`:76-81`)
- **Colorize already exists** as B&W Tint.
- **Parameter count:** about 49 parameters fit the 64-parameter limit (`crates/layer-core/src/effects.rs:337`).
- **No paged Properties UI:**
  - The only per-filter special case is Curves, in Rust (`crates/layer-ui/src/effects.rs:403-420`).
  - Hosts put the Curves channels in a host-owned dropdown (GTK `apps/layer-linux/src/effects.rs:558-580`; web `effects.js:139-153`).
  - Without a shared page concept, per-range controls would be about 50 visible sliders.
- **Hue depends on the working space:** hue is computed from encoded RGB in the document's primaries (`effects.wgsl:13,65-71`).
  - Range boundaries and the skin-protection center therefore move in ProPhoto, which the Photo preset uses (`document_creation.rs:93`).
  - `working_to_oklab` is already linked into effect shaders (`crates/layer-render-wgpu/src/working_color.wgsl:49-62`).
- **Old documents keep the old program:** documents embed their programs (`project-format.md:51-52`), and startup/library refresh preserves them (`runtime-filters.md:124-128`). Specify an upgrade using `rebind`.

**ADJ-3 (histogram in editors, panel, clipping view).** Verdict: overlooked infrastructure plus hidden cost.
- **The cited core is CPU-only:** `histogram.rs:34` is CPU, fed by a full-resolution band readback (`crates/layer-render-wgpu/src/snapshot.rs:371-403`). It forbids downsampled input (`histogram.rs:84-86`). This conflicts with the plan's own rule at line 789 (no CPU readback on interactive paths).
- **Use instead:**
  - GPU bins from finding 3
  - Scopes from `RegionSource` Composite, Layer, Layers or Selection (`crates/layer-render/src/lib.rs:347-360`)
  - The Visible/Editing/Reference choice from region tools
- **Levels has no custom editor.** Only Curves and Gradient Map have custom editors (`runtime-filters.md:6-9`). A histogram "behind Levels" therefore means a new Levels control in five hosts.
- **Curves editors draw no histogram** in any host: GTK `apps/layer-linux/src/effects.rs:1076-1150`, web `effects.js:98-131`, Windows `CurveView.cpp`, Android `Effects.kt`, Apple `PropertyControls.swift`.
- **Clipping view precedents:** the gamut-warning overlay (`crates/layer-ui/src/lib.rs:1237`; `proof_workflow.rs:200-216`) and the live tonal band mask.

**ADJ-4 (per-channel Levels, Auto).** Verdict: accurate gap, with overlooked prior art.
- **Percentiles:** `tonal.rs:37-51` computes luminance-only 5/95 quantiles. The approach is designed in shader-doc §15.
- **Lesson:** the SDR Proof "Auto" was built and later removed (`docs/history/color-management-proof-dial.md:26`).
- **Additional gap:** Levels `white` max is 1 (`manifest.json:85`), so it cannot address HDR highlights. The float-document range widening covers only curves and exposure (`crates/layer-ui/src/effects.rs:365-382`).
- Per-channel ordered constraints fit the limit (≤128, `crates/layer-core/src/effects.rs:338`).

**ADJ-5 (missing pointwise adjustments).** Verdict: partly already possible; imprecise on the web mirror and the LUT.
- **Already reachable:** Invert, Photo Filter and Threshold as above. Solarize at 0% nearly inverts, but it leaves pure black untouched (`filter_library.wgsl:147` uses `>`).
- **Selective Color:** reuse B&W's sector weights and Color Balance's tonal weights (`effects.wgsl:93-94`).
- **Color Lookup:**
  - Reuse `ProofLut`, the tetrahedral sampler and the profile-library pattern.
  - Effect "lookups" are not an upload path. They are computed on the GPU from parameters only (`prep_parameter`), with ≤4,096 records (`crates/layer-core/src/effects.rs:371`; `runtime-filters.md:142-150`), and 33³ = 35,937 is larger.
  - Effect shaders cannot add bindings (`crates/layer-render-wgpu/src/effects.rs:511-527`). So the LUT must be either an inline table value in `effect_data`, like Curves' 65-vector tables (`crates/layer-core/src/effects.rs:9,492`), or a new binding with an ABI bump.
  - Record it as the deliberate sampled-LUT exception to `runtime-filters.md:41-42`.
- **Web mirror:** `apps/layer-web/filters` is a gitignored build copy. It is untracked (`git ls-files` is empty) and produced by `apps/layer-web/build.sh:28-31`. The correction is "edit only `assets/filters`"; every host copies from it:
  - Android: `build.gradle.kts:143-144`
  - Apple: `prepare.py:61`
  - Windows: `stage-assets.ps1:23`

**ADJ-6 (Shadows/Highlights, Clarity, Dehaze).** Verdict: overlooked infrastructure plus sequencing error.
- Finding 4 already computes local illumination with tone compression and detail control. "ADJ-7's pyramid" is not needed.
- **Sequencing:** ADJ-6 is in M4 but depends on ADJ-7, which is in M5 (plan lines 775-776).
- **Constraints of the existing guide:**
  - It is document-wide, at most 768 px, and rebuilt after idle (`gtk-gpu-tone-guide-milestone.md:14-15`).
  - It is not qualified on Metal, D3D12 or the browser (`:48-50,133-135`).
- **Dehaze:**
  - Lookups cannot read the image.
  - Document-sampled filters fail above the 256 MiB image budget (`crates/layer-render-wgpu/src/scene/windows.rs:8,31-38`).

**ADJ-7 (large-radius and lens blurs).** Verdict: wrong claim plus overlooked infrastructure.
- **Line 640 is wrong:**
  - `effects.rs:102` is just the enum header; `Parameter` is at `:108-112`.
  - ×3 is manifest data, not code.
  - The validator allows up to 4,096 px per pass (`crates/layer-core/src/effects.rs:357,362`), so padding is not the cap.
- **The real caps:**
  - Manifest max 21
  - The kernel truncation at 63 px (`gaussian-prepare.wgsl:9`)
  - 33 lookup records and one 64-lane workgroup (`gaussian-prepare.wgsl:2-4,30-36`; `manifest.json:469-472`)
- **Moderate radii need no new ABI:** they are possible within the existing lookup limits (≤4,096 records, ≤256 lanes, ≤256 groups; `effects.rs:371,384-385`), at linear cost per pixel. A pyramid is a performance choice.
- **Pyramid code to reuse:** finding 5. Reduced-resolution intermediates were designed but never built (shader-doc §13).
- **Large halos hit the window budget:** 256 MiB, with explicit halo errors (`scene/windows.rs:8,40-54`).
- **Surface Blur overlaps T-8.**
- **Lens-blur "depth from a mask":** an effect's mask is its coverage (`filter-library-design.md:38`), and no auxiliary layer input exists (§18).
- **Field/Tilt-shift pins:** no on-canvas effect handles exist. Vignette, Swirl and Kaleidoscope centers are percentage sliders.
- **Bokeh reference:** the Dave Hoskins lead comes with a license caveat (`filter-library-design.md:207,216`).

**ADJ-8 (noise reduction, Dust & Scratches, Smart Sharpen).** Verdict: accurate, with notes.
- Denoise is a bilateral filter in linear straight RGB with spatial weight 1/(1+r²) (`filter_library.wgsl:68-77`). Its edge threshold is therefore weak in shadows.
- Split luminance from chroma via `working_to_oklab`.
- No median filter exists anywhere.
- Smart Sharpen can extend Unsharp Mask, which already has a threshold gate (`filter_library.wgsl:34-40`).

**ADJ-9 (frequency separation).** Verdict: overlooked infrastructure, one imprecise claim.
- High Pass computes `0.5 + (enc(orig) − enc(blur))·amount` (`filter_library.wgsl:41-44`), using the same prepared taps as Gaussian Blur. At amount 50% plus Linear Light, Gaussian and High Pass give the Low and High layers directly.
- **"Exact at 8-bit" is wrong:** halving the detail loses one code value at 8-bit. State a tolerance.

**ADJ-10 (presets, copying effects).** Verdict: partly implemented already.
- Reuse finding 8. The filter-preview renderer accepts up to 8 `EffectInstance`s (`crates/layer-ui/src/effects.rs:61-80`), which gives preset thumbnails.
- **Each document has its own session:**
  - `DocumentSessions<UiSession>` in Android `document_tabs.rs:18` and web `src/lib.rs:35`.
  - The mask clipboard (`art_layers.rs:284`) therefore does not cross documents; cross-document copy needs a window-level clipboard.
- **Constraints:**
  - `deny_unknown_fields` on `EffectDefinition` and the package (`effect_catalog.rs:32,38,66`) and on `Settings` (`settings.rs:96-99`).
  - Numeric values are in encoded document RGB, so pasting Levels or Curves between ProPhoto and sRGB changes the result. Colors are tagged and convert (`runtime-filters.md:44-58`).
  - Values from ranges widened for float documents are reset by `rebind` in U8/F16 documents (`effects.rs:320-324`).
  - Catalog ID conflicts for pasted embedded programs (`runtime-filters.md:86`).

**ADJ-11 (lens corrections).** Verdict: partly already possible.
- **Vignette removal:** the formula `exp2(-2·mask·strength)` would brighten with negative strength (`filter_library.wgsl:101-105`), but the manifest minimum is 0 (`manifest.json:782`). It is a range change.
- **Distortion:** CRT already has a one-coefficient radial barrel with Document sampling (`filter_library.wgsl:196`), a ready prototype.
- **Chromatic Aberration** cannot be negated into a correction, because it is not radial.

**ADJ-12 (Match Color).** Verdict: wrong target.
- A mean/covariance transfer is a 3×3 matrix plus offset. Neither per-channel Curves nor Color Balance can represent it; it needs ADJ-5's Channel Mixer to stay editable.
- **Source options:** use reference layers and `RegionSource::Layers`/`Selection` (finding 7).

**T-3 (raise blur limits).** Verdict: wrong reasoning, incomplete scope.
- **Six filters share σ:** Unsharp Mask, High Pass, Bloom, Soft Focus and Pencil share σ and the prepare step with Gaussian Blur (`runtime-filters.md:152-155`).
- **Logarithmic sliders:** `NumericMapping::Log` and `Power` already exist (`crates/layer-ui/src/numeric.rs:14-23`).
  - Log needs a positive soft minimum (`:144-145`), but σ's minimum is 0, so use `Power`.
  - Effect Number parameters cannot declare a mapping (`crates/layer-core/src/effects.rs:208-214`), and `control()` hard-codes linear (`crates/layer-ui/src/effects.rs:322-333`). Add an optional `mapping` and soft bounds; this is a UI-only change.
- **Scaling the default with document size:** no mechanism exists. The only precedent is the float-document range widening keyed on filter IDs (`crates/layer-ui/src/effects.rs:365-382,765`).
- **Motion Blur:** its cost is `ceil(d)+1` taps per pixel (`filter_library.wgsl:62-67`). It needs no pyramid.

**T-5 (histogram into a panel).** Verdict: duplicate of ADJ-3, and underestimated.
- It is a host request (`settings.rs:665`; `session.rs:3926-3927`) with five native implementations, each with its own debounce and cancellable worker:
  - GTK: `apps/layer-linux/src/histogram.rs:1-2,81-117,309-310`
  - `apps/layer-web/histogram.js`
  - `HistogramWindow.kt`
  - `HistogramController.swift`
  - `DocumentView.cpp:305`
- It keys on the committed revision, so it does not update during slider drags.
- **Panel precedents:** the Stats, Navigator and Proof panels (`crates/layer-ui/src/layout.rs:615-634`).

**T-7 (Levels channels, pickers, Curves numeric entry).** Verdict: mostly duplicate.
- Levels channels duplicate ADJ-4; the pickers duplicate ADJ-1.
- **Curves numeric entry:**
  - `CurvePoint` already takes exact coordinates: [0,1] bounds, Δx ≥ 0.002, ≤32 points (`crates/layer-ui/src/effects.rs:671-739`). The work is host UI only.
  - In Log HDR mode, show the values in EV using `curve_max`/`curve_white` (`:286-288,403-407`).
  - Only Windows has arrow-key point nudging (`apps/layer-windows/CurveView.cpp:106-118`), a parity gap.

**T-8 (rename Denoise).** Verdict: accurate line, incomplete change.
- `filter_library.wgsl:68` is correct.
- **Keep the id:** rename the label only; tests and documents use the id `denoise`. Existing documents keep the embedded label and layer names (`layer.name = effect.label()`, `crates/layer-ui/src/effects.rs:762`).
- **Radius cost:** it is (2r+1)² taps per pixel, so r = 10 means 441 taps.
- **Merge with ADJ-7:** turn Denoise into "Surface Blur" (radius plus threshold) and keep the "Noise Reduction" name for ADJ-8.

## Constraints the plan should cite

- **ABI:**
  - `EFFECT_ABI = 3` is checked by strict equality (`crates/layer-core/src/effects.rs:6,335`). ABI 2 was dropped with no adapter, including in documents (`runtime-filters.md:40`), so a bump strands every saved effect layer unless the reader accepts both 3 and 4.
  - New `EffectParameterKind`/`EffectValue` variants cannot be read by older builds.
  - UI-only metadata can be an optional field on `EffectParameter`.
- **Sandbox:**
  - A generated shader must have exactly 5 + 14 globals and 3 entry points, with no overrides (`crates/layer-render-wgpu/src/effects.rs:511-527`).
  - Lookups cannot declare resources.
  - Exposing compute or storage to embedded WGSL needs a security (RCE) review (shader doc 1590-1601).
- **Fusion:** empty `passes` means pointwise and fusable. Passes, `time` or `alpha: filter` create an image boundary (`crates/layer-core/src/effects.rs:177-181`). Other limits:
  - 14 mask slots per fused chain (`crates/layer-render-wgpu/src/effects.rs:44-46`)
  - At most 64 parameters, 8 passes and 8 lookups (`crates/layer-core/src/effects.rs:336-338,366`)
- **Tests:**
  - Catalog count assertions of 40 (`effect_catalog.rs:273,319,345,348`).
  - `runtime_filter_pixel_reference` iterates every filter against a PNG generated by the old pre-migration renderer (`crates/layer-render-wgpu/src/tests/filter_library.rs:657-686`; `tests/fixtures/README.md`). New filters break it, so they need their own independent oracles.
- **Metal/D3D12 parity:**
  - The strict v4 comparison already fails on Metal (max error 47) and D3D12 (max error 30) (`runtime-filters.md:480-499,530-533`). New spatial filters need independent oracles (`:536-560`).
  - FXC and Dawn constraints: `proof_view.wgsl:28-30`.
- **HDR/domain:**
  - Adjustments work in encoded document RGB (`effects.wgsl:3-15`) with declared semantics (`runtime-filters.md:68-82`).
  - Neutral settings bypass the math under `FX_EXTENDED` (for example `effects.wgsl:36-37,51-52`); Exposure is linear (`:103-132`).
  - 8-bit output uses explicit rounding (`runtime-filters.md:451-460`).
- **Licensing:** original WGSL only (`filter-library-design.md:207`).

## Additional gaps not in the plan

1. Effect Color parameters lack the "Use selected color" button.
2. No log mapping or soft bounds for effect sliders.
3. No shared paged or conditional Properties UI.
4. Hue-based operations (Hue/Sat, Vibrance skin protection, B&W) shift with the working-space primaries.
5. Levels cannot address HDR values above 1.
6. There is no user-visible "update embedded filter to the library version" flow.
7. Solarize at 0% leaves pure black untouched.
8. The histogram ignores preview edits during gestures.
9. Effect lookups cannot read the image, so no image-derived tables.
10. No on-canvas handles for effect parameters.
11. Sequencing: ADJ-6 depends on ADJ-7; the ADJ-1 Levels gray point depends on ADJ-4; T-5 and T-7 duplicate ADJ-1, ADJ-3 and ADJ-4.

## Absolute paths of key files

- assets/filters/manifest.json
- assets/filters/effects.wgsl
- assets/filters/filter_library.wgsl
- assets/filters/gaussian-prepare.wgsl
- crates/layer-core/src/effects.rs
- crates/layer-core/src/effect_catalog.rs
- crates/layer-core/src/color/histogram.rs
- crates/layer-core/src/color/hdr/local.rs
- crates/layer-ui/src/effects.rs
- crates/layer-ui/src/color_picker_session.rs
- crates/layer-ui/src/tonal_selection.rs
- crates/layer-ui/src/numeric.rs
- crates/layer-ui/src/region_tools.rs
- crates/layer-render/src/lib.rs
- crates/layer-render-wgpu/src/effects.rs
- crates/layer-render-wgpu/src/tonal.rs
- crates/layer-render-wgpu/src/tonal.wgsl
- crates/layer-render-wgpu/src/local_tone.wgsl
- crates/layer-render-wgpu/src/proof_view.wgsl
- crates/layer-render-wgpu/src/backdrop_blur.rs
- crates/layer-render-wgpu/src/scene/windows.rs
- crates/layer-render-wgpu/src/snapshot.rs
- crates/layer-color/src/icc/proof/view_lut.rs
- apps/layer-linux/src/effects.rs
- apps/layer-linux/src/histogram.rs
- docs/reference/runtime-filters.md
- docs/history/non_destructive_filters_wgsl_shader_subsystem.md
- docs/history/color-management-local-tone.md
- docs/development/gtk-gpu-tone-guide-milestone.md
