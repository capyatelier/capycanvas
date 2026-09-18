# Phase 4 UI review and corrections

The findings below record the original review. The user subsequently requested
implementation. Corrections and current qualification are recorded in
[the UI validation report](../history/color-management-gtk-m4-ui-validation.md).

2026-09-17. Reviewed checkout `87a361cf`, the shared command/form models, GTK
adapters and the captured GTK journey screens. Compared the proposed interaction
with current GNOME, Lightroom and Affinity documentation. This is a code and
captured-screen assessment, not a new physical-device usability qualification.
No UI changes are part of this review.

The HDR data path has useful capabilities, but its current presentation needs
another pass. Several issues are behavioral or misleading, not just verbose
labels. Keep the HDR master, temporary display preview and saved SDR appearance
as distinct concepts, with their controls located where users need them.

## Findings, in priority order

### 1. HDR export exposes choices it does not honor

GTK exposes Delivery profile, integer SDR bit depth, transparency/background and
advanced conversion options alongside its two HDR PNG entries. The shared draft
normalizes HDR to its fixed PQ delivery contract, overriding profile, depth,
background and encoding. Its returned choice lists still describe SDR choices.
The form can therefore display a selectable value that is not the output value.

This is a release-blocking UI correctness issue. The form must show the actual
output contract. For the current route: HDR PNG, BT.2020 PQ, 16-bit, retained
transparency. Fixed properties are concise information, not editable choices.
Keep supported resizing; do not generalize that all output settings are ignored.

Evidence: `apps/layer-linux/src/files/export.rs:323`, `:403`, `:658`;
`crates/layer-ui/src/export.rs:288`.

### 2. The HDR export preview overstates what it represents

The comparison is labeled Master / Output, but HDR recipes bypass output
simulation and show the document's mapped SDR preview. Strict and clamped PQ
exports therefore do not have a corresponding visual comparison. The explanatory
text is only made visible for JPEG, despite containing the HDR limitation.

Label the image "SDR preview" when that is what it is. Show target-specific range
validation before the final export, with an actionable error when strict output
cannot represent the image. A bounded asynchronous preflight must invalidate on
relevant edits and never certify a stale result. Keep the final writer check.
An HDR preview need not claim physical HDR accuracy to be useful, but it must
identify its limitations at the point of use.

Evidence: `apps/layer-linux/src/files/preview.rs:24`, `:100`;
`apps/layer-linux/src/files/export.rs:748`.

### 3. The HDR histogram has an SDR axis

The data bins cover negative/nonpositive values and -12 to +16 stops, but GTK's
horizontal axis remains 0 at the left and 1 at the right. The tooltip still
describes encoded RGB. The separate "Log scale" option changes count height,
not the already logarithmic HDR brightness axis.

Show a clear SDR-white divider, labeled stops above white and a useful fitted
range. Give nonpositive values a separate, explicit count. Label any optional
vertical logarithmic scale as count scaling. Move the technical paragraphs and
most statistics into Details. Above white is valid HDR, not a clipping error.

Evidence: `crates/layer-core/src/color/histogram.rs:93`;
`apps/layer-linux/src/histogram.rs:197`, `:274`, `:283`, `:376`.

### 4. SDR appearance cannot be adjusted while judging the picture

The current SDR Rendition alert covers and dims the artwork, contains a long
explanation, and offers three spin rows. There is no live preview, reset or
before/after comparison. The recipe is read and committed only after Apply.
Users must apply, inspect, reopen and repeat.

Use one compact SDR Appearance editor with live preview, familiar sliders plus
numeric entry, Reset, comparison and Apply/Cancel. Suggested controls: Exposure
(EV), Contrast and Highlights. The last control needs a perceptually meaningful
direction/range; simply renaming the internal knee is not sufficient.

Opening the editor temporarily shows the SDR appearance. Cancel restores its
prior state; Apply saves one undoable document change. Restore the prior display
mode when leaving. Keep draft preview separate from the saved document recipe.
One short line is enough: "Used for SDR viewing, export and print proofing."

Evidence: `apps/layer-linux/src/hdr.rs:23`, `:40`, `:48`;
`artifacts/color-m4/journey-final/sdr-rendition-controls.png`.

### 5. Menu placement mixes a view toggle with a saved document edit

View currently places SDR Rendition and Preview on SDR Display immediately after
Histogram, before proofing and ordinary navigation. Both remain visible but
disabled in SDR documents. The preview enablement only checks HDR document/idle
state, so on the qualified SDR display it changes the checkmark/status without
changing the picture. During proofing, the proof already uses mapped SDR.

Keep **View → Preview SDR**, near Soft Proof, as a checked temporary view action.
Show the effective viewing state; distinguish a user's requested preview from a
display that already requires SDR. Do not offer a misleading comparison while
proofing or when HDR presentation is unavailable. Explain that state briefly in
the associated control/details, without preventing editing of the SDR appearance.

Give the saved **SDR Appearance** editor a home in Document Properties and a
direct route from SDR Export and proof setup. Reuse one editor and transaction.
An export must not silently modify the saved recipe: accepting an appearance
edit is a separate, explicit undoable action. Avoid nested alert dialogs.

Evidence: `crates/layer-ui/src/lib.rs:203`;
`crates/layer-ui/src/application_menu.rs:154`;
`crates/layer-ui/src/session.rs:1760`, `:3421`.

### 6. Color entry is technically capable but poorly introduced

Edit Color always starts in Document RGB. The Linear RGB / HDR choice is inside
the model list and is advertised by the shared form even on hosts that reject HDR
documents. The graphical picker remains SDR-range. Existing gamut checks compare
channels against [0,1], so even a valid bright neutral HDR color is described as
"Outside document gamut".

Keep one color editor. Select an appropriate remembered model, with linear RGB
readily available in HDR documents. Separate brightness above white, chromatic
gamut and display capability in its feedback. Keep hex explicitly SDR and retain
the original color when merely changing readouts. A brightness-in-stops control
alongside the ordinary picker would make HDR painting accessible without typing
three linear channel values; it should preserve hue and keep alpha independent.
Precise signed numeric entry remains valuable under the model/advanced controls.

Evidence: `crates/layer-ui/src/color/editor.rs:28`, `:68`, `:215`;
`apps/layer-linux/src/color_editor.rs:47`, `:109`;
`crates/layer-core/src/color/value.rs:87`.

### 7. Curves expose a parameter without explaining the graph

HDR insertion correctly defaults to the Linear HDR domain. However, the global
filter schema adds "Curve domain" and "Linear HDR white at right edge" to the
ordinary property list. The generic curve graph has no reference-white marker
or HDR scale. At the default +4-stop maximum, white is at 1/16 of the axis, which
is hard to infer from the familiar quarter-grid drawing.

Retain normal Exposure and Curves adjustments. Label the HDR curve axes and white
point; use a suitable viewing range automatically. Put expert domain/range
choices under Advanced and expose them only where relevant. A graph-range change
must not silently change an existing curve's edit; keep display navigation and
the saved processing domain distinct. Existing SDR adjustments must retain their
meaning when the document is promoted.

Evidence: `assets/filters/manifest.json:47`;
`crates/layer-ui/src/effects.rs:590`; `apps/layer-linux/src/effects.rs:871`.

### 8. Document and delivery labels hide important distinctions

GTK Document Properties says "16-bit" for both U16 and F16, even though the
shared DocumentInfo already uses the correct depth label. The "Further editing"
export preset always chooses integer16 TIFF, including for HDR masters, so it
does not preserve the HDR range implied by its name. New has no HDR preset;
creation is discoverable only through the expanded Color/Bit depth controls.

Use "16-bit SDR" and "16-bit float (HDR)" consistently. Offer a simple HDR drawing
preset on capable hosts, while keeping profile/depth separate in advanced creation
options. For HDR, either label the existing handoff preset "Further editing
(SDR)" or provide a separately qualified HDR handoff. Save remains the editable
master route. Do not imply that raising storage range restores lost highlights.

Evidence: `apps/layer-linux/src/files/properties.rs:93`;
`crates/layer-color/src/document_info.rs:54`;
`crates/layer-ui/src/export.rs:164`;
`crates/layer-ui/src/document_creation.rs:89`.

### 9. Display status needs one clear explanation

The HDR footer adds "mapped SDR display" or a headroom multiplier, alongside the
independent proof status. It does not account for proof state when choosing its
own label. Its tooltip contains technical detail, but the label is not an
actionable route to display information. An SDR canvas promoted to HDR also
requires reopening before an HDR surface can be used.

Prefer concise effective states: "HDR", "Showing SDR", "SDR preview" or the
existing named proof state, with Details available by keyboard/touch. Keep the
document's HDR identity separately in Document Properties. Put capability reason,
headroom and reference-white details behind that disclosure. Fix live surface
reconfiguration or explain a required reopen at conversion time, not in a guide.

Evidence: `apps/layer-linux/src/workspace.rs:1737`;
`apps/layer-linux/src/render_thread.rs:1059`;
`docs/history/color-management-gtk-m4-validation.md`.

## Core flows and minimal supporting UI

| User goal | Flow | Required support |
| --- | --- | --- |
| Edit an HDR picture | Open → ordinary adjustments/painting → Save | Automatic supported-input recognition; honest viewing state; accurate HDR color/curve/histogram readouts; exact master persistence. Unsupported input retains the current drawing and explains the supported route. |
| Paint new HDR artwork | New → HDR drawing → paint/sample | One creation preset; ordinary picker plus accessible above-white brightness; expert linear values; correctly labeled document depth. |
| Share a normal image | Export → SDR → preview/adjust appearance → JPEG/PNG/TIFF | Direct access to the saved SDR appearance editor; live comparison; correct target profile/size; master range retained. |
| Deliver HDR | Export → HDR → supported PNG route → export | One format entry; honest preview; fixed PQ properties; range preflight and a deliberate, clearly labeled clipping fallback only when needed. |
| Prepare a print | Soft Proof Setup → compare → lab export preset | Same saved SDR appearance, existing proof controls, clear proof target versus delivery profile. No separate HDR print mode. |

Use **Dynamic range: SDR / HDR** as an output choice for HDR documents, separate
from **Format**. Show only supported combinations. For this implementation, HDR
currently means PQ PNG; retain that compatibility detail beside the choice.
Ordinary SDR documents should keep their existing simple export UI.

## What to remove, retain or move

- Remove the duplicate HDR PNG format entry. "Map to PQ range" is currently
  per-channel clipping, not a perceptual gamut/tone mapper. Keep a deliberate
  advanced "Clip out-of-range colors" fallback if needed; never relabel it as
  automatic visual optimization or enable it silently.
- Keep the saved SDR recipe. It serves viewing, sharing and proofing, but its
  current standalone View-menu alert is the wrong primary presentation.
- Keep the temporary SDR preview for meaningful comparison on HDR displays.
  Avoid an apparently effective toggle on a view already constrained to SDR.
- Keep precise linear entry; remove its role as the only convenient route to
  choosing an HDR paint color.
- Move raw knee/domain/headroom/reference-white details and explanatory paragraphs
  out of everyday controls. Do not add a user-adjustable reference-white setting,
  OS HDR switch, per-monitor preset system or separate HDR editing workspace.
- Keep gain maps, RAW, layered PSD, native CMYK, OCIO/ACES and Float32 documents
  outside this UI correction. More formats are separate interoperability work.

## Platform alignment and implementation ownership

Lightroom groups SDR appearance controls with HDR editing, distinguishes HDR
output from file type, and gives HDR histograms a white divider and stop markers.
That supports the proposed task grouping; it is not a reason to copy its full
control set or claim its format coverage. [Adobe HDR workflow](https://helpx.adobe.com/lightroom/desktop/edit-photos/hdr-output.html).

Affinity explicitly separates temporary HDR preview exposure from image edits.
Preserve that distinction, while making clear that our saved SDR appearance also
affects delivery. [Affinity 32-bit Preview](https://affinity.help/photo2/English.lproj/pages/Panels/32bitPanel.html).

GNOME recommends alternatives to disruptive dialogs and live feedback for image
adjustment sliders. Use native GTK controls with a visible result; other hosts
may use a sheet or inspector with the same behavior. [Dialogs](https://developer.gnome.org/hig/patterns/feedback/dialogs.html),
[Sliders](https://developer.gnome.org/hig/patterns/controls/sliders.html).

Rust should publish applicable commands, actual export choices, display/range
states and descriptions. GTK should present those models rather than maintaining
parallel format lists or depth labels. Keep unsupported-host HDR editing hidden
and errors explicit; a shared "HDR" label alone is not cross-platform support.

Before accepting the revised UI, exercise every visible choice, including keyboard
and touch/pen access, small windows and long labels. Verify live adjustment,
Cancel/Reset, one-step undo, saved versus temporary state, SDR and HDR displays,
proof interactions and every export field against the actual file. Pixel tests
alone did not cover these UI contracts. Physical HDR qualification is still open.
