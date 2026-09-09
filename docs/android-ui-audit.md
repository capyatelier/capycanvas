# Android editor visual audit

Reference: current GTK/web editor, compared in logical pixels (Android dp),
excluding Android's system bars. Reviewed all 24 slides in Android validation
run `1788886968498`, including both themes, portrait, menus, settings, expanded
panels, tab dragging, and drawing/recovery states.

## Inventory

| Area | Observed discrepancy | Correction / verification target |
| --- | --- | --- |
| Shared icons | 20dp instead of 16; large grips; color icon loses its outline | Shared SVG bank, 16dp icons in unchanged 36dp tools; trailing, oriented grips |
| Typography | Regular brush/tab/menu labels; Material defaults override control sizes | Core 11pt text, matching weight/line height; native Android font |
| Panel shell | 10dp radius, missing concave tab joins, incorrect grip inset | 8dp shell, 6dp tab shoulders, 8dp label/grip inset |
| Brushes | Tiny preview alongside name, 44dp rows | Full-width 40dp preview above right-aligned name, 2dp gaps |
| Brush size | Huge outlined input and extra label/slider rows | Label and plain editable value above a thin slider with minus/plus ends |
| Presets | Filled squares, no size visualization | Four-column responsive grid, brush dots above numbers |
| Layers | Oversized checks, whole-row selection, oversized actions and opacity editor | 16dp checks, separate compact selection buttons, thin opacity slider |
| Header / HUD | Off-center title without dimensions; centered HUD | Centered document title with dimensions; bottom/right-aligned HUD |
| Drawers | Giant controls and excessive nested padding | Same compact controls, joined outline/shadow, clear configuration hierarchy |
| Menus | Oversized rows, selected state replaces shortcut hint; panel actions in a centered alert | Compact grouped menu rows, separate selection and shortcut affordances; native popup anchored to the triggering tab/grip/tile |
| Preferences | Stretched controls; repetitive vertical labels | Full-screen list-detail overlay with bounded content width; single-pane navigation at compact widths |
| Preferences controls | Huge number fields/sliders; weak alignment and hierarchy | Label/description with trailing value/control, grouped rows, readable content width |
| Search / shortcuts | Oversized search, ungrouped shortcut rows | Filled search, grouped results/bindings, inline recording/conflicts |
| Portrait / IME | Same oversized controls; very narrow settings detail | Responsive settings navigation, bounded scrolling, safe system/keyboard insets |
| Drag positioning | Initial movement counted twice, shifting edge drops outside the window | Absolute native pointer coordinates for panel/tile/divider drags; edge-drop regression test |
| Number editing | Focused text can remain stale when a stepper changes its core value | Native focus handoff when using steppers/sliders; text/stepper synchronization test |

## Boundaries

Rust continues to own actions, settings metadata/validation/search, shortcut
capture/conflicts, panel contents and docking geometry. Compose owns native
widget editing, focus, menu placement, and responsive list/detail presentation.
The SurfaceView/Vulkan drawing and pen-input path is unchanged. The drag fix is
limited to native UI gesture-coordinate collection; docking decisions still
come from Rust.

Settings follow Android's [canonical list-detail guidance](https://developer.android.com/develop/adaptive-apps/guides/canonical-layouts):
side-by-side categories and detail when space permits, category-to-page navigation
with native Back on smaller windows. This needs no new UI framework/dependency.
Native switches, IME, Back, system insets, and font rendering deliberately remain
Android-specific; editor dimensions and visual roles follow GTK/web.

## Validation

The original 24-slide audit was followed by repeated implementation, screenshot
review and regression runs. Final coverage contains 31 PNGs in
`artifacts/android/ui-parity/final/` (ignored, not shipped in the repository).
All 31 images from final passing run `1788893788088` were visually reviewed;
no further unintended layout or styling discrepancies were found in these states.
Fresh web reference captures are in `artifacts/ui/customization/web/`.

- Native ARM64 and x86_64 debug APKs build; Android lint passes without errors.
- 13 emulator integration tests cover drawing/undo/redo, palm/eraser input,
  recovery/rotation, settings/search/theme/shortcuts, panel/tile/group dragging,
  dividers, Zen mode, number editing and horizontal/vertical ribbons.
- Geometry assertions verify 36dp tools/Zen/tabs, 40dp brush previews, 32dp number
  inputs and 20dp group grips. The settings-window assertion was subsequently
  replaced by full-screen geometry for the approved overlay design below.
- Portrait checks exercise category → detail → Back, not just a resized image.
- The high-rate drawing test retains allocation/snapshot-suppression assertions.
  This UI audit does not establish sustained 120 Hz on a physical Android tablet.
- The web customization suite passes, including drawers on all four edges.

Review inventory: 01–03 drawing/history; 04–08 settings/search/input/themes;
09–10 drawer/divider; 11 high-rate drawing; 12–13 custom toolbar/group moves;
14 shortcuts; 15–17 surface recovery/portrait; 18–21 menus/cursors/About;
22–24 palm/eraser/Zen; 25–26 default light/dark editor; 27–28 context/tool picker;
29–30 portrait detail/back; 31 vertical ribbon.

The corrected editor matches the reference's logical geometry, palette, assets
and control hierarchy. Native font rasterization, ripple/scroll behavior,
system bars, dialog navigation and shadows are intentionally not pixel-identical.
Physical-device usability and larger Android accessibility font scales still
need device validation.

## Full-screen settings follow-up

Settings is one full-screen, top-sliding overlay with a full-height sidebar
beside its main content, not a header spanning two columns. A persistent search
field replaces the sidebar heading/toggle. The main pane has a centered page title, a filled
Done button and a Back arrow for details; these controls stay put while detail
contents animate. Done dismisses; accepted edits auto-apply and persist through
Rust. Simple choices use anchored dropdowns; detailed editors remain in the
content pane without nested dialogs or recording/conflict popups. Narrow screens
retain category-to-page navigation.

Settings typography uses 16 sp body/sidebar labels, 14 sp descriptions, 18 sp
group headings and 20 sp pane titles. Done's 16 sp label sits in a 40 dp visible
button with a 48 dp hit target. Sidebar glyphs remain 20 dp in 48 dp rows, with
8 dp insets and 4 dp gaps. Content groups have 24 dp spacing and a 632 dp width
cap. Each row has name/description on the left and the value/control on the
right. [Numeric controls](numeric-controls.md) use a trailing stepper for small
integers; sliders span the row below all labels, with minus/plus ends and a
plain editable value above. Settings retain 48 dp touch height and up to 600 dp
numeric content width, with a contrasting track in both themes.
The editor's shared 11 pt typography, tool geometry and GPU path are unchanged.

Rust supplies every setting's identity, groups, labels, descriptions, choices,
defaults, ranges, steps and enabled/visible state. Numeric text is submitted on
IME Done/focus loss; slider positions use shared Rust mapping and resolution
before live updates through the ordinary typed preference action.
No per-setting Android renderer or secondary range/default catalog is required.
Choice values open native dropdowns with optional previews and a selected
checkmark; numbers edit directly in rows.

The compact-slider regression test found that the overlay's ancestor input
barrier cancelled child drags before they crossed touch slop. Moving that barrier
to a background sibling preserves canvas isolation and lets native controls
handle slow gestures. A focusable settings surface prevents the persistent
search field from receiving focus automatically on page changes.

Device tests check entry/exit and detail movement with the Compose clock,
no nested dialogs, choice-menu selection and dismissal, adjacent full-height panes, sidebar
alignment and font/glyph dimensions, filled Done pixels in both themes, slider
contrast/release behavior, catalog-driven labels and numeric ranges, dependencies,
invalid/valid numeric edits, accepted-value persistence
and real stylus events not reaching the covered canvas. Review captures are in
`artifacts/android/settings-inline/final/`: 32–33 cover inline numbers/validation,
34–35 inline shortcut recording/conflicts and 36 the two-pane layout in each
theme; 37 shows inline controls in dark mode. Settings views are visually checked
in light/dark and portrait layouts.
Final emulator run `1788912118247` passes all 17 tests and produces 38 PNGs.
Android arm64/x86_64 APK builds, test build and lint pass.

The touch-first numeric controls now share expression evaluation, formatting,
constraints and slider mapping with GTK and web in `layer-ui`. Panel value and
track rows are 24dp high, with thumb-free grey bars and 6dp end gaps. Settings
retain 48dp targets, larger value padding, accent fill and visible thumbs.
See [Numeric controls](numeric-controls.md) for the current cross-platform design.

The current shared-core suite passes 80 tests. Android run `1788922144800`
passes all 18 integration tests and produces 39 PNGs under
`artifacts/ui/numeric/android/1788922144800/`. Geometry checks cover the visual
bar spacing separately from Android's expanded touch targets. Pixel-based
eraser checks move the cursor outside the sampled area to measure pigment,
not the differently sized cursor overlays. Android build and lint pass; GTK
numeric feedback, native entry sizing, preferences and parity-reference tests
pass in a private headless Wayland session.
