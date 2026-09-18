# Live Proof and HDR export update — 2026-09-18

This supersedes the UI/default decisions in
[color-management-sdr-proof-update.md](color-management-sdr-proof-update.md).
Current code, tests and the review build are the implementation evidence;
this note is not hardware qualification.

## User flow

View → Proof opens a dockable panel beside Color in Paint and Photo layouts.
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

Print offers Printer & paper and Simulate (Colors / Black ink / Paper & ink).
The native Options expander contains intent, black point compensation and gamut
warning. The two simulation effects remain viewing-only. Export → Color &
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

## Checkpoint evidence

- Shared core/color/UI suites: 94 / 83 (+7 codec fixture tests skipped) / 471
  passed before the final peak-analysis addition; the new Float64 shoulder
  oracle also passes. Logs: `proof-export-shared-tests.log`,
  `proof-bt2390-reference.log` under `artifacts/color-m4/`.
- Actual native workflows passed: `gainmap-native.log` (HDR JPEG, transparent
  AVIF, default recommendation, real decoded previews, Auto and reopening),
  `proof-live-print.log` (CMYK print simulation, history, master save/reopen,
  independent RGB delivery), `proof-live-layout.log` (Paint/Photo compact and
  floating panels, immediate tab dragging, live changes and Undo),
  `proof-live-preservation.log` (failed preservation, automatic replacement,
  one embedded profile and a clean-machine reopening).
- `proof-bt2390-gpu.log`: CPU/GPU SDR mapping and mapped print proof agree on
  Vulkan for sRGB/ProPhoto, PQ/scRGB surfaces, signed highlights and alpha. The
  Float32 PQ tolerance is 0.0004 scRGB / 0.00008 PQ code, derived from the
  independent equation bound and scRGB's 203/80 scale. Artwork bytes stay exact.
- `export-gainmap-tests.log`: 64×48 edited HDR gradient up to RGB 8, JPEG and
  alpha AVIF reopen as F16; both JPEG metadata schemas present; 300 ppi survives.
  Quality-100 maximum absolute HDR channel error: JPEG 0.1174, AVIF 0.0036;
  alpha error below 0.0005. These are lossy route tolerances, not master precision.

These initial passes do not replace remaining independent viewer, large-image,
browser and physical-display qualification. Do not treat the checkpoint as a
release-signoff declaration.
