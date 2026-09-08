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
| Brush size | Huge outlined input and extra label/slider rows | Compact filled number stepper beside thin slider; no repeated label |
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
- Geometry assertions verify 36dp tools/Zen/tabs, 40dp brush previews, 31dp number
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
beside its main content, not a header spanning two columns. Search sits beside
Settings at the sidebar's top. The main pane has a centered page title, a filled
Done button and a Back arrow for details; these controls stay put while detail
contents animate. Done dismisses; accepted edits auto-apply and persist through
Rust. There are no nested settings dialogs, dropdowns or recording/conflict
popups. Narrow screens retain category-to-page navigation.

Sidebar/body text uses the shared 11 pt size, pane titles 18 sp, and sidebar
glyphs 20 dp within 48 dp navigation rows. Glyph centers and label starts align
with the search/Settings row. Sidebar insets are 8 dp, navigation gaps 4 dp and
content group spacing 20 dp. Choice/shortcut rows use 56 dp minimum height;
two-line setting rows use 64 dp. The editor slider is reused with a 48 dp touch
height and a visible neutral inactive track, fixing the light-on-light contrast
found during review. Compact editor controls and the GPU surface are unchanged.

The expanded 16-test device suite checks entry/exit and detail movement with the
Compose clock, no dialog/popup nodes, adjacent full-height panes, sidebar
alignment and font/glyph dimensions, filled Done pixels in both themes, slider
contrast/release behavior, invalid/valid numeric edits, accepted-value persistence
and real stylus events not reaching the covered canvas. Review captures are in
`artifacts/android/settings-panes/final/`: 32–33 cover numeric details/validation,
34–35 inline shortcut recording/conflicts and 36 the two-pane layout in each
theme. Settings views were visually checked in light/dark and portrait layouts.
Final emulator run `1788908967987` passes all 16 tests and produces 37 PNGs.
Android APK/test builds and lint pass. The slider's native touch bounds are
explicitly checked: reserving height around its 16 dp thumb preserves the small
visible knob while giving settings a real 48 dp target.

The preceding shared-model milestone also passed 72 core tests, packaged web
preferences/customization suites and GTK preferences in an isolated headless
Wayland session. This presentation-only refinement does not change those
implementations or the drawing/rendering path.
