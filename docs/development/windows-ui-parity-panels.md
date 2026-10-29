# Windows UI parity: panels

[Windows development](windows.md)

Bring the Windows layers, tools, toolbar components, bubbles, filters and
effects, curve, gradient, scopes, proof, navigator and palette panels to visual
parity with GTK. The plan also fixes reported bugs 5, 6, 7, 8, 17, 20, 22 and
the panel halves of 19 and 21.

## Prompt

> One agent implements this plan together with
> `docs/development/windows-ui-parity-shell.md`. Use the prompt, combined
> [order of work](windows-ui-parity-shell.md#order-of-work) and
> [validation](windows-ui-parity-shell.md#validation) in that plan. The panel
> findings land in its milestones M2 (bugs and Layers), M3 (drawer sizes,
> FX-1 to FX-3, LAYER-18) and M4 (tools, filters and the remaining panels).

## Files this plan owns

`LayersView.cpp/.h`, `LayerRow.cpp`, `LayerRowDrag.cpp/.h`, `LayerInspector.h`,
`LayerThumbnails.cpp/.h`, `ThumbnailOwners.h`, `WorkspaceRowDrag.cpp/.h`,
`ToolView.cpp/.h`, `ToolbarComponents.cpp/.h`, `WorkspaceBubbles.cpp/.h`,
`BubblePresentation.h`, `BubbleInput.cpp`, `StampPreview.cpp/.h`,
`TextureDialog.cpp/.h`, `PressureCalibration.h`, `StrokeRecording.h`,
`SwatchFace.h`, `IntensityImage.h`, `Checker.cpp/.h`, `FilterView.cpp`,
`EffectView.cpp`, `EffectControls.h`, `CurveView.cpp`, `GradientView.cpp`,
`FilterPreviews.cpp/.h`, `ScopesView.cpp/.h`, `PreviewPanel.h`,
`StatsView.cpp/.h`, `ProofPanel.h`, `ProofDial.h`, `ProofForm.h`,
`NavigatorView.cpp/.h`, `NavigationControls.h`, `PalettesView.cpp/.h` (all
under `apps/layer-windows/`), plus the shared list-reorder drop rule in
`crates/layer-ui` and any tool or preview metrics you add to the catalog.

`WorkspaceRowDrag` also serves the workspace manager and drawing tabs (Agent
C's pages). Keep its interface stable, and tell C in the commit message when
the drop rule changes.

## Order of work

See the combined [order of work](windows-ui-parity-shell.md#order-of-work) and
[validation](windows-ui-parity-shell.md#validation) in the shell plan.

## Shared context for all three plans

The Windows parity work is split into three plans that run in parallel on
different machines. Each plan owns a set of files, so none of them edits the
same file at the same time:

| Plan | Owner | Scope |
| --- | --- | --- |
| [Primitives and shell](windows-ui-parity-shell.md) | Agent A | Shared WinUI helpers and theme resources, workspace shell (tabs, drawers, bridges, columns, drag states), colour panels |
| [Panels](windows-ui-parity-panels.md) | Agent B | Layers, tools and brushes, toolbar components, bubbles, filters and effects, curves, scopes, proof, navigator, palettes |
| [Pages and chrome](windows-ui-parity-pages.md) | Agent C | Preferences, shortcuts, workspace manager, title bar, drawing tabs, canvas bar, notices, document and New drawing dialogs |

The findings were audited at `4cfc7e043`, and the root causes of the reported
bugs were rechecked at `ade9aad53`. Line numbers drift; search for the quoted
code when a line no longer matches. Two later commits change the reference:
`ade9aad53` keeps glass colours in the GTK tab drag preview, so SHELL-6 should
match it rather than the opaque GTK preview it describes, and `30d276f68`
resolves drawer placement in shared Rust (`crates/layer-ui/src/drawer_geometry.rs`),
so reread SHELL-1 to SHELL-4, FX-1 and FX-3 against it; the docked navigator
body now uses `NaturalHeight`, while the drawer path still sets 272 or 480. GTK (`apps/layer-linux/src`,
`style.css`) is the visual reference, shared Rust is the semantic authority.
Confidence tags: **CERTAIN** means the code proves it; **LIKELY** needs a
screenshot to confirm; **CHECK** is a suspicion to reproduce first or drop.

### Failure classes

Every finding carries one or more of these tags. Fix the class, not the
instance: when you fix one site, grep for the same construction elsewhere in
your files.

| Class | Typical cause on Windows |
| --- | --- |
| VCENTER | Segoe UI line box centred instead of glyphs; `ContentControl` icon hosts (top-aligned); asymmetric literal padding |
| SEAM | Adjacent surfaces at fractional coordinates, separately antialiased edges, mismatched radii |
| GLASS | Transient states (drag, float, expand, resize) switch to opaque `brush()` roles instead of glass roles |
| STACKED-BG | A child paints the row's fill again (selection on child plus row) |
| HIGHLIGHT-GEOMETRY | `button()` used for non-buttons; hover box inset by parent padding; unclipped full-bleed rows in rounded cards |
| DEFAULT-CONTROL | Fluent templates and system accent leak (no app-level palette resources; ComboBox, Flyout, TextBox, ToggleSwitch, ListViewItem, ToolTip) |
| METRIC | Literal pixels where `panel_metrics`, the `style` query or GTK values exist |
| LAYOUT-COLLAPSE | Exact heights instead of minimums; Canvas layouts sized in `SizeChanged` measure as 0 |
| ANIMATION-RETRIGGER | Views or drawers rebuilt or reset on unrelated model changes; stale paint caches after rebuild |
| HIT-TEST | Drop targets limited to list bounds; transparent (null) backgrounds not hit-testable; double-tap routing |
| NULL-STATE | Unchecked JSON reads, out-of-range `SelectedIndex`, null properties thrown from handlers |
| ALIGNMENT, BORDER/COLOR | Insets, border roles, error colours and opacities that differ from GTK |

### Reported bugs and owners

| # | Bug | Owner | Findings |
| --- | --- | --- | --- |
| 1 | Sketch brush drawer replays its open animation on every category change | A | SHELL-1, SHELL-2, TOOL-2 |
| 2 | Canvas bar "Scale" radio text sits low | A (`label()`), C verifies | PRIM-1, PRIM-2, HEAD-1 |
| 3 | Gap between tab bar and panel body | A | SHELL-5, SHELL-25 |
| 4 | Floating tab is opaque while dragged sideways | A | SHELL-6, SHELL-7 |
| 5 | Brush size panel dragged out drops at 0 height | B | TOOL-3, SHELL-8 |
| 6 | Pressing a layer highlights an oversized box around its name | B | LAYER-1 |
| 7 | Layer brush icon paints a second background | B | LAYER-2, LAYER-7 |
| 8 | Dragging a layer above the top row (over the button row) fails to drop at top | B | LAYER-3, LAYER-4 |
| 9 | Manage profiles button unstyled | C, using A's raised button | PREF-3, PRIM-15 |
| 10 | Cursor-shape icon in dropdown too high | C | PREF-6, PRIM-3 |
| 11 | Shortcut "All tools/All actions" dropdowns invisible | A (`choiceColors`), C (sites) | PRIM-8, PREF-1 |
| 12 | Preferences sidebar icons not centred | C | PREF-5, PRIM-3 |
| 13 | About "Dedicated to" row text not centred | A (`label()`), C verifies | PREF-7, PRIM-1 |
| 14 | Manage workspaces selector hover margin | C, using A's resources | PREF-22, PRIM-12 |
| 15 | Column-group bridge to drawer has transparent gaps | A | SHELL-17, SHELL-25 |
| 16 | Vertical panel drag/resize loses transparency | Deferred | SHELL-7 inventory only |
| 17 | Layers bubble alpha-lock icon too far right | B (measure first) | LAYER-17 |
| 18 | Layers drawer too narrow by default | A (shared Rust) | SHELL-10, LAYER-18 |
| 19 | Double-tap in the layers list closes or reopens the drawer | A (lifecycle, routing), B (hit-test fill) | SHELL-1, SHELL-24, LAYER-19 |
| 20 | Sketch size/strength sliders do not render | B | TOOL-1 |
| 21 | Filters drawer Cancel not at bottom, needless scroll | A (drawer heights), B (button face) | FX-1, FX-3, FX-2 |
| 22 | Blend mode with no filter selected crashes | B | FX-4, FX-5, FX-6, FX-7 |

### Helper contract

Agent A owns the shared primitives below and lands them first, as one or two
milestones, before migrating any call sites. Agents B and C do not edit the
files Agent A owns. They start with the work their plan marks *independent*,
then fetch `origin/main` and move their call sites onto the new helpers.
Agent A uses these names unless the code already has an equivalent. If A
chooses different names, it states the final names and signatures in the commit
message; B and C find them with `git log origin/main --grep='Windows UI parity'`.

| ID | Helper | Behaviour |
| --- | --- | --- |
| P1 | `label()` | Centres the glyphs, not the Segoe UI line box, for single-line captions (e.g. `TextLineBounds::Tight` or one font-derived correction); always sets its weight (Normal unless bold); deletes the inert `LineHeight(18)`. `button<T>` renders its text through `label()`. |
| P2 | Palette resource dictionary | One dictionary built from the cached palette and glass brushes, refreshed on theme change: ComboBox popup and items, Flyout and MenuFlyout presenters (`lines.popover_border`), TextBox states, ToggleSwitch, CheckBox, RadioButton, ListViewItem, ToolTip, HyperlinkButton, focus visuals, ContentDialog chrome. |
| P3 | `choiceColors(data, control, surface)` | Picks its fill by the surface it sits on (never `input` on `settings`); owns padding, height and font; themes items; reports open/closed to `data->popup()`; adds a guarded `selectIndex(control, index)`. |
| P4 | `fieldColors(data, textBox, surface)` | Dialog and settings field fill (`tint(text,18)`), no border, control radius; error and focus drawn without changing `BorderThickness`. |
| P5 | `raisedButton()` and dialog footer button | GTK default button face (fill, control radius, padding, min height); shares geometry with `pickerButton`. `button()` stays flat. |
| P6 | Neutral checked tint | `buttonColors<ToggleButton>` takes the checked role; toggles inside selected rows use a neutral checked tint, not `selected(data)`. |
| P7 | `segmentRow()` | One segmented control matching GTK `segment_buttons()`: transparent bar, `header_selection` checked fill, shared radius and height. |
| P8 | Clipped rounded frame | Card and swatch-pair container that clips children to its corner radius. |

`docs/development/windows-ui-parity-shell.md` lists A's own call-site work for
these helpers. Changing a helper's look changes every caller: each agent checks
its own pages after A's helpers land.

### Working rules

- Follow `AGENTS.md` and `apps/layer-windows/AGENTS.md`. Build with
  `apps/layer-windows/scripts/build.ps1`.
- Fix policy-level defects (drop rules, widths, readout markers, export rules)
  in shared Rust with a headless test, not in the host. Shared Rust changes
  touch common crates; keep them small and rebase often.
- Batch findings into the milestones of the [order of work](windows-ui-parity-shell.md#order-of-work):
  one VM round each. Push each milestone after its checks pass; fetch and
  rebase onto `origin/main` before every push.
- Remove a finding from your plan in the commit that fixes it. Record dropped
  CHECK items (not reproduced) in the commit message. Delete your plan in the
  commit that closes its last item.
- For every visual fix, compare light and dark themes, with glass transparency
  on and off, against the GTK values quoted in the finding. Add assertions to
  the existing `exercise-*.ps1` fixture for the area instead of new harnesses.
- Report what you could not run.

## Findings

### AREA=TOOL — Windows vs GTK parity audit

Scope read: `apps/layer-windows/ToolView.cpp`, `ToolbarComponents.cpp`, `WorkspaceBubbles.cpp`, `BubblePresentation.h`,
`BubbleInput.cpp`, `StampPreview.cpp`, `TextureDialog.cpp`, `PressureCalibration.h`, `StrokeRecording.h`, `SwatchFace.h`,
`IntensityImage.h`, `Checker.cpp` (+ the call sites they depend on: `PanelBody.cpp`, `WorkspaceDrawers.cpp`, `UiControls.h`,
`WorkspaceGeometry.h`). GTK: `tool_panels.rs`, `toolbar_components.rs`, `workspace_bubbles.rs`, `workspace_drawer.rs`,
`style.css`. Shared: `crates/layer-ui/src/{toolbar_transport.rs,toolbar_components.rs,tools.rs,layout.rs,layout_presets.rs,drawers.rs}`.

Sketch = `WorkspacePreset::Painter` (`layout_presets.rs:137-221`): Medium toolbars, a "Brush controls" toolbar
(BrushSizeSlider, BrushStrengthSlider, ColorPicker, Undo, Redo) on the centered left compact edge, and Brushes/Layers/Sizes as
drawer tiles.

---

#### User bugs in scope — root causes

#### Toolbar components (ToolbarComponents.cpp)

##### TOOL-5: Slider hit area is a 28 px band, not the tile's cross extent
- Class: HIT-TEST   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `ToolbarComponents.cpp:400-401` — the input `track` Canvas is 28 px across (`(width-28)/2`, `track.Width(28)` /
  `Height(28)`); the rest of a 36/54/72 px tile is `sliderRow` with a clear background and no handler. On Medium (Sketch) a
  vertical slider loses 13 px each side.
- GTK reference: `toolbar_components.rs:202-205` allocates the `gtk::Scale` the whole `bounds[1]`; `.brush-track { margin: 0 2px }`
  (`style.css:356,361`), so the full lane accepts presses.
- Fix direction: size `track` to the full `bounds[1]` (minus the 2 px margin) and keep the 28 px raster/thumb centered inside it.

##### TOOL-6: Track raster is rasterized on the UI thread
- Class: METRIC (architecture rule: no pixel work on UI thread)   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `ToolbarComponents.cpp:42-62` `trackRaster` runs a 4x4 supersampled per-pixel loop synchronously from `paintTrack`
  (`:412`) on every size/scale/theme change.
- GTK reference: `toolbar_components.rs:1505-1566` vector cairo path (clip + paint).
- Fix direction: draw the track as a `Path` (the same two-segment outline + `LinearGradientBrush` for Strength), or rasterize
  off-thread like `Checker.cpp`. A vector path also removes the stale-key problem in TOOL-1.

##### TOOL-7: Disabled slider dims the whole row (thumb, cap, readout) to 0.4
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: low
- Windows: `ToolbarComponents.cpp:371` `sliderRow.Opacity(enabled?1:.4)`.
- GTK: `toolbar_components.rs:1550` track alpha ×0.35 and `scale.set_sensitive(false)` (theme-disabled thumb).
- Fix direction: apply 0.35 to the track ink only and use the shared disabled state for thumb/readout.

##### TOOL-8: Vertical segmented-choice corner radius is a literal 6
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `ToolbarComponents.cpp:755` `radius=self->vertical?6:control_radius*CornerFit`.
- GTK: `style.css:339-340` vertical `.option-segments` first/last use `calc(var(--tile-radius) * var(--corner-fit))` (Small 18 →
  9.7, Medium 27 → 14.6).
- Fix direction: use `tileRadius*CornerFit` (already in `Impl::tileRadius`) for the vertical case.

##### TOOL-9: Non-segmented choice menu drops item icons; face padding fixed
- Class: DEFAULT-CONTROL, METRIC   Confidence: CERTAIN   Severity: low
- Windows: `ToolbarComponents.cpp:785-794` popup is `RadioMenuFlyoutItem` text-only (WinUI radio bullets, no icons);
  face padding `{6,0,6,0}` in every axis (`:772`).
- GTK: `toolbar_components.rs:1411-1415` DropDown with `choice_factory(.., 16)` rows (16 px icon + label, no radio bullet);
  vertical face `padding: 2px; background: transparent`, labeled vertical `padding-left 8px; right 6px` (`style.css:347-348`).
- Fix direction: build the menu with `NativeMenuItems`-style icon items (shared helper used elsewhere), and set face padding
  from orientation/labeled as GTK.

##### TOOL-10: Numeric/action fields use literal widths
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `ToolbarComponents.cpp:679` natural width `lead+4+value+14+(sliders?60:0)`; `:602` `-(labeled?38:4)`;
  `:605` font ×0.9 for ≥4 chars; `:536` gradient editor `body.Width(240)`; `:710` choice-grid extent `18*columns`;
  `:834` check width `+32`; `:850` text action `+12`.
- GTK: measures natural sizes of the actual widgets (`toolbar_components.rs:238-245`), `.toolbar-number scale { min-width:56px; padding:0 2px }` (`style.css:350`).
- Fix direction: measure the composed `numberView`/`check`/`pick` (`Measure(INF, controlHeight)`) as `choiceField` already does
  (`:818-822`) instead of hand sums.

##### TOOL-11: Slider thumb / bookmark metrics are literals (match GTK today)
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `ToolbarComponents.cpp:305` thumb radius `3.24` (=4×0.54), `:417-418` 12×28, `:426-427` marks 2×14, `:504`
  preview radius `*.54` instead of `CornerFit`.
- GTK: `style.css:363-364` (10×26 + 1 px border, radius 4), `toolbar_components.rs:1600-1613` (2×14). Values agree; only the source differs.
- Fix direction: use `CornerFit` constant and a shared thumb metric; low priority.

---

#### Tool Set / Brushes / Tools panels (ToolView.cpp — ToolSetView)

##### TOOL-12: Tool-group tiles are 112×36 fixed; GTK uses natural width × control height (26)
- Class: METRIC, LAYOUT-COLLAPSE   Confidence: CERTAIN   Severity: medium
- Windows: `ToolView.cpp:64-66` `tile=3*36+2*2` (112), `height=media?44:36`, uniform columns; media items take the full width.
- GTK: `tool_panels.rs:93-96` non-media `set_size_request(-1, PANEL_CONTROL_METRICS.control_height)` (26) in a non-homogeneous
  FlowBox up to 16 per line, 2 px gaps (`:35-42`); media `(BRUSH_SETS_MIN_WIDTH - 2*PANEL_CONTENT_INSET, 44)` = 88×44, 1 per line.
- Shared source: `panel_metrics.control_height` (26), `BRUSH_SETS_MIN_WIDTH` (104), `PANEL_CONTENT_INSET` (8).
- Fix direction: in the wrap helper of TOOL-3, size non-media groups to measured width × `panelMetric("control_height")`.

##### TOOL-13: Brush preview rows don't follow `tool_preview_metrics`; caption surfaces missing
- Class: METRIC, STACKED-BG, GLASS   Confidence: CERTAIN   Severity: medium
- Windows: `ToolView.cpp:99-112,133` — preview `Image.Height(40)` in an Auto row above a `toolLabel` caption (16 px icon column,
  right-aligned name, LineHeight 18), `Padding {6,3,6,3}`, `MinHeight 34`, frame radius literal 3. Selection uses
  `selected(data)` (glass selection in glass mode).
- GTK: `tool_panels.rs:128-138,181-203,237-256` + `style.css:300-312` — row height from `TOOL_PREVIEW_METRICS.heights` (min 54,
  natural 40+caption), caption ≥ control height (26) laid out by `TOOL_PREVIEW_METRICS.layout`; caption is a control-height
  square icon surface and a name surface with `field_padding`, both `alpha(row base, 0.2)` with `control_radius`;
  `.tool-preview-choice { padding: 0 6px; --glass-selection: var(--tool-caption-bg) }` (the selected row uses the solid
  selection colour even in glass mode).
- Shared source: catalog `tool_preview_metrics` is published (`layer-ui/src/lib.rs:403`) but unused on Windows.
- Fix direction: read `tool_preview_metrics` from the catalog, build the two caption surfaces, padding `{6,0,6,0}`, min height
  54, and use the opaque selection brush for preview rows.

##### TOOL-14: Selection-list row height uses the wrong condition and value
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `ToolView.cpp:121,133` `tonal` = tool_extra has `tonal-tones` → 36.
- GTK: `tool_panels.rs:114-115,140` rows are 32 when any subtool invokes `TonalSelect`, else 44 (`TOOL_ROW_HEIGHT`).
- Fix direction: same condition and 32.

##### TOOL-15: Tool-set divider is an 18 px centered dash
- Class: METRIC, ALIGNMENT   Confidence: CERTAIN   Severity: low
- Windows: `ToolView.cpp:53` `dividerLine(data,true)` → `DividerLength` 18 px wide, centered (`WorkspaceGeometry.h:157-165`).
- GTK: `tool_panels.rs:44-45` full-width horizontal `gtk::Separator` with 4 px start/end margins.
- Fix direction: a stretched 1 px line with `Margin{4,0,4,0}`.

##### TOOL-16: Empty groups canvas still takes a StackPanel spacing slot
- Class: LAYOUT-COLLAPSE   Confidence: LIKELY   Severity: low
- Windows: `ToolView.cpp:56` groups visible for every panel except `tools` even with zero groups; Height 0 + `root.Spacing(6)` → 6 px gap above the list.
- GTK: `tool_panels.rs:78` hides groups when `view.groups.is_empty()`.
- Fix direction: collapse `groups` when empty in `refresh`.

##### TOOL-17: Whole tool list rebuilt on theme change
- Class: ANIMATION-RETRIGGER   Confidence: CERTAIN   Severity: low
- Windows: `ToolView.cpp:123` key = `itemSchema(items)+theme` → full rebuild (buttons, tooltips, focus lost).
- GTK: `tool_panels.rs:157-165` keeps buttons and only re-textures previews on theme change.
- Fix direction: drop theme from the key; on theme change update `Image.Source` and icons in place.

##### TOOL-18: Group/subtool label icon size and weight
- Class: METRIC   Confidence: CHECK   Severity: low
- Windows: `ToolView.cpp:29-35` 16 px glyph column, `icon()` default 16; groups bold, media not bold.
- GTK: `style.css:189` `.dock-panel image { -gtk-icon-size: 14px; }` applies to `aligned_icon_label` images; all labels sit in
  buttons (Adwaita bold).
- Fix direction: verify on screen; use 14 px in dock panels if confirmed.

---

#### Tool Settings panel (ToolView.cpp — SettingsView)

##### TOOL-19: Command buttons lose their icon and are centered
- Class: ALIGNMENT   Confidence: CERTAIN   Severity: medium
- Windows: `ToolView.cpp:437-439` non-icon-only actions show the caption only (`label`), with the Button's default centered content.
- GTK: `tool_panels.rs:552-558` `icon_label(caption, command.icon)` — 16 px icon then left-aligned (`xalign 0`, hexpand) label.
- Fix direction: reuse the `toolLabel`-style row (icon + left text) and `HorizontalContentAlignment::Stretch`.

##### TOOL-20: Inline numeric captions use literal minimum widths instead of a shared size group
- Class: METRIC, ALIGNMENT   Confidence: CERTAIN   Severity: low
- Windows: `ToolView.cpp:415` `text.MinWidth(62)` (compact/tonal), `:408` `MinWidth(16)` (beside a choice grid).
- GTK: `tool_panels.rs:417-418,461-462` `inline_labels`/`inline_values` SizeGroups → all captions take the widest caption.
- Fix direction: compute the caption column with the existing `captionWidth()` helper (`:207-211`) over the group's labels.

##### TOOL-21: Segmented mode buttons stretch to full width with drawn separators
- Class: HIGHLIGHT-GEOMETRY, BORDER/COLOR   Confidence: CHECK   Severity: low
- Windows: `ToolView.cpp:248-260,427` star columns (stretched), non-compact segments get a 1 px left border `tint(text,51)` and clear background.
- GTK: `tool_panels.rs:515-523` + `style.css:287` `.selection-modes` linked toggle buttons, each `min-width: control_height`, not flat.
- Fix direction: confirm visually; if GTK is not stretched, use fixed `control_height` columns and the linked-button fill.

---

#### Brush size presets (ToolView.cpp — BrushSizePanel)

Tile size, dot gradient (0.4/0.65/1.0 stops), label `margin-bottom 2`, normal weight and radius
`small.size/2*CornerFit` all match `tool_panels.rs:636-673` / `style.css:322`. `actionTooltip` is placed on the pick
itself; the tile is also a drag source in some layouts — per `apps/layer-windows/AGENTS.md` keep tooltips off drag
surfaces (CHECK whether size presets are dragged).

---

#### Tool bubbles (WorkspaceBubbles.cpp, BubblePresentation.h, BubbleInput.cpp, SwatchFace.h)

Checked against `workspace_bubbles.rs` + `style.css:115-133` + `tool-bubbles.md:456-465`: face/item/brush-pill radii, glass
role (`glass(panel)`), selection overlay over glass, hover 7%/pressed 14% (`tint(text,18/36)`), face icon 20 px with Medium
−4 / preview −6 offsets, preview 32×8 at +11 and 0.78, variant mark 16 px inset 9, dots 1.75 px at 90/30%, readout
padding 2×7 tabular, tag radius 9 / padding 3×9 / ≤96 px, count badge ≥22 px SemiBold, palette bead icon 13, bead padding
1/3 px, history halves 20 px icons at 35% disabled, ring thumb 14/15 px with 60% text stroke, shadows 8/2/8% and
12/4.5/16% lifted. All match. Remaining differences:

##### TOOL-22: Layers face hit region is a square, drawn shape is a squircle
- Class: HIT-TEST   Confidence: CHECK   Severity: low
- Windows: `WorkspaceBubbles.cpp:35` `surface(..., hit=true)` uses radius 0 for the layers face, so the accessible button
  (`:375,380`) accepts taps in the squircle's transparent corners.
- GTK: `workspace_bubbles.rs:304-314` uses the shared `view.hit(p)` only.
- Fix direction: use the same squircle geometry for the hit path (or rely solely on the shared `hit` query).

##### TOOL-23: Brush chooser item without a preview shows no name
- Class: LAYOUT-COLLAPSE   Confidence: CERTAIN   Severity: low
- Windows: `WorkspaceBubbles.cpp:371-372` brush items without a preview draw only an 18 px icon.
- GTK: `workspace_bubbles.rs:699-705` brush items always show the name label; the icon is shown when there is no preview.
- Fix direction: draw the name for every `kind=="brush"` item.

##### TOOL-24: Bubble geometry literals
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `WorkspaceBubbles.cpp:211` brush pill `152×48`, item `44×44`, radii 24/22; `:219,257` card radius 18; `:352-354`
  layers face 48/46 squircle radii 14/13; `:363-364` pill buttons 32×30 / (w−68)×30 radius 15 (GTK `.bubble-pill button
  { min-height: 24px; padding: 0 4px }`, spacing 2).
- Fix direction: take part sizes from the shared bubble packet (sizes already exist in Rust: `BRUSH_PILL`, `HISTORY_BUBBLE`);
  verify the pill button height on screen.

---

#### Dialogs / other scoped files

##### TOOL-25: Pressure/tilt calibration rebuilds its curve editor on every palette change
- Class: ANIMATION-RETRIGGER   Confidence: CERTAIN   Severity: low
- Windows: `PressureCalibration.h:147-149` key includes `palette.Stringify()` → `ResponseCurveField` recreated whenever any
  palette value changes (glass strength, accent, preview colours), discarding editor state.
- Fix direction: key on `target()` only; refresh colours through bindings.

##### TOOL-26: Stroke recording host strings are hard-coded English
- Class: (UI text in host)   Confidence: CERTAIN   Severity: low
- Windows: `StrokeRecording.h:12` `"Start stroke recording"`, `:55` `"Capy stroke recording"` file-type label.
- Rule: UI text lives in shared Rust / Fluent catalogs (AGENTS.md).
- Fix direction: take both strings from the shared `stroke_recording` status/catalog.

##### TOOL-27: Texture picker metrics
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `TextureDialog.cpp:135` `TextureDialogHeight=600`; tile hover uses `buttonFeedbackBrush` default 20/255 (7.8%).
- GTK: `paper_picker.rs:180` `content_height(560)`; `style.css:261` hover `alpha(currentColor,.10)`. Tile 96+3 padding, gaps 12/14,
  ≤6 columns, 2 px accent ring and 82% caption match.
- Fix direction: 560 height, hover alpha 26.

`StampPreview.cpp`, `Checker.cpp`, `IntensityImage.h`: pixel conversion runs off the UI thread; no parity issues found.
`SwatchFace.h` matches `.bubble-bead` padding; edge alpha 64 / 2 px selected not cross-checked against GTK's swatch snapshot (CHECK).

---

#### Root causes / systemic patterns

1. **Theme refresh rebuilds retained views** (TOOL-17, 25): retain the Tool Set list and calibration curve while refreshing their palette.
2. **Tool group geometry** (TOOL-12, 16): use natural caption width and the control-height metric in the measurable wrap panel, and collapse empty groups.
3. **Hand-entered metrics instead of published ones** (TOOL-8, 10, 12, 13, 14, 15, 20, 24): `panel_metrics`,
   `tool_preview_metrics`, `TileStyle` size/radius and `BRUSH_SETS_MIN_WIDTH` are already in the catalog/`style` query;
   reading them removes most literal 36/112/40/34/62/6/18 values.

### FX area audit: Windows vs GTK (filters, properties, curves, gradients, scopes, stats, proof, navigator, palettes)

Shared metrics (crates/layer-ui/src/layout.rs:52): inset 8, control_spacing 4, group_spacing 6, compound 2,
control_height 26, field_padding 6, icon 16, control_radius 13 (x corner-fit .54, about 7px).

---

##### FX-8: Filter picker header-to-list gap is 0 instead of 6 (14 in a drawer)
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `FilterView.cpp:46,52-53,84`. The root Grid has `Padding(8)` and no RowSpacing, so the list starts directly under the header.
- GTK reference: `effects.rs:151` adjustments Box spacing 6. `style.css:379,383,389` header margin `inset inset 0`, body margin `0 inset inset`, and in the drawer `.content-drawer .filter-picker-body { margin: inset }`, so the gap there is 6+8.
- Fix direction: `root.RowSpacing(group_spacing)`, plus the drawer inset when hosted in a drawer.

##### FX-9: Filter list row spacing is 0 instead of 2
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `FilterView.cpp:82`. The `rows` StackPanel has no Spacing. `scrollMetrics` (`:167`) already adds `rows.Spacing()`.
- GTK reference: `effects.rs:169` `picker_body` spacing 2 (compound_spacing).
- Fix direction: `rows.Spacing(panelMetric("compound_spacing"))`.

##### FX-10: Category heading bottom margin and empty-label inset differ
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `FilterView.cpp:120` heading `Margin({8,8,8,4})`. `:151` empty label `Margin(8)`.
- GTK reference: `style.css:385` `.filter-category { margin: 8px }` on all sides. The empty label has no margin (`effects.rs:594-596`).
- Fix direction: margin 8 on all sides for headings, 0 for the empty label.

##### FX-11: Filter category glyph is not dimmed and sits 2px off
- Class: BORDER/COLOR, ALIGNMENT   Confidence: CERTAIN   Severity: low
- Windows: `FilterView.cpp:64-66`. The glyph overlays column 0 and the combo has `Margin({22,0,0,0})`, which puts the glyph-to-combo gap at 6. The glyph has no opacity.
- GTK reference: `effects.rs:157-158,165-166`. The icon is a separate Box child with `dim-label` (opacity .55), and the Box spacing is `CONTROL_SPACING` (4), so the combo starts at 20.
- Fix direction: make the glyph its own Auto column with `ColumnSpacing(control_spacing)` and `Opacity(.55)`, and drop the 22 literal.

##### FX-12: Filter row caption uses a hard-coded line box
- Class: VCENTER   Confidence: CHECK   Severity: low
- Windows: `FilterView.cpp:145` uses `LineStackingStrategy(BlockLineHeight)` and `LineHeight(18)` with font `textSize()` (about 14.7px). At other text scales the glyph sits off-centre next to the 16px icon. GTK uses the natural line height (`tool_panels.rs:209-219`, row `valign Center`).
- Fix direction: remove the LineHeight override, or derive it from the text size. Confirm with a screenshot at 100% and 150%.

##### FX-13: Search/name TextBoxes keep WinUI focus visuals; palette TextBoxes also use the default corner radius
- Class: DEFAULT-CONTROL, METRIC   Confidence: LIKELY (focus) / CERTAIN (radius)   Severity: medium
- Windows:
  - `FilterView.cpp:67-68` (filter search), `PalettesView.cpp:216` (palette search) and `:243` (name editor) set only `Background`/`BorderThickness`. The TextBox template switches to `TextControlBackgroundFocused`/`TextControlBorderBrushFocused` (default light or dark fill plus an accent underline) on focus and pointer-over. Nothing overrides those resources, unlike `choiceColors` for ComboBox.
  - `PalettesView.cpp:216,243`: no `CornerRadius`, so they get the template's 4px instead of the control radius (about 7). The filter search does set `controlCorner()`.
- GTK reference: `style.css:277` `.dock-panel entry { background: input }`, `:380` filter entry radius control, `:488` `.palette-entry { min-height control; padding 0 6px; background input; border 0; radius control }`.
- Fix direction: add a `textFieldColors(data,box)` alongside `choiceColors` in ChoiceFeedback.cpp that overrides the focused, pointer-over and disabled TextControl* resources with input/text roles and applies `controlCorner()`. Apply it to all three. Palette padding `{6,2,6,2}` should become `{6,0,6,0}`.

##### FX-14: Toggle properties are a CheckBox, while GTK uses a Switch on the right of the label
- Class: DEFAULT-CONTROL, ALIGNMENT   Confidence: CERTAIN   Severity: medium
- Windows: `EffectView.cpp:183-188` puts `compactCheck(data,caption)` with the box at the left and the text after it.
- GTK reference: `effects.rs:885-892` uses `gtk::Switch` in `row(label, input)`, with the label hexpand on the left and the switch right-aligned. CSS is at `style.css:290-291`.
- Fix direction: use a styled ToggleSwitch, or a native switch helper shared with other panels, in a `propertyRow`-like Grid (label Star, switch Auto).

##### FX-15: Property colour field swatch geometry differs, has no checker for alpha, and has square corners
- Class: METRIC, BORDER/COLOR   Confidence: CERTAIN   Severity: low
- Windows: `EffectView.cpp:29-37`. The swatch column is a `56px` literal and the inner `Rectangle` has `Margin 2` and a solid fill (no transparency checker, no radius). The compact version is `Width(36)` (`:32`).
- GTK reference: `color_editor.rs:1253-1258` uses `ColorPatch` 32x20 inside a dock-panel button (padding 0 6px, so the button is 44 wide and 26 high). ColorPatch draws a checker and has rounded corners.
- Fix direction: derive the size from metrics (patch 32x20 plus 2x field_padding), and reuse the checker patch used by PalettesView `paint()`.

##### FX-16: Raised (non-flat) panel buttons use three different fills, and some have none
- Class: BORDER/COLOR, DEFAULT-CONTROL   Confidence: CERTAIN   Severity: medium
- Windows: property action buttons use `brush("input")` (`EffectView.cpp:215`). Add Filter uses `brush("control_fill")` (`:314`). The colour "use selected" bucket uses `clear` (`:198`, plain `button()`). The gradient reverse/reset/remove/bucket buttons use `clear` (`GradientView.cpp:95-97`). Filter Cancel uses `clear` (FX-2). Proof mode buttons use `clear` or `tint(text,31)`.
- GTK reference: all of these are non-flat `gtk::Button`/`MenuButton` (`effects.rs:196,745,862`; `gradient_editor.rs:22-26` via `icons::button`, which only adds `panel-icon-action`). They get the same Adwaita raised fill under `.dock-panel button` (`style.css:185-186`, `:284`, `:298`).
- Fix direction: add one `raisedButton(data, button)` helper that applies the shared non-flat fill role (control_fill, the one dialog footers use in `ChoiceFeedback.cpp:60`) and call it from every site above. Remove the per-site Background calls.

##### FX-17: Properties header has an extra 2px bottom margin
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `EffectView.cpp:296` sets `header.Margin({0,0,0,group_spacing-control_spacing})`, which is 2px extra.
- GTK reference: `effects.rs:179,190-205` has uniform `CONTROL_SPACING` between title, header, toolbar and body, with no extra margin.
- Fix direction: remove the margin.

##### FX-18: Choice-label width sizing is applied to texture (paper) rows too
- Class: ALIGNMENT, LAYOUT-COLLAPSE   Confidence: CERTAIN   Severity: low
- Windows: `EffectView.cpp:124-145,171`. `propertyRow` always clamps the caption column to the widest choice label (capped at 45%) and stretches the value. TextureField uses it too.
- GTK reference: `effects.rs:979-988` only puts DropDown rows in the `property_choice_labels` SizeGroup with the input hexpand. Other rows (paper picker, colour) have the label hexpand and the control at natural width (`row()` at `:1154-1163`).
- Fix direction: pass a `choice` flag to `propertyRow`. Non-choice rows use label Star and value Auto.

##### FX-19: Curve editor guide, polygon and marker colours differ from GTK
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: medium
- Windows: `CurveView.cpp:240` white-point guides at `Opacity(.7)`. `:248-249` control polygon dashed `3,3`. `:252` marker is a filled 8px `accent` dot.
- GTK reference: `curve_editor.rs:81-95`. Whites are drawn with the same `color(0.2)` dashed 3,3. `:96-102` polygon alpha .35 is solid, not dashed. `:110-111` marker is a stroked ring, radius 5, foreground alpha .8.
- Fix direction: use opacity .2 for whites, drop the dash on the polygon, and draw the marker as a 10px Ellipse with Stroke text and Opacity .8 and no fill.

##### FX-20: Curve editor spacing and axis text size differ
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `CurveView.cpp:135` `frame.ColumnSpacing(control_spacing=4)`. `:161` coordinate row `ColumnSpacing(6)`. `:27` axis labels `textSize()*.85`. `:147` vertical gutter `Width(36)` literal.
- GTK reference: `curve_editor.rs:203` grid `column_spacing(6)`, `row_spacing(4)`. `:232` coordinate row `CONTROL_SPACING` (4), homogeneous. Axis labels are `dim-label` at normal size (`:208,226`).
- Fix direction: swap to 6 and 4 (group and control spacing), use the normal text size at opacity .55, and use a measured gutter.

##### FX-21: Curve chart background role
- Class: BORDER/COLOR   Confidence: CHECK   Severity: low
- Windows: `CurveView.cpp:133` uses `chart.Background(brush("input"))`.
- GTK reference: `curve_editor.rs:79` paints `color(0.12)`, which is the foreground at 12% inside the inset.
- Fix direction: use `tint("text", 31)` if the input role does not match. Compare in light and dark.

##### FX-22: Gradient ramp and stop markers
- Class: BORDER/COLOR, SEAM   Confidence: CHECK   Severity: low
- Windows: `GradientView.cpp:35-36` puts the ramp `Image` in a Border with `CornerRadius(controlCorner)`. Whether the image is clipped to the rounded corners needs a screenshot. Stops are drawn as Ellipses of radius 4/2.5 at a fixed `y=39` (`:202-209`), and the strip is a `44px` literal (`:118`).
- GTK reference: `gradient_editor.rs:16` uses `GradientPreview`, which draws its own stops.
- Fix direction: confirm visually. If the corners are square, clip with a RectangleGeometry or apply the radius to the image brush. Derive the stop y from the strip height.

##### FX-23: Stats chart line styles, spacing and numerals
- Class: BORDER/COLOR, METRIC   Confidence: CERTAIN   Severity: low
- Windows: `StatsView.cpp:31-32` samples are 1px at full ink. The budget line is solid in the `settings_secondary` role. `:27` spacing is control_spacing (4). Values are proportional digits.
- GTK reference: `effects.rs:228-243` budget line foreground alpha .3 dashed 3,3, samples foreground alpha .9 at 1.5px. `:216` stats Box spacing 6. `:1117` values use `.numeric` (tabular figures).
- Fix direction: match the strokes, use group_spacing, and set `Typography::SetNumeralAlignment(value, Tabular)`.

##### FX-24: Print proof form uses unstyled WinUI ComboBoxes with headers above
- Class: DEFAULT-CONTROL, LAYOUT-COLLAPSE, METRIC   Confidence: CERTAIN   Severity: medium
- Windows: `ProofForm.h:270-282`. Profile, intent and simulation ComboBoxes have no `choiceColors`, controlHeight, padding or popup tracking. They use `Header` (label above) and `root.Spacing(10)`. `bpc` is a default `CheckBox` rather than `compactCheck`. ProofPanel only sets them Bold (`ProofPanel.h:86`).
- GTK reference: `files/proof.rs:489-528` uses `panel_controls::row(label, dropdown, CONTROL_SPACING, SizeGroup)`, with the label on the left (width_chars 9, shared size group) and a styled dropdown. Spacing is `CONTROL_SPACING`. bpc and warning use `panel_controls::check`.
- Fix direction: build rows with the same label+control Grid as `propertyRow`, style the boxes via `choiceColors` (which also brings in FX-4's popup fix), use `compactCheck`, and use spacing control_spacing.

##### FX-25: SDR proof dial drawing differs (arcs, markers, readouts)
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: medium
- Windows: `ProofDial.h:149-160`. Arcs are solid `brush("border")`. The field marker is an ink ring 2px with no halo. Arc handles are ink-filled discs. Readout text and icons are full-opacity, straight text.
- GTK reference: `proof_dial.rs:239-266` arcs are gradients (brightness black→grey→white, highlight colour white→blue) with round caps. `:826-850` markers are a white 2px ring over a black .65 4px halo, plus a focus ring. `:891-897` readouts use ink alpha .62 and curved text along the arc (`:851-889`). Icons have opacity .62 (`:379-385`).
- Fix direction: use LinearGradientBrush strokes with `StrokeStartLineCap/EndLineCap Round`, two stacked Ellipses for the halo marker, `Opacity(.62)` on texts and icons, and render curved readouts from the Rust `readout.curve` geometry.

##### FX-26: Proof dial is capped at 256px wide
- Class: LAYOUT-COLLAPSE   Confidence: LIKELY   Severity: low
- Windows: `ProofDial.h:165` sets `root.MaxWidth(side=256)` and `MinWidth(128)`, inside a Viewbox.
- GTK reference: `proof_dial.rs:337-338` is hexpand and vexpand and fills the panel width.
- Fix direction: remove the MaxWidth, or take it from Rust, and keep the Viewbox square.

##### FX-27: Proof mode segmented control
- Class: HIGHLIGHT-GEOMETRY, BORDER/COLOR   Confidence: CHECK   Severity: low
- Windows: `ProofPanel.h:20-34,74` uses three Buttons in a Star Grid with `control_spacing` gaps. Selected is `tint(text,31)` and others are clear.
- GTK reference: `panel_controls.rs:106-118` uses an `adw::ToggleGroup` flat, homogeneous, joined (no gaps), styled by `style.css:187-188`.
- Fix direction: compare the screenshots. Likely ColumnSpacing should be 0 with the selection fill from the shared toggle style.

##### FX-28: Navigator letterbox is painted with the opaque `bg` role
- Class: GLASS, STACKED-BG   Confidence: CHECK   Severity: low
- Windows: `NavigatorView.cpp:115,155-162`. `surround.Fill(brush("bg"))` covers the overview rect outside the image. In glass themes this is an opaque window-bg patch inside a glass panel.
- GTK reference: `navigator.rs` paints nothing outside the image, so the panel background shows through.
- Fix direction: if the surround is only there to mask the canvas surface, use the panel surface role (`glass("panel")` when `glassSurfaces`) so it matches the panel.

##### FX-29: Palette tile corners and empty placeholders use a 6px literal
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `PalettesView.cpp:59-61,75,113,254,322` use `CornerRadius({6,6,6,6})`.
- GTK reference: `style.css:481-482,484-485` tile and patch use `var(--control-radius)` (13 x .54, about 7). The add button uses `control-radius + 3`.
- Fix direction: use `data->controlCorner()` everywhere, and `controlCorner()+3` for the add face.

##### FX-30: Selected palette swatch border shrinks the patch
- Class: HIGHLIGHT-GEOMETRY   Confidence: LIKELY   Severity: low
- Windows: `PalettesView.cpp:415` sets `BorderThickness(2)` on the selected tile Button. The template's border sits inside the bounds and pushes content by 2px, so the colour patch shrinks or shifts when selected.
- GTK reference: `style.css:483` `.palette-tile.selected { outline: 2px solid accent; outline-offset: -2px }`. An outline does not change layout.
- Fix direction: draw the ring as an overlay Border (non-hit-test) on top of the patch, and keep the button border at 0.

##### FX-31: Palette grid tiles have fractional widths
- Class: SEAM, METRIC   Confidence: LIKELY   Severity: low
- Windows: `PalettesView.cpp:78-81`. `x=round(i*(cell+Gap))`, but `Width(cell)` is fractional, so gaps alternate between about 3 and 5 px and edges render blurry. `Cell/Gap` are local literals (`:20`).
- GTK reference: `palette_grid.rs:10-41` uses integer pitch, `x=round(i*cell)`, `width=round((i+1)*cell)-x-GAP`, which gives exact 4px gaps.
- Fix direction: copy the `cell_bounds` arithmetic, with width from the next rounded x.

##### FX-32: Palette chooser active row ignores glass selection
- Class: GLASS   Confidence: CERTAIN   Severity: medium
- Windows: `PalettesView.cpp:437,446` `selectionBrush()` returns `brush("selection")` (opaque).
- GTK reference: `color_library.rs:938` uses `selected-tool`, and in glass mode `style.css:462` maps that to `--glass-selection`.
- Fix direction: use `selected(data)` (UiControls.h:410), which picks `glass("selection")`, and delete `selectionBrush`.

##### FX-33: Palette preview strip corner clip
- Class: SEAM, BORDER/COLOR   Confidence: CHECK   Severity: low
- Windows: `PalettesView.cpp:434-435` sets `StackPanel.CornerRadius(3)` with 12x18 child Borders. A panel CornerRadius rounds its own background, not necessarily the children.
- GTK reference: `style.css:494` radius 3 with `set_overflow(Hidden)` (`color_library.rs:928`).
- Fix direction: wrap the strip in a Border with CornerRadius 3, or round the first and last cells.

##### FX-34: Palette error/notice colours are literals
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: low
- Windows: `PalettesView.cpp:45-46,217` use `Color{255,0xee,0x55,0x55}` for both errors and the editor ring. The notice uses `tint(text,179)`.
- GTK reference: the validation label uses Adwaita `.error` (`color_library.rs:216`), which depends on the theme. The editor ring `#e55` (`style.css:489`) matches.
- Fix direction: use a shared `error` palette role, if one exists, for the label text.

##### FX-35: Palette footer selector arrow is 12px
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `PalettesView.cpp:531` `icon(L"chevron-down",theme,12)` rotated.
- GTK reference: `color_library.rs:177` `pan-up-symbolic` at the dock-panel icon size (`style.css:189` 14px), with metric icon_size 16.
- Fix direction: use `panelMetric("icon_size")`.

##### FX-36: Preview panel (Refine / Frequency Separation) literals and unstyled Cancel
- Class: METRIC, DEFAULT-CONTROL   Confidence: CERTAIN   Severity: low
- Windows: `PreviewPanel.h:37-41`. Buttons use `Padding{12,5,12,5}`, `MinHeight 34`, spacing 8, body padding `{16,14,16,12}`, spacing 10. Cancel uses `button()` with a clear fill.
- GTK reference: `preview_dialog.rs:35-41` is an `adw::AlertDialog` bottom sheet with standard response buttons (both raised).
- Fix direction: take the dialog metrics from the shared notice/dialog metrics and give Cancel the raised fill (FX-16).

##### FX-37: Menu item heights are inconsistent literals
- Class: METRIC   Confidence: CERTAIN   Severity: low
- Windows: `EffectView.cpp:236` `MinHeight(32)` for calibration group items. `PalettesView.cpp:186` `MinHeight(34)`.
- Fix direction: use one menu-row metric, the same one NativeMenuItems uses.

#### Root causes / systemic patterns

1. **Fixed drawer body heights** (`PanelBody.cpp:222-224`, `WorkspaceDrawers.cpp:348-353`): exact `Height(440/480/272)` instead of filling the column's placement height with a minimum. This closes FX-1 (bug 21), FX-3 and the adjustments 480-vs-440 mismatch. Replace it with one fill-height path in `placeColumns`.
3. **Remaining helper adoptions**: use `raisedButton()` and `fieldColors()` for FX-2, FX-13, FX-16 and FX-36.
4. **Corner, metric and spacing literals in place of panel metrics**: 6px palette radii, 56/36/44/18/12 sizes, RowSpacing 0/10, and `.85` axis text. Using `controlCorner()` and `panelMetric()` everywhere closes FX-8 to FX-11, FX-15, FX-17, FX-20, FX-23, FX-29, FX-35 and FX-37.
5. **Opaque roles where GTK uses glass-aware classes**: `brush("selection")` and `brush("bg")` where `selected(data)` or the panel surface role should be used (FX-32, FX-28).
