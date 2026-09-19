# Color-management user journeys and product requirements

[Workspace and UI](README.md) · [Implementation plan](../history/color-management-research.md) ·
[Delivery milestones](../history/color-management-milestones.md)

**Reassessed proposal, 2026-09-13; not implemented.** Start with the work a
painter, illustrator or photographer needs to finish. Existing APIs and GPU
formats determine implementation effort; they do not determine which user
workflows deserve support. The new project format stores editable raster content;
old Capy projects and legacy code paths are not supported.

**What users should be able to rely on**

A drawing or photo should open with the intended colors, remain editable without
unnecessary loss, look consistent across the app, and produce a correctly tagged
file for its destination. Common work should need no visit to Preferences.
Color management cannot make an uncalibrated screen accurate, recover clipped
source data, or control a receiving app that ignores/removes color metadata.

The priorities here are recommendations based on the audience and documented
workflows. Public manuals do not provide usage percentages. In particular, they
support a distinction between daily editing/delivery and occasional configuration;
they do not establish that every photographer needs the same default depth.

| Evidence | Product implication |
| --- | --- |
| Procreate exposes sRGB/P3 canvas profiles and profile import. [Handbook](https://help.procreate.com/procreate/handbook/colors/colors-profiles). | Make wide-gamut drawing a straightforward creation choice. |
| Affinity honors an opened image's profile and converts placed images to the document space. [Color management](https://affinity.help/photo2/English.lproj/pages/Clr/ClrProfiles.html). | Open and Place need different automatic policies. |
| GIMP distinguishes 8/16-bit integer storage from floating point and from processing precision. [Encoding](https://docs.gimp.org/3.0/en/gimp-image-encoding.html). | Keep gamut, depth, HDR range and internal arithmetic separate. |
| Lightroom Classic recommends 16-bit ProPhoto for detailed SDR handoff to another editor. [External editing](https://helpx.adobe.com/lightroom-classic/desktop/work-with-external-editors/external-editing-preferences.html). | A developed 16-bit photo must have a reliable editing and return-export route. |
| Photoshop offers reversible adjustment layers and both point and averaged color sampling. [Adjustments](https://helpx.adobe.com/photoshop/using/nondestructive-editing.html), [sampling](https://helpx.adobe.com/photoshop/desktop/adjust-color/choose-colors/set-foreground-and-background-colors.html). | Normal photo editing needs reversible corrections and useful inspection, not just profile dialogs. |
| WhiteWall supplies profiles specifically for proofing and instructs users not to convert/embed them in delivery files. [Lab instructions](https://service.whitewall.com/hc/en-us/articles/213813645-Does-WhiteWall-offer-color-management-ICC-color-profiles). | Store proof and delivery targets separately; obey the lab's actual specification. |
| Lightroom separates HDR editing from the SDR rendition used for viewing/export. [HDR workflow](https://helpx.adobe.com/lightroom/desktop/edit-photos/hdr-output.html). | HDR needs intentional SDR delivery and useful viewing on SDR hardware. |

**Decisions corrected by the reassessment**

- 8-bit is a standard editing option, including in Display P3 and Adobe RGB SDR
  documents. Wide gamut does not require float storage or HDR. Higher precision
  remains useful for gradients, repeated edits and very wide spaces.
- Ordinary SDR **16-bit** means integer precision; **16-bit float** is a separate
  range/precision tradeoff. Keeping an original 16-bit file does not make a
  lower-precision edited result lossless. Do not disguise half-float as ordinary
  16-bit photographic editing.
- Opening a JPEG does not inherently require promotion. Preserve source depth
  and profile by default. Offer higher precision for subsequent edits and make
  automatic promotion an explicit preference, not a mandatory cost.
- Reversible adjustments, undo and retained originals are different capabilities.
  Users need to reopen and change an adjustment, not merely recover an original.
- A proof profile is not automatically an export profile. RGB print delivery and
  CMYK simulation can coexist in the same workflow.
- “Linear” is not a universal promise of familiar artistic blending. Blend and
  adjustment behavior must be deliberate and remain stable when depth changes.
- RAW development and layered exchange are real future needs for parts of this
  audience. They need separate scoped work, not dismissal as irrelevant because
  the current engine is a drawing renderer.

**1. Create a drawing — essential**

**New → choose preset/size → Create → paint.**

Keep the first screen small: size, background/transparency and a preset. The
standard drawing preset is **sRGB, 8-bit**. A **Wide color** preset uses **Display
P3, 8-bit**. A photo-editing preset recommends **16-bit SDR** and shows its chosen
profile. Saved user presets keep these choices explicit and repeatable.

Under **Color**, provide independent **Color space** and **Bit depth** fields:
8-bit and 16-bit SDR for supported RGB spaces; Adobe RGB and ProPhoto are useful
advanced photographic spaces. Recommend 16-bit for ProPhoto. HDR creation is a
separate later preset, not the consequence of selecting P3 or increasing depth.

Document/canvas properties show the profile and depth. An optional compact status
item is a shortcut; the workflow does not require another permanent toolbar.
Users on an sRGB display can still create P3 artwork. Explain incomplete display
gamut when relevant without preventing editing or discarding those colors.

**2. Open a photo or place a reference — essential**

**Open → edit**, or **Import Image as Layer / Paste → position → edit**.

Open JPEG, PNG and TIFF with their supported embedded profile and source depth.
An ordinary 8-bit JPEG remains 8-bit; a 16-bit TIFF enters the 16-bit SDR path.
A developed ProPhoto TIFF can be edited and handed back without a forced sRGB
conversion. Supported HDR content takes its separate HDR path. Format-specific
encoding/profile interpretation is automatic; the user sees a meaningful profile
and depth, not decoder or GPU terminology.

Placing or pasting into a document preserves its existing color space. Convert
incoming colors appropriately. Keep the source's higher-depth/wider-gamut data
when it is a retained image layer; display it through the destination's mapping.
Do not silently reduce that source to the canvas depth before the user rasterizes
or merges it. Direct raster paste follows the destination format; disclose a
material range/depth reduction and offer to retain an image layer or raise depth.

Valid tagged images need no mismatch dialog by default. For an ordinary untagged
SDR RGB image, assume sRGB and record **Profile assumed: sRGB** in image details;
a quiet Review notice can explain it once. Ambiguous CMYK, corrupt profiles and
unidentified HDR need a specific interpretation choice or error. File import,
image paste and supported drag/drop follow the same policy.

**3. Correct a photograph and revisit the edits — essential**

**Adjustments → correct tone/color → mask locally → compare → Save → reopen.**

Provide exposure, white balance/tint or a neutral-point control, Levels/Curves,
hue/saturation and color balance as editable adjustments, with masks, bypass,
reset and before/after comparison. These controls use the document's color
meaning consistently. A white-balance control on a developed JPEG/TIFF is a
rendered-image correction, not a promise of RAW recovery.

A histogram and clipping indicators belong in ordinary SDR photo editing.
Allow RGB/luminance inspection and clearly state whether the histogram describes
the composite, selected layer or an output preview. Document inspection remains
independent of monitor/proof transforms. Persist adjustment settings so users can
change them after reopening; slider edits do not repeatedly bake new pixels.

**Change Bit Depth…** lets an 8-bit user choose 16-bit before demanding work.
The concise explanation is that it improves subsequent editing precision and
does not restore detail already lost. A document's bit depth must not silently
change the look of an otherwise unchanged blend/adjustment stack.

Keep painting/retouching available on separate raster layers over a retained
photo. Rasterize/merge is an explicit commitment. Source retention alone is not
marketed as nondestructive editing; undo history alone is not the saved editable
adjustment model.

**4. Choose and reuse precise colors — essential**

**Color panel → sample or enter a value → save a swatch → reuse it.**

Retain the compact picker. Add a directly reachable **Edit Color…** popover or
sheet with labeled RGB, hex and familiar hue-based input. Keep OKLCH available;
additional specialist readouts can live under advanced choices. Hex entry defaults
to clearly labeled **sRGB hex**; document RGB input names its space so numbers
copied from another application are not silently reinterpreted.

The eyedropper needs **Current layer / Visible composite** and **Point / Average**
(e.g. 3×3 and 5×5) in tool options. Average sampling is useful for noisy/textured
photos. It samples artwork, never checkerboards or proof-warning colors. Picking
paint color keeps brush opacity independent; an inspector may separately show
pixel alpha. Define averaging and fully transparent sample behavior consistently.

Swatches and palettes carry color definitions and convert appropriately between
documents. The picker, swatches, previews and canvas agree. An out-of-display-gamut
indicator means the color cannot be shown fully on this monitor; it must not
silently clip the stored choice. Output-gamut warnings are explicitly tied to a
selected output/proof target. Numeric readouts never depend on monitor calibration.

**5. Save the master and deliver an image — essential**

**Save** stores the editable raster project, layers, masks, color definitions and
live adjustments. Reopening must not reinterpret artwork according to the new
machine's defaults. Opening a source photo does not give Save permission to
replace that photo with a project. Use Save As for a separate editable version.

**Export → choose destination preset → preview → choose file.** Use one export
sheet with a compact primary view and advanced options:

| Preset | Main choices |
| --- | --- |
| Web / Share | Tagged sRGB, 8-bit, JPEG or PNG; dimensions, JPEG quality and transparency/background as applicable. |
| Wide-color image | Tagged Display P3 with supported JPEG/PNG encoding; preview the output rather than requiring conversion of the master. |
| Further editing | 16-bit SDR TIFF or PNG, with explicit profile; ProPhoto is available for an appropriate photographic handoff. |
| Custom / lab preset | Explicit format, size, profile, depth and transparency handling; advanced conversion settings and a reusable named preset. |

Output choices transform a copy of the composition. Selecting Web / Share does
not convert the master to sRGB or lower its depth. Embed matching color metadata
by default; profile choice must transform pixels, not just attach a new tag.
JPEG flattening shows its background. Resizing and precision reduction are
visible choices; exporting never clears unsaved-master state.

The preview represents the selected output, with an accessible before/after
comparison. It need not re-encode a full-size JPEG on every interaction; disclose
if the preview excludes compression artifacts. Advanced controls expose applicable
intent, BPC and dithering. Do not put obscure conversion-engine choices in the
normal flow. Remember per-destination presets without globally changing New/Open.

For handoff, verify the result in another editor with its profile and 16-bit data
intact. TIFF/PNG are the initial rendered-image route; they do not promise editable
layer exchange. Add separately scoped layered PSD interchange to the roadmap.

**6. Preview a print and deliver what the lab requests — expected print support**

**Proof → Print → compare → make optional print edits → Export.**

Select/import a printer/paper ICC profile and keep proof intent, BPC and paper/ink
simulation together. **Off / Print** selects the view; **Gamut warning** is directly visible in
Print, with an obvious **Proof: [target]** indicator. Compare with the normal
view without editing pixels. Use Save As or normal document duplication for a
print variant; a bespoke virtual-copy system is not a prerequisite.

Keep **Proof target** and **Delivery profile** separate. Importing a printer
profile changes neither document interpretation nor export settings. The lab
preset records the supplied requirements: it may simulate a CMYK device while
sending a tagged sRGB/Adobe RGB image. Only choose device-profile conversion or
CMYK output when the destination explicitly calls for it. A **Use proof profile
for delivery** action must be deliberate, not the default meaning of Export.

WhiteWall's service instructions explicitly prohibit using its supplied proof
profiles for file conversion/embedding; Saal likewise requests RGB delivery
rather than its proof profile. Follow actual service specifications over generic
print advice. [WhiteWall](https://service.whitewall.com/hc/en-us/articles/213813645-Does-WhiteWall-offer-color-management-ICC-color-profiles),
[Saal](https://www.saal-digital.eu/service/professional-zone/soft-proof-in-lightroom-photoshop-and-other-programs/).

Paper simulation and gamut warnings never enter output pixels. Proofing needs
calibrated viewing to be useful and cannot guarantee a physical match under all
lighting. Native CMYK painting, separations, spot inks, page layout and direct
printer-driver integration are distinct future workflows, not prerequisites to
RGB lab delivery and soft proofing.

**7. Resolve an unexpected color change — essential escape hatch**

**Document Color… / image properties → inspect → choose a specific correction.**

Show the document profile/depth, source profile or assumption, active proof state,
export destination and display status. This helps distinguish a wrongly tagged
image from a wrong export setting or a viewing limitation.

- **Assign Profile…** corrects interpretation while preserving the declared RGB
  numbers. Appearance may change. Internal storage conversion is an implementation
  detail; do not define this action as “keep GPU bytes unchanged.”
- **Convert Color Space…** changes RGB numbers to preserve appearance as far as
  destination gamut and precision allow. Preview the complete result. If editable
  layer conversion changes blend/effect appearance, disclose it and offer an
  explicit flattened copy; ordinary export does not need document conversion.
- **Source Color Profile…** corrects a retained image's original interpretation.
  If pixel edits were already baked, keep them and offer a corrected source as a
  new layer. Do not claim that replay can recover arbitrary edited pixels.
- **Change Bit Depth…** is separate from profile conversion. Preview reductions
  and keep one-step undo. No silent depth/range reduction under memory pressure.

Show Before/After, Apply and Cancel for consequential changes. Apply is one undo
step. Routine edits and settings do not trigger repeated confirmation dialogs.

Display handling is automatic where supported, including calibrated profiles and
monitor moves. **Display Details…** reports active management and capabilities;
show limitations truthfully. Do not offer a monitor profile as a working-space
fix or ask users to configure technical display settings on every launch.

**8. Edit HDR and provide an intentional SDR version**

**Open HDR → edit → preview SDR rendition → export SDR or supported HDR.**

Preserve supported HDR sources and show whether the view is HDR, mapped SDR or
capability-unknown. Extend existing exposure/curves, picker and histogram controls
above reference white; do not make HDR an unrelated editing application. Users
can edit on an SDR monitor without silently losing HDR source data.

**Proof** is a dockable panel with a common **Off / SDR / Print** segmented
control. In the Paint and Photo
starting layouts it shares Color's tab group. Drag its tab to float or relocate
it using the normal workspace controls. SDR documents offer Off / Print.

For HDR artwork, **Proof → SDR** uses a circular control. Its glass-like field
shows the directions: blended on the left, fine ripples on the right, stronger
contrast above and gray below. The **center is the balanced baseline**, at 100%
contrast and 0% relative texture balance. This baseline includes the reviewed
130% contrast / +30% fine-texture treatment.
Move **up** for more contrast (up to 200%), or **down** for less (down to 50%).
Move **left** to favor broad shapes and lighting (**macro**), or **right** to favor
fine texture (**micro**). At centered balance, both change together. Range fitting
stays fixed while adjusting the circle, so more contrast does not turn back into
more compression at the top.

The **top arc** adjusts Brightness from −50% to +50%, keeping black and white fixed. The **bottom
arc** adjusts Color intensity: left lets bright highlights become white; right
retains more color by lowering their brightness. Their ramps show dark-to-light
and white-to-color respectively. Color intensity defaults to **30%**.
Small icons identify each percentage; the side values are horizontal with their
icons above, while the arc values follow their tracks. Control names and keyboard
guidance remain available to assistive technology. There are no tooltips, labels
or Auto button in the dial.

Drag, use arrow keys (Shift for larger steps), or double-click a control to reset
it. Escape cancels an adjustment. The small refresh icon resets all appearance
controls while preserving the stored HDR range. Reset returns the circle to its
center. There are no older algorithm modes or custom-recipe placeholders.
The glass field is a fixed direction guide. Use the canvas and export preview
to judge the image. The
[algorithm, fixtures and qualification notes](../history/color-management-local-tone.md)
explain the bounded local-Laplacian approximation and its limits.
The [circular-control review](../history/color-management-proof-dial.md) records
its original geometry. The [contrast update](../history/color-management-contrast-dial.md)
records the contrast model. The [Proof polish review](../history/color-management-proof-polish.md)
records the centered treatment, input fix and current validation.

Changes are live, saved document edits, with one undo step per slider gesture.
**Off** restores normal viewing without discarding the saved SDR rendition.
These settings are shared by SDR viewing, delivery and mapped print proofing;
they never change HDR artwork. **View → Proof** (Ctrl+Alt+P) toggles Off and the
last selected SDR/Print mode, revealing the panel when enabling. On first use,
HDR artwork selects SDR; SDR artwork opens Print setup. Document Properties can
also open the panel. Adjust the rendition before opening **Export image**, which
uses the saved settings without a separate Proof section. There is no Apply/Revert,
Preview checkbox or overflow menu beside the selector.

**Proof → Print** has a compact **Profile** dropdown, **Simulate**, **Intent**,
**Black point compensation** and **Gamut warning**. All five controls are visible
in one flat panel, with common row spacing and no Options fold. A changed profile
prepares asynchronously with cancellation
and retains the previous recipe if validation fails. View simulation and gamut
warnings never enter artwork or export. The master saves one print recipe;
Export's deliberate **Use print profile** action converts delivery pixels and
tags them correctly. Normal sharing keeps its chosen RGB delivery profile. See
the [mapping research and validation](../history/color-management-proof-export-update.md).

Export offers **HDR JPEG**, **HDR with transparency · AVIF**, **HDR native · PNG**,
or **SDR** in one Output selector. The pinned Linux codecs regenerate one gain
map from edited HDR and authored SDR at the final size; JPEG carries ISO and
Ultra HDR metadata for that map. The HDR/SDR preview shows decoded output and
its actual embedded base. Transparency recommends AVIF; JPEG requires explicit
flattening. Size, Color & transparency and Preset have separate detail pages.

GTK's review implementation uses linear half-float storage with Float32
processing and fixed reference white of 203 cd/m². It supports noninterlaced
16-bit PQ PNG input and BT.2020 PQ PNG output, Ultra HDR JPEG and a constrained
gain-map AVIF route, plus authored SDR PNG/TIFF/JPEG. Unsupported AVIF profiles,
transforms and gain-map layouts fail explicitly. Other hosts reject the new
gain-map output choices until their codec integration is qualified.
The footer reports **HDR**, **SDR preview** or **Showing SDR**; click it for display
and reference-white details. New Drawing offers an **HDR drawing** preset.
HDR Edit Color opens in **Linear RGB**, accepting above-white and negative
values. Only HDR documents show the colored intensity arc below the hue ring.
Double-click resets it to 1× (0 EV); the angled EV caption is read-only. The
upper-right pencil, or a double-click on either paint bubble, opens Edit Color,
which includes editable EV and side-by-side Base / Adjusted previews. +2 EV
multiplies the circle/square/triangle field and paint bubbles by four in linear
light; black stays black and alpha is unchanged. Hue and field edits retain the
chosen intensity, and EV edits retain the marker. The field, ramp and bubbles
use managed half-float display textures on a capable GTK display, or the
shared SDR appearance on an SDR-only display. The hue guide stays an SDR
reference. SDR documents retain the existing picker/layout.
Curves and the histogram mark SDR white; curve processing options are in Advanced.

Export's overview chooses **Dynamic range: SDR or HDR** and format. Size,
Color & transparency and Presets open focused panels with a Back action and
summaries on the overview. SDR Appearance is reachable for SDR delivery. Native
preset actions use standard rows and name their target; unavailable controls
are absent. JPEG offers opaque backgrounds and hides its fixed 8-bit depth.

HDR exposes its fixed PNG / BT.2020 PQ / 16-bit / retained-transparency contract.
On a capable display, Export shows the HDR master beside the selected HDR or
SDR output. Master viewing ignores the temporary canvas SDR/proof toggles and
the saved SDR rendition. Unsupported displays show explicitly labeled SDR
previews. Display changes update the open preview. A cancellable full-size range
check gates export;
when it finds unsupported values, **Clip out-of-range colors** appears beside
the warning. The writer checks again. SDR files use the saved SDR appearance.

GTK negotiates scRGB or parametric BT.2020 PQ on a floating surface and responds
to compositor feedback even while idle. Promoting an existing SDR drawing does
not require reopening it. The displayed headroom is a compositor hint, not a
measurement of monitor luminance. Other hosts explicitly reject HDR masters.
See [the feedback validation](../history/color-management-gtk-m4-feedback.md)
for the current workflow, review runtime and remaining hardware qualifications.

“16-bit float” is not a promise that all HDR workflows or 32-bit source values fit. Full 32-bit float, scene-based
VFX/OCIO and specialist EXR processing remain separately scoped.

**Settings, scope and delivery**

Add a small **Preferences → Color** page: defaults/presets for New, **Open images:
Preserve source depth/profile** (default), an optional promote-to-16-bit photo
policy, missing-profile handling, profile import/management and display details.
Proof and output presets belong beside those tasks. Valid embedded profiles
should not produce routine mismatch warnings; an advanced ask/convert policy can
serve users who deliberately want a fixed working space.

Document color and source settings are saved with the artwork. Live adjustments
and accepted color conversions participate in document history. Saved proof
recipes and SDR renditions are document metadata. View toggles, monitor changes
and temporary export choices do not dirty the artwork. Preferences apply to new
work, not silently to every open document.

Menu placement proposal: **File** for New/Open/Import/Save/Export and Document
Color; **Adjustments/Filter** for editable corrections; **View** for Histogram,
the unified Proof panel; **Color panel / tool options** for numeric entry and
sampling. The same actions need explicit touch and keyboard access. Exact native
placement can adapt without altering meaning; no flow depends on existing menu
or transport limitations.

Deliver journeys 1–5 and 7 as the core SDR release, with 8/16-bit SDR editing,
profile-aware P3/Adobe RGB/ProPhoto handling, calibrated viewing, reversible photo
adjustments and reliable interchange. Print proofing (6) is the next expected
prosumer milestone; HDR (8) follows as a separate complete workflow.

RAW development and layered exchange are important future photography/illustration
work, with their own feature and format contracts. In the first release, a photo
can be developed externally and handed off as a profiled 16-bit TIFF. Do not
market that boundary as a complete RAW editor. Standard external-format support
is interoperability; it does not require an old Capy project importer.

Efficiency is a user requirement throughout: smooth interaction, responsive
previews, manageable large-photo/layer memory, background saves and bounded undo.
Use sharing, caching and bounded processing before reducing user data. A storage
size ratio is not a measured speedup or whole-application memory ratio.

Validate with task sessions covering ordinary sRGB drawing, 8-bit P3 painting,
16-bit ProPhoto photo edits and handoff, an untagged reference, transparent color
sampling, lab proof-only profiles and SDR/HDR delivery. Users should complete
ordinary tasks without visiting global settings or confusing Assign with Convert.
Check cancelled operations, reopen/editability, exported color/depth and work on
multiple displays. Numerical/performance tests remain in the implementation plan;
manual availability is evidence of expectations, not a substitute for these checks.


Web and Android's [phase-4 integration report](../development/color-management-web-android-m4.md)
records their supported HDR editing and delivery routes. Both reuse the GTK
picker placement and shared Proof dial; displays currently show mapped SDR.
Web admits HDR documents up to 12 MP and rejects larger ones while retaining the
open artwork. Neither host currently offers gain-map output or physical HDR
presentation. The report distinguishes implemented controls from outstanding
workspace projection and hardware qualification.
