# GTK milestone 3 implementation and qualification contract

2026-09-16, before production implementation. Status: **in progress**.
Scope is the [GTK handoff](color-management-m3-gtk-handoff.md); phase 2 platform
gaps remain in the [integration record](../history/color-management-m2-port-handoff.md).
Delivery ends with a runnable GTK build and the user's manual review, not inferred
acceptance. Physical print matching and calibrated-display accuracy are unverified.

## Evidence and decisions

- [ICC BPC explanation](https://www.color.org/AdobeBPC.pdf) and
  [ICC WP40](https://www.color.org/WP40-Black_Point_Compensation_2010-07-27.pdf)
  describe black/white endpoint mapping in XYZ. Black-point tags alone are not
  reliable estimates. Validate detected source and destination endpoints separately.
- [LittleCMS 2.16 proof construction](https://github.com/mm2/Little-CMS/blob/lcms2.16/src/cmsxform.c)
  connects source → proof device → relative proof PCS → display. The image intent
  and BPC apply to the first connection, independently of simulation. Its ordinary
  proof helper does not compensate display black; an extended reference chain is
  needed to validate the explicit “simulate black ink off” option.
- [LittleCMS endpoint detection](https://github.com/mm2/Little-CMS/blob/lcms2.16/src/cmssamp.c)
  and [connection rules](https://github.com/mm2/Little-CMS/blob/lcms2.16/src/cmscnvrt.c)
  are the independent numerical oracle. Relative BPC uses measured/estimated
  device black, including CMYK ink limits. V4 perceptual/saturation have a defined
  PCS black and mandatory mapping. Absolute intent has no BPC; reject an explicit
  conflicting request instead of silently changing it.
- [WhiteWall](https://service.whitewall.com/hc/en-us/articles/213813645-Does-WhiteWall-offer-color-management-ICC-color-profiles)
  supplies simulation-only profiles. [Saal](https://www.saal-digital.eu/service/professional-zone/soft-proof-in-lightroom-photoshop-and-other-programs/)
  requests RGB delivery even for some CMYK targets. Therefore importing a proof
  profile never selects a delivery profile or changes document interpretation.

Production dependencies remain portable Rust (`moxcms`); system LittleCMS is used
only by the offline Python validation tool. Inspecting the current adapter confirms
that BPC and non-matrix absolute connections cannot be obtained merely by exposing
existing options. Implement explicit PCS connection policy and validate it before
enabling controls. Keep independent-reference fixtures outside the application.

Pre-integration measurement: the first PCS prototype using moxcms's compiled
profile connections exceeds the declared XYZ/DeltaE and gamut thresholds. Inspection
shows fixed 33-point resampling and forced trilinear interpolation in some PCS
connections, plus automatic perceptual black policy. Revise proof evaluation to
read the original parsed mft1/mft2/mAB/mBA stages, evaluate original device CLUTs with
tetrahedral interpolation (4D: linear first axis, tetrahedral remaining axes), use
trilinear interpolation for Lab-indexed reverse CLUTs as the reference CMM does, and
apply explicit PCS policy. Retain moxcms parsing/curve evaluators. This avoids a
new native dependency and redundant resampling before the measured viewing LUT.
The failed measurements remain in `artifacts/color-m3/references/proof-cmm-*.log`.

Oracle correction from measurement: an unbounded LittleCMS floating multiprofile
matrix round trip cancels the target matrix even for physically impossible device
RGB, hiding its gamut. Reference construction therefore uses two independent CMM
legs with an explicit normalized device-coordinate clamp between them. This is
also used for the reference gamut round trip. It does not change rendering intent,
black-point policy or the predeclared physical-device boundary. Original-table
evaluation already passes all lab LUT cases against the original unsplit oracle;
only the matrix target exposed this fixture limitation.

## Supported contract and transform order

Qualification covers U8 and U16 artwork in sRGB, Display P3, Adobe RGB and ProPhoto.
Proof targets are bidirectional RGB matrix/shaper and RGB/CMYK LUT ICC v2/v4 image
profiles. Missing usable reverse transforms, malformed profiles, device links,
named colors and unsupported channels fail with an actionable error. A parsed
profile is not automatically a usable proof target. Actual profile bytes and CMM
versions are recorded with fixtures; downloadable lab profiles stay under ignored
`artifacts/`, not redistributed with the app.

1. Evaluate the final artwork composition in its working space, before any
   checkerboard, selection, warning or monitor transform. Unassociate coverage.
2. Convert working color through the selected image intent/BPC into the proof
   device. Convert those device values back through its relative colorimetric
   characterization. Clamp device coordinates only at this physical boundary.
3. “Simulate paper” uses absolute media-white scaling and implies black-ink
   simulation. “Simulate black ink” alone keeps relative white and the target's
   black. With both off, compensate target black into viewing black. Display
   conversion is independent of image-to-proof BPC, never a second print BPC.
4. Transform the simulated PCS appearance into the declared managed surface
   (sRGB or Display P3); the existing compositor owns the monitor conversion.
   Reassociate the original alpha, then composite over the unchanged checkerboard.
   Fully transparent pixels show no warning. No simulated value feeds editing,
   sampling, histogram, native saving or delivery export.

Gamut detection uses an independent relative-colorimetric PCS/device round trip,
with a second round trip to distinguish inverse-table error from unreproducible
color. Use CIE76 distance and the LittleCMS threshold of 5, not channel clipping
or difference between normal and perceptually mapped preview. Persist neither
warning pixels nor a gamut raster. Warn for source colors outside the bounded SDR
proof domain; do not silently describe extrapolation as a reliable print match.

## Ownership and GTK workflow

`layer-core`: optional saved proof recipe with exact embedded target bytes,
intent, BPC and simulation options. Recipe edits are document metadata changes;
save/reopen and normal Save As preserve them. Keep export recipes separate.
`layer-color`: validation, PCS/black policy, reusable proof evaluator and LUTs.
`layer-ui`: setup and toggle actions, view state and dirty/history policy.
GTK: View → Soft Proof Setup, Soft Proof, Gamut Warning, conflict-free shortcuts,
profile import/management, accessible controls and visible `Proof: target` state.
Temporary toggles and monitor updates leave document dirty state unchanged.
Preparing/error states must not label an old or ordinary view as the new proof.

Build transforms and bounded LUTs on a worker. Publish a complete result atomically
only if document/working space/recipe identity still matches. Cancel or supersede
pending requests; never queue unbounded profile jobs. Reuse the compiled presenter
shader. Proof changes invalidate the viewing transform only, not source/composite
caches. No per-stroke compilation and no full-resolution proof copy.

The LUT is tetrahedral Float32, 65³ in encoded working RGB, refined to 129³
when the independent off-grid probes require it. Measurement rejected interpolating
the discontinuous gamut score: nine ProPhoto/CMYK cases failed even at 129³.
Instead store RGB appearance and both continuous round-trip distances, and evaluate
the classifier after interpolation. All 36 CMYK preview cases then pass unchanged
tolerances. This costs 5.24 MiB at 65³ or 40.95 MiB at 129³. Revise the GPU bound
to 96 MiB for atomic replacement of active/pending caches; CPU active, pending and
generation scratch remain bounded to 128 MiB. Upload initialized Float32 samples
directly into a shader storage buffer to avoid an extra full-size packing copy.
Extended linear composition must be handled explicitly before lookup. Source
storage, exact editing and export retain the existing precision contract.

## Numerical gates declared before implementation

- Synthetic matrix/media-white/black endpoints: XYZ absolute error ≤ 0.0002;
  alpha bitwise unchanged. U8/U16 identity storage and export remain exact.
- Independent CMM LUT proof: PCS XYZ maximum component difference ≤ 0.02 and
  CIE76 p99 ≤ 2, maximum ≤ 4. Report dark, neutral and saturated subsets separately.
  These are interoperability limits, not perceptual equivalence or print accuracy.
- Derived viewing LUT versus direct production evaluator: display-clipped CIE76
  p99 ≤ 0.5, maximum ≤ 2; neutral encoded error ≤ 1/255. Test interior off-grid
  samples and dark ramps, not only LUT vertices. Refine the method if it fails.
- Gamut decisions must agree with the independent round-trip classifier away
  from a ±1 CIE76 band around its threshold. Report boundary ambiguity explicitly;
  do not count boundary exclusions as overall classification accuracy.
- Include all intents, BPC enabled/disabled where meaningful, three simulation
  states, RGB matrix/LUT and CMYK targets, both depths/all working spaces, alpha
  0/near-zero/fractional/opaque, invalid and unsupported profiles.

## Baseline, final gates and review

Baseline source: `7d2511e5d44e841975f82dec6a42c57b52506e31`. A fresh release GTK
test executable and provenance are retained in `artifacts/color-m3/baseline/`.
The first 61 MP 4K/200% native run presented 957/960 requests (all 3 omissions in
initial fit), zero missed refresh slots, first response 32.998 ms, whole-run
request-to-present p99 7.906 ms. This is baseline evidence, not proof qualification.
Record drawing, pen-up, memory and repeated navigation before rendering changes.
Keep the M2 fixed program baseline and known startup/regeneration limits visible.

Final qualification repeats native navigation with proof enabled, cold and warm,
including 24/45/60 MP and concurrent save/export; measure presented distinct poses,
missed intervals, CPU/GPU times, software input-to-present and peak/steady memory.
Preserve smooth sustained 120 Hz on the reference hardware and investigate an
unchanged-path p95/p99 increase exceeding max(5%, 0.2 ms). Physical input-to-photon
is unmeasured. Dirty-image regeneration and resolution-aware previews stay deferred.

Acceptance also requires exact history/save/reopen, unchanged samples on view
toggles, explicit profiled export without overlays, diagnostics/GPU recovery,
actual GTK setup/compare/edit/Save As/reopen/export with pointer and keyboard, and
an app build identity/manual guide. Preserve the running user's drawings. Commit
significant milestones locally; no other platform or HDR work is authorized here.
