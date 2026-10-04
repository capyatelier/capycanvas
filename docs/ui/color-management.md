# Color-management user journeys and product requirements

[Workspace and UI](README.md) · [Open and import](open-and-import.md) ·
[Color picking](color-picker.md)

These journeys set the product requirements for color management. They start
with the work a painter, illustrator or photographer needs to finish. Existing APIs and GPU
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

**Decisions**

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
A **Blending** field follows them: **Perceptual**, "like Photoshop and Clip Studio
Paint" (the default), or **Linear light**, "physically based". At float depth it
is disabled with "Float documents blend in linear light". Presets and the
defaults remember it; settings saved before it existed use Perceptual.

Document/canvas properties show the profile, depth and Blending. An optional compact status
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
| Web / Share | Tagged sRGB, 8-bit, JPEG, PNG or lossless WebP; dimensions, JPEG quality and transparency/background as applicable. |
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
intent and dithering. Do not put obscure conversion-engine choices in the
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
  Converting to float makes the document blend in linear light in the same step.
- **Blending ▸ Perceptual Blending / Linear Light Blending** changes how layers
  and retouching filters combine colors and how later strokes lay down paint,
  with no dialog and in one undo step. Painted pixels keep their values.
  Both are unavailable at float depth, with the reason above.

Show Before/After, Apply and Cancel for consequential changes. Apply is one undo
step. Routine edits and settings do not trigger repeated confirmation dialogs.

Display handling is automatic where supported, including calibrated profiles and
monitor moves. Show limitations truthfully. Do not offer a monitor profile as a
working-space fix or ask users to configure technical display settings on every
launch.

**Trusting what the screen shows**

The goal is that painters can trust the canvas, and know when they cannot. There
is no display setting, menu item or command. A footer chip speaks only when the
screen matters:

| Situation | Footer |
| --- | --- |
| SDR drawing, every visible color fits the screen | Nothing |
| Visible artwork or proof has colors the screen can't show | **Colors clipped**, with a warning icon |
| Print proof or gamut warning, and the screen may not match the print | **May not match print** |
| HDR drawing, no proof | **HDR**, **SDR preview** or **Showing SDR**, unless colors are clipped |

A screen that can show everything in view, converted by a desktop that knows the
monitor's colors, gets no chip, even while proofing: "Proof: …" already says what
is on the canvas, and a chip that only reports the absence of a problem trains
people to ignore the warnings. The app cannot claim the screen is accurate, only
that nothing it knows of gets in the way.

sRGB follows the platform convention. Where the system sends sRGB to the panel
unconverted (GNOME's default mode, desktops without color management, Android's
Saturated color mode), Capy Canvas does the same, so the canvas matches web pages
and other apps, and that alone is never flagged. The chip speaks only when an
intended wider or different space can't be shown truthfully: colors clip, or a
proof is shown on a screen whose colors Capy Canvas can't tell or whose lightest
tones merge.

Clicking the chip opens a small popover with the screen's name, a one-line
headline and, when it helps, one or two plain sentences: what the painter is
seeing, why, and what they can do about it. It never lists values the app
doesn't know, and uses numbers only inside the sentence that needs them, for
example "Your brightness setting is so high that there's no extra brightness
left for HDR highlights. Lower the screen brightness to see them." **Highlight these colors** appears only while
colors are clipped (or highlighting is on);
it paints them blue on the canvas, distinct from the proof's gray gamut warning.
Highlighting is temporary view state, never saved.

Write this text for a painter, not an engineer, and for every platform:

- Say what they can't rely on on this screen, why in their terms, and the one
  thing that fixes it. Say nothing when nothing needs fixing.
- Name real places: HDR or color settings in the operating system's display
  settings, the screen brightness. The popover shows the next step once that one
  is done.
- Scope every claim to this screen; never imply that HDR or
  proofing is unreliable in general. Don't state consequences that may not
  apply, and hedge only facts the app cannot know.
- Use one word per thing (screen; monitor only to contrast the physical monitor
  with how the system treats it) and only terms
  the app already teaches (sRGB, HDR, ×, EV). Say "your operating system" when
  it is the one doing something, and "Capy Canvas can't tell" when the app lacks
  information, so each sentence is true on every platform, including browsers.
- No metaphors or personification, no invented tasks such as confirming or
  checking elsewhere, and no reassurance nobody asked for, such as "your file
  is unchanged".

"May not match print" means Capy Canvas can't tell how the screen shows the
proof: the screen reports only its signal format (HDR mode) or no gamut, or
white is at the screen's peak so the lightest tones merge. The system's own
description always wins; a monitor's declared values only fill what the system
leaves unknown and are never used to convert artwork.

Shared Rust (`layer_color::screen`, `UiSession::screen_chip` and
`screen_details`) owns the assessment, wording and chip rules. Hosts supply a
`ScreenReport`: the system description of the window's screen and, where
available, the monitor's EDID. Once the view has been idle for 250 ms, the
presenter checks one pixel in every 4×4 block for clipping, after proof
simulation, and never during motion. The check's pipeline compiles on the
background shader compiler, never on the render thread, and each workgroup
records at most one hit. On the tested tablets a check takes 1.3–2.9 ms of GPU
time and at most 2 ms on the render thread; the first check used to compile
there for 187–307 ms and full-resolution checks took 5–25 ms. **Highlight these
colors** still tests every pixel as it is drawn.

- **GTK** reads the compositor's preferred description for the canvas surface
  (`wp_color_management_surface_feedback_v1`) and the monitor's EDID from
  `/sys/class/drm/*-<connector>/edid`, which needs no permission. GNOME 50 reports
  sRGB in its default mode, EDID primaries in its native mode, and only the
  BT.2020 PQ signal in HDR mode; it never uses an assigned ICC profile for its
  conversion. TV EDIDs that report BT.709 while accepting BT.2020 leave the HDR
  gamut unknown.
- **Android** reports the display's name, whether Android offers apps wide
  color (`Configuration.isScreenWideColorGamut`: Display P3 or sRGB), whether it
  supports any HDR type and its desired maximum luminance
  (`ScreenReport::managed`). When Android offers wide color, SDR presents on a
  Display P3 surface and the window uses `COLOR_MODE_WIDE_COLOR_GAMUT` if the
  display prefers 8-bit Display P3, so the canvas and interface agree. Android's
  Saturated color mode disables wide color for every app and sends colors to the
  panel unconverted; the canvas then stays sRGB like the rest of the interface.
  When the display itself is wide gamut and `persist.sys.sf.native_mode` is 1,
  the report sets `wide_color_off`, and **Colors clipped** names the setting that
  limits apps to sRGB. The idle count runs from the 200 ms tone-status poll.
- **Web** reports the `color-gamut` and `dynamic-range` media queries; peak
  brightness is never known. The canvas is sRGB, or extended sRGB for HDR, and
  the idle count runs from the same 200 ms display poll.
- **macOS/iPadOS** report whether the screen shows Display P3
  (`NSScreen.canRepresent(.p3)`, the trait collection's display gamut) and its
  potential EDR headroom, which gives the peak and whether it can show HDR; the
  Mac also reports the screen's name. SDR drawings present on a Display P3
  surface and HDR drawings on extended linear sRGB, and the idle count runs from
  the renderer poll every 250 ms (200 ms for HDR drawings).
- **Windows** can use the DXGI output description and still shows its own HDR
  status.

A new drawing in the same window keeps the screen report and the highlight
choice, and is checked again for clipping.

**8. Edit HDR and provide an intentional SDR version**

**Open HDR → edit → preview SDR rendition → export SDR or supported HDR.**

Preserve supported HDR sources and show whether the view is HDR, mapped SDR or
capability-unknown. Extend existing exposure/curves, picker and histogram controls
above reference white; do not make HDR an unrelated editing application. Users
can edit on an SDR monitor without silently losing HDR source data.

**Proof** is a dockable panel with a common **Off / SDR / Print** segmented
control. In the Paint and Photo
starting layouts it follows Navigator in its tab group. Drag its tab to float or relocate
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
to judge the image. The texture balance uses a bounded local-Laplacian
approximation.

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
tags them correctly. Normal sharing keeps its chosen RGB delivery profile.

HDR documents export **HDR JPEG**, **HDR AVIF with transparency**, **HDR PNG ·
BT.2020 PQ** or **OpenEXR · 32-bit float**; SDR delivery offers PNG, TIFF, JPEG
and WebP. [`export.rs`](../../crates/layer-ui/src/export.rs) owns the valid
combinations. Gain-map outputs regenerate one gain map from the edited HDR and
the authored SDR rendition at the final size, and the SDR base is the Proof
rendition, so the fallback is predictable. JPEG carries ISO and Ultra HDR
metadata for that same map. The HDR/SDR preview shows decoded output and its
actual embedded base. Transparency recommends AVIF; JPEG requires explicit
flattening onto white or black. HDR PNG is a native HDR route with alpha, not a
float archive. Size, Color & transparency and Preset have separate detail pages.

Documents store linear half-float with Float32 processing and a fixed reference
white of 203 cd/m². Unsupported AVIF profiles, transforms and gain-map layouts
fail explicitly.
The footer reports **HDR**, **SDR preview** or **Showing SDR**; click it for the
screen details described above. New Drawing offers an **HDR drawing** preset.
Only HDR documents show the colored intensity arc below the hue ring.
Double-click resets it to 1× (0 EV); the angled EV caption is read-only. The
upper-right pencil, or a double-click on either paint bubble, opens Edit Color,
whose rows show the base color with an editable Intensity (EV) row, and whose
Current and New compare the full colors. +2 EV
multiplies the circle/square/triangle field and paint bubbles by four in linear
light; black stays black and alpha is unchanged. Hue and field edits retain the
chosen intensity, and EV edits retain the marker. The field, ramp and bubbles
use managed half-float display textures on a capable GTK display, or the
shared SDR appearance on an SDR-only display. The hue guide stays an SDR
reference. SDR documents retain the existing picker/layout.
Curves and the histogram mark SDR white. HDR curves use a log (EV) axis from
8 stops below white to the HDR range above it, with a linear toe through zero.
The curve space and HDR range sit directly below the curve.
Clicking adds a point. Dragging a point off the graph removes it (it returns if
dragged back before release), as does double-clicking or double-tapping it.
Once a channel is edited, a reset icon in the graph's bottom-right corner
restores it. Curves have no separate point buttons.

Export's overview chooses **Dynamic range: SDR or HDR** and format. Size,
Color & transparency and Presets open focused panels with a Back action and
summaries on the overview. SDR Appearance is reachable for SDR delivery. Native
preset actions use standard rows and name their target; unavailable controls
are absent. JPEG offers opaque backgrounds and hides its fixed 8-bit depth.
**WebP · lossless** is 8-bit RGB that keeps transparency; gray and CMYK
delivery profiles leave it out. Its encoder writes at most 16,384 pixels per
side, so a larger output size disables Export with that reason on GTK; the Web
and Android dialogs show the same reason when Choose File… is pressed, before
the save picker opens or any pixels render.

**Metadata** decides which of an opened photo's descriptive metadata the copy
keeps: **All** (the default), **Copyright & Contact** or **None**. With All,
**Remove location** is on by default and leaves out GPS coordinates and place
names; camera, lens, exposure, dates, artist and copyright stay. Copyright &
Contact keeps the artist, copyright, creator, rights and contact details only.
File paths, edit history and raw-development settings never travel, and the
orientation, pixel dimensions and print density are written fresh for the
delivered image. JPEG, PNG, TIFF, WebP, HDR PNG and the HDR JPEG and AVIF gain-map
files carry the kept Exif and XMP; HDR JPEG merges the XMP into its own gain-map
packet. OpenEXR carries none, and the dialog says so. A JPEG whose kept metadata
does not fit one marker segment is refused with a reason that names the smaller
choices. The row appears only for documents opened from a photo, and presets
remember the choice. The shared view data is
[`ExportMetadataView`](../../crates/layer-ui/src/export.rs); what is read on open
is in the [project format](../reference/project-format.md#photo-metadata).

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
measurement of monitor luminance.

“16-bit float” is not a promise that all HDR workflows or 32-bit source values fit. Full 32-bit float, scene-based
VFX/OCIO and specialist EXR processing remain separately scoped.

**Settings, scope and delivery**

Add a small **Preferences → Color** page: defaults/presets for New, **Open images:
Preserve source depth/profile** (default), an optional promote-to-16-bit photo
policy, missing-profile handling and profile import/management.
Proof and output presets belong beside those tasks. Valid embedded profiles
should not produce routine mismatch warnings; an advanced ask/convert policy can
serve users who deliberately want a fixed working space.

Document color and source settings are saved with the artwork. Live adjustments
and accepted color conversions participate in document history. Saved proof
recipes and SDR renditions are document metadata. View toggles, monitor changes
and temporary export choices do not dirty the artwork. Preferences apply to new
work, not silently to every open document.

RAW development and layered exchange are important future photography/illustration
work, with their own feature and format contracts. In the first release, a photo
can be developed externally and handed off as a profiled 16-bit TIFF. Do not
market that boundary as a complete RAW editor. Standard external-format support
is interoperability; it does not require an old Capy project importer.

Efficiency is a user requirement throughout: smooth interaction, responsive
previews, manageable large-photo/layer memory, background saves and bounded undo.
Use sharing, caching and bounded processing before reducing user data. A storage
size ratio is not a measured speedup or whole-application memory ratio.

Web uses extended-range `rgba16float` WebGPU canvases when the browser accepts
extended tone mapping and the display reports HDR support. Canvas and Navigator
pass signed, above-white sRGB values to the browser; the browser and system
choose display brightness. Web admits HDR documents up to 12 MP and rejects
larger ones while keeping the open artwork. Android 15 and later use a
floating-point BT.2100 PQ surface for HDR artwork with Proof Off when both the
HDR10 display and the Vulkan format support it; Android chooses brightness and
tone mapping, and reported headroom does not precompress the artwork. SDR, Print,
gamut warning and appearance drafts use the shared SDR mapping on the SDR
surface, Display P3 when Android offers wide color and sRGB otherwise, and
unsupported hosts keep mapped SDR. PQ output is bounded to BT.2020
and 0–10,000 cd/m² at the 203 cd/m² reference white. Color controls and layer
thumbnails remain SDR previews. The left footer chip matches the zoom readout
and opens the screen details described above; **HDR** describes the active
output route, not measured screen brightness.

## Export Again

**File → Export Again** sits beside Export and becomes
available after a successful export from that drawing. It exports the current
artwork with the last successful recipe and destination, without reopening the
options dialog. Each open drawing remembers its own export for the session;
closing and reopening the drawing starts fresh. There is no default shortcut.

If the previous file is missing or access has expired, the existing file chooser
asks for a destination. Browsers without reusable file access repeat a named
download and its confirmation; the iPad export picker grants no lasting access,
so it asks again with the remembered name. Cancellation or failure keeps the previous
successful recipe and destination. Export Again does not save the editable
master, change its location or clear unsaved edits, and refuses that master as
its output destination.
