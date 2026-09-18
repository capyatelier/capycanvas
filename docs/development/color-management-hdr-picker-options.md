# HDR color picker options after user feedback

The user finds the separate Brightness slider above the wheel odd. This is an
assessment for choosing the next interaction; the picker has not been redesigned
as part of the export-preview fix.

## What feels wrong in the current implementation

- “Brightness” is log2 of the largest positive linear RGB component, not measured
  luminance or an exposure offset from a separately retained base color.
- The -16 to +15 EV span makes ordinary choices occupy a tiny part of the slider.
  It also offers no visible SDR-white boundary or explanation of the range.
- In a narrow dock the label truncates, while the editor button competes with the
  number. The extra row competes with the wheel's own value/lightness control.
- Hue edits preserve a positive HDR peak. Field/value edits choose bounded color
  values and can abruptly discard that peak. This makes the controls feel unlike
  two coordinated ways of editing one color.
- The wheel and ordinary color patches use SDR derivatives. Several different
  HDR colors can therefore look similar even though their numeric values differ.

## Patterns in other editors

| Editor | Documented interaction | Useful lesson |
| --- | --- | --- |
| Photoshop | Familiar color field plus a separate intensity-in-stops control in the HDR picker. | Keep chromatic choice and intensity understandable; distinguish color intensity from document viewing exposure. |
| Affinity Photo 2 | Standard wheel/sliders; an Intensity slider is available in the panel's Opacity/Noise/Intensity control for 32-bit documents. | HDR intensity can live with color attributes without occupying the wheel's header. Avoid hiding the current intensity behind an unrelated mode. |
| Krita | The Small Color Selector offers a nits control; linear numerical entry and sampling can select values above one. The manual also describes changing viewing exposure. | Preserve HDR through the eyedropper and swatches; offer exact input without making it the default painting interaction. |
| Photoshop's April 2025 beta announcement | A configurable picker range extending above SDR white, with a visible SDR/HDR divider. | An integrated range is another viable design. This announcement is evidence of a beta design, not proof of current shipping behavior. |

Sources checked 2026-09-17:
[Photoshop HDR picker](https://helpx.adobe.com/photoshop/using/high-dynamic-range-images.html),
[Affinity Colour panel](https://affinity.help/photo2/English.lproj/pages/Panels/clrPanel.html),
[Krita scene-linear painting](https://docs.krita.org/en/general_concepts/colors/scene_linear_painting.html),
[Adobe's beta announcement](https://community.adobe.com/questions-700/new-32-bit-color-picker-in-photoshop-public-beta-669959).

## Options for CapyCanvas

1. **Wheel plus compact HDR intensity popover — recommended.** Keep ordinary
   color selection familiar. Put a small `HDR +2.0` control next to the selected
   swatch, below the wheel; clicking opens intensity with direct entry, reset and
   stop increments. Hue/saturation edits retain the intensity multiplier; normal
   value edits operate on the base color without silently resetting it. Start
   with a practical visible range, allow exact entry across the supported range,
   and expand the control when sampling brighter artwork. This is the smallest
   change to learn and removes the permanent slider from the header.
2. **One brightness strip with an HDR extension.** Use one value control with a
   marked SDR-white boundary and a clearly separated HDR region. The field and
   strip stay synchronized; sampling an HDR color expands the visible range.
   This makes HDR a continuation of ordinary color selection, but requires a
   fuller wheel/triangle interaction redesign to avoid two competing brightness
   axes. The range selector changes the picker range, never the selected color.
3. **HDR controls inside Edit Color.** Remove the dock slider, keep an `HDR`
   indicator on the swatch, and put intensity plus signed linear RGB fields in
   the existing editor. This is the cleanest dock for occasional HDR editing,
   at the cost of an extra click during repeated painting adjustments.

All options need an actual HDR selected-color preview on a capable display,
explicit SDR fallback, exact linear input, preserved alpha, and sampling/swatches
that retain the stored color. Opening the picker or changing its range/readout
must not normalize the color or change it. “Intensity” must not masquerade as
monitor brightness. A nits readout, if offered, must define document luminance
relative to the 203 cd/m² reference and use the correct primaries/luminance
weights; peak-channel value times 203 is not a colored pixel's luminance.

Recommendation: option 1 for frequent painting, with option 3's existing linear
RGB editor as the precise companion. No new global preference or separate HDR
mode is needed; the document type supplies the context. The source color should
remain one Rust-owned value, with native controls expressing the same semantics
on every host.
