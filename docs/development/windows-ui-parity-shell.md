# Windows UI parity: primitives and shell

[Windows development](windows.md)

Bring the Windows client's shared WinUI helpers, workspace shell and colour
panels to visual parity with GTK. The plan also fixes reported bugs 1, 2, 3,
4, 11, 13, 15, 18 and 19 and owns the [helper contract](#helper-contract) the
other two plans build on.

## Prompt

> You are Agent A for the Windows UI parity work in Capy Canvas. Read
> `AGENTS.md`, `apps/layer-windows/AGENTS.md` and
> `docs/development/windows-ui-parity-shell.md`. Implement that plan on
> Windows. Land the helper contract (P1–P8) first and push it, because
> Agents B and C are working `windows-ui-parity-panels.md` and
> `windows-ui-parity-pages.md` in parallel on other machines and depend on it.
> Edit only the files your plan owns. Fix root causes, add regression checks,
> push each milestone to `origin/main` as soon as its checks pass, and remove
> each finding from the plan in the commit that fixes it. Reproduce CHECK
> findings before fixing them, or drop them. Finish with a list of what you
> fixed, what you dropped and what you could not run.

## Files this plan owns

`UiControls.h`, `ButtonFeedback.h`, `ChoiceFeedback.cpp/.h`,
`ScrollIndicators.h`, `PanelScroll.h`, `NativeMenus.h`, `CommandSearch.h`,
`PickerDialog.h`, `NumberControl.cpp`, `RangeControl.cpp/.h`, `TextInput.cpp`,
`NativePresentation.h`, `App.cpp`, `WorkspaceView.cpp/.h`,
`WorkspaceDrawers.cpp/.h`, `WorkspaceTabDrag.cpp/.h`, `WorkspaceTabSurface.h`,
`WorkspaceSurface.h`, `WorkspaceSurfaceGeometry.h`, `WorkspaceGeometry.h`,
`WorkspaceShadow.cpp/.h`, `WorkspaceExpansion.cpp/.h`,
`WorkspaceGestures.cpp/.h`, `WorkspacePublication.h`, `WorkspaceQuery.h`,
`PanelBody.cpp/.h`, `PanelBridgeSurface.h`, `CollapsedColumns.cpp/.h`,
`OverviewOcclusion.cpp/.h`, `ChromeVisibility.h`, `WorkflowMotion.h`,
`PanelConfiguration.cpp/.h`, `ColorView.cpp/.h`, `ColorEditor.cpp/.h`,
`ColorWheel.h`, `ColorStrip.h`, `ColorPair.h`, `ColorTouch.h` (all under
`apps/layer-windows/`), plus shared panel layout in `crates/layer-workspace`
and colour view models in `crates/layer-ui`.

Changing a helper signature may require a mechanical edit at call sites in
other plans' files. Make that edit in the same commit, keep it mechanical (no
visual retuning there), and push it promptly so the others rebase onto it early.
Delete per-site workarounds the new helpers supersede in the same commit, e.g.
the `alignNames` baseline shift in `WorkspaceManagerView.cpp` and the
duplicated `DropDownOpened`/`DropDownClosed` popup blocks.

## Order of work

1. **Helper contract, P1–P8** (PRIM-1, 2, 3, 6–15, 17–19, 23; COLOR-1).
   P1 fixes bugs 2 and 13 everywhere. P3 fixes bug 11. Verify the canvas bar
   "Scale" segment and the About rows before and after with a zoomed capture:
   the gaps above and below the glyphs should match within 0.5 px. Push.
2. **Drawer lifecycle** (SHELL-1, 2, 3, 4, 26; TOOL-2): port GTK
   `Drawer::refresh`. Retire a drawer only when its model is gone, keep the
   measured heights, animate only on open, close or resize, and don't rebuild
   on theme change. Fixes bug 1 and the drawer half of bug 19. Then SHELL-24
   (double-tap routing).
3. **Pixel snapping** (SHELL-5, 17, 25, 16): one rounding step for frames,
   drawers, bridges and columns, with the same rounded bounds passed to
   `capy_panel_bridges`. Fixes bugs 3 and 15.
4. **Glass in transient states** (SHELL-6, 7, 14): tab slide preview and
   expanded panels use glass roles. Fixes bug 4. Leave vertical resize
   (bug 16) as recorded in SHELL-7.
5. **Drawer sizes** (FX-1, FX-3, SHELL-9, SHELL-10): panels that fill a drawer
   column take the column's height with the GTK minimum. Fixes the drawer half
   of bug 21. Make the drawer width breakpoint in shared Rust independent of
   display scaling (bug 18), with a headless test.
6. **Shell metrics and headers** (SHELL-11–13, 15, 18–23): one tab-header
   builder and catalog metrics.
7. **Colour panels** (COLOR-2 onward).
8. Remaining PRIM items (PRIM-4, 5, 16, 20–22, 24).

## Validation

- `apps/layer-windows/scripts/test-without-gpu.ps1`;
  `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib`;
  `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`.
- Fixtures: `exercise-button-feedback`, `exercise-popup`, `exercise-tooltips`,
  `exercise-scroll-indicators`, `exercise-drawers`, `exercise-drawer-layout`,
  `exercise-layer-drawer`, `exercise-tab-drag`, all `exercise-tab-pickup`
  variants, `exercise-panel-bridge-layout`, `exercise-column-stacks`,
  `exercise-transparency`, `exercise-expansion`, `exercise-workspace-layout`,
  `exercise-color-editor`, `exercise-color-picker`, `exercise-compact-color`.
  Because P1–P3 change every page, also run `exercise-canvas-bar`,
  `exercise-shortcut-page` and `exercise-header-settings` after step 1.
- Journeys, light and dark, glass on and off: switch brush categories with the
  sketch drawer open (no replay); double-tap in the layers drawer list (drawer
  stays); drag a floating tab sideways (glass); open a single panel from a
  collapsed column group (no bridge gap at 100%, 125% and 150% scaling).
- Drawer animation and tab drag affect how frames are produced: measure the
  affected rows by [measuring](../performance/measuring.md) on the Windows
  reference device and record them in the tier tables.

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
- One finding or one class per milestone. Push each milestone after its checks
  pass; fetch and rebase onto `origin/main` before every push.
- Remove a finding from your plan in the commit that fixes it. Record dropped
  CHECK items (not reproduced) in the commit message. Delete your plan in the
  commit that closes its last item.
- For every visual fix, compare light and dark themes, with glass transparency
  on and off, against the GTK values quoted in the finding. Add assertions to
  the existing `exercise-*.ps1` fixture for the area instead of new harnesses.
- Report what you could not run.

## Findings

### PRIM audit: shared Windows UI primitives and typography

Scope: apps/layer-windows/UiControls.h, ButtonFeedback.h, ChoiceFeedback.cpp/.h, ScrollIndicators.h, PanelScroll.h,
NativeMenus.h, CommandSearch.h, PickerDialog.h, NumberControl.cpp, RangeControl.cpp/.h, TextInput.cpp, KeyNames.h,
NativePresentation.h, App.cpp, plus a tree-wide sweep of call sites.

I checked the WinUI template facts against the exact package the project uses:
`~/.nuget/packages/microsoft.windowsappsdk.winui/2.3.9/lib/native/Microsoft.UI/Themes/generic.xaml` (cited as `generic.xaml:N`).
I also computed the Segoe UI metrics for the 11 pt (14.67 px) UI size: unitsPerEm 2048, ascent 2210, descent 514, cap 1434, x-height 1024.

TextInput.cpp, KeyNames.h and NativePresentation.h have no visual surface, and I found no parity differences in them.

---

#### VCENTER: text and icon vertical centring

##### PRIM-1: Segoe UI text sits low in every symmetric box, and Windows helpers drop Fluent's compensation
- Class: VCENTER   Confidence: LIKELY (the metric arithmetic is certain; how it compares with GTK needs a screenshot)   Severity: high   Reported bug: 2, 13 (and contributes to 10, 12)
- Windows:
  - `UiControls.h:424-430` `label()` uses `FontFamily("Segoe UI")` at `textSize()` = 11 pt*96/72 = 14.67 px. The line box is ascent+descent = 1.330 em = 19.5 px.
  - Within that box, cap-height ink centres 0.064 em (about 0.9 px) below the box centre, and x-height ink (lower-case words such as "scale") centres 0.164 em (about 2.4 px) below it.
  - Fluent offsets this with templates that pad about 2 px less on top: `ComboBoxPadding 12,5,0,7` (generic.xaml:34570), `TextControlThemePadding 10,5,6,6` (:178), `ComboBoxItemThemePadding 11,5,11,7` (:133), `ButtonPadding 11,5,11,6` (:8463), `MenuFlyoutItemThemePadding 11,9,11,10` (:1533).
  - Our helpers replace those paddings with symmetric ones:
    - `button<T>` sets `Padding(Thickness{0})` (`UiControls.h:470`) and centres an implicit TextBlock.
    - Sites use symmetric paddings, for example `CanvasActionBar.h:75` `worded()` → `{padding,0,padding,0}`, `ToolView.cpp:158/222`, `FilterView.cpp:56` and `ScopesView.cpp:151` ComboBox `{6,0,0,0}`, `SettingsView.cpp:240` rows `{14,8,14,8}`, and `NumberControl.cpp:233` entries `{6,textInset,6,textInset}`.
  - Result: text renders up to about 1–2.4 px below its optical centre.
- GTK reference: `style.css:185/227/299` uses `padding: 0 6px` / `0 var(--panel-field-padding)`, symmetric, and Pango centres the logical box of the GTK font. Adwaita Sans/Inter has hhea 0.969/0.242 and cap 0.727, so caps sit exactly centred and lower-case only 0.09 em low. Windows therefore draws about 1 px lower than GTK for the same symmetric box.
- Shared source: `panel_metrics.control_height` 26, `field_padding` 6 (crates/layer-ui/src/layout.rs:52-62).
- Fix direction: put the correction once in the text primitives, not per site.
  - Make `button<T>` build its content with `label()` instead of `box_value(text)`.
  - Give `label()` a single optical correction derived from the font, for example a `RenderTransform` TranslateY of `-(ascent - descent - capHeight)/2` (about -0.9 px), or `TextLineBounds::Tight` for single-line labels, so it does not move layout.
  - Use the same rule in `NumberControl` entries and `comboOptionText`.
  - `WorkspaceManagerView.cpp:280-289` already computes this kind of baseline shift by hand from `ITextRange::GetPoint` for one field (`Padding({fieldPadding,padding+shift,fieldPadding,padding-shift})`). Generalise that into the helper and delete the one-off.
  - Verify against a GTK screenshot of the canvas-bar "scale" segment and the About rows.

##### PRIM-2: `label()` LineHeight(18)+MaxHeight has no effect at 11 pt; the value only misleads
- Class: VCENTER, METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `UiControls.h:427` sets `LineHeight(18); LineStackingStrategy::MaxHeight`.
  - With MaxHeight, LineHeight is only a minimum. The natural Segoe UI line is 19.5 px, larger than 18, so every `label()` is 19.5 px tall (fractional) whatever the 18.
  - The same no-op overrides appear at these call sites (each sets LineHeight below the natural height or sets 0):
    - `HeaderStatus.h:33` (`LineHeight(14)`)
    - `ToolView.cpp:481` (`LineHeight(textSize())`)
    - `SettingsView.cpp:248,451`, `ShortcutPage.cpp:26` (`LineHeight(15)` at 12.2 px, natural 16.3)
    - `NumberControl.cpp:116`
    - `TextureDialog.cpp:172`, `NewDrawingDialog.cpp:399`, `ToolbarComponents.cpp:382/387` (LineHeight 0)
  - These sites go further than a no-op:
    - `FilterView.cpp:145` switches to `BlockLineHeight` with 18, which compresses the line box below the font's extent and moves the baseline.
    - `NumberControl.cpp:107/135` and `LayerRow.cpp:184-185` set `LineHeight(20)`, half a pixel above natural, so the read-only number display (20 px) and the TextBox editor (19.5 px measured, `NumberControl.cpp:226` uses LineHeight 0) differ by about 0.25 px.
- GTK reference: `style.css:22` `label { font-size: var(--ui-text-size) }`, with no line-height.
- Fix direction: remove the LineHeight/LineStackingStrategy defaults from `label()` and the per-site no-op overrides. Snap label height to whole pixels in the helper, or rely on the PRIM-1 correction. Drop the `FilterView.cpp:145` BlockLineHeight override.

##### PRIM-3: icons hosted in a bare `ContentControl` are top-aligned (bug 12 and bug 10 root cause)
- Class: VCENTER   Confidence: CERTAIN   Severity: high   Reported bug: 10, 12
- Windows: the WinUI `ContentControl` default style is `VerticalContentAlignment=Top` (generic.xaml:8553). These sites wrap `icon()` in a `ContentControl` that stretches to the row height, so the 16 px image pins to the top:
  - `SettingsView.cpp:199`: Preferences sidebar `tabIcon` in a Grid next to `tabLabel` (19.5 px or taller when it wraps). The icon sits at y=0 instead of centred, so it is 1.75 px high or more and worse with wrapped titles. This is bug 12.
  - `SettingsView.cpp:336`: preference choice dropdown (cursor shape). `ContentControl glyph` sits in a horizontal StackPanel beside `label()`. Bug 10 comes from four offsets adding up:
    - the icon is top-aligned, 1.75 px high;
    - Segoe text ink is low, about 1–2 px (PRIM-1);
    - in the closed ComboBox the default `ComboBoxPadding 12,5,0,7` lifts the content another 1 px;
    - in the popup, `ComboBoxItemThemePadding 11,5,11,7` does the same.
  - `LayersView.cpp:53`: blend-mode button `chevron` ContentControl beside a centred `blendLabel`, so the chevron is 1.75 px high.
- GTK reference: GTK images and labels in a box use `valign` Center by default, as in the `preferences.rs` sidebar rows and `.dock-panel dropdown > button`.
- Fix direction: drop the ContentControl wrappers. Keep a retained `Image` and re-theme it with the existing `setIcon(image,name,theme)` helper (`UiControls.h:193`), which already exists for this purpose. If a wrapper must stay, set `VerticalContentAlignment(Center)`. Also give option rows in a ComboBox a symmetric padding through `choiceColors` (PRIM-10).

##### PRIM-4: number entry error border shifts the text by 1 px
- Class: VCENTER, BORDER/COLOR   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `NumberControl.cpp:61-65` `showNumericError` toggles `entry.BorderThickness` between `{0}` and `{1,1,1,1}`.
  - The TextBox template sets the text host's margin from BorderThickness: `ContentElement Margin="{TemplateBinding BorderThickness}"` (generic.xaml, DefaultTextBoxStyle, `ScrollViewer x:Name="ContentElement"`).
  - Showing an error therefore moves the text 1 px down and in, inside a fixed-height field.
  - The colour is `{221,85,85}` = #dd5555.
- GTK reference: `style.css:252` `.number-control.error entry { box-shadow: inset 0 0 0 1px #e55; }` draws the ring without moving layout, in #ee5555. `docs/ui/theme-colors.md` says the invalid numeric border is `#ee5555`.
- Fix direction: draw the error ring as an overlay Border (or keep a constant transparent 1 px border and change only BorderBrush), and use `#ee5555`. A shared semantic role would be better; the `palettes` entry error uses the same rule.

##### PRIM-5: `compactCheck` box is 20 px and uses Fluent geometry; GTK is 16 px
- Class: METRIC, DEFAULT-CONTROL   Confidence: CERTAIN (template), LIKELY (visual)   Severity: medium   Reported bug: none
- Windows:
  - `UiControls.h:431-435` keeps the default CheckBox template: `CheckBoxSize 20` (generic.xaml:29311), glyph `Margin 0,1,0,-1`, system-accent fill (`CheckBoxCheckBackgroundFillChecked`, not overridden), Fluent stroke when unchecked.
  - MinHeight is `controlHeight()` (26) and the gap is `Padding{control_spacing,0,0,0}` (4).
  - Call sites:
    - `EffectView.cpp:184`, `ScopesView.cpp:218`, `ToolView.cpp:432`, `ToolbarComponents.cpp:829`, `ProofPanel.h:62`;
    - raw `CheckBox` without the helper: `HeaderView.cpp:587`, `PanelConfiguration.cpp:162` (default `CheckBoxPadding 8,5,0,0` and `VerticalContentAlignment Top`), `DocumentView.cpp:354`, `ExportForm.h:11`, `ProofForm.h:11`, `SizeDialog.cpp:16`, `WorkspaceDialogs.cpp:147`, `CanvasWindow.h:75`.
- GTK reference:
  - `style.css:267` `.dock-panel checkbutton check { min 16px; -gtk-icon-size 16px; padding 0; border-radius 4px }`, with the check icon `layer-check-symbolic` (:268).
  - `style.css:292` `checkbutton { min-height 18px; padding 4px 0 }`.
  - Checked fill is the resolved palette accent.
- Fix direction: give `compactCheck` a template or resource set: 16 px box, `layer-check` icon, `accent`/`accent_foreground` brushes, palette text overlay for the unchecked stroke. Route the raw CheckBox sites through it.

---

#### DEFAULT-CONTROL: WinUI templates and colours that leak through

##### PRIM-6: no app-level theme resources, so every unthemed WinUI part uses Fluent and the system accent
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN   Severity: high   Reported bug: 11, 14 (enables 10)
- Windows: `App.cpp:136` only merges `XamlControlsResources()`.
  - No `ThemeDictionaries` exist, and no `SystemAccentColor` / `AccentFillColor*` override.
  - Theme is followed only through `RequestedTheme` on the root (`CanvasWindow.cpp:1820`).
  - All other colour work is per-instance resource insertion. A grep for resource keys in the tree finds only:
    - `ChoiceFeedback.cpp:45-48` (ComboBox body);
    - `NumberControl.cpp:124-126, 280-296` (TextControl…Disabled, Slider…);
    - `WorkspaceManagerView.cpp:323` (TextControlBackground/PointerOver/Focused for one entry);
    - `PickerDialog.h:10-13` (ContentDialog sizes);
    - `UiControls.h:420` (ContentDialogTopOverlay).
  - So whenever the palette accent differs from Windows' accent, every CheckBox, ToggleSwitch, RadioButton, ComboBox item pill, ListView selection pill and TextBox focus underline shows the system accent.
- GTK reference: `style.css:8` maps every libadwaita variable to palette roles in one place: `--accent-bg-color: var(--capy-accent)`, `--popover-bg-color: var(--capy-panel)`, `--window-fg-color`, `--dialog-bg-color` and so on. Native widgets inherit them.
- Fix direction: build one resource dictionary from the palette brushes when the palette is applied, at the window root or the Application. They are already mutable cached `SolidColorBrush`es in `WorkspaceData`, so `refreshPalette` keeps them current. Map at least:
  - `SystemAccentColor`, `AccentFillColorDefault/Secondary/Tertiary`;
  - `ComboBoxDropDownBackground/BorderBrush`, `ComboBoxItem*`, `ComboBoxItemPillFillBrush`;
  - `MenuFlyoutPresenterBackground/BorderBrush`, `MenuFlyoutItemBackgroundPointerOver/Pressed`;
  - `FlyoutPresenterBackground`;
  - `TextControlBackground*/BorderBrush*`;
  - `ToggleSwitch*`, `CheckBox*`, `ListViewItemBackground*`, `ToolTipBackground/Foreground`.
  Then delete the per-control insertions that duplicate it.

##### PRIM-7: ComboBox popup, items, selection pill and chevron are unthemed
- Class: DEFAULT-CONTROL, HIGHLIGHT-GEOMETRY, METRIC   Confidence: CERTAIN (code/template), LIKELY (visual)   Severity: high   Reported bug: 14 (popup item hover), 10
- Windows:
  - `ChoiceFeedback.cpp:42-53` overrides only the closed body (Background, Foreground, PointerOver/Pressed/Disabled/Focused, glyph foreground, border).
  - Never overridden:
    - `ComboBoxDropDownBackground` = `AcrylicInAppFillColorDefaultBrush` (generic.xaml:691);
    - `ComboBoxDropDownBorderBrush`;
    - all `ComboBoxItem{Foreground,Background}{PointerOver,Pressed,Selected,…}`, Fluent subtle fills;
    - `ComboBoxItemPillFillBrush` = system accent (:2632).
  - Item layout: `LayoutRoot Margin="5,2,5,2"` plus `ComboBoxItemThemePadding 11,5,11,7`. The hover plate is inset 5 px horizontally but 2 px vertically, with the popup content margin `0,4`.
  - The chevron is a 12 px Segoe Fluent glyph `E70D` at `Margin 0,0,14,0` in a fixed 38 px column (generic.xaml ComboBox template).
  - `comboOptionText` (`UiControls.h:444-452`) creates a bare TextBlock with no FontFamily/FontSize. Items inherit `XamlAutoFontFamily` (Segoe UI Variable) and 14 px unless the site sets FontSize. Only `ToolView`, `FilterView` and `ScopesView` do, so labels and dropdowns mix Segoe UI 14.67 with Segoe UI Variable 14. `ToolView.cpp:158/222`, `FilterView.cpp:56` and `ScopesView.cpp:152` set Bold on the ComboBox, which items inherit.
- GTK reference:
  - `style.css:8` `--popover-bg-color: var(--capy-panel)` (dark #414141, light #ededed; `theme-colors.md` "popovers/menus").
  - `style.css:15` popover border `--line-popover-border` (palette `lines.popover_border`).
  - Item hover alpha(fg,.08), pressed .16 (`style.css:574-632`).
  - The dropdown arrow is a 16 px icon with `padding: 0 6px` (`style.css:299`).
- Fix direction: theme these from PRIM-6's dictionary:
  - `panel` popup background;
  - `line(L"popover_border")` border, a role Windows never reads today;
  - text 8%/16% item hover and press;
  - `selection` for the selected item with no accent pill;
  - item padding and margin `0,0` with `field_padding`.
  Let `choiceColors` also own Padding/Height/FontSize/FontFamily (PRIM-10).

##### PRIM-8: `choiceColors` always paints `input`, so dropdowns vanish on dialog and settings surfaces (bug 11 root cause)
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN   Severity: high   Reported bug: 11
- Windows: `ChoiceFeedback.cpp:65-66` uses `control.Background(drawing ? tint(text,18) : brush(input))`.
  - `input` and `settings` are the same colour: dark both `#333333`, light `#fafafa` vs `#fafafb` (crates/layer-ui/src/theme.rs:359-364).
  - The Keyboard Shortcuts filter dropdowns sit directly on the settings page (`ShortcutPage.cpp:194-195, 253, 257`, placed in the `filters` grid, not in a card), so their face is invisible.
  - Same exposure, on dialog or settings surfaces or cards: `SettingsView.cpp:331`; `DocumentView.cpp:214,354,434,449,496,513,520`; `SizeDialog.cpp:46`; `WorkspaceManagerView.cpp:189`; `PanelConfiguration.cpp:41`.
- GTK reference: `docs/ui/theme-colors.md`: "Search field, text fields on a dialog surface … Text at 10% … fields inside cards and on panels use the input background". Dropdown faces use `control_fill` (text 6%/16%), and libadwaita's default dropdown button in `layer-preferences` is alpha(currentColor,.1). The new-drawing dialog uses alpha .07 (`style.css:555`), which is why only `drawing=true` works.
- Fix direction: replace the `bool drawing` flag with the surface the control sits on (panel/card → `input`; dialog/settings → `tint(text,26)`, or `control_fill` per the doc), so the class is fixed in one helper. Add a windowless check that `choiceColors` on the settings surface gives a face different from `settings`.

##### PRIM-9: TextBox hover and focus states replace the input fill and add a Fluent accent underline
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN (template), LIKELY (visual)   Severity: medium   Reported bug: none
- Windows:
  - The DefaultTextBoxStyle visual states set `BorderElement.Background` to `TextControlBackgroundPointerOver` / `TextControlBackgroundFocused` (`ControlFillColorInputActiveBrush`, generic.xaml:877-878).
  - They also set `BorderElement.BorderBrush` to `TextControlBorderBrushFocused` (accent) and `BorderThickness` to `TextControlBorderThemeThicknessFocused 1,1,1,2` (:1965).
  - These override the TemplateBinding Background/BorderThickness the code sets:
    - Number entries (`NumberControl.cpp:123`) override only the `…Disabled` keys (`:124-126`), and `entry.Background(input)` on GotFocus (`:463`) is cancelled by the Focused state.
    - Only `WorkspaceManagerView.cpp:323` overrides `TextControlBackground{,PointerOver,Focused}`. That shows the class is known but was fixed for one instance.
  - Completely unstyled TextBoxes (default Fluent fill and stroke):
    - `CommandSearch.h:60` command entry;
    - `SettingsView.cpp:150` search and `:355` hex entry;
    - `ShortcutPage.cpp:45` search and pickerSearch;
    - `WorkspaceDialogs.cpp:21` name and search;
    - `WorkspaceManagerView.cpp:42` search, name and description;
    - `ColorEditor.cpp:35,70` entry and search;
    - `DocumentView.cpp:434,522` profileSearch and name;
    - `PalettesView.cpp:216` and `TextureDialog.cpp:323` rename inputs.
  - Partly styled TextBoxes, which set Background but not the state keys: `FilterView.cpp:67-68`, `PalettesView.cpp:515/542`, `NewDrawingDialog.cpp:253 input()`, `LayerRow.cpp:189`.
- GTK reference: `style.css:228` `entry.number-entry { border:0; background: var(--input-bg) }`, `:498` `popover.command-bar entry { background: var(--input-bg) }`, `:371-372` `.preference-entry { background: var(--input-bg) } :focus-within { box-shadow: none }`. Focus is libadwaita's accent outline on the same fill.
- Fix direction: one `textField(data, surface)` helper, or a PRIM-6 dictionary entry, that sets `TextControlBackground{,PointerOver,Focused,Disabled}` to the surface fill. It should set `TextControlBorderBrush*` to clear and `TextControlBorderThemeThicknessFocused` to 0. If a focus ring is wanted, use the palette accent as GTK does. Remove the `WorkspaceManagerView` one-off and the futile GotFocus background set.

##### PRIM-10: ComboBox padding, height and font are left to each call site
- Class: VCENTER, METRIC, ALIGNMENT   Confidence: CERTAIN   Severity: medium   Reported bug: 10, 14
- Windows: `choiceColors` sets only colours, border and corner, so geometry varies by site.
  - Sites that set `Padding {6,0,0,0}` and `Height controlHeight()`: `ToolView.cpp:158,222`, `FilterView.cpp:56`, `GradientView.cpp:104-105`, `EffectView.cpp:174-175,286`, `ScopesView.cpp:151`.
  - Sites that keep Fluent `ComboBoxPadding 12,5,0,7` (12 px inset, 1 px upward bias) and `ComboBoxMinHeight 32`: `SettingsView.cpp:331`, `ShortcutPage.cpp:195`, `DocumentView.cpp:*`, `PanelConfiguration.cpp:41`, `WorkspaceManagerView.cpp:189`, `SizeDialog.cpp:46`, `NewDrawingDialog.cpp:253-258,470` (`input()` sets Height but not Padding).
- GTK reference: `style.css:299` `.dock-panel dropdown > button { min-height: var(--panel-control-height); padding: 0 6px }` and `:555` `.new-drawing-dialog dropdown > button { min-height 34px; padding 0 10px }`.
- Fix direction: `choiceColors(data, control, surface)` sets the padding (`field_padding`, symmetric, with the PRIM-1 correction), the height (`controlHeight()` on panels, 34 on dialogs), and the label FontFamily/FontSize/weight. Remove the per-site copies.

##### PRIM-11: MenuFlyout and Flyout presenters use Fluent acrylic, borders and item geometry
- Class: DEFAULT-CONTROL, METRIC   Confidence: CERTAIN (code), LIKELY (visual)   Severity: medium   Reported bug: none
- Windows:
  - `NativeMenus.h:57-62` sets only `FontSize(textSize())` and `MinHeight(34)` (a literal) per item. FontFamily stays XamlAutoFontFamily (Segoe UI Variable), not `label()`'s Segoe UI.
  - The presenter uses `MenuFlyoutPresenterBackground = DesktopAcrylicTransparentBrush` (generic.xaml:1537), the Fluent border, `MenuFlyoutItemBackgroundPointerOver = SubtleFillColorSecondary` (:1540), `OverlayCornerRadius 8`, and item padding `11,9,11,10` (touch) or `11,4,11,7` (narrow).
  - The only presenter style anywhere is the MaxHeight setter in `HeaderView.cpp:255-257`.
  - Plain `Flyout`s use `FlyoutPresenterBackground = AcrylicInAppFillColorDefaultBrush` (:908) and `FlyoutContentThemePadding 12,11,12,12` (:153): `LayersView.cpp:238`, `HeaderView.cpp:394`, `NewDrawingDialog.cpp:118`, `ToolbarComponents.cpp:537,619`.
  - Menu sites include `BubbleInput.cpp:259`, `ColorView.cpp:216,420`, `EffectView.cpp:233,322`, `HeaderInput.cpp:180`, `HeaderView.cpp:242`, `LayersView.cpp:353,380`, `PalettesView.cpp:170`, `PanelConfiguration.cpp:196`, `SettingsView.cpp:428`, `ShortcutPage.cpp:231`, `TextureDialog.cpp:198`, `ToolView.cpp:262`, `ToolbarComponents.cpp:785`, `WorkspaceGestures.cpp:609`.
- GTK reference: `--popover-bg-color: var(--capy-panel)` (`style.css:8`), popover border `--line-popover-border` (`:15`), libadwaita menu rows of about 30 px with 0 12px padding, hover alpha .08.
- Fix direction: PRIM-6 dictionary entries for presenter background, border and item states. Move the 34 px literal into a shared menu row metric and set the menu font family with the label font.

##### PRIM-12: ToggleSwitch, RadioButton and ListView items are unthemed
- Class: DEFAULT-CONTROL, HIGHLIGHT-GEOMETRY   Confidence: CERTAIN (code), LIKELY (visual)   Severity: medium   Reported bug: 14 (ListView hover inset)
- Windows:
  - `SettingsView.cpp:267` and `ShortcutPage.cpp:469` use a default ToggleSwitch: `ToggleSwitchFillOn = AccentFillColorDefaultBrush` (:520), and the off state is an outlined ring with no fill.
  - `WorkspaceDialogs.cpp:147` uses a default RadioButton.
  - `WorkspaceManagerView.cpp:44,300` uses a ListView/ListViewItem with the default `ListViewItemPresenter`. Its backplate is rounded and inset 4,2 inside the item, with Fluent hover/selected fills and an accent selection-indicator pill (`DefaultListViewItemStyle`, Padding 16,0,12,0). The site sets only `Padding 0`, `MinHeight 56`, `BorderThickness 0`, so hover and selection plates have different inset margins from the row content. That is the likely cause of bug 14.
- GTK reference: `docs/ui/theme-colors.md` "Settings slider inactive track / inactive switch: Text at 12% / 20%"; accent = palette accent. `.workspace-manager-row` buttons hover alpha .08 (`style.css:582-583`).
- Fix direction: PRIM-6 dictionary for `ToggleSwitch*` (off fill text 20%, on fill accent) and `ListViewItemBackground*`. Set `SelectionIndicatorVisualEnabled=false` and a zero-margin presenter so hover and selection fill the row like GTK.

##### PRIM-13: preference Slider uses accent for the thumb and `input` for the inactive track
- Class: BORDER/COLOR, DEFAULT-CONTROL   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows:
  - `NumberControl.cpp:291-296`: for `preference`, `SliderThumbBackground*` = `accent` and `SliderTrackFill*` = `input`.
  - The Fluent outer thumb ring (`SliderOuterThumbBackground`, thumb size) is left at default for preference sliders (`:284-289` only clears it when `!preference`).
- GTK reference: `style.css:233-234` `.number-control scale slider { min 16px; margin -6px; background: var(--capy-thumb) }`, `:265` `.number-preference scale trough { background: alpha(fg,.12) }`. `theme-colors.md`: "Settings slider inactive track: Text at 12%", thumb role `thumb`.
- Fix direction: preference thumb `brush(thumb)` 16 px with no Fluent outer ring, track `tint(text,31)`, value fill accent.

##### PRIM-14: ContentDialog footer buttons use `control_fill` and default Fluent padding and weight
- Class: BORDER/COLOR, METRIC, VCENTER   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `ChoiceFeedback.cpp:55-63` `footer()` paints non-default buttons `control_fill` (text 6%/16%) and leaves `DefaultButtonStyle` `Padding 11,5,11,6`, MinHeight and FontWeight Normal. `pickerButton` (`PickerDialog.h:31-38`) paints the same role with `buttonBackground` = `tint(button,13)` at `MinHeight 34`, `Padding 16,0`, Medium. Two fills and geometries for one role.
- GTK reference: `style.css:511` `.dialog-footer > button, .dialog-actions > button { min-height 34px; padding 0 16px; border-radius control-radius; font-weight 500 }`; `theme-colors.md` "Plain button fill: White/Black at 13/255".
- Fix direction: `footer()` reuses `pickerButton` styling: `buttonBackground`, 34/16/Medium.

##### PRIM-15: `button<T>` has no face, so plain buttons look like text (bug 9 root cause)
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN (code), LIKELY (bug 9)   Severity: medium   Reported bug: 9
- Windows:
  - `UiControls.h:472` `button<T>` sets `Background(clear())`, so every button is faceless unless the site adds `buttonBackground`.
  - `SettingsView.cpp:208-213` "Manage profiles" is a centred standalone faceless button (`Padding 10,5,10,5`, MinHeight 34, Bold). `actionButton` (`SettingsView.cpp:36-41`) has the same problem.
- GTK reference: `preferences.rs:925-939` puts the button as a suffix of an `adw::ActionRow` (title `library_title`, subtitle `menu_help`) inside a PreferencesGroup card. The default libadwaita button has a filled face; `theme-colors.md` "Plain button fill".
- Fix direction: give the helper an explicit variant: `button(data, text, action, Face::Plain)` applies `buttonBackground` and `Face::Flat` keeps clear. Build the Manage-profiles row like any other preference row (title, description, trailing button) instead of a free-floating button.

##### PRIM-16: command search entry, frame border and literals
- Class: DEFAULT-CONTROL, BORDER/COLOR, METRIC, VCENTER   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows (`CommandSearch.h`):
  - `:60` entry is a default TextBox (PRIM-9) with `MinHeight 32` and `Padding {32,5,6,6}` (asymmetric 5/6, inherited from Fluent).
  - `:66` close button is 32x32 (literal).
  - `:113` frame border is `tint(text,26)`.
  - `:138` row `CornerRadius {8,8,8,8}` (literal).
  - `:75` detail `FontSize(12)`, `Opacity .7`; `:134` shortcut `FontSize(12)`, `Opacity .6`; `:73` empty `Opacity .6`; `:63` unit `Opacity .7`.
  - `:71` `resultScroll` is not passed to `mouseScrollbars`, so touch shows the bar.
- GTK reference:
  - `command_bar.rs:48` `gtk::SearchEntry` with `style.css:498` `popover.command-bar entry { background: var(--input-bg) }`.
  - Popover border `--line-popover-border` (`style.css:15`).
  - Rows `.navigation-sidebar` (libadwaita row radius) and `dim-label` (.55) / `caption` classes for detail, shortcut, unit and empty (`command_bar.rs:54,78,84-85,372,381-382`).
- Fix direction: input-filled field through the PRIM-9 helper; `line(L"popover_border")` for the frame border; one `dim` opacity constant (.55); caption size `textSize()*0.8` instead of 12; `mouseScrollbars(resultScroll)`.

---

#### Other primitive differences

##### PRIM-17: ChoiceGrid paints `control_fill` on unselected cells and ignores disabled items
- Class: STACKED-BG, BORDER/COLOR, NULL-STATE   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows:
  - `UiControls.h:547` gives unselected cells `brush(control_fill)`, so every 18 px cell carries a text-6%/16% plate on the panel.
  - `ChoiceGrid::update` (`:542-551`) never reads `enabled`/`disabled_reason`, so disabled items stay clickable and their reason tooltip is lost.
- GTK reference: `style.css:319` `.choice-grid button { … background: transparent; border-radius: var(--control-radius) }`, `:317` checked → `--selection-bg` (glass `--glass-selection`, `:462`), `:320-321` icon alpha .45 / 1. `tool_extra.rs:111-115` sets `set_sensitive(item.enabled)` and the tooltip `disabled_reason`.
- Fix direction: unselected cells `clear()`. Apply `IsEnabled` and the tooltip through `explain()`/`explainable()` as other controls do. The 18 px cell, 6 px icon and 7 px corner already match GTK.

##### PRIM-18: ButtonFeedback uses system focus visuals
- Class: DEFAULT-CONTROL   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ButtonFeedback.h:95` `button.UseSystemFocusVisuals(true)` gives the Fluent double white/black focus rectangle. Hover 20/255 and press 41/255 match GTK .08/.16, and disabled 92/255 = .36 matches.
- GTK reference: libadwaita accent focus outline (`outline: 2px solid alpha(accent,.5)`).
- Fix direction: set `FocusVisualPrimaryBrush` to the accent and `FocusVisualSecondaryBrush` to clear in the shared feedback state, or in the PRIM-6 dictionary.

##### PRIM-19: tooltips use the Fluent theme ToolTip
- Class: DEFAULT-CONTROL   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `UiControls.h:509` creates `ToolTip` with the default `ToolTipBackground = SystemControlBackgroundChromeMediumLowBrush` (:732), which is light in light theme.
- GTK reference: `style.css:7` Adwaita tooltip `rgba(0,0,6,.8)`, radius 9, padding 6px 10px, white text.
- Fix direction: style the shared tooltip once in `tooltip()` or the PRIM-6 dictionary.

##### PRIM-20: NumberControl header spacing differs from GTK
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `NumberControl.cpp:98` `header.ColumnSpacing(panel ? 0 : compact ? control_spacing(4) : 6)`.
- GTK reference: `number_control.rs:543` `Box spacing: panel ? CONTROL_SPACING(4) : compact ? 2 : 6`.
- Fix direction: use the GTK values (panel 4, compact 2). Better, publish them in `panel_metrics` so neither host hard-codes them.

##### PRIM-21: NumberControl literals where GTK uses metrics
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows (`NumberControl.cpp`):
  - `:93` `valueHeight` `preference ? 34 : (ranged ? 24 : 32)` and `stepSize` `panel ? 16 : … 24 : 32`;
  - `:120` entry width `68/72/60`;
  - `:137` read-only margin `9/6`;
  - `:233` padding `9/6`;
  - `:258` focused width `92`;
  - `:591` panel track margins `{36,0,76,0}`;
  - `:99` value column `76`.
  GTK has the same numbers as CSS literals (`style.css:227,229,239,263`), so they agree today, but neither comes from `panel_metrics` (`field_padding` is 6).
- GTK reference: `style.css:227` `padding: 0 6px`, `:263` `padding: 0 9px`, `:239` `.number-panel .number-track { margin: -6px 76px 0 36px }`.
- Fix direction: no visual change needed. When touching this code, move 6, 9, 68, 76 and 36 into shared metrics (docs/ui/numeric-controls.md already names 36, 76 and 68).

##### PRIM-22: RangeControl has no disabled, focus or keyboard state
- Class: HIT-TEST, NULL-STATE   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows:
  - `RangeControl.cpp:303-354`: the track is a Canvas with no IsTabStop, no focus ring and no keys except Escape.
  - Thumbs are always `brush(text)` at alpha 1.
  - Geometry (28 tall, 8 px inset, 4 px trough, 6x14 half-handles at x-7/x+1, alphas .2/.65) matches GTK.
- GTK reference: `range_control.rs:47` thumb alpha `.4` when insensitive, `:54-56` 12x20 focus border when focused. Handles are focusable `gtk::Scale`s with keyboard stepping.
- Fix direction: add enabled opacity and a focus ring. Expose keyboard stepping through the shared numeric `step` operation, which RangeControl already calls for drags.

##### PRIM-23: PickerDialog and ContentDialog chrome keep Fluent border and smoke
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `UiControls.h:416-423` `dialogSurface` sets Background `settings` and clears `ContentDialogTopOverlay`. It leaves `ContentDialogBorderBrush` (Fluent stroke) and `ContentDialogSmokeFill` at default. `PickerDialog.h:6` `PickerDialogHeight=486.67` and `fitPicker` 280/720/24 are literals.
- GTK reference: `style.css:9-11` dialog lines from `palette.lines.settings`; `theme-colors.md` "modal dimming" and "dialog edge" roles.
- Fix direction: set the dialog border to `line(L"settings",L"border")` (already used by `pickerFooter`) and the smoke to the documented dimming.

##### PRIM-24: `icon()` call sites that overflow their host
- Class: METRIC, ALIGNMENT   Confidence: CHECK   Severity: low   Reported bug: none
- Windows:
  - `LayerRow.cpp:212` `grip=pick(L"",12,…)` is a 12 px-wide button with `grip.Height(16)` holding a 16 px `icon(L"grip")`. The image is wider than its button and is clipped or overflows.
  - `LayerRow.cpp:324` uses a 10 px link icon. GTK matches (`style.css:220-221`, 10px).
  - `NativeMenus.h:181-185` `ToolGroupMarker` is a 16 px icon pinned Bottom/Right; GTK uses the same asset.
- Fix direction: check `LayerRow.cpp:212` with a screenshot. Size the grip button to the icon or the icon to the button.

---

#### Tree-wide call sites with asymmetric vertical padding or margin (text- or icon-bearing)

These sites hand-tune vertical centring. Each one is a candidate for removal once PRIM-1, PRIM-3 and PRIM-10 are fixed in the helpers:

- `CommandSearch.h:60` entry `Padding{32,5,6,6}`
- `NewDrawingDialog.cpp:325,441,467` `Padding{10,7,10,0}` and `:663` `{4,7,10,0}`, `:673,693` `{10,7,0,0}`. These TextBoxes in fixed-height `input()` boxes centre text with a 7 px top pad and 0 bottom pad.
- `WorkspaceManagerView.cpp:289` `Padding{fieldPadding,padding+shift,fieldPadding,padding-shift}`, an instance fix of PRIM-1
- `SettingsView.cpp:149` `Padding{6,0,6,6}`; `:357` `Padding{0,inlineRow?8:12,0,0}`; `:175` close `Margin{0,11,11,0}`; `:179` `{18,0,18,6}`; `:218` heading `Margin{0,8,0,0}`
- `ShortcutPage.cpp:83` reset `Margin{11,7,0,0}`; `:110` `{0,11,11,0}`; `:113` `{12,0,12,24}`; `:137` `{0,8,0,6}`; `:242` heading `{0,8,0,0}`
- `HeaderStatus.h:37` `Margin{23,4,0,0}`, `:40` `{18,1,0,0}` (battery glyph placed by offsets); `HeaderView.cpp:358` `Padding{5,5,0,5}`, `:573` `{0,0,8,0}`
- `FilterView.cpp:120` `Margin{8,8,8,4}`; `FilterView.cpp:145` BlockLineHeight 18 (PRIM-2)
- `ToolView.cpp:375` `Margin{0,2|0,0,0}`, `:482` `{0,0,0,2}`; `EffectView.cpp:369` `{0,2|0,0,0}`; `EffectView.cpp:199` bucket `VerticalAlignment::Top`
- `ColorEditor.cpp:207,818` `{0,6|8,0,0}`; `ColorStrip.h:39` `Padding{8,8,12,8}`; `PreviewPanel.h:41` `{16,14,16,12}`; `PressureCalibration.h:52,67`
- `LayerRow.cpp:161` `groupMode Margin{0,0,3,3}`. This is a deliberate corner badge, not a centring problem.
- ComboBox sites that keep Fluent `12,5,0,7` (PRIM-10 list), and raw CheckBoxes keeping `8,5,0,0` (PRIM-5 list).

---

#### Root causes and systemic patterns

1. **Text primitives give no optical centre for Segoe UI.** Fixing `label()` and `button<T>` (PRIM-1, PRIM-2) closes bug 2, bug 13 and the general "centering errors everywhere".
   - `button<T>` should render its text through `label()`, so one rule covers buttons and labels.
   - The rule should be a single font-derived correction, replacing `WorkspaceManagerView.cpp:289`'s per-field version and the dead LineHeight(18).
2. **No palette resource dictionary (PRIM-6).** One dictionary built from the existing cached palette brushes would close PRIM-7, 9, 11, 12, 13, 18 and 19 at once and remove per-control insertions. It covers accent, popup and menu backgrounds, popover border (`lines.popover_border` is unused on Windows), TextControl states, ToggleSwitch, CheckBox, ListViewItem and ToolTip. Bugs 11 and 14 fall in this class.
3. **`choiceColors` knows colours but not surface or geometry (PRIM-8, PRIM-10).** Giving it the surface role and making it own padding, height and font fixes bug 11 (invisible on settings) and the ComboBox half of bugs 10 and 14 for every dropdown.
4. **Icons wrapped in `ContentControl` instead of a retained `Image` + `setIcon` (PRIM-3).** This class covers bug 12 (Preferences sidebar) and bug 10 (cursor-shape dropdown), plus `LayersView.cpp:53`.
5. **`button<T>` has a single faceless style (PRIM-15).** It needs explicit Plain/Flat faces with `buttonBackground` for plain buttons. This covers bug 9, and dialog footers should share it (PRIM-14).
6. **Field state rendered by layout properties (PRIM-4).** Error and focus rings change BorderThickness and move the text. Draw them as overlays, as GTK's inset box-shadow does.

### Windows vs GTK parity audit: workspace shell (AREA=SHELL)

Scope: apps/layer-windows WorkspaceView, WorkspaceDrawers, WorkspaceTabDrag, WorkspaceTabSurface, WorkspaceSurface(+Geometry), WorkspaceGeometry, WorkspaceShadow, WorkspaceExpansion, WorkspaceGestures, PanelBody, PanelBridgeSurface, CollapsedColumns, PanelConfiguration, ChromeVisibility, WorkflowMotion.
GTK reference: workspace.rs, workspace_drawer.rs, workspace_tab_drag.rs, workspace_columns.rs, panel_tabs.rs, workspace_customization.rs, style.css. Shared: crates/layer-ui (drawers.rs, layout.rs, session.rs), crates/layer-host (lib.rs, scene.rs).
Read-only audit. Nothing was built or run, so everything visual below comes from reading the code.

---

##### SHELL-1: Drawers are torn down and replayed whenever `toolbar_context_generation` changes (root cause of bugs 1 and 19)
- Class: ANIMATION-RETRIGGER, HIT-TEST   Confidence: LIKELY (the code proves the teardown; a screen check would confirm it is the visible cause)   Severity: high   Reported bug: 1, 19
- Windows: `apps/layer-windows/WorkspaceDrawers.cpp:523-524`: `if(context!=next){context=next;reset();}` disposes **every** drawer, including drawers the new model still contains. `apply()` then recreates them at `:531-541`. Each new `Drawer` starts with empty `geometry`, so `from` is empty and the shared `drawer` query interpolates from `end.closed()` (`crates/layer-host/src/lib.rs:1059`). The result is a full open animation.
- Shared: `toolbar_context()` (`crates/layer-ui/src/toolbar_components.rs:252-268`) includes `brush: self.brush.preset`, `tool` and `layer: editing_layer.id`. Changing the brush or category in the sketch brush drawer changes the preset, and tapping a layer row in the Layers drawer changes the editing layer. Both bump the generation (`session.rs:6164-6170`).
  - Bug 1: a category change replays the drawer's open animation.
  - Bug 19: the first tap of a double tap selects a layer, and the drawer visibly closes and reopens. `frame.IsHitTestVisible(!closing)` (`WorkspaceDrawers.cpp:100`) does not cause this. The cause is the dispose and recreate.
- GTK reference: `apps/layer-linux/src/workspace_drawer.rs:1024-1033`. `replaced` is used only when the new model has no drawer (`if replaced && state.is_some() { retire }`). When the drawer survives, GTK keeps its view and animates from its current `placement()`.
- Fix direction: in `WorkspaceDrawers::Impl::apply`, on a generation change dispose only drawers whose id is missing from `models` (mirror GTK `refresh`). Add a fixture test: change the brush preset or the editing layer while the drawer stays open, and check that the drawer is not re-created and the animation does not restart.

##### SHELL-2: Any ContentDrawer field change resets measured heights to 0 and restarts the drawer animation (second cause of bug 1)
- Class: ANIMATION-RETRIGGER   Confidence: CERTAIN   Severity: high   Reported bug: 1
- Windows: `WorkspaceDrawers.cpp:76-81`. On any change to the drawer model JSON (including `tool_set` selection flags, which change on every brush or category pick), it sets `from=placement; started=now; animating=true; ... heights.assign(n,0.)`. The next `drive()` sends `heights:[0..]` (`:186-188`), so shared `drawer_placement` targets a zero-content drawer. The drawer eases toward that small size until `measureHeights()` (`:197`) re-measures, then grows back. That is a visible shrink and regrow.
- GTK reference: `workspace_drawer.rs:1121-1131` plus `target()` `:803-840`. GTK animates from `placement()` toward a target built from **synchronous** measurements (`body.measure(...)`), so an unchanged size produces no motion. A changed `anchor` (`switching`) calls `finish_animation()` and does not animate (`:1116-1120`). Windows has no `switching` case.
- Fix direction: keep `heights` across model changes (re-measure in place, as `measureHeights()` already can). Restart the animation only when the target bounds change (`resized` in GTK) or the drawer opens or closes. Add GTK's `switching` rule: an anchor change snaps instead of animating.

##### SHELL-3: Expansion animation restarts on every layout JSON change and every configuration height change
- Class: ANIMATION-RETRIGGER   Confidence: LIKELY   Severity: medium   Reported bug: none (same class as 1)
- Windows: `WorkspaceExpansion.cpp:41-44`. It restarts (`from=geometry; started=now`) when `layout!=layoutKey` (the entire `model.layout` stringified: dividers, scroll, measurements and so on) or when `|measured-height|>.5`. `configurationHeight()` (`WorkspaceView.cpp:813-819`) returns `content.ActualHeight()`, which is 0 until loaded (`PanelConfiguration.cpp:234`). It is re-queried on every `queueMeasurements` frame (`WorkspaceView.cpp:956`), so opening restarts the ease at least once while the content lays out, and again on any unrelated layout publication.
- GTK reference: `workspace_customization.rs` `animate(w,true,from)` runs once per open or close (`:830`). Geometry changes during the animation retarget through allocation and do not restart the clock.
- Fix direction: restart only on panel change or open/close. For layout and height changes, retarget without resetting `started`.

##### SHELL-4: Drawer animation uses a literal 200 ms instead of the shared `panel_expansion_ms`
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceDrawers.cpp:169` `/.2f` and `:135` `/.2`. `WorkspaceExpansion.cpp:47` already reads `num(data->catalog,L"panel_expansion_ms",200)`.
- GTK reference: `workspace_drawer.rs:974` `PANEL_EXPANSION_MS` (`crates/layer-ui/src/layout.rs:1461` = 200).
- Fix direction: read `panel_expansion_ms` from the catalog in the drawer and the inspector page too.

##### SHELL-5: Tab strip and body seam: two independently antialiased edges meet at y=36 on a fractional device position
- Class: SEAM   Confidence: LIKELY   Severity: high   Reported bug: 3
- Windows:
  - The tab strip is one composition visual clipped to `squircleOutline(width,36,{R,R,0,0})` with `BorderMode::Soft` (`WorkspaceTabSurface.h:58,63,69`).
  - The body is a second ShapeVisual (`WorkspaceSurface.h:251`, Soft) placed at `Canvas::SetTop(background,36)` (`WorkspaceView.cpp:1125-1126`), with square top corners `{0,0,br,bl}` (`:1154-1155`).
  - The group frame is placed at raw fractional shared bounds with `UseLayoutRounding(false)` (`WorkspaceView.cpp:579`, tabs `:279`).
  - So the strip's bottom clip edge and the body's top edge both fall at frame-local y=36 on a non-pixel boundary. Each gets partial coverage, and coverage a + (1−a) composited with source-over is below full. A hairline of canvas shows through, more so with glass (alpha < 1).
- GTK reference: `panel_tabs.rs:181-278`. `selected_regions` snaps every region to a `PixelGrid` (`grid.floor/ceil`). `workspace.rs:499-512` `native_allocation` rounds every slot to whole pixels before allocating.
- Fix direction:
  - Root fix: snap group, drawer, column and bridge placement to device pixels in one place (`place()` or the frame placement in `present()`), as GTK does in `native_allocation`.
  - Alternatively, paint the active-tab foot and the body as one contour so no panel-coloured edge abuts another, or use `CompositionBorderMode::Hard` on the shared horizontal edge.
  - Add a pixel test at fractional offsets (GTK has `narrow_tabs_paint_their_joined_top_once_at_fractional_device_phases`).

##### SHELL-6: Tab slide preview paints the active tab with the opaque `brush(panel)` and removes the strip cut (bug 4)
- Class: GLASS   Confidence: CERTAIN   Severity: high   Reported bug: 4 (and part of 16)
- Windows:
  - `WorkspaceTabDrag.cpp:102`: `panelTabShell(content, entry.active?Brush(data->brush(L"panel")):Brush(nullptr))`. `:160` does the same on refresh.
  - The originals are set to `Opacity(0)` (`:106,122,165`). `cutStrip` (`WorkspaceView.cpp:1210`) skips the active-tab cut when `tab.Opacity()<=0`, so the strip under the held tab becomes plain strip glass, with an opaque panel-colour tab copy drawn over it.
  - Inactive copies have no fill. The overlay sits at z 9000 outside every glass region.
- GTK reference: `workspace_tab_drag.rs:275-311` also bakes opaque colours (`palette.panel` or `palette.tabbar`, alpha 1) into the slide nodes. GTK has the same design defect, so this is a shared design gap rather than a Windows-only divergence. GTK at least paints the inactive tabs with tabbar.
- Fix direction: use `data->glass(L"panel")` for the active copy and `glass(L"strip")` for the inactive copies (fix GTK to its glass roles too). Keep the strip's active cut while the original is hidden: in `cutStrip`, test the shell's grab state instead of `Opacity()>0`, or hide only the shell's content and leave the fill. Add the overlay copies to the glass regions, or publish them as panel-tab regions so the blur follows the drag.

##### SHELL-7: Enumeration of drag, float, preview and resize states and their surface roles (bugs 4 and 16)
- Class: GLASS   Confidence: CERTAIN (roles read from code)   Severity: medium   Reported bug: 4, 16
- Windows states:
  1. Docked or floating at rest: body `glass(panel|source)` (`WorkspaceView.cpp:1127`), strip `glass(strip)` plus active `glass(panel)` (`:1218`), glass region from `group.frame` (`:1268`).
  2. Open column (GPU bridge): native fills at opacity 0 (`:1172-1174`). The GPU draws `glass(panel/strip)` × frame opacity (`:1088-1095`).
  3. Native window resize: `brush(panel)`/`brush(tabbar)` forced to `A=255` (`:1127-1128,1218-1219`); collapsed column too (`CollapsedColumns.cpp:153`). This is intentional, but it is opaque.
  4. Expanded (configuration): body `brush(panel)` opaque, header `brush(tabbar)` opaque, active tab Path `brush(panel)` (`:1127,1129-1131`), and no glass region (`:1268` excludes `presented`). GTK paints `var(--panel-bg)` (`style.css:325`), which is the glass-collected colour (`workspace.rs:396-401` collects by colour). Windows expanded is opaque and GTK is glass: CHECK.
  5. Tab slide: see SHELL-6 (opaque).
  6. Floating move: `RenderTransform` only (`:662-677`). The glass region is updated one frame late through `queueMeasurements → publishOverviews → glassChanged` (`:696,1261`), so the blur trails the panel during motion.
  7. Floating resize: each motion is a reflow (`apply` with `layout`), which rebuilds `group.backgroundKey`. The role stays glass in these files, so the bug 16 opacity loss is not caused here. Check the host glass path (not in scope).
  8. Drawer: `glass(panel)` body and bridge (`WorkspaceDrawers.cpp:60-61`), plus region and connection (`:457-461`).
  9. Edge-contact retired elements: composition opacity 0 (`WorkspaceGestures.cpp:314-315`).
- Fix direction: bug 4 per SHELL-6. For (4), use the glass roles and add the expanded outline as a glass region, matching GTK. For (6), call `glassChanged` directly from `applyMotion` when `moved`.

##### SHELL-9: Hard-coded body heights differ from GTK
- Class: METRIC, LAYOUT-COLLAPSE   Confidence: CERTAIN   Severity: medium   Reported bug: 18 (related)
- Windows: `WorkspaceDrawers.cpp:348-349` sets layers and adjustments to **480** and navigator to 272. `PanelBody.cpp:225-226` sets filter_types 440, layers 480, adjustments 480. `PanelConfiguration.cpp:93,102,105` sets 440, 480 and 272.
- GTK reference: `workspace_drawer.rs:432` layers `set_height_request(360)`, `:440` adjustments 440, `:442` filter_types 440. Navigator uses its natural height (no literal).
- Fix direction: move these to one shared metric (catalog `panel_metrics`), use GTK's values (layers 360, adjustments 440), and delete the per-site literals.

##### SHELL-10: Default drawer width (bug 18): Windows uses the shared width as is; the "narrow" look comes from the DIP breakpoint
- Class: LAYOUT-COLLAPSE, METRIC   Confidence: CHECK   Severity: medium   Reported bug: 18
- Windows: `WorkspaceDrawers.cpp:309,343` sets each body to `Width(placement.width)` from the shared drawer query. Windows adds no policy of its own.
- Shared: `ContentDrawer::column_widths` (`crates/layer-ui/src/drawers.rs:468-485`) → `Panel::default_width` (`layout.rs:716-723`): `PANEL_MIN_WIDTH + (viewport_width>=1600 ? TILE_SIZE+TILE_GAP : 0)`. On Windows the viewport is in DIPs, so a 1920 px screen at 125–150 % scale (1536/1280 DIP) never crosses 1600. A typical GTK session at scale 1 does. Shared policy, so a Windows-only workaround is not the fix.
- GTK reference: `workspace_drawer.rs:42-44`. When `natural_height` is set (layer inspector pages), GTK widens a page to `child.measure(Horizontal).0` if the panel's minimum is larger. Windows never widens.
- Fix direction: decide the breakpoint in shared Rust, for example on content need rather than raw DIP width, or measure the Layers panel minimum width and report it. Check whether the Windows Layers row minimum exceeds `PANEL_MIN_WIDTH` at Windows font metrics.

##### SHELL-11: Drawer tab header has no tab-bar tint and no active-tab foot
- Class: GLASS, BORDER/COLOR, HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `WorkspaceDrawers.cpp:250` `header.Background(clear())` and `:274` `panelTabShell(pick,nullptr,hover)`. `placeColumns:336` `selectPanelTabShell(shell,selected,nullptr,...)` sets a null active fill. The whole drawer, tab row included, is one `glass(panel)` Path (`:60,444-455`). The active tab is not marked by any surface, and the strip is not tinted.
- GTK reference: `workspace_drawer.rs:477-537` uses `PanelTabs` with `tab-strip-tint` (`--tabbar-bg`) and `tab-foot-tint` (`--panel-bg`) through `tab_background` (`panel_tabs.rs:98-142`), plus `.tab-strip-end { background: var(--tabbar-bg) }` (`style.css:176-178`).
- Fix direction: reuse `WorkspaceTabSurface` (strip band plus active cut) for drawer headers, through the same `cutStrip` helper as docked groups. Do not hand-roll it again.

##### SHELL-12: Drawer grip column is 28 px; docked header uses 20 px
- Class: METRIC, ALIGNMENT   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceDrawers.cpp:252` `gripColumn.Width(28)` with a 20 px left-aligned grip (`:282-283`), leaving 8 px dead space. `WorkspaceView.cpp:313,315` uses 20/20.
- GTK reference: `workspace_drawer.rs:534` and `workspace.rs:3364` both use `grip.set_size_request(20, 24)` inside `strip_end`.
- Fix direction: use one grip-width constant (20) for both. Better, share one header builder between docked groups and drawers. They duplicate about 40 lines (tab button, label row, ScrollView, grip).

##### SHELL-13: Literal 36 for tab bar height throughout instead of the shared `TAB_BAR_HEIGHT`
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceView.cpp:264,279,315,986,1125,1209`; `WorkspaceTabSurface.h:71-72,92`; `WorkspaceDrawers.cpp:241,246,261,282,384`; `WorkspaceGeometry.h:134` (automatic tab min 36).
- Shared: `crates/layer-ui/src/layout.rs:69` `TAB_BAR_HEIGHT = TILE_SIZE`.
- Fix direction: read the tab bar height from the catalog or `panelMetric` once and use it everywhere.

##### SHELL-14: Expanded (configuration) corners use a literal 8 instead of SurfaceRadius×CornerFit (9.72), and the non-expanded path overwrites the group radius every present
- Class: BORDER/COLOR, METRIC, SEAM   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows:
  - `WorkspaceView.cpp:1002-1003`: `CornerRadius{tabs?8.:0.,8,8,0}`.
  - `:1009`: `group.border.CornerRadius({8,8,8,8})` on **every** present for non-expanded groups. This overrides the `groupRadius*CornerFit` set at `:253,590`, which is 9.72, or the tile radius for tile groups.
  - `WorkspaceGeometry.h:252,254`: `expansionShape` uses radius 8.
  - `:1165`: `presentPanelBridges` resets `group.frame.CornerRadius` to `SurfaceRadius*CornerFit`, overriding the tile-group radius from `:590`. The two sites flip-flop every frame.
- GTK reference: `style.css:325-331`, `.expanded-panel > .panel-preview, .panel-configuration { border-radius: var(--surface-radius) }`, with `--surface-radius: calc(18px * .54)`.
- Fix direction: derive every radius from `groupRadius(geometry)` (and squircle paths from `SurfaceRadius`). Set it in one place and remove the `8` literals and the duplicate setter in `presentPanelBridges`.

##### SHELL-15: Shadow parameters differ from GTK and are inconsistent across surfaces
- Class: METRIC, BORDER/COLOR   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows:
  - Group: `Rounded(...,12,2,.16f)` (`WorkspaceView.cpp:1136`).
  - Column bridge: `12,2,.16f` (`:1199`).
  - Drawer: `Shape(...,21,5,1.f/3)` (`WorkspaceDrawers.cpp:448`).
  - Expanded: `36,8,.4f` (`WorkspaceView.cpp:1135`).
  - `WorkspaceShadow.cpp:129` defaults to `blur=8,offset=2,opacity=.16f`.
- GTK reference:
  - `.dock-panel` and `.drawer-shadow`: `0 2px 8px alpha(black,.16)` (`style.css:137,139`).
  - Expanded: `Shadow(rgba 0,0,0,.4, dx 0, dy 8, radius 24)` (`workspace.rs:367-372`).
  - Drawers use the **same** shadow as panels in GTK.
  - Windows' drawer shadow is about 2× the opacity, 2.5× the offset and blur 21. Groups and expanded use blur ×1.5 (12/8, 36/24). The drawer does not follow that conversion at all.
- Fix direction: define the panel shadow once (blur, offset, opacity) from GTK's values with one documented composition conversion factor. Use it for groups, drawers and bridges, and a second token for expanded.

##### SHELL-16: Group shadow ignores squared corners (drawer source or bridge joins)
- Class: GLASS, SEAM   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `WorkspaceView.cpp:1132-1136`. The shadow key and shape use `radius` for all four corners, while the fill uses `corners` with joined corners squared (`:1111-1114,1154`). Under translucent glass, the shadow casts inside the squared corner region and shows through as a dark patch.
- GTK reference: `style.css:159-170`. `.dock-panel.join-*` and `.drawer-shadow.join-*` square the shadow box too. The drawer path on Windows does this through `shadow.Cut(radii,…)` (`WorkspaceDrawers.cpp:451`).
- Fix direction: pass `corners` to `group.shadow.Rounded` and include them in `shadowKey`.

##### SHELL-17: Open column bridge (bug 15): native fills hidden while the GPU surface may be dropped; bridge geometry built from raw, unrounded placements
- Class: SEAM, GLASS, NULL-STATE   Confidence: CHECK   Severity: high   Reported bug: 15
- Windows:
  - `group.gpu=…panelBridgeGroup(id)…` (`WorkspaceView.cpp:1172`) hides the native body and strip (opacity 0) whenever the bridge frame lists the group.
  - `panelSurfaces` (`:1078-1091`) `continue`s, dropping the whole column surface, when any receiver is hidden, presented or not loaded (`receivers.Size()!=expected.Size()`). The host also rejects mismatched receivers (`crates/layer-host/src/scene.rs:163-173`).
  - In that state neither the native fill nor the GPU fill is drawn, so the area is transparent.
  - Bridge plane and width come from `capy_panel_bridges` fed with `placement(group)` and `collapsed->Geometry()`, the raw fractional shared bounds (`:1050-1055`). `tab_bar`/`active_tab`/`tab_clip` instead come from `allocatedBounds` of live elements (`:1085-1087`).
  - `CollapsedColumns::apply` squares source corners (`CollapsedColumns.cpp:79-80`). `present()` then overwrites `corners` with the bridge-frame radii (`:159-160`), so drawer-source squaring is lost whenever the column is in the bridge frame.
- GTK reference: `workspace_columns.rs:28-66` builds bridges from `compute_bounds` of the allocated, pixel-rounded widgets (`workspace.rs:509-512`). `present_surface_ownership` (`:68-79`) sets `gpu-panel-surface` from the same bridge list used to build surfaces, so ownership and drawing cannot disagree.
- Fix direction:
  - Derive `group.gpu` and the column `gpu` from the surfaces actually published in `panelSurfaces` (one source of truth), not from `panelBridgeFrame` alone.
  - Feed `capy_panel_bridges` the rounded allocated bounds, as GTK does.
  - Merge source-corner squaring with the bridge radii; do not replace one with the other.
  - Repro needed: open a column group with two groups, then open one panel. If the gap tracks fractional x positions, SHELL-5's pixel snapping fixes it as well.

##### SHELL-18: Drawers lack the column divider GTK draws between multi-column drawer bodies
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceDrawers.cpp:329-363` places columns with no separator.
- GTK reference: `style.css:173`, `.content-drawer > scrolledwindow + scrolledwindow { border-left: 1px solid alpha(var(--window-fg-color), .12) }`.
- Fix direction: add a 1 px `line(surface,role)` divider between drawer columns using the shared line role.

##### SHELL-19: Tab focus and template corner 6 px vs GTK tab radius 9.72
- Class: HIGHLIGHT-GEOMETRY, METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceView.cpp:279` and `WorkspaceDrawers.cpp:261` `CornerRadius{6,6,0,0}`. The hover and active path uses `SurfaceRadius` squircle (`WorkspaceGeometry.h:203-211`), so the focus rectangle and the pressed template do not match the painted tab.
- GTK reference: `style.css:273` `.dock-tabs button { border-radius: var(--surface-radius) var(--surface-radius) 0 0 }`; `panel_tabs.rs:8` `TAB_RADIUS = SURFACE_RADIUS*CORNER_FIT`.
- Fix direction: use `SurfaceRadius*CornerFit` for the tab button's CornerRadius.

##### SHELL-20: Tab slide preview ignores the automatic tab-name fit
- Class: LAYOUT-COLLAPSE, ALIGNMENT   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `WorkspaceTabDrag.cpp:99-101` builds the copy from `presentation.show_icon` and `show_name`. With `windows_tab_styles == automatic`, the live tab shows the icon always and the name only if `fitAutomaticTabs` allowed it (`WorkspaceView.cpp:335-336`). The copy can therefore show a hidden name, clipped to the source width (`:103-104`), or drop the forced icon.
- GTK reference: `workspace_tab_drag.rs:26-48` snapshots the real widget (`snapshot_child`), so the preview always matches.
- Fix direction: build the copy from the live tab's current child visibilities, or render the live shell into the copy, rather than re-deriving them from the model.

##### SHELL-21: Drop hint corner radius and colour differ from GTK
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceGestures.cpp:654-662`: `CornerRadius{2}` for non-body hints, colours from `accent(data)` and `tint(accent,64)`.
- GTK reference: `workspace.rs:440-457`: square rect, fixed `rgba(.38,.68,1,.95|.25)` with a 2 px border for body hints.
- Fix direction: put the drop hint colour and radius in shared style. Pick one; the theme accent is probably right, so fix GTK too.

##### SHELL-22: Panel configuration controls use literal sizes instead of panel metrics
- Class: METRIC, DEFAULT-CONTROL   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `PanelConfiguration.cpp`:
  - `:42` ComboBox `MinHeight(34)`; `:119` and `:137` 34 px buttons; `:185` `MinHeight(34)`, `Padding{8,4,8,4}`.
  - `:163` CheckBox `MinHeight(28)` with no `compactCheck`.
  - `:181` section spacing 2.
  - Toolbar options are plain Buttons with a check or chevron glyph.
  - Item `hint` text is not shown, and items without `action` are still shown and enabled.
- GTK reference: `workspace_customization.rs:726-817`. `.dock-panel button` uses min-height `--panel-control-height` (shared 26, `layout.rs:57`). Selectable options are radio `CheckButton`s in a `boxed-list` (card shadow, `style.css:14`), with margins 9. Items with a `hint` show a dim right label. Items without `action` are skipped.
- Fix direction: use `data->controlHeight()`, `panelMetric` and `compactCheck`. Render options as a card list with radio semantics and the hint label. Skip items that have no action.

##### SHELL-23: Configuration placeholder width literal 380
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceView.cpp:1011`: `std::min(380.f, root width)` for the pre-measurement configuration frame.
- GTK reference: no literal. The configuration width comes from shared expansion geometry.
- Fix direction: take the width from the shared expansion query (the `configuration` bounds) or from a shared metric.

##### SHELL-24: Double-tap routing: root `DoubleTapped` with handledEventsToo forwards every unhandled double tap to `edge_double_press`
- Class: HIT-TEST   Confidence: CHECK   Severity: medium   Reported bug: 19 (secondary path)
- Windows:
  - `WorkspaceGestures.cpp:742-744` registers DoubleTapped on root with `handledEventsToo=true`.
  - `doubleClick` (`:626-627`) calls `edgeDoublePress` for any target without `workspace_double`, even when a child (for example a LayerRow name, `LayerRow.cpp:200`) already handled it.
  - `PanelBody.cpp:218` tags every body root with `workspace_action={}`, and `target()` (`:157-171`) returns `J{}` under a tab-stop control.
  - Drawers carry no `edge_surface` tag, so `edgeContact` currently returns empty and nothing fires. Any future edge tag on drawers would route list double taps to column-width reset.
  - The confirmed bug 19 path is SHELL-1.
- GTK reference: GTK attaches double-click only to its specific handle and strip gestures (`install_panel_drag`). Handled events stop.
- Fix direction: skip `doubleClick` when `e.Handled()` is already true, and register the handler without handledEventsToo.

##### SHELL-25: Docked groups and drawers place surfaces at fractional coordinates (no pixel snapping anywhere in the shell)
- Class: SEAM   Confidence: LIKELY   Severity: high   Reported bug: 3, 15
- Windows:
  - `group.frame.UseLayoutRounding(false)` (`WorkspaceView.cpp:579`) and `tab.UseLayoutRounding(false)` (`:279`).
  - `place(frame,bounds)` uses raw shared floats in drawers (`WorkspaceDrawers.cpp:370`), the drawer bridge (`:374`) and collapsed columns (`CollapsedColumns.cpp:74`).
  - Adjacent surfaces (drawer and bridge, column and bridge, strip and body) all end on fractional device pixels with Soft antialiasing.
- GTK reference: `workspace.rs:499-512`. `allocate_at` → `native_allocation` rounds x/y and the far edges to integers for every slot, and the panel tabs snap regions to the pixel grid.
- Fix direction: one shared rounding helper (round the origin and far edge, as in `native_allocation`) applied in the shell's `place` for frames, bridges and columns. The bridge query must also receive the rounded bounds (SHELL-17).

##### SHELL-26: Theme or palette change rebuilds every group and drawer (animations replay, retained state lost)
- Class: ANIMATION-RETRIGGER   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceView.cpp:555-559`. Any change to `palette` JSON, including glass slider edits, triggers `expansion->Reset(); drawers->Reset(); collapsed->Reset(); root.Children().Clear(); groups.clear(); bodies.clear()`. Open drawers then replay their open animation, and scroll and focus are lost.
- GTK reference: `workspace_drawer.rs:580-584` `refresh_theme` only redraws.
- Fix direction: rebind brushes on theme or palette change (`presentBackgrounds`, `cutStrip`, ButtonFeedback refresh) instead of tearing down. At minimum, do not reset on palette-only changes.

---

#### Root causes / systemic patterns

1. **Drawer lifecycle in `WorkspaceDrawers::Impl::apply` and `Drawer::apply`** (SHELL-1, 2, 26; bugs 1 and 19). The generation reset disposes live drawers, and every model change zeroes `heights` and restarts the clock. Port GTK `Drawer::refresh`: retire only when the model is gone, snap on `switching`, animate only on open, close or `resized`, and keep measured heights.
2. **No pixel snapping in shell placement** (SHELL-5, 17, 25; bugs 3 and 15). Add one rounding step, like GTK `native_allocation`, to `place()` for frames, drawers, bridges and columns, and feed the same rounded bounds to `capy_panel_bridges`. This also stops the Soft-edge seam between the strip and body visuals.
3. **Opaque roles in transient states** (SHELL-6, 7; bugs 4 and 16). Tab slide copies use `brush(panel)`, and expanded panels use `brush(panel/tabbar)`. One helper should pick the glass role whenever the surface is not in native resize. Make preview overlays and expanded outlines publish glass regions.
4. **Measurement for SizeChanged-sized bodies** (SHELL-8, 9; bug 5). `PanelBody::ContentHeight` trusts `Measure()`, but Canvas-based bodies (BrushSizePanel) only size themselves in `SizeChanged`. Give such bodies a width → height function, and replace the 480/440/272 literals with shared metrics matching GTK (360/440).
5. **Literal metrics duplicated across docked and drawer headers** (SHELL-11, 12, 13, 14, 15, 19, 22). One shared tab-header builder (tab button, strip surface, grip) and catalog metrics for tab height, grip width, radii and shadows would close most of the low-severity items. That includes the missing drawer strip tint and active foot, the 28/20 grip, the radius 6/8 vs 9.72 and the drawer shadow 21/5/.33 vs 8/2/.16.

### COLOR area: Windows vs GTK parity audit

Scope read in full: `apps/layer-windows/ColorView.cpp/.h`, `ColorEditor.cpp/.h`, `ColorWheel.h`, `ColorStrip.h`, `ColorPair.h`, `ColorTouch.h`, `SwatchFace.h`, `PickerDialog.h` (Edit Color popup shell), plus the `ButtonFeedback.h`/`UiControls.h` helpers they rely on.
GTK reference: `apps/layer-linux/src/color_editor.rs`, `tool_panels.rs` (ColorWheel, WheelButton), `color_readout.rs`, `hdr_color_scale.rs`, `color_pair.rs`, `display_color.rs`, `style.css`.
Shared: `crates/layer-ui/src/color.rs` (`ColorPanelLayout`, `ColorWheelGeometry`, `stage_for_wheel`), `crates/layer-ui/src/layout.rs` (`control_radius: 13`), docs `docs/ui/color-picker.md`, `docs/ui/theme-colors.md`.

Corner-radius reference: GTK `--corner-fit: .54`, `--control-radius = 13 * .54 = 7.02px`, `--surface-radius = 18 * .54 = 9.72px` (style.css:8). Windows equivalents: `controlCorner()` (UiControls.h:399) and `SurfaceRadius=18` for squircles (WorkspaceGeometry.h:156).

None of the 22 user-listed bugs are in the colour area. The findings below are other instances of the same failure classes.

---

##### COLOR-1: Swatch pairs do not clip their patches to the rounded frame (square corners poke through the border)
- Class: SEAM, BORDER/COLOR   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `ColorEditor.cpp:365-368` `framePair` gives a `Grid` `CornerRadius({6,6,6,6})` plus a 1px border. The children are `current` (a Button with `CornerRadius({0,0,0,0})`, line 179) and `fresh` (a plain `Border` with no radius, line 181). The mini pair (`miniCurrent`/`miniNew`, lines 378-381) and the picking strip pair (`ColorStrip.h:143-145`: `Grid pair;pair.CornerRadius({6,6,6,6})` holding two square `Border`s) are built the same way. In WinUI, `Grid.CornerRadius` rounds only the Grid's own background and border. It does not clip children, so the colour patches keep square corners that cover the rounded border arc, and the strip pair renders fully square.
- GTK reference: `color_editor.rs:180-192,250-256`. `pair()` / `pair_box` use `set_overflow(gtk::Overflow::Hidden)` with `.edit-color-pair { border-radius: var(--control-radius); border: 1px solid alpha(currentColor,.12) }` (style.css:513), so the patches are clipped to the rounded frame.
- Fix direction: give `framePair` (and reuse it for the strip) a real clip. Use a parent `Border` with `CornerRadius`, which clips its single child, wrapping the 2-column Grid, or set a `RectangleGeometry`/composition clip with the radius. Set the outer radius from `controlCorner()` and the inner patches' outer corners from `controlCorner()-1`. One helper fixes the editor pair, the mini pair and the strip.

##### COLOR-2: Edit Color number/hex entries and the swatch search use the default WinUI TextBox template
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: LIKELY   Severity: high   Reported bug: none (same class as #11)
- Windows: `ColorEditor.cpp:306-308`. `f->entry` sets `Padding({4,2,4,2})`, `BorderThickness({2,2,2,2})`, `BorderBrush(accent)` and leaves `Background` and the template resources at their defaults. `ColorEditor.cpp:403`: `search` only sets `MinHeight(34)`. The Fluent TextBox Focused state replaces the border brush/thickness with `TextControlBorderBrushFocused` (accent underline) and the background with `TextControlBackgroundFocused` (opaque). An editing entry is always focused, so the custom accent/red border (line 613) is overridden, and the field shows an opaque Fluent box. The default DeleteButton (X) can also appear (CHECK). The search box has no search glyph and the default rounded Fluent chrome.
- GTK reference: style.css:517 `.picker-dialog entry.number-entry { min-height:28px; padding:0 4px; border:0; border-radius:var(--control-radius); background:alpha(currentColor,.10); box-shadow:none }`. style.css:529 `.edit-color-sheet entry { min-height:34px; border:0; border-radius:var(--control-radius); background:alpha(currentColor,.10) }`. theme-colors.md:53 says "Search field, text fields on a dialog surface … Text at 10%".
- Shared source: palette `text` at 10% (`tint(L"text",26)`), `controlCorner()`.
- Fix direction: add one dialog-field styler next to `NumberControl.cpp:122-126` / `WorkspaceManagerView.cpp:323` that overrides `TextControlBackground{,PointerOver,Focused}` = `tint(text,26)`, `TextControlBorderBrush{,PointerOver,Focused}`, `TextControlBorderThemeThickness{,Focused}` = 0 and `CornerRadius(controlCorner())`, and hides the DeleteButton. Apply it to the value entries, hex entry and search. Show the invalid state through a resource the template honours (see COLOR-12).

##### COLOR-3: Swatch sheet "more" chevron has no button background
- Class: BORDER/COLOR, DEFAULT-CONTROL   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `ColorEditor.cpp:386-387`. `more=button(...)` keeps `button()`'s `Background(clear())` (UiControls.h:472), so the chevron floats with no fill. Only hover/press feedback shows.
- GTK reference: `color_editor.rs:759-762` adds only `edit-color-more`, not `flat`. style.css:524 `button.edit-color-more { min-width:34px; min-height:34px; padding:0; border-radius:var(--control-radius) }` keeps the libadwaita filled button background, like Cancel.
- Fix direction: `more.Background(buttonBackground(data))`, as `pick` (line 189) and `pickerButton` (PickerDialog.h:56) already do.

##### COLOR-4: Literal corner radii instead of control/surface metrics across Edit Color and the strip
- Class: METRIC   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows, with GTK equivalents:
  - Shape well `ColorEditor.cpp:163` `CornerRadius({17,...})` vs GTK `.choice-well { border-radius: calc(18px*var(--corner-fit)) }` = 9.72 (style.css:86).
  - Shape pills `ColorEditor.cpp:172` `CornerRadius({13,...})` vs `.choice-well button { border-radius: calc(13px*var(--corner-fit)) }` = 7.02 (style.css:87).
  - Pair `ColorEditor.cpp:367` `CornerRadius({6,...})` vs `.edit-color-pair { border-radius: var(--control-radius) }` = 7.02.
  - Tiles `ColorEditor.cpp:425,427` `CornerRadius 6` (button) / `5` (patch) vs `.palette-tile` and `.palette-tile > *` both `var(--control-radius)` (style.css:481-482).
  - Badges `ColorEditor.cpp:196,282` `CornerRadius 4` vs `.edit-color-badge { border-radius: calc(8px*var(--corner-fit)) }` = 4.32 (style.css:520).
  - Strip pair `ColorStrip.h:143` `6` vs `.edit-color-strip .edit-color-pair { border-radius: calc(8px*var(--corner-fit)) }` = 4.32 (style.css:531).
  - Strip surface `ColorStrip.h:209` squircle radii `12` vs `.edit-color-strip { border-radius: var(--surface-radius) }`. On Windows a squircle radius corresponds to GTK's pre-fit value, so it should be `SurfaceRadius` (18), as other surfaces use.
- Shared source: `panel_metrics.control_radius` (13) × `CornerFit`, and `SurfaceRadius`.
- Fix direction: use `data->controlCorner()` for pair/tiles/patches and `r*CornerFit` for the 18/13/8 well, pill and badge radii (NewDrawingDialog.cpp:508/511 repeats the same 17/13 literals, so add a shared `pillWell()` helper). Use `SurfaceRadius` for the strip squircle.

##### COLOR-5: Picking strip uses the panel background instead of the dialog (settings) surface
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `ColorStrip.h:184` `surface.Fill(data->brush(L"panel"))`.
- GTK reference: style.css:530 `.edit-color-strip { background: var(--dialog-bg-color); color: var(--dialog-fg-color) }`. `--dialog-bg-color` is `--capy-settings` (style.css:8). theme-colors.md:37/126 says `settings` is used for "Every dialog, sheet and alert".
- Fix direction: `surface.Fill(data->brush(L"settings"))`.

##### COLOR-6: Picking strip pair has no 1px frame
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorStrip.h:143` sets no BorderThickness/BorderBrush on `pair`.
- GTK reference: the strip reuses `pair(20, 40)` (`color_editor.rs:1100`), which carries `.edit-color-pair { border: 1px solid alpha(currentColor,.12) }`.
- Fix direction: build the strip pair with the same `framePair` helper (border `tint(text,31)`), together with COLOR-1.

##### COLOR-7: Picking strip insets and text sizes differ
- Class: ALIGNMENT, METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorStrip.h:155` `content.Padding({8,8,12,8})`, a left inset of 8. `ColorStrip.h:147` `hex.FontSize(15); FontWeight SemiBold`. `intensity`, `space` and `values` never get a FontSize, so they use the WinUI default 14 instead of `data->textSize()` (14.67). `space.Opacity(.6)`.
- GTK reference: style.css:530 `.edit-color-strip { padding: 8px 12px }`, so left and right are both 12. style.css:532 `.edit-color-strip-hex { font-weight: 500 }` at the UI text size. `space` uses `dim-label` (libadwaita opacity .55).
- Fix direction: Padding `{12,8,12,8}`. Use `FontSize(data->textSize())` on all four labels, with Medium weight for hex. Build them with `label(data,...)` so LineHeight/centering match the rest of the UI.

##### COLOR-8: Edit Color patch height and spacing differ from GTK
- Class: METRIC, LAYOUT   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows:
  - `ColorEditor.cpp:179,181`: `current.Height(48)`, `fresh.Height(48)`.
  - `ColorEditor.cpp:175`: `left.RowSpacing(6)` between the wheel and the shape well.
  - `ColorEditor.cpp:212`: `body.RowSpacing(14)` is also the title→content gap, because the title is a row of `body`.
  - `ColorEditor.cpp:183`: caption columns are `54` each while the pair is 54+54+2 (border).
- GTK reference:
  - `color_editor.rs:244` `patch.set_size_request(54, 46)`.
  - `color_editor.rs:238` `gtk::Box::new(Vertical, 12)` for `left`.
  - `color_editor.rs:737` `page = Box(Vertical, 16)` holding the title and body (title→content 16). The body's right column has spacing 14.
  - captions homogeneous across the bordered pair.
- Fix direction: patch height 46, `left.RowSpacing(12)`, and either move the title out of `body` into a page StackPanel with Spacing 16 or give the title `Margin({0,0,0,2})`. Size the caption columns from the pair's actual width.

##### COLOR-9: Edit Color wheel is fixed at 232 instead of fitting 160–232; stage size is a literal
- Class: LAYOUT-COLLAPSE, METRIC   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `ColorEditor.cpp:20` `WheelSide=232`. `ColorEditor.cpp:679-682` uses `double next=WheelSide;` and `stageSize=side+28;`. `ColorEditor.cpp:515` makes the wide column a fixed `GridLength{WheelSide,Pixel}`. In short or narrow dialogs the 232px wheel cannot shrink, so the body scrolls.
- GTK reference: `tool_panels.rs:1012` `const BARE_WHEEL: (i32, i32) = (160, 232)`. `measure()` (tool_panels.rs:877-886) and `stage_bounds()` (1199-1210) pick the largest wheel in 160..232 that fits the allocation. The stage comes from shared `ColorPanelLayout::stage_for_wheel` (`bare_stage`, tool_panels.rs:1021-1032).
- Shared source: `ColorPanelLayout::stage_for_wheel` (crates/layer-ui/src/color.rs:1191). The `+28` holds only while the inset is 14 (size ≥ 176, color.rs:1236).
- Fix direction: expose `stage_for_wheel`/`bare_stage` through `color_ui` (e.g. a `bare_layout` request taking the available width and height and returning wheel side, stage, inset and bottom). Size the wheel from the available space and drop the `+28` literal.

##### COLOR-10: Edit Color shape pills and well: hover-on-selected and well padding
- Class: HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:567` `shapeButtons[i].Background(on?brush(L"header_selection"):clear())`. Hover then lays the generic `text@20` overlay from `button()` on top of the selection colour.
- GTK reference: style.css:89-90 `.choice-well button:checked { background: var(--header-selection) }` and `:checked:hover { background: var(--header-selection-hover) }`. Unchecked hover is `alpha(fg,.08)` (style.css:575).
- Fix direction: pass `hoverSurface=header_selection_hover` / `checkedSurface=header_selection` through `buttonFeedback` (ButtonFeedback.h:121), ideally as `ToggleButton`s. Share this with the New Drawing well (NewDrawingDialog.cpp:508-511) via one helper.

##### COLOR-11: Copy buttons lack GTK's -6px trailing margin
- Class: ALIGNMENT   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:349` `node.Width(28);node.Height(28)` with no margin, so the copy column sits 6px further in from the right edge than on GTK.
- GTK reference: style.css:521 `button.edit-color-copy { min-width:28px; min-height:28px; margin-right:-6px; padding:0; border-radius:var(--control-radius) }`.
- Fix direction: `node.Margin({0,0,-6,0})` in `copyButton`.

##### COLOR-12: Error colour is one fixed red for text and border in both themes
- Class: BORDER/COLOR   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:21` `Invalid{255,0xe0,0x1b,0x24}`, used for the status text (207) and the entry border (613) in light and dark. Status `FontSize(textSize*.9)`.
- GTK reference: `validation.add_css_class("error")` (color_editor.rs:338) and `entry.add_css_class("error")` (503) use the native semantic error colour per theme at the normal text size. theme-colors.md:60: "Error / warning text, invalid numeric border … native semantic roles", with border `#ee5555`.
- Fix direction: read an error role from the shared palette (or a per-theme pair) and use it for both. Drop the `.9` size scaling. Together with COLOR-2, apply the invalid border through the TextBox focused-border resource so it shows while focused.

##### COLOR-13: Format-menu chevron is 12px; GTK uses a 16px pan-down icon
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:275` `icon(L"chevron-down",data->theme(),12)`.
- GTK reference: `color_editor.rs:306` `gtk::Image::from_icon_name("pan-down-symbolic")` at the default 16px.
- Fix direction: use the default 16px icon size (and the same glyph used for other dropdown affordances).

##### COLOR-14: Edit Color format menus, value cells and badges create raw TextBlocks with no line metrics
- Class: VCENTER   Confidence: CHECK   Severity: medium   Reported bug: none (same class as #2, #10, #13)
- Windows:
  - `ColorEditor.cpp:304-305`: value `f->text` has FontSize/VerticalAlignment Center but no LineHeight / LineStackingStrategy.
  - `ColorEditor.cpp:585`: format names.
  - `ColorEditor.cpp:196,282`: badge TextBlocks inside `Padding({5,1,5,1})` Borders.
  - `ColorEditor.cpp:293`: `intensityName`.
  - `ColorEditor.cpp:382`: `miniHex`.
  - `ColorStrip.h:146`: strip labels.
  Segoe UI's ascent/descent asymmetry makes centred digits read about 1px low in the 28px cells and 40px hex cell. The shared `label()` (UiControls.h:424-429) sets `LineHeight(18)`/`MaxHeight` for this reason, but these sites bypass it.
- GTK reference: Pango baseline-centred labels in `button.number-value` (min-height 28, style.css:516) and `.edit-color-badge { padding: 0 5px }` (no vertical padding, style.css:520).
- Fix direction: build these with `label(data,...)` (or a shared `centeredText` helper that sets LineHeight and `TextLineBounds::Tight` where appropriate). Make the badge padding `{5,0,5,0}`. Confirm with a screenshot against GTK.

##### COLOR-15: Panel readout omits the out-of-gamut "!" marker
- Class: NULL-STATE (missing state), BORDER/COLOR   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `ColorView.cpp:462-468` draws only `readout_label`. There is no gamut indicator anywhere in `updateReadout`.
- GTK reference: `color_readout.rs:63-77` draws a bold "!" at `label_width + 6` when the colour is outside the display space or the document RGB space (`in_gamut` / `in_hdr_gamut`).
- Shared source: the check runs in the GTK host. `ColorPanelView` (crates/layer-ui/src/color.rs:180-200) has no gamut field, so Windows cannot show it. This is a policy implemented per host (AGENTS.md hard rule).
- Fix direction: add an `out_of_gamut: bool` (display and RGB space) to `ColorPanelView` in shared Rust, with a test. GTK reads it instead of computing it, and Windows draws the "!" with the same rasterizer at `label width + 6`.

##### COLOR-16: HDR intensity arc lacks the 0 EV tick and the keyboard focus ring (panel and editor)
- Class: NULL-STATE (missing affordance), HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows:
  - Panel `ColorView.cpp:586-609,517-552`: draws ramp lines, marker and caption only. The keyboard target is a hidden `Slider` (`intensity.Width(1);Height(1);Opacity(0)`, line 413), so keyboard focus on it shows nothing.
  - Editor `ColorEditor.cpp:685-715`: ramp and marker only. There is no slider at all, so the arc cannot be reached by keyboard, and it has no double-tap reset (the panel has one, `ColorView.cpp:411`).
- GTK reference: `hdr_color_scale.rs:156-162`. When focused it draws a ring at `radius+5`, 2px wide, ink α .9, then always strokes the `paths.zero` tick (1px, ink α .9). The editor uses the same `HdrColorScale` (bare wheel, caption hidden, `tool_panels.rs:1038-1039`), so it keeps focus, keyboard and double-click reset.
- Shared source: `HdrIntensityArc` geometry (zero point = `point((0-min)/(max-min))`).
- Fix direction: factor the arc (track, ramp, marker, zero tick, focus ring, hidden slider, double-tap reset) into one Windows helper used by both `ColorView` and `ColorEditor`. The two copies are currently near-identical (`ColorView.cpp:586-608` vs `ColorEditor.cpp:685-699`). Add the zero tick and focus ring from shared geometry.

##### COLOR-17: HDR ramp drawn as discrete round-capped line segments instead of a continuous antialiased raster
- Class: SEAM   Confidence: CHECK   Severity: low   Reported bug: none
- Windows: `ColorView.cpp:597-604` / `ColorEditor.cpp:697-698` create one `Shapes::Line` per path segment, each with a flat colour and round caps (`ramp[i].Stroke(fill(rgba(...)))`). This gives visible colour steps at segment joins and overlapping caps.
- GTK reference: `hdr_color_scale.rs:55-68` rasterizes per pixel with continuous gain and baked coverage. docs/ui/color-picker.md:270 notes the arc's antialiased coverage is baked into its raster.
- Fix direction: rasterize the arc on the existing `FieldWorker` (ColorWheel.h already has the `ArcRaster` path at lines 259-283 for the ring), or use a `LinearGradientBrush` per segment. Check the stepping on a screenshot first.

##### COLOR-18: Transparent slot leaves the HDR arc fully opaque and active-looking
- Class: NULL-STATE   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `ColorView.cpp:400,411,415` ignore input when `transparentSlot()` but never dim `arc`. The edit button is dimmed (`ColorView.cpp:631` `Opacity(.36)`).
- GTK reference: `tool_panels.rs:1060` `intensity.set_sensitive(state.slot != ColorSlot::Transparent)`, so the scale renders insensitive.
- Fix direction: apply the same disabled opacity to `arc` and `intensity.IsEnabled(false)` when the transparent slot is selected.

##### COLOR-19: Shape-glyph hover ignores pen
- Class: HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorView.cpp:385` `shapeHovered[i]=...==PointerDeviceType::Mouse`, while swatches use `!=Touch` (line 375).
- GTK reference: `.color-panel .color-shape:hover { color: var(--accent-color) }` (style.css:402) applies on any prelight, pen included.
- Fix direction: use `!=Touch`, matching the swatches.

##### COLOR-20: Tile and add-tile geometry in the swatch sheet
- Class: METRIC   Confidence: CHECK   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:469` add tile `Width/Height(SheetTile-6)` = 34 + `Margin 3`.
- GTK reference: `color_editor.rs:198` `set_size_request(SHEET_TILE + 2*TILE_PADDING)` = 40, plus `.picker-dialog button.palette-add { margin: 3px; padding:0 }` (style.css:527), so the visible + tile is 40 on GTK (larger than the 34 patches). Tile sizes otherwise match: 28/34 patch + 3 padding = Windows `RecentTile=34` / `SheetTile=40`. Section titles in GTK ellipsize (`color_editor.rs:898`). Windows `label()` titles do not trim (`ColorEditor.cpp:460`).
- Fix direction: confirm which size is intended, align Windows to GTK, and add `TextTrimming` to the section title.

##### COLOR-21: Hex cell reserves 4px more than GTK
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:192` `textWidth(L"#DDDDDD",...)+12`, with show padding `{4,0,4,0}` (line 302).
- GTK reference: the hex stack reserves the widest of `#888888/#DDDDDD/#BBBBBB` inside `button.number-value { padding:0 4px }` (style.css:516), which is +8.
- Fix direction: `+8`, or derive it from the show button's padding.

##### COLOR-22: Eyedropper touch offset is a fixed 44px instead of 10mm
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:734` `{L"touch_offset",N(44*scale())}`.
- GTK reference: `color_picker.rs:263-275` `finger_offset` = 10mm from the monitor's physical size, clamped to 36..64, with 44 only as the fallback.
- Fix direction: move the mm→px policy into shared Rust (take physical DPI as input), or compute it on Windows from `GetDpiForMonitor`/`DisplayInformation.RawDpiY` and pass the result.

##### COLOR-23: Dim/caption opacity .6 vs libadwaita dim-label .55
- Class: BORDER/COLOR   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `ColorEditor.cpp:185` captions `Opacity(.6)`, `453` empty note `.6`, `461` count `.6`, `ColorStrip.h:147` `space.Opacity(.6)`.
- GTK reference: `dim-label` (`color_editor.rs:263,891,900,1106`), libadwaita opacity .55.
- Fix direction: add one shared `dimOpacity` constant (or a palette/metric value) and use it at every site.

##### COLOR-24: Format and value popups are default WinUI MenuFlyouts, not themed panels
- Class: DEFAULT-CONTROL   Confidence: CHECK   Severity: low   Reported bug: none (same class as #11)
- Windows: `ColorEditor.cpp:280,586-592` attaches an unstyled `MenuFlyout` with `RadioMenuFlyoutItem`s. It is not passed to `TrackPopup`, so it is not counted in `data->popup` like the panel's `editMenu` (`ColorView.cpp:423`). It uses Fluent presenter colours inside a ContentDialog whose theme is set per dialog (`dialogSurface`, UiControls.h:416-418).
- GTK reference: `color_editor.rs:312` `gtk::Popover` with radio `CheckButton`s on `--popover-bg-color: var(--capy-panel)` (style.css:8).
- Fix direction: route it through `TrackPopup` and the app's shared themed MenuFlyout presenter style, if one exists. Check light and dark legibility on a screenshot.

---

#### Root causes / systemic patterns

1. **No clipped rounded-frame helper.** `framePair` (ColorEditor.cpp:365) and the strip pair (ColorStrip.h:143) depend on `Grid.CornerRadius`, which does not clip children. A single clipped pair helper using the control radius and a 1px `tint(text,31)` border closes COLOR-1, COLOR-6 and part of COLOR-4.
2. **Literal radii instead of `controlCorner()` / `CornerFit` / `SurfaceRadius`.** This affects the well/pill (17/13, also in NewDrawingDialog.cpp:508-511), tiles (6/5), badges (4), strip (6/12). A `pillWell()` helper plus consistent use of `controlCorner()` closes COLOR-4 and COLOR-10.
3. **Dialog text fields use the stock TextBox template.** A shared "dialog field" styler that overrides the `TextControl*` resources (text@10%, no border, control radius, error border via resource, no DeleteButton) closes COLOR-2 and the invisible-invalid-border half of COLOR-12. It belongs next to the existing per-site overrides in NumberControl.cpp:122-126 and WorkspaceManagerView.cpp:323.
4. **The HDR arc is implemented twice on Windows** (ColorView.cpp:586-608, ColorEditor.cpp:685-715), and both copies omit the zero tick, focus ring, disabled state and (editor) keyboard/double-tap reset. One shared arc helper closes COLOR-16, COLOR-17 and COLOR-18.
5. **A host-side policy exists only in GTK.** The out-of-gamut readout marker is computed in `color_readout.rs`, and the editor wheel's 160..232 fit and the 10mm touch offset are also host code. Moving these into `ColorPanelView` / `color_ui` closes COLOR-15, COLOR-9 and COLOR-22, as the architecture rule requires.
6. **Ad-hoc TextBlocks bypass `label()`** (no LineHeight, default FontSize 14, ad-hoc opacity). Routing the editor and strip text through `label()` plus one dim-opacity constant closes COLOR-7 (sizes), COLOR-14 and COLOR-23.

### Moved from the panels audit

##### FX-1: Drawer bodies use fixed heights, so the Filter Types Cancel button never docks at the drawer bottom and the list scrolls when it doesn't need to
- Class: LAYOUT-COLLAPSE, METRIC   Confidence: CERTAIN   Severity: high   Reported bug: 21
- Windows:
  - `apps/layer-windows/PanelBody.cpp:222-224`: in a drawer (`scrollable=false`) `filter_types` gets `root.Height(440)` and `adjustments` gets `root.Height(480)` (or 440 when split). These are exact heights, not minimums.
  - `apps/layer-windows/WorkspaceDrawers.cpp:348-353` (`placeColumns`): `adjustments`/`layers` are forced to `Height(480.)` and the navigator to `272.`. The 440 split value is overwritten. `filter_types` falls through to `ContentHeight()`, and because PanelBody never sets `contentHeight` for it, that returns -1, so it keeps 440.
  - `WorkspaceDrawers.cpp:290` sets `column.stack.VerticalAlignment(Top)` inside a `ScrollView`, and `measureHeights()` (`:381-395`) reports the stack's natural height. The drawer therefore grows to the tallest column (adjustments at 480). The Filter Types column is placed at that height, but its body stays 440 tall and is pinned to the top. The result is a gap under Cancel. The category list gets a Star row of only `440-16-4-26` px, so it scrolls even though the column has room.
  - `apps/layer-windows/FilterView.cpp:180-189`: FilterTypesPanel is a Grid with a Star list row and an Auto footer row. That layout is correct, but it only works when the Grid gets the full column height.
- GTK reference: `apps/layer-linux/src/workspace_drawer.rs:435-442` uses `set_height_request(440)` on both `filter_types` and `adjustments`, which is a minimum. `effects.rs:140-149` makes `types_scroll.set_vexpand(true)` and then appends Cancel. The column Box sits in a Viewport (`workspace_drawer.rs:454-456`) and fills the drawer, so Cancel stays at the bottom and the list only scrolls when the drawer really is shorter than its content.
- Shared source: the drawer column heights come from Rust `drawer_placement(... heights)`, and each column's `height` is in `geometry.placement.columns[i].height`.
- Fix direction: give drawer "fill" bodies (filter_types, adjustments, layers, navigator) the column's placement height (`num(placement,"height")` minus tab height and insets) in `placeColumns`. Use the 440 minimum only for measurement (`MinHeight`, or report 440 as the measured height). Delete the `Height(440/480/272)` literals in PanelBody and placeColumns. Also drop the 480 for adjustments in a drawer, since GTK uses 440 (the duplicate in `PanelConfiguration.cpp:93,102` follows the same pattern). One helper, `fillHeight(body, placement)`, covers every drawer panel that has a docked footer. Audit result: only FilterTypes has bottom-docked action buttons in this area. Palettes (footer in a Grid Star layout, measured via contentHeight) and Stats/Proof (natural StackPanels) are not affected.

##### FX-3: Navigator in a drawer is fixed at 272px whatever the width
- Class: LAYOUT-COLLAPSE, METRIC   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `WorkspaceDrawers.cpp:348-349` sets `body.view->Root().Height(body.view->navigator?272.:480.)`. This overrides the `NaturalHeight(width)` that PanelBody sets (`PanelBody.cpp:225`). NavigatorLayout clamps the image to `available-fixedHeight` (`NavigatorView.cpp:71-74`), so a wide drawer shows a letterboxed overview, and the buttons are not at the bottom when the column is taller.
- GTK reference: `navigator.rs:300-321` uses a natural vexpand Box with no fixed height in drawers.
- Fix direction: use `navigator->NaturalHeight(width)`, or the placement height as in FX-1.

##### TOOL-2: Drawer replays its opening on any drawer-model change, e.g. brush category (user bug 1)
- Class: ANIMATION-RETRIGGER   Confidence: LIKELY   Severity: high   Reported bug: 1
- Windows: `WorkspaceDrawers.cpp:73-81` (`Drawer::apply`) — any change to the serialized drawer model (`current.Stringify()`,
  which includes `tool_set` for the Brushes/tool drawer, see the projection at `:302`) sets `from=placement; started=now;
  animating=true` and **`heights.assign(columns,0.)`**. The next `drive()` asks for `drawer` geometry with zero heights
  (`:186-188`), stores the collapsed placement in `geometry` (`:193`), and `measured()`→`paintBackground()` (`:397-409`,
  `:423-426`) paints the background from that collapsed geometry before the remeasured heights come back. The panel then grows
  back over 200 ms, which reads as the open animation replaying. Selecting a category changes `tool_set.subtools` → new model
  key → the drawer collapses and reopens.
- GTK reference: `workspace_drawer.rs:1034-1131` — `changed` marks the origin at the *current* target (`mark_origin`) and
  animates only from the presented bounds to the new bounds (`animate`, `:955-990`); measured content sizes are never
  discarded, so an unchanged size produces no visible motion. Content changes are refreshed in place (`body.refresh(..., next.tool_set)`, `:1107`).
- Fix direction: keep `heights` across model changes unless the column structure (`columns` panel ids) changes; only adopt a
  queried placement after `measureHeights()` agrees; and treat content-only changes (tool_set, selection) as a resize tween
  from the presented placement, not a new opening. Same rule should apply to every drawer kind (tool and column drawers share
  `Drawer`). Add a test that a `tool_set`-only change keeps `heights` and the placement bounds.
- Contributing, in scope: `ToolView.cpp:118-126` rebuilds the subtool list on category change (expected — the item list
  differs; GTK does the same, `tool_panels.rs:110-152`), so the drawer's stack is re-measured immediately after its heights
  were zeroed. See TOOL-17 for the theme-only rebuild.
