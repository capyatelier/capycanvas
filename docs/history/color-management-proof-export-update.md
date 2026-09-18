# Live Proof and HDR export update — 2026-09-18

This supersedes the UI/default decisions in
[color-management-sdr-proof-update.md](color-management-sdr-proof-update.md).
Current code, tests and the review build are the implementation evidence;
this note is not hardware qualification.

## User flow

View → Proof (Ctrl+Alt+P) toggles Off and the last selected SDR/Print mode.
Enabling it reveals/selects the dockable panel beside Color in Paint and Photo
layouts; disabling leaves its visibility alone. A first use defaults to SDR for
HDR artwork and Print setup for SDR artwork. Missing print profiles open setup
without claiming a rendered proof. Mode memory is transient, per document
session. First-profile preparation can be cancelled by the same toggle.
The common native segmented control selects Off, SDR or Print (Off/Print for an
SDR document). Off changes viewing only. No Apply, Revert, Preview checkbox or
header ellipsis. SDR edits are document edits, saved with the master; dragging
one control produces one undo step. Print selections apply after cancellable
profile/LUT validation, preserving a replaced embedded profile locally first.

SDR offers Perceptual and Browser, Exposure, Contrast, HDR range, Auto and Reset.
Auto measures the edited full-resolution composite using bounded bands, ignores
zero-coverage RGB, and fits the Rec.2020 max-channel range. It resets exposure
and contrast. Cancellation and revision checks prevent stale analysis replacing
new edits. It is explicit, not continuous re-analysis while painting. HDR range
is the endpoint in stops above reference white: higher values preserve more
bright distinctions; lower values make the rendition brighter. Its tooltip/name
must not imply that it measures the connected monitor.

Print offers a compact Profile dropdown, Simulate (Colors / Black ink /
Paper & ink), Intent, Black point compensation and Gamut warning. All options
are directly visible with common spacing; there is no Options fold. The two simulation effects remain viewing-only. Export → Color &
transparency → Use print profile explicitly selects the saved profile and its
conversion intent/BPC for delivery; CMYK selects a supported opaque format.
An RGB sharing file never accidentally receives a printer ICC tag describing
unconverted RGB pixels. The `.capy` master stores one print recipe. Standard
image formats do not have a universally interoperable extra soft-proof slot.

Export has one Output selector: HDR JPEG, HDR with transparency (AVIF), HDR
native (PQ PNG), or SDR (PNG/JPEG/TIFF). Show only relevant quality/background
controls and navigate into Size, Color & transparency and Preset. Gain-map
output has an HDR/SDR preview switch showing decoded output and its actual base.
The existing Master comparison remains. An SDR monitor maps the HDR preview;
this never changes the exported master range. Explicit user choices win over
asynchronous recommendations. Transparent JPEG requires explicit flattening;
white/black backgrounds are currently available. Custom background colors and
HEIC encoding are not part of this initial route.

## Algorithm research and decision

There is no single universally best HDR-to-SDR transform. It depends on whether
the input is scene-referred, display-referred, has an authored SDR rendition,
and on source/target luminance and gamut. Capy's master is display-referred
linear RGB with RGB 1 = 203 cd/m². A camera-development filmic transform is not
a drop-in replacement for that contract.

Primary sources checked:

- [ITU-R BT.2390 (2016), §5.4](https://www.itu.int/dms_pub/itu-r/opb/rep/R-REP-BT.2390-2016-PDF-E.pdf):
  normalized PQ-domain Hermite shoulder, with a linear segment. Implementation
  is independently derived from these equations. The 2025 revision reorganized
  the report; the 2016 document is the actual equation reference used here.
- [libplacebo options](https://libplacebo.org/options/#tone_mappingfunction):
  its current default is an average-adaptive spline, not BT.2390. BT.2390 remains
  supported; its knee offset defaults to 1, rather than the report's 0.5.
  Peak measurement and gamut mapping are separate parts of its rendering path.
- [Skia RWTMO](https://github.com/google/skia/blob/12dcafc24c49f369b1df66efd17369fc1c3d5474/src/codec/SkHdrAgtm.cpp)
  and [Apple HAGC](https://developer.apple.com/documentation/colorsync/headroom-adaptive-gain-curve):
  a reference-white fallback when an authored adaptive curve is absent. See the
  previous note for the pinned numerical browser comparison and stable Bezier
  evaluation. Browser is useful for matching that fallback; it is not proof of
  identical output across browser versions, image codecs or platform pipelines.
- [Adobe Camera Raw HDR output](https://helpx.adobe.com/camera-raw/desktop/hdr-and-advanced-output/hdr-output.html):
  separate authoring of SDR appearance alongside the HDR master supports keeping
  the fallback recipe persistent and distinct from temporary display proofing.

The selected **Perceptual** default uses BT.2390 with knee offset 1, zero black,
203-nit reference white and a 1000-nit initial endpoint. This is the best of the
implemented/tested choices for keeping ordinary tones bright while smoothly
compressing highlights. It is not libplacebo's entire renderer or a claim of
universal perceptual superiority. Auto removes the fixed-peak assumption when
requested. No LGPL source is copied into this implementation.

At neutral settings, linear gray 0.18 becomes approximately 0.18 and white 1
becomes 0.661, versus 0.09 and 0.5 with Browser. Thus the previous browser default
halved normal midtone luminance in a 1000-nit master. Exposure changes the input
to the curve; Contrast pivots at linear 0.18. A highlight shoulder necessarily
reduces the apparent effect of exposure near white. A Float64 equation oracle
checks the Float32 implementation over 16,384 log-spaced inputs, including
0–16 stops of headroom. Extended peaks clamp the knee at zero so negative PQ
codes cannot generate NaNs. Shared CPU and GPU math must agree.

Only Perceptual and Browser are offered for new selections. Saved Scale/Clip
recipes still reopen unchanged, identified as saved legacy methods, until the
user chooses another method. This preserves previous authored files. Reset is
an intentional migration to the new default.

We considered BT.2446, ST 2094 methods, libplacebo's adaptive spline, Reinhard,
filmic curves, and spatial/local operators. Adding a long algorithm list would
not fix poor peak estimates or stale SDR fallbacks. Local operators need more
memory, halo/edge qualification and a spatially consistent export/print path.
They are not exposed as unqualified choices. Destination gamut limiting remains
component limiting after conversion; hue-preserving perceptual gamut compression
is still a limitation, particularly for highly saturated wide-gamut highlights.
The subsequent [highlight-rendering audit](color-management-highlight-rendering.md)
explains the brightness loss in saturated highlights and recommends photographic
highlight color roll-off. That rendering change is not in the current build.

## Interchange and codec boundaries

A gain map describes the relation between a specific edited HDR rendition and
a specific authored SDR base; it is regenerated at the final output size. JPEG
uses one RGB gain image and common channel bounds/offsets for ISO 21496-1 and
Ultra HDR XMP. It is generated against the decoded selected-quality SDR JPEG,
so the ratio does not pretend that the lossy base retained its input samples.
AVIF stores base, independent alpha and a 12-bit gain image in the ISO tone-map
structure. Selected AVIF quality can affect its base; preview decodes the result.
Neither is a lossless Float16 archive.

Both use BT.2020 primaries and an sRGB transfer curve for their SDR base. JPEG
embeds its ICC profile; AVIF uses CICP. This permits the same color application
space in ISO and XMP while retaining wide-color HDR. Legacy color-managed JPEG
viewers display the base; viewers ignoring ICC can show incorrect colors.
Compatibility should not be described as universal sRGB compatibility.

The Linux codec worker uses pinned libultrahdr 2.0.0, libavif 1.4.2, libaom
3.14.1 and libjpeg-turbo 3.1.4.1. Rust owns rendition math, private disk staging,
row/band processing, memory admission, cancellation and atomic publication.
The codec process has address-space, CPU-time, output-file and wall-time limits;
cancellation kills and reaps it. It cannot publish the destination itself.
The bundle must include source archives and licenses. Other hosts reject these
new output types explicitly until their codec integration is qualified.

Import currently supports Ultra HDR JPEG and a constrained AVIF gain-map subset:
still, untransformed, full-resolution maps; sRGB base transfer; matching base/map
application gamut with supported CICP primaries. Unsupported ICC/transform/gamut
combinations fail explicitly. libavif's convenience gain application clamps
linear output, so reconstruction uses the canonical Rust Float32 calculation
before checked half-float storage instead. No silent HDR-to-SDR import.

A `.cube` 3D LUT can represent one fixed bounded transform, but cannot by itself
specify reference white, adaptive headroom, source peak analysis, alpha or a
paired gain-map rendition. ICC HAGC / SMPTE ST 2094-50 carries adaptive gain
curves; ISO 21496-1 carries image-specific gain maps. Neither is an arbitrary
printer profile or a generic creative LUT. LUT/OCIO import stays outside scope.

Validation logs and review qualification are listed in the review README.
Physical HDR luminance, Apple/mobile and Windows HDR output require separate
hardware qualification; passing GPU numerical tests does not replace it.

## Validation evidence

All logs are retained under `artifacts/color-m4/`. The branch includes
`origin/main` through `09285af3` (Apple-only changes after the shared-renderer
merge at `3a7eb637`; they do not alter the tested Linux renderer).

- Shared suites: core **95**, color **83** (+7 ignored fixtures), UI **471**,
  Windows Rust **104** (+1 native D3D test ignored on Linux). The final color
  suite with Linux codecs enabled passes **86**, with 20 opt-in integration
  tests excluded from that default run. Logs: `proof-export-final-shared.log`,
  `proof-export-final-ui.log`, `proof-export-final-windows.log`,
  `proof-export-final-color-heif.log`. The first UI run had one stale format-list
  expectation; the final UI log records the corrected full pass.
- Workspace check and browser Wasm build pass: `proof-export-final-workspace.log`,
  `proof-export-web-build.log`. Browser workspace unit tests pass in
  `proof-export-web-unit.log`. The two renderer contact tests added by the main
  merge pass in `proof-export-main-contact.log`.
- Float64 BT.2390 oracle: `proof-bt2390-reference.log`. GPU view suite **6** pass
  (+1 licensed CMYK fixture ignored), HDR suite **4** pass:
  `proof-export-final-gpu-view.log`, `proof-export-final-gpu-hdr.log`. CPU/GPU SDR
  mapping and mapped print agree on Vulkan for sRGB/ProPhoto, PQ/scRGB surfaces,
  signed highlights and alpha. Float32 PQ tolerance is 0.0004 scRGB / 0.00008 PQ
  code, derived from the independent equation bound and scRGB's 203/80 scale.
- Native workflows: `gainmap-native-final.log` covers JPEG/AVIF recommendations, Auto,
  decoded HDR/SDR previews, flattening, file export and F16 reopening using the
  staged review codec bundle. `proof-live-hdr-final.log` covers editing, history,
  live SDR, save/reopen, HDR/SDR delivery and retained export drafts.
  `proof-live-layout.log` covers compact Paint/Photo panels, floating/tab dragging,
  live edits and Undo. `proof-live-print-final.log` covers CMYK simulation,
  save/reopen, independent RGB delivery and explicit print-profile selection.
  `proof-live-picker-final.log`, `proof-live-cancellation-final.log` and
  `proof-live-preservation.log` cover native profile controls, stale work,
  cancellation, failed validation and preservation of a replaced embedded profile.
- Final native export navigation and range-preflight checks pass in
  `proof-export-navigation-final.log` and `proof-export-preflight-final.log`.
  Repeated fixture exports now clear only their own generated outputs; an initial
  rerun had stopped at GTK's overwrite confirmation. The navigation assertion
  now checks that advanced controls are not mapped on the main page, rather than
  assuming that retained detail-page widgets do not exist.
- `proof-export-desktop/test.log`: the connected Wayland HDR desktop negotiates
  49.261× headroom (compositor reports 10000-nit peak / 142-nit reference; these
  are capabilities, not measurements). The decoded master texture retains red
  4.470145 and GSK renders 4.444563, including the alpha/checker composite.
  Temporary canvas SDR preview does not replace the export master. Simulated
  capability transitions update the export preview correctly. This verifies
  above-white transport, not calibrated emitted luminance.
- `export-gainmap-tests.log`: quality-100 64×48 edited HDR gradient up to RGB 8,
  both JPEG schemas, 300 ppi and AVIF alpha. Maximum absolute HDR channel error
  is 0.1174 for JPEG and 0.0036 for AVIF; alpha error is below 0.0005. These are
  lossy route tolerances, not master precision. `export-gainmap-renditions.log`
  verifies changed recipes regenerate the actual SDR base while preserving HDR,
  and independently forces the ISO and XMP JPEG reconstruction paths to agree.
- `export-codec-cancellation.log`: cancelling an active 2048² AVIF encode kills
  and reaps the codec process and removes private staging in **12.63 ms**.
- Independent SDR decoding: Chrome **152.0.7977.64** matches neutral/dark authored
  JPEG and AVIF bases within two 8-bit codes; AVIF alpha survives. A colored JPEG
  matches the independent Pillow/LittleCMS ICC-aware legacy decode.
  `gainmap-browser-final.log`, `gainmap-interchange/browser-sdr-results.json` and
  `gainmap-interchange/legacy-sdr-reference.json` retain the evidence.
  The accelerated canvas returned transparent pixels even for an ordinary SDR
  JPEG control in this NVIDIA/Dawn test environment. The successful decoder
  checks use `willReadFrequently` CPU canvas. This does not establish physical
  HDR browser presentation. Browser-app HDR rejection and existing SDR/print
  workflows also pass in `proof-export-web.log`.

### 60 MP responsiveness and memory

Single runs of the retained 8192×7324 F16 ProPhoto document with 20 effects,
RTX PRO 6000 Blackwell, NVIDIA 610.57.04, Vulkan, isolated 1600×1000@120 Wayland:

| Preview | Time | Max UI heartbeat gap | Cancel | Process-tree peak RSS | GPU residency |
| --- | ---: | ---: | ---: | ---: | ---: |
| SDR, saved Browser recipe | 8.40 s | 41.35 ms | 144 ms | 1,273,712 KiB | 1890 MiB |
| SDR, new Perceptual default | 10.94 s | 22.45 ms | 133 ms | 1,302,360 KiB | 1894 MiB |
| HDR JPEG, saved recipe | 19.73 s | 33.90 ms | 151 ms | 2,447,972 KiB | 1894 MiB |
| HDR AVIF, saved recipe | 29.62 s | 19.86 ms | 147 ms | 4,246,224 KiB | 1890 MiB |

Logs: `proof-export-large-{sdr,perceptual,jpeg,avif}.log` with corresponding
`.memory.json` and `.time.txt`. The old fixture has no explicit method and retains
Browser; the Perceptual run explicitly replaces its recipe in memory without
rewriting the fixture. JPEG/AVIF codec subprocesses are included in RSS; maximum
private staging is approximately 1.92 / 1.99 GB respectively. Sampling every
0.5 seconds can miss shorter peaks. These are encoded previews followed by
cancellation, **not final 60 MP file-publication tests**. The fixture contains
out-of-range colors; output remains blocked until explicit clipping is chosen.

The previous pre-update SDR baseline was 8.91 s / 19.56 ms gap / 152 ms cancel
and 1,313,440 KiB process RSS (`main-merge-large-sdr.log`). Renderer changes from
main and uncontrolled thermal/power conditions prevent attributing differences
solely to the tone mapper. UI heartbeats are not physical input latency or
120 Hz qualification. Constrained devices still need separate memory budgets.

Physical HDR luminance/calibration, mixed-monitor movement, actual print matching,
touch/pen contacts and Apple/Windows/mobile HDR viewing remain unqualified.
Sharing services may strip auxiliary metadata. This is a review build, not a
cross-platform release-signoff declaration.

## Flat print controls follow-up

Print now uses a native, compact **Profile** menu alongside **Simulate** and
**Intent**. **Black point compensation** and **Gamut warning** remain directly
visible. Profile import and management stay inside the Profile menu. Long names
ellipsize with their full text available as a tooltip. The panel uses common
six-logical-pixel spacing, native dropdowns/checks and the shared flat segmented control;
there is no separate preference card or Options expander.

`layer-ui::proof_panel` owns typed print settings, control order and labels,
intent/simulation choices, absolute-intent/BPC normalization, and SDR numeric
specifications using the existing `NumericControl`. GTK builds native widgets
from those definitions. Profile parsing, validation, library actions and stale
selection handling share one `ProfilePicker`; dialog rows and compact panel
menus are presentations of that picker, not separate implementations. Other
hosts can consume these shared Rust definitions without adopting GTK widgets.
The current change does not claim that their native Proof UIs are already ported.

Follow-up evidence lives in `artifacts/color-m4/proof-flat/`: workspace and wasm32
checks, shared print-option round-trip/BPC tests, native compact Paint/Photo layout,
profile picker, print save/reopen/export, cancellation, source-profile repair and
HDR/SDR editing workflows. Native layout
checks confirm the five print controls are mapped and fit the panel width.

## Proof toggle follow-up

The GTK View menu exposes a checked **Proof** toggle using the existing
Ctrl+Alt+P command. Shared Rust remembers the selected SDR/Print mode separately
from Off, and distinguishes first-profile setup from an active print transform.
Enabling reveals a hidden panel, selects its tab, or opens its collapsed drawer.
Disabling does not reveal it. Pending print preparation is cancelled by Off and
checks the current shared selection again before publishing. Document recipes,
pixels and history are unchanged by comparison toggles.

Evidence is in `artifacts/color-m4/proof-toggle/`: seven shared Proof tests,
workspace and wasm32 checks, the release build, and native workflows for mode
memory/hidden and collapsed panels, cancellation, Paint/Photo panel layout,
print save/reopen/export, and HDR editing/SDR delivery. The old print test
explicitly toggled Gamut Warning after switching print simulation off; that
expectation was updated because unified Proof Off already disables it. The
first failed expectation is retained as `*-old-expectation.log`.

This follow-up changes viewing controls only. Earlier numerical, codec,
large-document and hardware evidence remains applicable to the unchanged
rendering pipeline; these measurements were not repeated for the toggle.
