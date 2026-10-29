# Windows UI parity: primitives and shell

[Windows development](windows.md)

Bring the Windows client's shared WinUI helpers, workspace shell and colour
panels to visual parity with GTK. The plan also fixes reported bugs 1, 2, 3,
4, 11, 13, 15, 18 and 19 and owns the [helper contract](#helper-contract) the
other two plans build on.

## Prompt

> You are Agent A+B for the Windows UI parity work in Capy Canvas: you
> implement both `docs/development/windows-ui-parity-shell.md` and
> `docs/development/windows-ui-parity-panels.md`. Read `AGENTS.md`,
> `apps/layer-windows/AGENTS.md` and both plans, then follow the combined
> [order of work](#order-of-work) below: five milestones, each one commit (or a
> few pushed together), one VM build and one test round. Land M1, the helper
> contract (P1–P8), first and push it, because Agent C is working
> `windows-ui-parity-pages.md` in parallel and waits on it. Edit only the files
> the two plans own. Fix root causes, add regression checks, push each
> milestone to `origin/main` as soon as its checks pass, and remove each finding
> from its plan in the commit that fixes it. Reproduce CHECK findings before
> fixing them, or drop them. Aim to finish in about two to three hours. Finish
> with a list of what you fixed, what you dropped and what you could not run.

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

The same agent also owns every file the panels plan
lists.

Changing a helper signature may require a mechanical edit at call sites in
Agent C's files. Make that edit in the same commit, keep it mechanical (no
visual retuning there), and push it promptly so C rebases onto it early. Delete
per-site workarounds the new helpers supersede in your own files in the same
commit, e.g. the duplicated `DropDownOpened`/`DropDownClosed` popup blocks.
Leave cleanups in C's files that a signature change doesn't force, such as the
`alignNames` baseline shift in `WorkspaceManagerView.cpp`, to C's helper
adoption milestone.

## Order of work

One agent implements this plan and the panels plan
in five milestones. Each milestone is one commit, or a few commits pushed
together, with one VM build and one test round, pushed to `origin/main` when
its checks pass. Fix every finding listed for a milestone before building, so
the VM builds and captures once per milestone, not once per finding.

- Overlap work with the VM. Once a round's sync has finished, write the next
  milestone's code while the VM builds and runs, and stage commits by explicit
  path so each holds only tested changes. Run shared Rust tests and GTK
  reference captures on Linux while the VM works.
- Spend at most a few minutes reproducing a low-severity CHECK finding. If it
  doesn't reproduce, drop it and name it in the commit message.
- A low-severity METRIC finding whose values already match GTK needs no visual
  change; move the literal to the shared metric only when you are editing that
  code anyway, otherwise drop it with a note.

**Baseline, before M1.** Start the VM and build the unchanged tree with one
small fixture run, which also warms the build caches. While you write M1, use
that build for M1's before captures and to reproduce bug 22 with its stack, so
M2 knows whether M1's P3 alone fixed it.

**M1. Helper contract** (critical path for Agent C, push it first): P1–P8 with
PRIM-1 to PRIM-23 and COLOR-1, in `UiControls.h`, `ButtonFeedback.h`,
`ChoiceFeedback`, `NumberControl`, `RangeControl`, `PickerDialog.h`,
`NativeMenus.h`, `CommandSearch.h`, `ScrollIndicators.h`, `PanelScroll.h` and
`App.cpp`. P1 fixes bugs 2 and 13, P3 fixes bug 11 and gives every ComboBox
the popup bookkeeping FX-4 points at. Verify the canvas bar "Scale" segment and
the About rows before and after with zoomed captures: the gaps above and below
the glyphs should match within 0.5 px. Name the final helpers and signatures in
the commit message, which must contain "Windows UI parity" so C finds it with
`git log origin/main --grep='Windows UI parity'`.

**M2. Panel bugs and the Layers panel**: bugs 5, 6, 7, 8, 17, 20, 22 and the
hit-test half of 19, then every remaining LAYER finding except LAYER-18.

- Bug 22 (FX-4 to FX-7): fix the cause the baseline stack shows (debugger or a
  Windows Error Reporting local dump), with a fixture step that opens Blend Mode
  with no filter selected and fails without the fix. Guard
  every `SelectedIndex`/`GetObjectAt`/`GetNamedValue` read in the panel files,
  using M1's `selectIndex`.
- Bug 20 (TOOL-1, TOOL-4): stop rebuilding the slider on context changes, or
  reset `trackKey`/`marksKey` with the rebuilt elements; keep matching option
  fields.
- Bug 5 (TOOL-3, SHELL-8): one wrap panel that measures rows from the width it
  gets, replacing the three Canvas layouts in `ToolView.cpp` and
  `TextureDialog.cpp`.
- Bugs 6 and 7 (LAYER-1, 2, 7, 8, 14): a non-control tappable-surface helper in
  `LayersView.h` for the name, grip and paper row; the selection fill paints
  only on the row; LAYER-7 uses P6.
- Bug 8 (LAYER-3, 4, 5): a shared Rust before-first/after-last list-edge rule
  with a headless test; both drag hosts hit-test against the panel root and
  clamp to the first and last row. `WorkspaceRowDrag` also serves C's
  workspace manager and drawing tabs: keep its interface stable and say in the
  commit message how the drop rule changed.
- Bug 19 hit-test (LAYER-19): give the layer panel content a hit-testable clear
  background. Bug 17 (LAYER-17): measure against a GTK capture first and fix
  only a measured offset.
- LAYER-6, 9 to 16 and 20 to 22, with LAYER-13 on P4.

**M3. Workspace shell**: bugs 1, 3, 4, 15, 18, 21 and the lifecycle and routing
halves of 19. Reread SHELL-1 to SHELL-4, FX-1 and FX-3 against
`crates/layer-ui/src/drawer_geometry.rs` first.

- Drawer lifecycle (SHELL-1 to 4, 26, TOOL-2): port GTK `Drawer::refresh`.
  Retire a drawer only when its model is gone, keep the measured heights,
  animate only on open, close or resize, and don't rebuild on theme change.
  Then SHELL-24 (double-tap routing, CHECK).
- Pixel snapping (SHELL-5, 16, 17, 25): one rounding step for frames, drawers,
  bridges and columns, with the same rounded bounds passed to
  `capy_panel_bridges`.
- Glass in transient states (SHELL-6, 7, 14): the tab slide preview and expanded
  panels use glass roles. Leave vertical resize (bug 16) as recorded in SHELL-7.
- Drawer sizes (FX-1, FX-3, SHELL-9, SHELL-10, LAYER-18): panels that fill a
  drawer column take the column's height with the GTK minimum; FX-2 gives the
  Filter Types Cancel its P5 face. Make the drawer width breakpoint in shared
  Rust independent of display scaling, with a headless test.
- Shell metrics and headers (SHELL-11 to 13, 15, 18 to 23): one tab-header
  builder and catalog metrics.

**M4. Tools, filters and the remaining panels**: TOOL-5 to TOOL-27 and FX-8 to
FX-38, including every remaining helper adoption in the panel files (FX-13,
FX-16, FX-24, FX-36, FX-38, TOOL-8, TOOL-21, the `PressureCalibration` segments
and the palette text fields). Split it into two rounds at the TOOL/FX boundary
only if a failure in one half blocks the other.

**M5. Colour panels and close-out**: COLOR-2 to COLOR-24 and PRIM-24 (CHECK).
Then run every fixture listed below once, recheck every panel against GTK
(M1's `label()` change moves text everywhere), and delete each plan in the
commit that closes its last item, keeping deferred items (bug 16) recorded in
the commit message.

## Validation

Use the `capycanvas4` VM only (the helper's default for this worktree). Keep it
warm between milestones and stop it when you finish. Use ordinary WARP
`fixtures` for layout, metrics and colours. Claim at most one pool GPU, only when
a capture needs hardware (glass, bridge surfaces, drawer motion): a running VM
cannot change its GPU, so stop it and `start --gpu`. Keep one adapter mode per
round, because each mode builds separately.

- Every milestone: `fixtures --profile dev-perf` with that milestone's fixtures
  (`--no-build` when only fixture scripts changed), then strict Clippy from the
  synced tree over SSH:
  `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`.
- M1, M2, M3 and M5 also run `check` (`test-without-gpu.ps1` and the Windows Rust
  tests). When shared Rust changes (M2, M3), run
  `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib`
  on Linux before the VM round, with `CARGO_TARGET_DIR` under `/mnt/localssd`.
- Fixtures (names as `fixtures` takes them; a bare name runs every variant):
  - M1: `button-feedback`, `popup`, `tooltips`, `scroll-indicators`,
    `command-search`, `canvas-bar`, `shortcut-page`, `header-settings`,
    `compact-color`, `effects`.
  - M2: `layers`, `layer-drawer`, `layer-pickup`, `workspace-pickup`,
    `toolbar-components`, `toolbars`, `tools`, `tool-variations`,
    `tool-bubbles`, `effects`, `illustration-filters`, `manager`,
    `switcher-drag`.
  - M3: `drawers`, `drawer-layout`, `layer-drawer`, `tab-drag`, `tab-pickup`,
    `panel-bridge-layout`, `column-stacks`, `transparency`, `expansion`,
    `workspace-layout`, `navigator`, `navigator-allocation`, `effects`.
  - M4: `toolbar-components`, `toolbars`, `tools`, `tool-variations`,
    `tool-bubbles`, `pressure`, `stroke-recording`, `effects`,
    `illustration-filters`, `palettes`, `navigator`, `scopes`, `proof`.
  - M5: `color-editor`, `color-picker`, `compact-color`, then the union of every
    list above once.
- Before blaming your change for a fixture failure, check the known failures in
  [testing](testing.md) (for example `proof` timing out choosing sRGB after save
  and reopen) and rerun it on `origin/main` when unsure.
- Captures, light and dark, glass on and off, of what each milestone changed,
  batched into one scripted run per theme, measured against the GTK values
  quoted in each finding. M3 journeys: switch brush categories with the sketch
  drawer open (no replay); double-tap in the layers drawer list (drawer stays);
  drag a floating tab sideways (glass); open a single panel from a collapsed
  column group (no bridge gap). M2 journeys: sketch size and strength sliders
  after switching tool, brush, layer and document; drag the Brush Sizes panel
  out and drop it; press and select layers; drag a layer onto the header buttons
  and below the last row; Blend Mode with and without a selected filter.
- Drawer animation, tab drag, sliders and bubbles affect how frames are
  produced, but the Windows reference PCs are not reachable from this machine
  and a VM cannot qualify a frame-rate target. List those rows as not run, and
  make sure motion paths gain no per-frame rebuilds, allocations or layout
  passes.

## Shared context for all three plans

The Windows parity work is split into three plans that run in parallel on
different machines. Each plan owns a set of files, so none of them edits the
same file at the same time:

| Plan | Owner | Scope |
| --- | --- | --- |
| [Primitives and shell](windows-ui-parity-shell.md) | Agent A | Shared WinUI helpers and theme resources, workspace shell (tabs, drawers, bridges, columns, drag states), colour panels |
| Panels | Agent B | Layers, tools and brushes, toolbar components, bubbles, filters and effects, curves, scopes, proof, navigator, palettes |
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

#### DEFAULT-CONTROL: WinUI templates and colours that leak through

#### Other primitive differences

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

### COLOR area: Windows vs GTK parity audit

Scope read in full: `apps/layer-windows/ColorView.cpp/.h`, `ColorEditor.cpp/.h`, `ColorWheel.h`, `ColorStrip.h`, `ColorPair.h`, `ColorTouch.h`, `SwatchFace.h`, `PickerDialog.h` (Edit Color popup shell), plus the `ButtonFeedback.h`/`UiControls.h` helpers they rely on.
GTK reference: `apps/layer-linux/src/color_editor.rs`, `tool_panels.rs` (ColorWheel, WheelButton), `color_readout.rs`, `hdr_color_scale.rs`, `color_pair.rs`, `display_color.rs`, `style.css`.
Shared: `crates/layer-ui/src/color.rs` (`ColorPanelLayout`, `ColorWheelGeometry`, `stage_for_wheel`), `crates/layer-ui/src/layout.rs` (`control_radius: 13`), docs `docs/ui/color-picker.md`, `docs/ui/theme-colors.md`.

Corner-radius reference: GTK `--corner-fit: .54`, `--control-radius = 13 * .54 = 7.02px`, `--surface-radius = 18 * .54 = 9.72px` (style.css:8). Windows equivalents: `controlCorner()` (UiControls.h:399) and `SurfaceRadius=18` for squircles (WorkspaceGeometry.h:156).

None of the 22 user-listed bugs are in the colour area. The findings below are other instances of the same failure classes.

---

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
