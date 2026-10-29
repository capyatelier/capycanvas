# Windows UI parity: panels

[Windows development](windows.md)

Bring the Windows layers, tools, toolbar components, bubbles, filters and
effects, curve, gradient, scopes, proof, navigator and palette panels to visual
parity with GTK. The plan also fixes reported bugs 5, 6, 7, 8, 17, 20, 22 and
the panel halves of 19 and 21.

## Prompt

> You are Agent B for the Windows UI parity work in Capy Canvas. Read
> `AGENTS.md`, `apps/layer-windows/AGENTS.md` and
> `docs/development/windows-ui-parity-panels.md`. Implement that plan on
> Windows. Agents A and C are working `windows-ui-parity-shell.md` and
> `windows-ui-parity-pages.md` in parallel on other machines. Agent A owns the
> shared helpers (`UiControls.h`, `ChoiceFeedback`, `ButtonFeedback`,
> `NumberControl`, drawers and panel bodies); do not edit them. Start with the
> independent items, then fetch `origin/main` and adopt A's helpers when they
> land. Edit only the files your plan owns. Fix root causes, add regression
> checks, push each milestone to `origin/main` as soon as its checks pass, and
> remove each finding from the plan in the commit that fixes it. Reproduce
> CHECK findings before fixing them, or drop them. Finish with a list of what
> you fixed, what you dropped and what you could not run.

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

Independent of Agent A (start immediately):

1. **Bug 22 crash** (FX-4, then FX-5, 6, 7). Reproduce Blend Mode with no
   filter selected under a debugger and capture the stack before you fix it.
   Guard every `SelectedIndex`/`GetObjectAt`/`GetNamedValue` read in your views.
   If the cause is missing popup bookkeeping, report the popup at the
   EffectView and FilterView sites the way `GradientView.cpp` does; Agent A
   later folds this into `choiceColors` (P3).
2. **Bug 20 sliders** (TOOL-1, TOOL-4): stop rebuilding the slider on context
   changes, or reset `trackKey`/`marksKey` with the rebuilt elements; keep
   matching option fields.
3. **Bug 5 zero height** (TOOL-3, SHELL-8): one wrap panel that measures rows
   from the width it gets, replacing the three Canvas layouts in `ToolView.cpp`
   and `TextureDialog.cpp`.
4. **Bugs 6 and 7** (LAYER-1, LAYER-2, LAYER-8, LAYER-14): add a non-control
   tappable-surface helper in `LayersView.h` for the name, grip and paper row;
   paint the selection fill only on the row.
5. **Bug 8 drops** (LAYER-3, LAYER-4): a shared Rust before-first/after-last
   list-edge rule with a headless test; both drag hosts hit-test against the
   panel root and clamp to the first and last row.
6. **Bug 19 hit-test** (LAYER-19): give the layer panel content a hit-testable
   clear background.
7. **Bug 17** (LAYER-17): measure against a GTK capture first; fix only a
   measured offset.
8. Remaining LAYER, TOOL and FX metric, border and null-state items.

After Agent A lands P2–P7: LAYER-7 (P6), FX-2, FX-13, FX-16, FX-36 (P5),
FX-24, FX-38 (P3), TOOL-8, TOOL-21 and the PressureCalibration segments (P7),
LAYER-13 and the palette text fields (P4). Check every panel again after P1,
because `label()` changes text position everywhere.

## Validation

- `apps/layer-windows/scripts/test-without-gpu.ps1`;
  `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib`;
  `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`.
- Fixtures: `exercise-layers` (all variants incl. `-Relationships`),
  `exercise-layer-drawer`, `exercise-layer-pickup`, `exercise-workspace-pickup`,
  `exercise-toolbar-components`, `exercise-toolbars`, `exercise-tools`,
  `exercise-tool-variations`, `exercise-tool-bubbles`, `exercise-pressure`,
  `exercise-effects`, `exercise-illustration-filters`, `exercise-palettes`,
  `exercise-navigator`, `exercise-navigator-allocation`, `exercise-scopes`,
  `exercise-proof`.
- Journeys, light and dark, glass on and off: sketch size and strength sliders
  after switching tool, brush, layer and document; drag the Brush Sizes panel
  out and drop it; press and select layers; drag a layer onto the header
  buttons and below the last row (mouse, touch, pen); Blend Mode with and
  without a selected filter; filters drawer Cancel at the bottom.
- Slider and bubble changes affect frames: measure the affected rows by
  [measuring](../performance/measuring.md) on the Windows reference device.

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

### Layers area: Windows vs GTK parity audit (AREA=LAYER)

Read in full: LayersView.cpp/.h, LayerRow.cpp, LayerRowDrag.cpp/.h, LayerThumbnails.cpp/.h, LayerInspector.h,
ThumbnailOwners.h, WorkspaceRowDrag.cpp/.h, WorkspaceBubbles.cpp, BubblePresentation.h; ButtonFeedback.h and the
`button()`/`selected()` helpers in UiControls.h. GTK: layers.rs, layers/connections.rs, swipe_row.rs, the
workspace_bubbles.rs card, and the style.css layer rules. Shared: layer-ui layout.rs, drawers.rs, effects.rs, session.rs.

The Windows row column grid (`{slot+gap, slot+gap, 0, 5, 32, 0, 12, 32, 0, *, 0, 14, 16}`, LayerRow.cpp:121) adds up
to the same widths as GTK's 2-px Box spacing: eye 26+2, gutter 3+2, thumbnail 30+2, link 10+2, name margin 6, lock 12,
grip 16, edge 6. Row height is the same (40 = 36 + 2×2 padding). The swipe colours match (.16/.24/#c01c28), as do the
group indentation (min(depth·8, 24)), the 14-px type badge (padding 1, radius 2, input fill, margin 3), the 1-px/3-px
thumbnail ring geometry and the connection-line widths. Everything else is listed below.

##### LAYER-1: Pressing or hovering a layer name lights up a large rounded box (user bug 6)
- Class: HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: high   Reported bug: 6
- Windows: `apps/layer-windows/LayerRow.cpp:180-182`. The name is made with `button(data,…)`, which calls
  `buttonColors` and so `buttonFeedback` with hover 20 and press 41 (UiControls.h:460-476). It is stretched across the
  star column 9, with `MinHeight(36)`, `Margin({6,0,2,0})` and `CornerRadius(controlCorner())` (≈7 px). The result is
  an overlay box from the thumbnail to the lock column, the full row height, on hover and on press. It also gets
  `UseSystemFocusVisuals(true)` (ButtonFeedback.h:109), which draws a focus rectangle around the same box.
- GTK reference: `apps/layer-linux/src/layers.rs:570-601`. The name is a plain `gtk::Label` inside a
  `text` Box (`margin_start 6`, `hexpand`), with no hover or press style. Row selection comes from the row's
  `GestureClick` (layers.rs:687-722), and rename from a double-press gesture on the label only (layers.rs:723-736).
  `.layer-row` has no :hover/:active rule in style.css.
- Fix direction: replace the name `Button` with a non-control element (the `caption` StackPanel inside a Grid or
  Border, transparent background, hit-testable). Selection is already handled by `root.Tapped` (LayerRow.cpp:223-227),
  and `DoubleTapped`/F2 can move to that element. If a keyboard stop is needed, keep `buttonFeedback(...,fixed=true)`
  rather than an overlay. Delete the `name` Click path; the root tap already selects.

##### LAYER-2: The selection-column icon (brush, check or reference) paints a second selection fill on a selected row (user bug 7)
- Class: STACKED-BG, BORDER/COLOR   Confidence: CERTAIN   Severity: high   Reported bug: 7
- Windows: `apps/layer-windows/LayerRow.cpp:336` sets `check.Background(flag(layer,L"selected")?selected(data):clear())`,
  while LayerRow.cpp:277 already sets `body.Background(...selected(data)...)` for the same row. The brush icon is
  `selection_icon = "layer-brush-symbolic"` for the drawing target (crates/layer-ui/src/session.rs:6361-6371), so a
  selected row shows a 26×36 rounded box of selection colour on top of the selection colour. In glass mode
  (`selected()` returns `glass(L"selection")`, which is translucent) the stacked fill comes out a visibly different,
  darker colour. In opaque mode its rounded edge shows against the row.
- GTK reference: `layers.rs:527-532` and `:1723-1726`. `selection` is a flat `.layer-icon.layer-column` button whose
  background is never changed. Selection is shown only by `.layer-row.selected { background: var(--selection-bg) }`
  (style.css:205; glass version at :462).
- Fix direction: remove `check.Background(...)` at LayerRow.cpp:336 and leave it `clear()`. Keep the selected state in
  automation only (`SetItemStatus` is already there).

##### LAYER-3: Dropping a layer above the top row (over the header buttons) or below the last row finds no target (user bug 8)
- Class: HIT-TEST   Confidence: CERTAIN   Severity: high   Reported bug: 8
- Windows: `apps/layer-windows/LayerRowDrag.cpp:266-279`, `pick()`:
  `if(at.X<0||at.Y<0||at.X>list.ActualWidth()||at.Y>list.ActualHeight())return {};`. It then matches only
  realized row bounds. A pointer over the header (blend/opacity row and the alpha-lock/lock/attachment/reference row,
  LayersView.cpp:105) returns no hit. So do the empty list space under the last row and the "Add paper" row, which is
  not in `rows`. `target()` then clears `placement`, and `complete()` commits nothing (LayerRowDrag.cpp:282). Auto-scroll
  in `tick()` (LayerRowDrag.cpp:373-377) keeps scrolling while the pointer is above the list, but the drop stays
  invalid. Shared Rust also rejects fractions outside [0,1] (crates/layer-ui/src/session.rs:11169-11170), so the host
  cannot pass a negative fraction instead.
- GTK reference: `layers.rs:866-902`. GTK also installs a `DropTarget` per row only, with no list- or panel-level
  target, so it probably has the same gap.
- Fix direction (class fix): add one list-edge rule in shared Rust. A drop above the first visible row resolves to
  Above(first), and a drop below the last resolves to Below(last); one way is a `layer_drop` query that takes
  a list-relative y plus the first/last row ids, or one that accepts `edge: "before_first" | "after_last"`. Then
  widen the Windows `pick()` horizontal test to `view->root` (header + list + footer) and clamp y into the first/last
  realized row. Add a GTK `DropTarget` on the `LayerPanel` root/overlay that uses the same query. Add a
  headless Rust test for both edges.

##### LAYER-4: Every WorkspaceRowDrag list (workspace manager, drawing tabs) has the same edge-extent failure
- Class: HIT-TEST   Confidence: CERTAIN   Severity: medium   Reported bug: 8 (same class)
- Windows: `apps/layer-windows/WorkspaceRowDrag.cpp:166-168`,
  `if(!dragging||!current()||position.X<0||position.X>surface.ActualWidth()||position.Y<0||position.Y>surface.ActualHeight())return;`.
  `surface` is the list's own `listSurface` Grid (WorkspaceManagerView.cpp:206, DrawingTabs.h:232). Dragging above the
  first row into the manager's header or action row, or below the list, sets `valid=false`, and release does nothing
  (`commit=dragging&&valid` at :217). Inside the surface, above-first and below-last already resolve (:170-181), so
  only the surface extent is wrong. Auto-scroll (:244-250) works only inside the 32-px band of the surface.
- GTK reference: rows use `.workspace-drop-before/after` (style.css:110-111) on per-row targets.
- Fix direction: test `position.Y` against the panel or dialog containing the list (pass a hit surface separate
  from the hint surface into `WorkspaceRowDrag`) and clamp to the first/last realized row. This is the same
  edge-extent rule as LAYER-3, so define it once.

##### LAYER-5: The WorkspaceRowDrag insertion line uses a hard-coded blue instead of the accent role
- Class: BORDER/COLOR, METRIC   Confidence: CERTAIN   Severity: medium   Reported bug: none
- Windows: `WorkspaceRowDrag.cpp:290`, `hint.Background(fill(color(L"#3584e4")))`, a fixed GNOME blue that ignores
  the theme accent and dark mode. `:189` draws it inset by 8 px on each side (`Width(surface-16)`, `Margin({8,line,0,0})`).
- GTK reference: style.css:110-111, `.workspace-drop-before { box-shadow: inset 0 2px var(--accent-bg-color); }`, full row
  width, drawn inside the row's top or bottom edge.
- Fix direction: use `accent(data)`, which means passing `WorkspaceData` into `WorkspaceRowDrag`. Span the full row
  width. Place the line inside the target row edge, not on the boundary.

##### LAYER-6: A layer being dragged has no floating preview; the source row only dims to .55
- Class: GLASS, HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: medium   Reported bug: none (related to 4)
- Windows: `LayerRowDrag.cpp:338-339`, `sourceRow->root.Opacity(.55)`. Nothing follows the pointer. WorkspaceRowDrag
  does the same (`row.Opacity(.55)`, WorkspaceRowDrag.cpp:205).
- GTK reference: `layers.rs:293-315`, `drag_preview`. A snapshot of the row is taken once at pickup, drawn over an
  opaque `palette.panel` fill at `push_opacity(0.7)`, converted to a squircle, and attached at the grab hotspot. The
  source row keeps full opacity. For the workspace manager, `.workspace-row-dragging { opacity: .45 }`
  (style.css:109), not .55.
- Fix direction: add a shared native row-preview helper (a snapshot of `row->root` placed in a top-level Canvas at the
  grab offset, panel fill, opacity .7) used by both drag classes. Match the source-row opacity to GTK (layer row unchanged;
  manager row .45).

##### LAYER-7: The alpha-lock, lock and attachment toggles use the selection colour when checked; GTK uses a neutral tint
- Class: BORDER/COLOR, STACKED-BG   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `LayersView.cpp:81` sets `pick.Background(flag(layer,spec.property)?selected(data):clear())`, and
  `buttonColors<ToggleButton>` also sets `checkedSurface=selected(data)` (UiControls.h:461-463). The attachment
  toggle does the same at LayersView.cpp:92.
- GTK reference: `layers.rs:179-186`, flat `.layer-icon` toggles. There is no `.layer-icon:checked` rule, so
  libadwaita's flat `:checked` (a neutral `alpha(currentColor,…)` tint) applies. Only `.layer-reference:checked` is
  overridden, to `alpha(fg,.12)` (style.css:207); Windows matches that one with `tint(text,31)`.
- Fix direction: use one neutral checked tint (`tint(L"text",31)` or the Adwaita flat value) for every layer header
  toggle, as already done for `reference`. Drop the duplicate explicit `Background(...)` assignments, because the
  checkedSurface already drives the template.

##### LAYER-8: The grip is a button with its own hover/press box, disabled dimming and a slightly different opacity
- Class: HIGHLIGHT-GEOMETRY, METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `LayerRow.cpp:212`, `grip=pick(L"",12,[]{})`, a `button()` 16×16 with hover/press feedback. `grip.Opacity(.6)`
  at :212 and :256. `grip.IsEnabled(drag_from_handle)` at :359 also applies the feedback disabled-content dimming
  (ButtonFeedback.h:85).
- GTK reference: `layers.rs:610-613`, a `gtk::Image` with `.dim-label` (Adwaita opacity .55). It has no hover
  style and is not dimmed when locked.
- Fix direction: make the grip an Image or Border, not a Button (it has no click action). Use .55 to match dim-label,
  and keep the swipe fade multiplier.

##### LAYER-9: The link-mask button is 30 px tall; GTK's is the full 36-px row content height
- Class: HIGHLIGHT-GEOMETRY, METRIC   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `LayerRow.cpp:129-130`. `pick` gives every column `Height(30)`, so `link` (10×30) gets a 30-px hover box.
- GTK reference: `layers.rs:554-556`. `link` keeps the default valign FILL in `thumbnails`, so it is 10×36. Only the
  thumbnail buttons set `valign Center`.
- Fix direction: stretch `link` vertically, like eye and check (LayerRow.cpp:135).

##### LAYER-10: Name and description lines are 20 px; two-line rows grow past GTK's height
- Class: VCENTER, METRIC   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `LayerRow.cpp:184-185`, `title.LineHeight(20)` and `meta.LineHeight(20)` (overriding `label()`'s 18 with
  `MaxHeight` stacking). With a description, the caption is 40 px, more than the 36-px name slot, so the row grows to
  44. A single-line title sits in a 20-px line box, which does not match GTK's natural Pango line.
- GTK reference: `layers.rs:570-600`. Both lines are plain labels at the natural line height (≈18 px at 11 pt) inside
  a `valign Center` Box, so the row stays at 40.
- Fix direction: remove both `LineHeight(20)` overrides and keep the shared `label()` metrics, so both lines fit in 36.

##### LAYER-11: Group folder and adjustment icons are 28 and 16 px; GTK uses 24 px for both
- Class: METRIC   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `LayerRow.cpp:311-318`, `contentSymbol` size `preview?14:adjustment?16:28` and glyph
  `preview?12:adjustment?16:28`.
- GTK reference: `layers.rs:539-541` and `:1693`. `effect_icon.set_pixel_size(if thumbnail {12} else {24})` is used for
  folders, adjustments and type icons alike. The `.layer-folder image {-gtk-icon-size:28px}` rule (style.css:216) is
  overridden by the explicit pixel size.
- Fix direction: use 24 for every non-thumbnail content symbol. Better, publish the symbol size from shared Rust
  next to `content_icon` so both hosts read it.

##### LAYER-12: Adjustment layers with `has_thumbnail` show a thumbnail and badge; GTK shows the large icon only
- Class: NULL-STATE, LAYOUT   Confidence: CHECK   Severity: low   Reported bug: none
- Windows: `LayerRow.cpp:308`, `preview=flag(layer,L"has_thumbnail")`.
- GTK reference: `layers.rs:1690`, `let thumbnail = s.has_thumbnail && !s.adjustment_effect;`.
- Fix direction: add `&&!adjustment`. Better, have shared Rust publish one `show_thumbnail` flag so the two hosts do
  not each derive it.

##### LAYER-13: The rename TextBox uses the default WinUI template
- Class: DEFAULT-CONTROL   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `LayerRow.cpp:189-190`. Only `Background(input)`, height, padding and margin are set. The default
  template's border, focus underline (accent bottom border), focused-background resource (which replaces `Background`
  on focus), 4-px corners and foreground stay in place.
- GTK reference: `layers.rs:580-587` with `has_frame(false)`, `.layer-name-entry { padding: 0 2px; min-height: control-height }`
  (style.css:219) and `.dock-panel entry { background: var(--input-bg); border-radius: var(--control-radius) }`
  (style.css:185, 277). No frame and no underline.
- Fix direction: route it through the shared Windows text-field styling helper used elsewhere for panel entries
  (zero border, `controlCorner()`, focused/pointer-over background resources set to `input`, foreground `text`).

##### LAYER-14: The "Add paper" row has button hover and press; GTK's paper row has none
- Class: HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `LayersView.cpp:109-115`. `addPaper=button(...)`, with feedback overlay across the full 40-px row.
- GTK reference: `layers.rs:140-158`. A `.layer-row` Box with a click gesture and no hover or active style.
- Fix direction: use `buttonFeedback(addPaper,…,fixed=true)`, or a non-button row with a Tapped handler, matching LAYER-1.

##### LAYER-15: The layer edge inset is hard-coded 6 in Windows; GTK reads the shared `LAYER_EDGE_INSET`
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `LayersView.h:19` `LayerRowPadding{6,2,6,2}`, `LayersView.cpp:47` `header.Padding({6,4,6,4})`, `:177`
  `footerFrame.Padding({6,4,6,4})`, `LayerRow.cpp:236` `dropMark.Margin({-6,-2,-6,-2})`, and `LayerInspector.h:31`
  `footer.Padding({6,0,6,0})`.
- GTK reference: `--layer-edge-inset` from `layer_ui::LAYER_EDGE_INSET` (lib.rs:72-85, style.css:195, 204, 476).
- Shared source: `crates/layer-ui/src/layout.rs:35`. It is not part of the `panel_metrics` catalog.
- Fix direction: add `layer_edge_inset` to `PanelControlMetrics`/catalog and read it through `panelMetric`. Derive the
  `dropMark` margin from the same row padding.

##### LAYER-16: Literal 49 for the disclosure footer and literal 40 for the fallback row height
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `WorkspaceBubbles.cpp:265` `inspector.place(...,49,open)`, `LayerInspector.h:28` `footerRow.Height({49,…})`,
  `WorkspaceDrawers.cpp:360` `num(inspector->sizes,L"footer_height",49.)`, and `LayerInspector.h:75`
  `unit_height` fallback `N(40)`. Also `footer.CornerRadius({0,0,6,6})` at LayerInspector.h:31.
- GTK reference: `workspace_bubbles.rs:744-745`, `row.set_height_request(PropertyDisclosure::HEIGHT)` (48) plus a
  `gtk::Separator`. The fallback is `content_measurement` → `layer_ui::TILE_SIZE` (36) (layers.rs:1108).
- Shared source: `crates/layer-ui/src/effects.rs:408` `PropertyDisclosure::HEIGHT = 48`.
- Fix direction: publish the disclosure height (and separator) and the fallback unit from Rust, and use them in all
  three places.

##### LAYER-17: The layers bubble's alpha-lock icon position (user bug 17)
- Class: ALIGNMENT   Confidence: CHECK   Severity: medium   Reported bug: 17
- Windows: in simple (bubble) mode `full=false`, so column 11 is `12` and column 12 is `0` (LayerRow.cpp:356-358). The
  lock is a 12×12 left-aligned Image (:211), after the name button's right margin of 2 (:182), then the 6-px row
  padding.
- GTK reference: `layers.rs:606-613` and `:1757`. The lock is a 12-px image after 2-px Box spacing. The grip is hidden
  when `simple`, so the lock is followed only by the 6-px padding.
- Finding: the column arithmetic gives the same x position on both hosts, so the code does not prove an offset. Two
  things to check against a GTK screenshot of the same bubble: (a) whether `icon(...,12)` adds asset padding that GTK's
  `pixel_size 12` does not; (b) whether the bubble passes `full=true` (`flag(p->data,L"full")`, WorkspaceBubbles.cpp:293)
  when GTK uses `simple`. With `full=true`, column 11 becomes `14` when `can_drop_below` is set, and the 16-px grip
  column is added, which moves the lock left, not right. If the measured offset is to the right, check whether the
  row `root` is wider than the card (`layers.Width(width)` in LayerInspector.h:69 has no edge inset, while GTK's
  card has `.dock-panel` padding).
- Fix direction: measure one bubble row on both hosts. If the card width differs, align `LayerInspector::place`
  with GTK's card insets rather than adjusting the lock column.

##### LAYER-18: The layers drawer default width (user bug 18)
- Class: LAYOUT-COLLAPSE   Confidence: CHECK   Severity: medium   Reported bug: 18
- Windows: the drawer width is not decided by the host. `WorkspaceDrawers.cpp:239-309` sizes each column to
  `placement.columns[i].width` from Rust. `LayerInspector::place` fills that width.
- Shared source: `crates/layer-ui/src/drawers.rs:468-485` `column_widths` → `Panel::default_width`
  (`crates/layer-ui/src/layout.rs:716-722`). That is `PANEL_MIN_WIDTH` (242) plus `TILE_SIZE+TILE_GAP` only when
  `viewport_width >= 1600` (giving 280). The breakpoint is in DIPs, so a 1920-px display at 125 % (1536 DIP) gets 242,
  while GTK at scale 1 on the same panel gets 280. `LAYERS_MIN_WIDTH` (226) is applied to docked columns
  (layout.rs:2643) but not to drawers.
- Fix direction: confirm that the viewport Windows sends for drawer layout is in DIPs and equals GTK's for the same
  window size. If the layers drawer should not depend on the 1600 breakpoint, change `default_width` for
  `Panel::Layers` in shared Rust (one place, both hosts), with a drawers.rs test. No host change is needed.

##### LAYER-19: Double-tapping in the layer list's empty space can close and reopen the drawer (user bug 19)
- Class: HIT-TEST   Confidence: CHECK   Severity: medium   Reported bug: 19
- Windows evidence:
  - The list's empty space is not hit-testable. `panelScrolling` sets `scroll.Background(nullptr)` and the
    ScrollPresenter background to null (PanelScroll.h:12). `repeater`, `listFrame` and the `LayersView` root Grid have
    no Background (LayersView.cpp:27, 106-120). In the drawer, `LayerInspector::pages` is a Canvas with no
    background (LayerInspector.h:10, 29). A tap below the last row therefore lands on whatever is under the
    drawer content, and is not handled by the layer panel.
  - `WorkspaceGestures` sees every press as a chrome `contact` (`canvas:false`, WorkspaceGestures.cpp:524) and every
    `DoubleTapped` with `handledEventsToo=true` (:742). `Drawer::appendBounds` skips the `tool` drawer
    (WorkspaceDrawers.cpp:472), so the outside-contact test for that drawer relies only on Rust's placement, not on
    measured native bounds.
- GTK reference: the `ListView` is a hit target over its whole viewport. Row clicks are claimed per row
  (layers.rs:687-722), rename is limited to the name label (:723-736), and a card claims contacts outside rows
  (workspace_bubbles.rs:797-799).
- Fix direction: reproduce on Windows with `CAPY_TRACE_UI` and log the chrome contact and drawer facts for each tap.
  The likely fix is to give `listFrame` (or `LayersView::root`) a `clear()` background so empty list space belongs to
  the panel, and to publish the tool drawer's measured bounds like the other drawers, so Rust's
  outside-contact test uses the native frame.

##### LAYER-20: The thumbnail selection ring animates over 200 ms on Windows; GTK has no transition in its CSS
- Class: ANIMATION-RETRIGGER   Confidence: CHECK   Severity: low   Reported bug: none
- Windows: `LayerRow.cpp:37-59`. `ThumbnailEdge::update` runs a 200-ms scale and opacity storyboard each time
  `content_selected` or `mask_selected` changes. It only runs on a real change (`selected==editing` returns early),
  so it is not replayed on refresh.
- GTK reference: `.layer-thumbnail.layer-editing` (style.css:209) has no explicit transition. Any animation comes
  from libadwaita's default button transitions.
- Fix direction: confirm visually against GTK. Keep the animation only if GTK shows an equivalent transition.

##### LAYER-21: The opacity popup is 240 wide in a default Flyout; GTK uses 220 plus 12-px margins
- Class: METRIC, DEFAULT-CONTROL   Confidence: CHECK   Severity: low   Reported bug: none
- Windows: `LayersView.cpp:238`, `content.Width(240)`, `Flyout()` with the default FlyoutPresenter padding and
  min-width (TrackPopup styling not verified).
- GTK reference: `layers.rs:427-433`, `opacity_editor.set_size_request(220,-1)` with 12-px margins on all sides in a
  `gtk::Popover`.
- Fix direction: set the presenter padding to 12 and the content width to 220 (or publish both from shared Rust).

##### LAYER-22: The swipe reveal width (72) and threshold (.4) are literals on both hosts
- Class: METRIC   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: `LayerRow.cpp:240, 244, 254, 256` and `LayerRowDrag.cpp:352-353` use `72` and `72*.4`.
- GTK reference: `swipe_row.rs:6` `const REVEAL: f64 = 72.`, and `:126-127` `REVEAL * 0.4`.
- Fix direction: publish the reveal width and threshold with `swipe_actions` from shared Rust. The values match
  today; this only prevents them drifting apart.

#### Root causes / systemic patterns

1. **`button()` used for things that are not buttons** (LAYER-1, 8, 14, and the name focus rectangle). The
   `CapyUi::button` helper always adds `buttonFeedback` hover and press plus focus visuals. In the layer row it is
   used for the name, the grip and the paper row, which have no hover or press in GTK. Fix: add a non-control
   "tappable surface" helper (transparent hit-testable Border with Tapped/DoubleTapped) and use it wherever GTK uses
   a label, image or gesture on a Box.
2. **Selection fill painted on children as well as the row** (LAYER-2, 7). `selected(data)` is assigned to child
   controls (`check`, the header toggles) while the row or panel already carries it. Fix: the selection fill belongs only
   on the row (`body`). Toggles use one neutral checked tint inside `buttonColors<ToggleButton>`.
3. **Drop targets limited to the list or row bounds** (LAYER-3, 4). `LayerRowDrag::pick()` and
   `WorkspaceRowDrag::target()` both reject positions outside the scroller or list surface, and GTK uses per-row targets
   only. Fix: one shared Rust list-edge rule (before-first / after-last), with both hosts testing against the panel
   root and clamping to the first/last realized row. This fixes layers, the workspace manager and drawing tabs together.
4. **Drag presentation** (LAYER-5, 6). There is no shared preview or marker helper, and WorkspaceRowDrag uses
   `#3584e4` and an 8-px inset. Fix: one `RowDragPresentation` (accent role, full-width inset line, pointer-following
   snapshot at .7 over panel fill) used by both drag classes.
5. **Literals where GTK uses shared metrics** (LAYER-15, 16, 22, plus the 20-px line heights in LAYER-10). Fix: add
   `layer_edge_inset`, the disclosure height and the swipe reveal to the catalog and read them through `panelMetric`.
6. **Hit-transparent panel surfaces** (LAYER-19). `panelScrolling` nulls scroller backgrounds, and the layer panel
   containers have none, so empty list space does not belong to the panel. Fix: give the panel root a `clear()`
   background once (in `LayersPanel` or `panelScrolling`'s content owner), and publish native bounds for the tool
   drawer.

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

##### TOOL-1: Toolbar slider track vanishes after any slider rebuild (user bug 20)
- Class: ANIMATION-RETRIGGER, NULL-STATE   Confidence: CERTAIN (code) / LIKELY as the visible symptom   Severity: high   Reported bug: 20
- Windows: `ToolbarComponents.cpp:195-201` — `Update()` rebuilds the whole slider (`buildSlider`) whenever `schemaKey(component)`
  changes. `schemaKey` (`:164-191`) keeps `context` (generation, document, tool, brush, layer, mask — `layer-ui/src/toolbar_components.rs:62-70`)
  and the presence of `numeric`, so the slider is rebuilt on every tool/brush/layer/document change and when the document first
  becomes ready (numeric goes from absent to present). `buildSlider` (`:301-304`) creates a **new** `trackImage` and `marks`
  canvas, but `trackKey`/`marksKey` (`:91`) are never reset. `paintTrack` (`:411-412`) then sees an unchanged key
  (`w x h @ scale, axis, ink`) and never sets `Source` on the new image; `paintSlider` (`:420-421`) returns before drawing
  bookmarks. Result: after the first context change (or after the document arrives, in Sketch at startup) Size/Strength
  render with no track and no bookmarks; only the thumb (and readout if enabled) remains.
- GTK reference: `toolbar_components.rs:620-789` builds the `gtk::Scale` once; `refresh` (`:791-830`) only updates value,
  readout and bookmarks and calls `queue_draw`; `paint_track` (`:1505-1566`) repaints every snapshot. A context change only
  closes the preview (`:799-801`).
- Shared source: `ToolbarComponentView` (`layer-ui/src/toolbar_components.rs:75-81`).
- Fix direction: do not rebuild the standalone slider on schema/context change — build once in `init`, and in `Update` only
  call `updateSlider()` (close the popup when `context` changes, like GTK). If a rebuild is kept anywhere, clear
  `trackKey`/`marksKey` inside `buildSlider`. Add a windowless test that a context change keeps a non-null track source.

##### TOOL-3: Brush Sizes panel (and tool-group grids) report zero/incorrect content height (user bug 5)
- Class: LAYOUT-COLLAPSE   Confidence: LIKELY   Severity: high   Reported bug: 5
- Windows: `ToolView.cpp:470-501` (`BrushSizePanel`) — the preset grid is a `Canvas` whose `Height` is assigned only inside its
  `SizeChanged` handler (`:491-500`). A Canvas with no explicit `Height` measures to 0, so any measurement taken before the
  first arrange — `PanelBody::contentHeight` (`PanelBody.cpp:157`, `content.Measure({width,INF})`) and the drawer's
  `measureHeights` (`WorkspaceDrawers.cpp:389`) — returns 0 for the grid. Dropping the panel out creates a new `PanelBody`
  (new Canvas, no Height yet) whose first measurement is reported to shared layout, so the floating group is sized to
  zero content height and nothing re-reports it.
- Same pattern: `ToolView.cpp:58-76` (`ToolSetView::arrange`) — the tool-group `Canvas` is laid out in `SizeChanged`; before the
  first arrange `ActualWidth()==0` → `count=1`, non-media `itemWidth=min(112,0)=0` and `Height=rows*36+gaps` (all groups on
  separate rows). The first measured height is too tall and the tiles are zero-width until a later arrange.
  `TextureDialog.cpp:156,211-224` uses the same SizeChanged-driven column assignment (hidden by the fixed 600 px dialog height).
- GTK reference: `tool_panels.rs:636-641` `adw::WrapBox` and `:35-42` `gtk::FlowBox` report height-for-width in `measure`, so the
  first measurement is correct.
- Fix direction: one shared wrap helper that computes columns/rows from the width it is *measured* with (a `Panel` subclass
  with `MeasureOverride`/`ArrangeOverride`, or an `ItemsRepeater` + `UniformGridLayout`), used by the size presets, tool groups
  and texture grid. Delete the three `SizeChanged` arrangers. Add a test that measures `BrushSizePanel` at a fixed width
  before arrange and gets `rows*tile+gaps`.

---

#### Toolbar components (ToolbarComponents.cpp)

##### TOOL-4: Toolbar options rebuild every field when any one field's schema changes
- Class: ANIMATION-RETRIGGER   Confidence: CERTAIN   Severity: medium   Reported bug: none (same class as 1)
- Windows: `ToolbarComponents.cpp:197-210` — if the concatenated schema of all options differs, every field is disposed and
  rebuilt and `closePopup()` dismisses open editors/menus.
- GTK reference: `toolbar_components.rs:832-861` — within one context, fields whose `same_schema` matches are retained and
  only new ones are built; docs `toolbar-components.md:102-104` ("adding or removing fields retains compatible editors and open lists").
- Fix direction: key each field by its own `schemaKey(option)`; reuse matching fields in order when `context` is unchanged.

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

(Main issue: TOOL-3.) Tile size, dot gradient (0.4/0.65/1.0 stops), label `margin-bottom 2`, normal weight and radius
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

1. **Rebuild-on-update with stale caches** (TOOL-1, 2, 4, 17, 25): views are destroyed and rebuilt whenever a serialized
   key changes, and caches that guard painting/measuring (`trackKey`, `marksKey`, drawer `heights`) outlive the rebuilt
   elements or are cleared too eagerly. Fixing `ToolbarComponent::Update` (never rebuild the slider; retain compatible
   option fields) and `Drawer::apply` (keep heights, animate from presented bounds) closes both user bugs 1 and 20 and the
   general "animation replays" class.
2. **Layout computed in `SizeChanged` instead of measure** (TOOL-3, 12, 16; TextureGrid): a shared wrap-panel helper with
   `MeasureOverride` (width → rows) replaces three hand-rolled Canvas arrangers and fixes user bug 5.
3. **Hand-entered metrics instead of published ones** (TOOL-8, 10, 12, 13, 14, 15, 20, 24): `panel_metrics`,
   `tool_preview_metrics`, `TileStyle` size/radius and `BRUSH_SETS_MIN_WIDTH` are already in the catalog/`style` query;
   reading them removes most literal 36/112/40/34/62/6/18 values.

### FX area audit: Windows vs GTK (filters, properties, curves, gradients, scopes, stats, proof, navigator, palettes)

Shared metrics (crates/layer-ui/src/layout.rs:52): inset 8, control_spacing 4, group_spacing 6, compound 2,
control_height 26, field_padding 6, icon 16, control_radius 13 (x corner-fit .54, about 7px).

---

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

##### FX-2: Filter Types "Cancel" is drawn as bare text with no button face or padding; row gap also differs
- Class: DEFAULT-CONTROL, BORDER/COLOR, METRIC   Confidence: CERTAIN   Severity: medium   Reported bug: 21 (same control)
- Windows: `FilterView.cpp:187-188`. `button()` (UiControls.h:467-472) gives `Background(clear())` and `Padding(0)`, and only `MinHeight` is added. `root.RowSpacing(control_spacing=4)` (`:180`).
- GTK reference: `effects.rs:146-149` uses a plain non-flat `gtk::Button`. `style.css:185-186` `.dock-panel button` gives min-height 26, `padding: 0 var(--panel-field-padding)` (6px) and control radius, with the Adwaita raised fill. `filter_types` Box spacing is 6 (`effects.rs:140`).
- Fix direction: give the Cancel button `Padding({field_padding,0,field_padding,0})` and the shared raised-button fill (see FX-17). Change RowSpacing to `group_spacing`.

##### FX-3: Navigator in a drawer is fixed at 272px whatever the width
- Class: LAYOUT-COLLAPSE, METRIC   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `WorkspaceDrawers.cpp:348-349` sets `body.view->Root().Height(body.view->navigator?272.:480.)`. This overrides the `NaturalHeight(width)` that PanelBody sets (`PanelBody.cpp:225`). NavigatorLayout clamps the image to `available-fixedHeight` (`NavigatorView.cpp:71-74`), so a wide drawer shows a letterboxed overview, and the buttons are not at the bottom when the column is taller.
- GTK reference: `navigator.rs:300-321` uses a natural vexpand Box with no fixed height in drawers.
- Fix direction: use `navigator->NaturalHeight(width)`, or the placement height as in FX-1.

##### FX-4: Clicking Blend Mode in the filter drawer's Properties (no filter selected): property dropdowns never register as popups
- Class: NULL-STATE, HIT-TEST, DEFAULT-CONTROL   Confidence: LIKELY for the mechanism, CHECK for the exact crash frame   Severity: high   Reported bug: 22
- Context: the filters drawer is `[FilterTypes],[Adjustments],[Properties]` (`crates/layer-ui/src/drawers.rs:382`). With no filter (effect layer) selected, `effects.rs:727-733` (shared) publishes `opacity` (number), `blend` (Choice of up to 25 options, value = `LayerBlend::code()`), and `color_mode`. Windows builds the blend control at `EffectView.cpp:173-181`.
- Windows:
  - `EffectView.cpp:174-179` (property choices), `:285-289` (`styleChoice` for page/resource) and `FilterView.cpp:56` (category) style ComboBoxes with `choiceColors` but never call `data->popup(true/false)` on DropDownOpened, DropDownClosed or Unloaded. Every other panel dropdown does: `GradientView.cpp:110-113`, `ScopesView.cpp:166-169`, `ToolView.cpp:167,228`, `PanelConfiguration.cpp:51`. Without the popup count, `popup_open` stays false in the chrome facts (`WorkspaceGestures.cpp:149`, `CanvasWindow.cpp:1684-1689`). Rust then treats the next contact as an outside contact and closes the drawer (`crates/layer-ui/src/session.rs:1366-1414`, `CloseExpanded` gated on `!facts.popup_open`). The 25-row blend list is the tallest dropdown in the drawer and the only one that reliably extends past the drawer bounds and over the canvas. Closing the drawer removes the column frame (`WorkspaceDrawers.cpp:55`, `:318-321`) while the ComboBox popup is still open and its `SelectionChanged` → `Property::set` → `dispatchDocument` is still pending. That is the most likely crash path. A drawer filter with a choice parameter normally has short lists that fit inside the drawer, which is consistent with the crash being tied to Blend Mode.
  - A second candidate in the same binding is `EffectView.cpp:180`: `choices.SelectedIndex(int(num(value)))` with no range check against `Items().Size()`. WinUI throws E_INVALIDARG for an out-of-range index, and an exception thrown out of a binding is fatal. The option list is filtered per layer (`effects.rs:727-731` drops PassThrough for non-groups) while the value is a raw code. Today the codes line up because PassThrough is last in `LayerBlend::ALL` (layer-core/src/layers.rs:245-270), but nothing guards the invariant.
  - Rust `LayerAction::Blend` (`art_layers.rs:1195-1207`) only returns errors and has no panic path. `Property::action` (`EffectControls.h:258-265`) guards `updating`, `current()` and `enabled`.
- GTK reference: `effects.rs:900-911` uses a native `gtk::DropDown`, whose own popover grab keeps the drawer open.
- Fix direction: move popup bookkeeping into `CapyUi::choiceColors` (`ChoiceFeedback.cpp:65`), which every styled ComboBox already calls, and delete the four copies in Gradient/Scopes/Tool/PanelConfiguration. Clamp every model-driven `SelectedIndex` (helper: `selectIndex(box,int)` that writes -1 when out of range). Repro to confirm: open the filters drawer on a paint layer, open Blend Mode, pick an item that sits over the canvas, and capture the stack with WinDbg (`!analyze`).

##### FX-5: Other unguarded `ComboBox::SelectedIndex` writes from model values (same failure class as #22)
- Class: NULL-STATE   Confidence: LIKELY   Severity: medium   Reported bug: 22 (class)
- Windows:
  - `ScopesView.cpp:172-176` (`syncChoices`) writes `box.SelectedIndex(int(num(current,"source")))` even when `sources`/`channels` are empty (default view before the first publish gives index 0 with 0 items).
  - `EffectView.cpp:180` (see FX-4).
  - `ProofForm.h:277-283`: `bpc.IsEnabled(flag(intents.GetObjectAt(intent.SelectedIndex()),...))` is called with SelectedIndex -1 when the draft intent is not in the list, which gives GetObjectAt(0xFFFFFFFF) and throws inside a binding. `current()` (`:226-235`) has the same unguarded indexing, but its caller wraps it in try/catch (`ProofPanel.h:90-92`).
- GTK reference: GTK uses `unwrap_or(0)` (`files/proof.rs:446-451`) and `set_selected(INVALID_LIST_POSITION)` for none (`effects.rs:778`).
- Fix direction: one guarded `selectIndex` helper in UiControls.h, used by all of these.

##### FX-6: ResponseCurveField dereferences a null `property` if coordinate readouts are ever enabled
- Class: NULL-STATE   Confidence: CERTAIN (latent)   Severity: low   Reported bug: 22 (class)
- Windows: `CurveView.cpp:110,113-114,118`. `coordinate()` uses `property->id()` and `property->view()`, but `ResponseCurveField` (`:270-272`) constructs the editor with `property==nullptr`. It is safe today only because `response_calibration.rs:81` sets `coordinate_readouts:false`.
- Fix direction: derive the id the same way as `:129` (`property?property->id():L"pen-"+responseTarget`), and use `enabled = property?flag(property->view(),"enabled"):true`.

##### FX-7: Filter preview timer throws if `document_file.epoch` is absent
- Class: NULL-STATE   Confidence: LIKELY   Severity: low
- Windows: `FilterView.cpp:29-30` uses `file.GetNamedValue(L"epoch")` with no default, called from a 200ms timer tick. JsonObject throws on a missing key. Everywhere else the code uses `GetNamedValue(L"epoch",Null)`.
- Fix direction: `GetNamedValue(L"epoch",JsonValue::CreateNullValue())`.

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

##### FX-38: Dropdown popup items are not themed
- Class: DEFAULT-CONTROL   Confidence: CHECK   Severity: low   Reported bug: 11 (class)
- Windows: `ChoiceFeedback.cpp:44-48` overrides only ComboBox host resources (Background*, Foreground*, BorderBrush*). The `ComboBoxItem*` and `ComboBoxDropDown*` popup resources are not overridden, so every property/category/scope dropdown list uses the WinUI default template colours, and can mismatch the app theme or be hard to see.
- Fix direction: extend `choiceColors` with ComboBoxDropDownBackground, ComboBoxItemForeground, ComboBoxItemBackgroundPointerOver and the related keys from the panel/text/selection roles.

---

#### Root causes / systemic patterns

1. **Fixed drawer body heights** (`PanelBody.cpp:222-224`, `WorkspaceDrawers.cpp:348-353`): exact `Height(440/480/272)` instead of filling the column's placement height with a minimum. This closes FX-1 (bug 21), FX-3 and the adjustments 480-vs-440 mismatch. Replace it with one fill-height path in `placeColumns`.
2. **`choiceColors` is incomplete** (`ChoiceFeedback.cpp:65`): it styles the host but not popup bookkeeping, item colours or index safety. Moving `DropDownOpened/Closed/Unloaded → data->popup()` in, adding item resources, and adding a guarded `selectIndex` helper closes FX-4 (bug 22 candidate), FX-5, FX-24's popup part and FX-38, and lets four duplicated popup blocks be deleted.
3. **No shared "raised button" or "text field" styling**: `button()` defaults to a clear, zero-padding face, and each site improvises (input, control_fill or nothing). TextBoxes leak WinUI focus visuals. A `raisedButton()` helper and a `textFieldColors()` helper close FX-2, FX-13, FX-16 and FX-36.
4. **Corner, metric and spacing literals in place of panel metrics**: 6px palette radii, 56/36/44/18/12 sizes, RowSpacing 0/10, and `.85` axis text. Using `controlCorner()` and `panelMetric()` everywhere closes FX-8 to FX-11, FX-15, FX-17, FX-20, FX-23, FX-29, FX-35 and FX-37.
5. **Opaque roles where GTK uses glass-aware classes**: `brush("selection")` and `brush("bg")` where `selected(data)` or the panel surface role should be used (FX-32, FX-28).
6. **Unchecked JSON indexing in bindings** (`GetNamedValue` without default, `GetObjectAt(SelectedIndex())`, null `property` in the shared CurveEditor): FX-5, FX-6, FX-7. Each one throws out of an event handler or binding, which ends the process.

### Moved from the shell audit

##### SHELL-8: Brush size panel measures 0 content height, so a dragged-out Sizes panel drops at 0 body height (bug 5)
- Class: LAYOUT-COLLAPSE   Confidence: LIKELY (strong code evidence)   Severity: high   Reported bug: 5
- Windows:
  - `ToolView.cpp:470-500`: `BrushSizePanel` is a `Canvas`. Its `Height` is set only in `grid.SizeChanged`.
  - A Canvas reports `DesiredSize.Height=0` from `Measure`. `PanelBody` measures with `content.Measure({width,INF})` (`PanelBody.cpp:157`). The offscreen copy is measured synchronously with Measure/Arrange (`WorkspaceView.cpp:906-910`) before any SizeChanged fires.
  - So `content_height` is about `2*inset`. Shared `settle_floating_drop` (`crates/layer-ui/src/layout.rs:3711-3721`) uses `content_height + chrome` as the natural height, which gives a body of almost 0.
  - The same panel is used in `PanelConfiguration.cpp:101`.
- Other panel kinds in the same class:
  - Any body whose height comes from `SizeChanged`, `ActualWidth` or explicit `Height` literals: `PanelBody.cpp:225-227` (440/480/navigator) for non-scrollable bodies.
  - Drawer `placeColumns` (`WorkspaceDrawers.cpp:348-353`): layers/adjustments 480, navigator 272, others `ContentHeight`.
  - Tile panels deliberately report 0 (`WorkspaceView.cpp:890`) and rely on the shared toolbar path.
- GTK reference: `crate::tool_panels::SizePanel` is a measurable GTK container. `measure()` returns its wrapped rows for the given width.
- Fix direction: make `BrushSizePanel` measure for any width. Compute `rows*height+(rows-1)*gap` from the width passed to `contentHeight`, using the same column formula, or override `MeasureOverride` with a custom panel. Add a regression test: measure an offscreen Sizes body at a width that has never been arranged, and expect a height greater than the inset.
