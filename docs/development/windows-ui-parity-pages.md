# Windows UI parity: pages and chrome

[Windows development](windows.md)

Bring the Windows Preferences, shortcut page, workspace manager and dialogs,
title bar, drawing tabs, canvas bar, notices, zoom readout, document dialogs
and New drawing, Size and Export dialogs to visual parity with GTK. The plan
also fixes reported bugs 9, 10, 12 and 14, the page half of 11, and verifies
2 and 13 after Agent A's `label()` fix.

## Prompt

> You are Agent C for the Windows UI parity work in Capy Canvas. Read
> `AGENTS.md`, `apps/layer-windows/AGENTS.md` and
> `docs/development/windows-ui-parity-pages.md`. Implement that plan on
> Windows. Agents A and B are working `windows-ui-parity-shell.md` and
> `windows-ui-parity-panels.md` in parallel on other machines. Agent A owns the
> shared helpers (`UiControls.h`, `ChoiceFeedback`, `ButtonFeedback`,
> `NumberControl`, `PickerDialog.h`, `NativeMenus.h`); do not edit them. Start
> with the independent items, then fetch `origin/main` and adopt A's helpers
> when they land. Edit only the files your plan owns. Work in the batched
> milestones of the order of work: one commit and one VM test round each. Fix
> root causes, add regression checks, push each milestone to `origin/main` as
> soon as its checks pass, and remove each finding from the plan in the commit
> that fixes it.
> Reproduce CHECK findings before fixing them, or drop them. Finish with a list
> of what you fixed, what you dropped and what you could not run.

## Files this plan owns

`SettingsView.cpp/.h`, `ShortcutPage.cpp/.h`, `WorkspaceManagerView.cpp/.h`,
`WorkspaceStorageView.cpp/.h`, `WorkspaceDialogs.cpp/.h`, `NavigationCursors.h`,
`HeaderView.cpp/.h`, `HeaderInput.cpp/.h`, `HeaderStatus.h`, `CanvasActionBar.h`,
`DrawingTabs.h`, `ZoomReadout.h`, `CanvasNotice.h`, the chrome parts of
`CanvasWindow.cpp/.h`, `DocumentView.cpp/.h`, `NewDrawingDialog.cpp/.h`,
`SizeDialog.cpp/.h`, `ExportForm.h`, `ExternalImages.h` (all under
`apps/layer-windows/`), plus export-form rules moved into shared Rust
(HEAD-DLG-35) and any localized strings you add (update every registered Fluent
catalog and run the localization checks).

## Order of work

Each milestone is one commit, one VM build and one
test round, pushed to `origin/main` when its checks pass. Fix every listed
finding in the milestone's files before building, so the VM builds and captures
once per milestone rather than once per finding. Write the next milestone's
code while the VM runs the current one, but commit only tested code.

Findings marked *(A)* need Agent A's helpers. Leave them in the plan until M4.
If you need a value that A's helper will own (button fill, field fill, popup
style), do not create a local copy in your files; wait for M4.
`PickerDialog.h` and `NumberControl.cpp` belong to Agent A: put New-drawing-only
constants in `NewDrawingDialog.cpp`, and fix detached number entries in
`numberField`, not in NumberControl.

**M4. Adopt Agent A's helpers** once P1–P8 are on `origin/main` (find them with
`git log origin/main --grep='Windows UI parity'`). Move every *(A)* item above
onto the helpers in one commit: bugs 2 and 13 (HEAD-1, PREF-7, with a zoomed
capture; remove per-site centring offsets such as `alignNames`' shift maths),
bug 11 and the prompt dropdown (PREF-1, 2), PREF-3 face, PREF-4, 8, 11, 16,
28 face, 29, HEAD-2, 3, 8 opacity, 11 size row, 21 parent radius, 22, 26, 27, HEAD-DLG-1, 2, 4, 5, 8, 9, 10,
11, 16, 28, 29, 32, 33. Then recheck every page of this plan, because P1
changes every caption. If P1–P8 are not on `origin/main` when M3 is pushed,
stop and report the M4 items as blocked on Agent A.

## Validation

Use the `capycanvas1` VM only (`tools/windows-vm/windows-vm.py`, which defaults
to this worktree's VM). Keep it warm between milestones and stop it when you
finish.

- Every milestone: `tools/windows-vm/windows-vm.py check` (runs
  `test-without-gpu.ps1` and the Windows Rust tests) and strict Clippy over SSH;
  then `fixtures` with only the milestone's fixtures below. Use
  `--profile dev-perf` for repeated rounds; `--no-build` when only fixture
  scripts changed.
- Shared Rust (M3): `cargo test --locked -p layer-host -p layer-ui -p layer-workspace --lib`
  and the localization checks on Linux.
- Fixtures per milestone (they live in `apps/layer-windows/scripts/exercise-*.ps1`):
  - Preferences and workspaces: `header-settings`, `settings-storage`, `shortcut-page`, `shortcuts`,
    `manager`, `manager-focus`.
  - M2: `switcher`, `switcher-drag`, `header`, `header-placement`,
    `canvas-bar`, `notice`, `zoom-readout`, `window`, `manager-focus`.
  - M3: `documents`, `size-dialogs`, and `localization:default` for the new text.
  - M4: all of the above, once, plus the full `localization` matrix if M4
    changes text.
- Captures, light and dark, glass on and off, of what the milestone changed:
  every Preferences page incl. colour management and About, shortcut page
  filters, manage workspaces list hover and selection (M1); title bar
  customization, drawing tabs with close hover, canvas bar segments, notice,
  zoom and screen popups (M2); New drawing, Canvas size, Image size and Export
  dialogs (M3). Batch all of a milestone's captures into one scripted run per
  theme. Measure the captures against the GTK values quoted in each finding.
  The software adapter is fine for layout, metrics and colours; use a pool GPU
  (`start --gpu`, one card at most) only if a glass capture needs it.

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
- Batch findings by file area, as this plan's order of work lists them. Push
  each milestone after its checks pass; fetch and rebase onto `origin/main`
  before every push.
- Remove a finding from your plan in the commit that fixes it. Record dropped
  CHECK items (not reproduced) in the commit message. Delete your plan in the
  commit that closes its last item.
- For every visual fix, compare light and dark themes, with glass transparency
  on and off, against the GTK values quoted in the finding. Add assertions to
  the existing `exercise-*.ps1` fixture for the area instead of new harnesses.
- Report what you could not run.

## Findings

### PREF audit: Preferences, shortcuts, workspace manager and dialogs (Windows vs GTK)

Scope read in full: `apps/layer-windows/SettingsView.cpp`, `ShortcutPage.cpp`, `WorkspaceManagerView.cpp`,
`WorkspaceStorageView.cpp`, `WorkspaceDialogs.cpp`, `NavigationCursors.h` (it only holds two cursor resource IDs, so there is nothing to
compare), and the helpers they use: `UiControls.h` (`button`, `label`, `icon`, `selected`, `dialogSurface`),
`ChoiceFeedback.cpp` (`choiceColors`, dialog footer) and `ButtonFeedback.h`. GTK reference: `preferences.rs`,
`shortcut_page.rs`, `workspace_manager.rs`, `workspace_manager_dialog.rs`, `workspace_customization.rs`,
`swatch_selector.rs`, `image_selector.rs`, `transparency_choice.rs` and `style.css`. Palette: `crates/layer-ui/src/theme.rs`.
The colour-management settings page is built in `SettingsView.cpp:207-214`. The Windows profile manager itself runs
through the DocumentView workflow, which is outside this scope.

Key palette fact (theme.rs:390-396): `input = surface([51;3],[250;3])` and `settings = surface([51;3],[250,250,251])`.
**`input` and `settings` are the same colour**, dark and light. `card = surface([65;3],[255;3])`.
`button` is `#ffffff` in dark and `#000000` in light. It is meant to be used through `tint("button",13)`, never as an opaque brush.

---

##### PREF-1: Shortcut filter dropdowns use the input fill, which equals the settings background (bug 11)
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN   Severity: high   Reported bug: 11
- Windows: `ShortcutPage.cpp:194-195` `dropdown()` calls `choiceColors(data,result)`. `ChoiceFeedback.cpp:65-66` sets
  `control.Background(drawing?data->tint(L"text",18):data->brush(L"input"))` and `BorderThickness({0})`. The All tools
  (`contextChoice`, :253) and All actions (`showChoice`, :257) boxes sit straight on the page in the `filters` grid
  (:243-261), not inside a card. The page background is `brush("settings")` (`dialogSurface`, UiControls.h:419).
  `input`==`settings` (theme.rs), so the closed ComboBox has no visible fill or border. Only its text and chevron show.
- GTK reference: `shortcut_page.rs:378-384` uses a plain `gtk::DropDown`, so it gets the Adwaita default button
  (`alpha(currentColor,.1)` fill, 6px radius). `shortcut_page.rs:759-770` puts the filters in an `adw::PreferencesGroup`
  body with no card.
- Shared source: palette roles `input`, `settings`, `card`.
- Fix direction: make `choiceColors` pick its fill from the surface the control sits on, rather than always using
  `input`. On `settings` or dialog surfaces use the Adwaita button fill, `tint("text",26)` (the same value the Windows
  close and Cancel buttons already use). Keep `input` for cards and panels. One helper change also fixes PREF-2.

##### PREF-2: The workspace prompt's "choices" ComboBox is invisible on the dialog background (same class as bug 11)
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN   Severity: medium   Reported bug: none (class of 11)
- Windows: `WorkspaceManagerView.cpp:189` `choiceColors(copyData,choices)` puts an `input` fill on a ContentDialog
  whose background is `settings` (`dialogSurface`, :469). Those are the same colour.
- GTK reference: `workspace_manager_dialog.rs:728` uses `gtk::DropDown::from_strings`, which has the Adwaita default button fill.
- Fix direction: same helper fix as PREF-1.

##### PREF-3: Manage profiles still needs the raised button face *(A)*
- The profile-library row is after the colour groups, with its localized title, help and suffix action.
- Use Agent A's raised-button helper for the suffix action in M4.

##### PREF-4: Every action-row button and every sheet `flat()` button lacks the Adwaita button fill; `flat()` uses an opaque white/black brush
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN   Severity: high   Reported bug: class of 9
- Windows:
  - `SettingsView.cpp:262-265` `type=="action"` rows (for example Pen pressure **Adjust…**) use `button(data,label)`. That
    means a clear fill, `Padding 0`, `MinHeight 0`: bold text with no chrome and a hover rectangle that hugs the text.
  - `SettingsView.cpp:36-41` `actionButton()` adds padding but still has no fill. It is only used for sidebar search results.
  - `ShortcutPage.cpp:116-118` `flat()` sets `Background(data->brush(L"button"))`. `button` is `#ffffff` in dark and
    `#000000` in light (theme.rs:405), and the text is (250,250,251) dark or (46,46,50) light. **The text of Cancel (recording
    row :297), Reset (picker header :80), and keymap-import Cancel (:337) is invisible**, white on white or near-black on
    black. Confirm and Import override the background with the accent, so they look correct.
- GTK reference: `preferences.rs:890-897` and `shortcut_page.rs:409-421` use `gtk::Button` with default Adwaita styling
  (`alpha(currentColor,.1)` fill). Confirm adds `suggested-action`.
- Shared source: `buttonBackground()` (UiControls.h:408) is `tint("button",13)`. `WorkspaceManagerView.cpp:522` and the
  dialog footer (ChoiceFeedback.cpp:60) use `tint("text",26)` / `control_fill`.
- Fix direction: add one "default button" helper (fill `tint("text",26)` or palette `control_fill`, `controlCorner()`,
  `Padding{10,5,10,5}`, `MinHeight 34`). Use it for action rows, Manage profiles, `flat()` and `actionButton()`. Delete
  `flat()`'s `brush("button")`.

##### PREF-7: Systemic text-centring error in `label()` (bug 13 and "centering errors are everywhere")
- Class: VCENTER   Confidence: LIKELY (font metrics; needs a pixel check)   Severity: medium   Reported bug: 13 (and 2)
- Windows: `UiControls.h:424-429`. `label()` sets `FontFamily("Segoe UI")`, `FontSize(textSize)` (11pt, about 14.67px),
  `LineHeight(18)` and `LineStackingStrategy::MaxHeight`. MaxHeight means the line box is
  max(18, natural ≈ 19.5px). Segoe UI's ascent/descent (2210/514 per 2048 em) puts the baseline about 15.8px down a
  19.5px box. Cap centre ends up about 0.9px below the box centre and x-height centre about 2.4px below. Every row that
  centres a `label()` therefore renders its text slightly low; the About **Dedicated to / Nagu** row (`field()` Info
  branch, `SettingsView.cpp:420-421`, both columns `VerticalAlignment Center` in a `MinHeight 54`, `Padding 14,8,14,8` row)
  is an example. `WorkspaceManagerView.cpp:259-290` (`alignNames`) already measures baselines by hand to work around the
  same offset.
- GTK reference: Pango centres the label box with the theme font, and libadwaita `ActionRow` suffix labels are `valign center`.
- Fix direction: fix it once in `label()`. Use `TextLineBounds::Tight` (or `TrimToCapHeight` plus a measured descent
  inset) for single-line labels that are centred in fixed-height rows or buttons, so the glyph box itself is centred.
  Then delete per-site offsets such as `alignNames`' shift maths once they are no longer needed.

##### PREF-8: Default WinUI ToggleSwitch: unstyled colours and a reserved content column that pushes the switch away from the row's right edge
- Class: DEFAULT-CONTROL, ALIGNMENT   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `SettingsView.cpp:267` and `ShortcutPage.cpp:469` use `ToggleSwitch` with `MinWidth(0)`, empty On/Off content and no
  colour or template overrides. The WinUI template keeps its own `ToggleSwitchThemeMinWidth` (154) and the content-presenter
  column with pre/post content margins, so the 40×20 knob track sits well left of the card's 14px right inset. Track colours
  are system accent and outline, not the palette `accent` / `control_fill`.
- GTK reference: `adw::SwitchRow` (`preferences.rs:857-874`, `shortcut_page.rs:218`) puts the switch flush with the row's
  suffix edge, Adwaita switch fill `accent` on, `alpha(fg,.2)` off.
- Fix direction: add one `compactSwitch(data)` helper with a minimal template or resource overrides (palette accent,
  control_fill, no content column), and use it in both places.

##### PREF-11: Row hover and press strength is 8%/16%, compared with Adwaita's 3%/8% for list rows
- Class: HIGHLIGHT-GEOMETRY, METRIC   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `buttonColors` uses `hover=20,press=41` (UiControls.h:461-463 and ButtonFeedback.h:43) for every row button
  (sidebar tabs, `navRow`, `buttonRow`, picker rows, search results). `WorkspaceManagerView.cpp:164` uses `38` for create,
  remove, Cancel and Apply.
- GTK reference: preference list rows use libadwaita's `row.activatable:hover`/`:active` (about `alpha(currentColor,.03/.08)`).
  Custom buttons use `.08/.16` (`style.css:580-632`). `.workspace-list-actions button` is `flat` (Adwaita flat hover).
- Fix direction: give `buttonColors` a list-row preset and use it for row buttons. Drop the ad-hoc `38`.

##### PREF-16: Choice rows draw a filled ComboBox field; GTK ComboRow has no field box
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `SettingsView.cpp:331` and keymap `ShortcutPage.cpp:225`. The ComboBox uses an `input` fill (darker than `card`) and
  WinUI's default 32px height, `ComboBoxPadding 12,5,0,7` and chevron glyph.
- GTK reference: `adw::ComboRow` (`preferences.rs:726`, keymap combo in `shortcut_page.rs`) shows the selected value as a
  plain label with a pan-down arrow on an activatable row. There is no filled field, and the whole row hovers.
- Fix direction: decide on parity. Either style the closed ComboBox as a transparent suffix (no fill, symmetric padding,
  row-height hit area) or document the divergence. At minimum apply the padding fix from PREF-6.

##### PREF-28: Storage actions still need the raised button face *(A)*
- The notice uses the surround fill, 6px/12px padding, surface radius and inline actions with shared text metrics.
- Adopt Agent A's raised-button helper for Retry, Save as new and Backup in M4.

##### PREF-29: Text entries are default WinUI TextBoxes
- Class: DEFAULT-CONTROL   Confidence: CERTAIN   Severity: low   Reported bug: none
- Windows: preference search `SettingsView.cpp:150`, swatch hex entry `:355`, shortcut search `ShortcutPage.cpp:246-252`,
  picker search `:85`, workspace search/name/description `WorkspaceManagerView.cpp:181-188`, and WorkspaceDialogs name/search
  (:85-88). None sets a fill, border, radius or font. They show WinUI's default template (bottom focus accent bar, system
  fill). Only the inline rename entry (:317,323) is styled, with `input`, `controlCorner`, `field_padding` and no border.
- GTK reference: Adwaita entries (`alpha(currentColor,.1)` fill, 6px radius). The swatch entry is
  `.preference-entry { background: var(--input-bg); box-shadow:none }` (`style.css:371-372`).
- Fix direction: one `entryColors(data,box,surface)` helper next to `choiceColors`, using the same surface-dependent fill
  rule as PREF-1, and use it at all of these sites.

---

#### Root causes / systemic patterns

1. **`choiceColors` always paints the `input` role** (ChoiceFeedback.cpp:66), and `input`==`settings` in the palette. Every
   ComboBox placed on a settings or dialog surface (not on a card) disappears (bug 11, PREF-1, PREF-2). Fix: choose the fill
   by surface (`tint(text,26)` or `control_fill` on settings and dialogs, `input` on cards and panels), add symmetric
   padding, centred content and `controlHeight` (PREF-6, PREF-16). Add a sibling `entryColors` for TextBoxes (PREF-29).
2. **No "default button" helper.** `button()` is deliberately a flat, zero-padding, transparent control, and callers reach
   for ad-hoc fills: none (action rows, Manage profiles: bug 9, PREF-3/4), opaque `brush("button")` (ShortcutPage `flat()`:
   invisible text), raw WinUI buttons (workspace tabs, Retry, storage banner: PREF-23/28). One helper with the Adwaita button
   fill (`tint("text",26)`), `controlCorner`, `Padding 10,5`, `MinHeight 34` and Bold closes PREF-3/4/20/21/23/28.
4. **`label()` line box metrics** (Segoe UI, `LineHeight 18`, `MaxHeight`) put text about 1-2px low in every centred row
   (bug 13, likely bug 2, PREF-7), and WinUI default paddings (`ComboBoxPadding 12,5,0,7`, `ButtonPadding 11,5,11,6`) add
   another 0.5-1px. Fix it in `label()` (`TextLineBounds`) and with symmetric paddings in the colour helpers.
5. **Native switches and entries still need the palette helpers** (PREF-8/29). Adopt the compact switch and entry resources from P2/P4 in M4.


### HEAD audit: title bar, drawing tabs, status, canvas action bar, notice, zoom readout, document dialogs

Scope: apps/layer-windows HeaderView, HeaderInput, HeaderStatus, CanvasActionBar, DrawingTabs, ZoomReadout, CanvasNotice, CanvasWindow (chrome), DocumentView. Dialog files (NewDrawingDialog, SizeDialog, ExportForm, ExternalImages) are in the HEAD-DLG section at the end.
GTK reference: workspace_header.rs, workspace_header_editor.rs, documents.rs, system_status.rs, canvas_bar.rs, toolbar_components.rs, notice.rs, zoom_readout.rs, screen_view.rs, style.css.
Background facts used below:
- `label()` (UiControls.h:424-430): Segoe UI, `FontSize(textSize())` (11pt = 14.67px), `LineHeight(18)` with `LineStackingStrategy::MaxHeight`. MaxHeight keeps the larger of LineHeight and the font's natural line, so at 14.67px the 18 does nothing. The line box is Segoe UI's ascent+descent (2210+514 units of 2048, about 19.5px).
- `button()` (UiControls.h:465-476) sets `FontWeight Bold` on the control, padding 0, `controlCorner()`, a clear background, and the custom template from ButtonFeedback.h:31-35. Hover is 20/255 (.08) and press 41/255 (.16).
- libadwaita buttons are bold by default. style.css overrides this only where it says so, for example `.document-tab button.document-tab-select { font-weight: normal }`.

---

##### HEAD-1: Canvas bar radio text sits low (user bug 2): the root cause is the label line box, not the button
- Class: VCENTER   Confidence: LIKELY (code plus Segoe UI metrics; confirm with a 400% capture)   Severity: medium   Reported bug: 2
- Windows: `CanvasActionBar.h:93-107`. `segments()` builds each mode (Scale / Distort / Warp) as `ToggleButton` + `worded(toggle,8)` (`Height(controlHeight())`=26, `VerticalAlignment Center`). The content is `labelled()` (`CanvasActionBar.h:84-91`): a horizontal `StackPanel{ icon(...,16), label(data,text,true) }` with the text set to `VerticalAlignment::Center`. Every box is centred correctly: ContentPresenter VerticalContentAlignment defaults to Center (ButtonFeedback.h:33 binds it), the 16px icon is centred in the StackPanel, and the TextBlock is centred. What gets centred is the TextBlock's line box, and `label()` (`UiControls.h:427`) leaves that box at the font's natural ascent+descent. In Segoe UI the ascent (1.079em) is much larger than the descent (0.251em), so the cap and x-height band sits about 0.064em (about 1px at 14.67px) below the box centre. Next to the geometrically centred 16px icon, the word reads low. `LineHeight(18)` has no effect because MaxHeight keeps the larger, natural line.
- GTK reference: `canvas_bar.rs:332-338` → `toolbar_components.rs:571-604` `segment_buttons()` + `captioned()` (`toolbar_components.rs:532-541`): a `gtk::Box` with spacing 6 holding the icon and a `gtk::Label`. style.css:342-345: `.canvas-action-bar .selection-modes > button { min-height: var(--panel-control-height); padding: 0 8px }`. The construction is the same. GTK's UI fonts (Cantarell / Adwaita Sans) have near-symmetric ascent and descent, so the logical-rect centring matches the visual centre.
- Shared source: `panel_metrics.control_height`=26 (`layer-ui/src/layout.rs:57`).
- Fix direction: fix it in `label()` (or a `controlCaption()` helper used by every single-line caption inside a fixed-height control), not per button. Give single-line captions `TextLineBounds::Tight` (or TrimToCapHeight, plus a bottom margin equal to the descent), so the centred box is the glyph band, and turn MaxHeight/LineHeight(18) into a deliberate `BlockLineHeight` only where multi-line text needs it. Add a fixture that measures the ink bounds of "Scale" against its 26px toggle (top gap equals bottom gap ±0.5px).
- Same construction elsewhere in scope (all inherit the offset): canvas bar text tiles (`CanvasActionBar.h:133-136`, `worded()`+`labelled()`), menu fields (`:118`), value dropdowns (`:167`), notice actions (`CanvasNotice.h:76-78`, `label(...,true)` in a `controlHeight()` button), workspace switcher pills (`HeaderView.cpp:140-147`, TextBlock in a 26px ToggleButton), application menu labels (`HeaderView.cpp:319-320`, boxed string in a 26px button), drawing tab titles (`DrawingTabs.h:185-186`), the clock (`HeaderStatus.h:21-23`) and the battery percent (HEAD-19). Out of scope but built the same way: `ToolbarComponents.cpp:715-740` worded segments, `ToolView.cpp:268-275` selection-menu button, `PressureCalibration.h:74-76` axis segments.

##### HEAD-2: Canvas bar radio segments use the opaque `input` fill; GTK uses the translucent default button tint
- Class: GLASS, STACKED-BG, BORDER/COLOR   Confidence: CERTAIN (code)   Severity: medium   Reported bug: none
- Windows: `CanvasActionBar.h:101` `toggle.Background(data->brush(L"input"))`. `input` is an opaque palette role (`theme.rs:360` `surface([51;3],[250;3])`: #333 dark / #fafafa light). It sits on the bar's `glass(L"panel")` surface (`:203`), so unselected segments are opaque chips on a glass bar, and in light theme they are near-white on a grey bar.
- GTK reference: the segment `ToggleButton`s get no `flat` class (`toolbar_components.rs:584`), so they keep libadwaita's default `alpha(currentColor,.1)` fill over the glass. Checked: `.canvas-action-bar button:checked { background: var(--selection-bg) }` (style.css:149), and in glass mode `window.capy-workspace.glass .selection-modes button:checked { background-color: var(--glass-selection) }` (:462).
- Fix direction: give unselected segments `data->tint(L"text",26)` (10%) instead of `brush(L"input")`. The checked state through `selected(data)` is already right. The same `input` fill is used for segments in `ToolView.cpp:256,344` (out of scope); one shared `segmentFill(data)` helper would close both.

##### HEAD-3: Canvas bar segments have no shared builder, so the radio is assembled differently from every other segmented control
- Class: HIGHLIGHT-GEOMETRY, METRIC   Confidence: CERTAIN   Severity: low   Reported bug: 2 (class)
- Windows: `CanvasActionBar.h:97-103` computes the radii by hand (`left=i==0?r:0 ... CornerRadius({left,right,right,left})`). `ToolView.cpp:341`, `PressureCalibration.h:76` and `ToolbarComponents.cpp:721` each repeat their own version with different heights (26 / 36 / stretch), fills (`input` / none / clear) and padding. In RTL these hand-written radii also do not flip the way GTK's `:first-child` / `:last-child` rules do (style.css:344-345, 443-444).
- GTK reference: one helper, `segment_buttons()` (`toolbar_components.rs:571`), plus the CSS rules `.selection-modes > button` / `:first-child` / `:last-child`.
- Fix direction: add one `segmentRow(data, items, captions)` helper in UiControls.h that owns the joined radii (control radius on the outer corners, square joins), the fill (HEAD-2), the caption (HEAD-1) and the checked surface, and use it in the canvas bar, ToolView, ToolbarComponents and PressureCalibration.

##### HEAD-8: Exact disabled title-bar content opacity still needs the shared helper *(A)*
- M2 keeps tile surfaces opaque and applies GTK sensitivity: editing and groups with variants stay enabled.
- ButtonFeedback dims disabled content once at its existing 92/255 default. No glyph opacity is stacked on it.
- In M4, use Agent A's per-control disabled-content opacity override with 0.45 for header tools, including Zen Capy.

##### HEAD-11: Header editor size choices still need joined segments *(A)*
- Windows: `HeaderView.cpp` builds separate size buttons with full corners.
- GTK uses one linked row with square interior joins.
- Adopt Agent A's segment helper in M4.

##### HEAD-21: The zoom readout's parent overwrites its fixed corner radius *(A)*
- `ZoomReadout.h` uses `12*CornerFit`, but `WorkspaceView.cpp` replaces it with `height*.5*CornerFit`.
- GTK `.status-bubble` keeps `12*CornerFit` for every header size.
- Adopt the parent-radius fix in M4 after Agent A's shell helpers land.

##### HEAD-22: Zoom, drawings and screen popups use default WinUI Flyout and ListView chrome
- Class: DEFAULT-CONTROL   Confidence: CHECK (needs a light/dark screenshot)   Severity: medium   Reported bug: 11 (class: popups against the background)
- Windows: `ZoomReadout.h:62` (`Flyout` + ScrollViewer + hand-built rows), `DrawingTabs.h:231` (`Flyout` holding a `ListView` with default `ListViewItem` selection visuals: accent pill, default hover), `DrawingTabs.h:61-70` (plain `MenuFlyoutItem`s and not `NativeMenuItems`), `CanvasWindow.cpp:271-280` (`screenPopup`). None set `FlyoutPresenterStyle`, a background or a theme. `TrackPopup` (NativeMenus.h:107) only tracks lifetime.
- GTK reference: `popover` takes `--popover-bg-color: var(--capy-panel)` (style.css:8) and line colours (:12, :15). The drawings popup is a plain list of flat buttons (`documents.rs:632-680`), and the zoom menu is a `PopoverMenu`.
- Fix direction: route every Flyout through one helper that sets the presenter background to `brush(L"panel")`, a border from `line(L"panel",L"border")`, the panel corner radius and `RequestedTheme`. Replace the ListView item template's selection visuals with the shared `selected(data)` fill, and build the tab context menu with `NativeMenuItems`.

##### HEAD-26: Document dialogs use the stock ContentDialog layout and raw TextBlocks
- Class: DEFAULT-CONTROL, METRIC   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `DocumentView.cpp:130-142, 211-226, 318-323, 351-375, 442-560, 621-646`. These are `ContentDialog`s whose footer (ChoiceFeedback.cpp:55-63) recolours the template buttons (accent or `control_fill`, `controlCorner`) but keeps the stock layout: equal-width stretched buttons, 32px tall, Normal weight, 24px dialog padding. Body text is often a raw `TextBlock` (`:140, :321, :354 status, :623, :633`) at the WinUI default font size and family instead of `label()`. The progress state is a bare `ProgressRing` 48×48 (`:646`).
- GTK reference: style.css:502-511: `.dialog-page { padding: 20px }`, `.dialog-footer { padding: 16px 20px 20px }`, `.dialog-footer > button { min-height: 34px; padding: 0 16px; border-radius: var(--control-radius); font-weight: 500 }` (right-aligned natural width).
- Fix direction: extend `dialogSurface()`/`footer()` to set button `MinHeight 34`, `Padding {16,0,16,0}`, `SemiBold`, and natural width aligned right (template part `CommandSpace`), with dialog padding 20. Replace the raw TextBlocks with `label()`.


##### HEAD-27: Canvas bar and notice disabled content must use the shared policy *(A)*
- Adopt Agent A's disabled-content opacity handling in M4 and remove per-site whole-button opacity.
- Preserve the tile surface and let the shared helper dim the content once. Header tools alone use the 0.45 override from HEAD-8.


---

#### Root causes / systemic patterns

1. Single-line caption centring still depends on Agent A's shared label helper (HEAD-1).
2. Joined segments and their surfaces need the shared segment builder (HEAD-2, HEAD-3, HEAD-11).
3. Flyout, list and document dialog chrome still needs the shared popup and dialog helpers (HEAD-22, HEAD-26).

### HEAD-DLG section (NewDrawingDialog, SizeDialog, ExportForm, ExternalImages)

### HEAD-DLG: dialogs (New drawing, Canvas/Image size, Export form, external drops)

Windows files: `apps/layer-windows/NewDrawingDialog.cpp`, `SizeDialog.cpp`, `ExportForm.h`, `ExternalImages.h`,
helpers `UiControls.h`, `PickerDialog.h`, `ButtonFeedback.h`, `ChoiceFeedback.cpp`, `NumberControl.cpp`.
GTK: `apps/layer-linux/src/new_drawing.rs`, `canvas_size.rs`, `image_size.rs`, `files/export.rs`, `style.css`.
Shared: `crates/layer-ui/src/theme.rs` (palette), `crates/layer-ui/src/layout.rs` (control_radius 13 -> 7.02 after CornerFit .54).

Things that match and need no change: footer padding (`pickerFooter` 20,16,20,20 = `.dialog-footer` 16 20 20), footer spacing 8,
footer button 34 min-height / padding 0 16 / weight 500 / control radius (`pickerButton`), footer order (Presets, gap, Cancel, Create),
hover/press alpha (ButtonFeedback 20/41 of 255 = .08/.16), section/row/control spacing (14/10/6), body 24, colours grid 4:5:4,
paper-tile homogeneous row, ComboBox .07 fill in New drawing (`choiceColors(...,true)` = tint(text,18)), swatch tile 40 = 34+2*3.

---

##### HEAD-DLG-1: Text fields and number fields use the `input` role, which equals the dialog's `settings` background, so they don't show up
- Class: BORDER/COLOR   Confidence: CERTAIN (code + palette values)   Severity: high   Reported bug: none (same family as #11, controls invisible against the background)
- Windows: `NewDrawingDialog.cpp:253` `input()` sets `control.Background(data->brush(L"input"))` for name (`:325`), search (`:441`), saveName (`:467`); `:335` W/H field Borders `fields[i].Background(data->brush(L"input"))`; `:368` `resolutionField.Background(data->brush(L"input"))`. The dialog surface is `brush(L"settings")` (`UiControls.h:419`, drawers `NewDrawingDialog.cpp:425`). SizeDialog combos: `SizeDialog.cpp:46` `choiceColors(data,box)` (drawing=false), which becomes `data->brush(L"input")` (`ChoiceFeedback.cpp:66`). Export combos: `DocumentView.cpp:513` `choiceColors(data,box)` (same). Number entries in every dialog: `NumberControl.cpp:122` `entry.Background(data->brush(L"input"))`.
- GTK reference: `style.css:543` `.new-drawing-dialog entry, .new-drawing-field { ... background: alpha(var(--window-fg-color), .07) }`; `:555` `dropdown > button` the same .07; `:517` `.picker-dialog entry.number-entry { background: alpha(currentColor, .10) }`. GTK never puts the opaque input colour on the dialog surface.
- Shared source: `theme.rs:360` `input = surface([51;3],[250;3])`, `:365` `settings = surface([51;3], [250,250,251])`. In dark mode the two are identical (#333333); in light mode they differ by one unit of blue.
- Fix direction: add one dialog-field fill helper in UiControls (`tint(L"text",18)`, the GTK .07) and use it in `input()`, the field Borders, `choiceColors` for dialog combos (or make `drawing=true` the dialog default), and NumberControl entries hosted in dialogs. Don't use `brush(L"input")` on `settings` surfaces.

##### HEAD-DLG-2: Number entries inside New drawing fields keep their own background on top of the field background
- Class: STACKED-BG   Confidence: CERTAIN   Severity: medium   Reported bug: none (same class as #7)
- Windows: `NewDrawingDialog.cpp:661-663` `numberField` detaches the NumberControl TextBox and sets only Height/TextAlignment/Padding. The TextBox keeps `Background(brush(L"input"))` and `CornerRadius(controlCorner)` from `NumberControl.cpp:122`, so it paints a second rounded box inside the field Border (`:335`, `:368`, relief `:494`). With the `tint(text,15)` relief field this is a visibly different inner rectangle.
- GTK reference: `style.css:546` `.new-drawing-dialog .new-drawing-field entry.number-entry { ... border: 0; background: none; }`.
- Fix direction: in `numberField`, clear the entry background (and its PointerOver/Focused resources, see HEAD-DLG-4) so only the field Border paints.

##### HEAD-DLG-4: Default WinUI TextBox hover and focus states show through on every dialog text entry
- Class: DEFAULT-CONTROL   Confidence: LIKELY (no override exists; the WinUI template swaps Background/BorderBrush/BorderThickness on PointerOver and Focused)   Severity: medium   Reported bug: none
- Windows: `NewDrawingDialog.cpp:252-256` `input()` sets only `Background`, `BorderThickness{0}`, `CornerRadius`. There are no `TextControlBackgroundPointerOver/Focused` or `TextControlBorderBrushFocused` resource overrides for name, search, saveName or the number entries. `NumberControl.cpp:124-126` overrides only the `...Disabled` keys. Another view already fixes this: `WorkspaceManagerView.cpp:323` inserts `TextControlBackground`, `TextControlBackgroundPointerOver` and `TextControlBackgroundFocused`. `NewDrawingDialog.cpp:843` gives the invalid saveName a 1px red border, which the focused state's `TextControlBorderThemeThicknessFocused` and accent underline will override.
- GTK reference: `style.css:543` entries have `border: 0; box-shadow: none` with a constant .07 fill. Focus is shown by the GTK focus ring only.
- Fix direction: add a `textEntry(data,box)` helper in UiControls that inserts the TextControl* Background/BorderBrush keys for Rest/PointerOver/Focused/Disabled (as WorkspaceManagerView does). Use it in `input()`, NumberControl and the export form.

##### HEAD-DLG-5: Checked segments and pills use the `selection` role, not `header_selection`
- Class: BORDER/COLOR, HIGHLIGHT-GEOMETRY   Confidence: CERTAIN   Severity: medium (light theme), low (dark)   Reported bug: none
- Windows: `NewDrawingDialog.cpp:271-273` `mark()` -> `target.Background(on?Brush(selected(data)):off)`. `selected()` = `glassSurfaces?glass(L"selection"):brush(L"selection")` (`UiControls.h:410`). It is used for orientation (`:578`), colour/transparent mode (`:582`), category pills (`:817`) and the link toggle (`:733`). A checked segment also gets ButtonFeedback's 20-alpha hover overlay on top.
- GTK reference: `style.css:89` `.choice-well button:checked { background: var(--header-selection) }`, `:90` checked hover -> `--header-selection-hover`, `:573` the hover overlay is `:not(:checked)` for wells, `:563` `button.new-drawing-pill:checked { background: var(--header-selection) }`.
- Shared source: `theme.rs:323-324` `header_selection`, `header_selection_hover`. `theme.rs:495-498`: they are equal in dark, but light is `selection #c0d7f6` vs `header_selection #afc6e5`. In a modal dialog the glass `selection` variant is also translucent on an opaque sheet.
- Fix direction: give `mark()` (or a shared `segmentSelected(data)`) `brush(L"header_selection")`, with `buttonFeedback(..., hoverSurface=brush(L"header_selection_hover"))` while checked, as `HeaderView.cpp:172` already does. The canvas-bar segmented control likely shares this.

##### HEAD-DLG-8: Segmented toggle text sits low (same build as the canvas-bar "scale" radio)
- Class: VCENTER   Confidence: LIKELY (needs a screenshot to measure)   Severity: medium   Reported bug: #2 (same class), #13
- Windows instances built the way reported bug 2 is (fixed-height ButtonFeedback button, content = horizontal StackPanel {icon, `label()` TextBlock, VerticalAlignment Center}):
  - Colour/transparent mode segments: `NewDrawingDialog.cpp:511-514`, `toggle(id,0,26)`, content `StackPanel{icon(glyph,14), caption() Medium}`. This is a direct VCENTER instance.
  - Presets footer button: `:417-419`, `StackPanel{presetsLabel=caption() Medium, chevron 14}`. It sits beside Cancel/Create, whose content is a plain `box_value` string (`pickerLabel`, `PickerDialog.h:39`) with no LineHeight/MaxHeight. The two text paths in one row can land on different baselines.
  - Resolution menu rows: `:709-711`, `Grid{caption(), caption(false,1,.6)}` in a 34px button.
  - Paper tiles: `:398-402`, `Grid{thumbnail, StackPanel{caption(true), textureRelief}}`. `textureRelief.LineHeight(0)` at `:399`, while every other caption keeps 18, so the stack uses mixed line models.
  - Row labels centred against 34px fields: `nameLabel :324`, `resolutionLabel :359`, prefixes `:334`, units `:360`, `:492`.
  - Not the same build: orientation and link toggles (icon only); category pills (`:809`, plain string content).
- Root cause: `UiControls.h:424-430` `label()` sets `LineHeight(18)` and `LineStackingStrategy::MaxHeight`, and `caption()` (`NewDrawingDialog.cpp:248-251`) adds `VerticalAlignment::Center`. At textSize 14.67px, Segoe UI's natural line (ascent ~1.08em, descent ~.25em, about 19.5px) is taller than 18, so MaxHeight keeps the natural box. Its ascent is about 4x its descent, so cap-height and x-height glyph centres sit roughly 1-2px below the box centre, while the icon is centred geometrically. For the `.82` captions (12px, natural ~16px), the 18px line box adds leading as well (CHECK).
- GTK reference: Pango labels in GtkBox with `valign` centre and theme font metrics. `style.css:557` buttons have `padding: 0 9px` (no vertical padding), so GTK's glyphs centre optically.
- Fix direction: fix it once in `label()`: set `TextLineBounds(TextLineBounds::TrimToCapHeight)` (or `Tight`) for single-line control labels so the cap band is what gets centred. Drop the fixed `LineHeight(18)`, or derive it from textSize. Use the same helper for button text set through `pickerLabel`/`box_value` so a row shares one text model. This also covers the canvas-bar radio (#2).

##### HEAD-DLG-9: Text entries fake vertical centring with literal padding (top 7, bottom 0)
- Class: VCENTER, METRIC   Confidence: LIKELY   Severity: medium   Reported bug: none
- Windows: `NewDrawingDialog.cpp:325` name `Padding({10,7,10,0})`; `:441` search `{10,7,10,0}`; `:467` saveName `{10,7,10,0}`; `:663` number entries `{4,7,10,0}`; `:673` relief `{10,7,0,0}`; `:693` resolution `{10,7,0,0}`. These replace NumberControl's symmetric inset `textInset=(valueHeight-measuredHeight)/2` (`NumberControl.cpp:232-233`). If textSize or TextScaleFactor changes, text drifts off centre in the 34px box.
- GTK reference: `style.css:544` `.new-drawing-dialog entry { padding: 0 10px }`; `:546` number entries `padding: 0 10px 0 4px`; `:548` combo entries `0 2px 0 10px`. Vertical centring is natural, with no vertical padding.
- Fix direction: compute the vertical inset as NumberControl does (measure the line, then (34-h)/2) in the shared `input()`/text-entry helper, and keep only horizontal values per site.

##### HEAD-DLG-10: New drawing dropdowns are not bold and keep the WinUI asymmetric ComboBox padding
- Class: METRIC, VCENTER, DEFAULT-CONTROL   Confidence: LIKELY (weight CERTAIN)   Severity: medium   Reported bug: none
- Windows: `NewDrawingDialog.cpp:257-259` `choice()` = `input()` + `choiceColors(...,true)`. No `FontWeight` (Normal) and no `Padding`, so the WinUI default `ComboBoxPadding` (12,5,0,7, asymmetric vertically) stays and nudges the selected text up inside the 34px box. The same applies to saveGroup (`:470`). Items are created by `comboOption` (`UiControls.h:444-456`) as bare TextBlocks with no font, size or LineHeight.
- GTK reference: `style.css:555` `.new-drawing-dialog dropdown > button { min-height: 34px; padding: 0 10px; border-radius: var(--control-radius); ...; font-weight: 700; }`.
- Fix direction: `choice()` (or `choiceColors`) sets `FontWeight Bold` and `Padding{10,0,...,0}` with VerticalContentAlignment Center. Give `comboOption` the `label()` font so the closed combo and its items share one text model.

##### HEAD-DLG-11: Dropdown lists, the resolution menu and context menus use default WinUI popup chrome
- Class: DEFAULT-CONTROL   Confidence: LIKELY   Severity: medium   Reported bug: none (same class as #11)
- Windows: `ChoiceFeedback.cpp:45-48`: `choiceColors` restyles only the closed ComboBox (Background/Foreground/Border resource keys). `ComboBoxDropDownBackground`, `ComboBoxItem*` and the popup border are untouched, so open lists use the WinUI flyout surface, item hover/selected colours and the accent selection pill. Resolution menu: `NewDrawingDialog.cpp:118` `Flyout resolutionMenu`, `:370` `resolutionMenu.Content(resolutionItems)` with no `FlyoutPresenterStyle`, so it keeps the default presenter background, border, padding and min/max width, plus its own `Padding 4`. The preset context menu (`:872`) and swatch menu (`:158`) are default `MenuFlyout`s. `HeaderView.cpp:257` shows the app already sets `MenuFlyoutPresenterStyle` elsewhere.
- GTK reference: `new_drawing.rs:404` resolution `gtk::Popover ... has_arrow(false)`, items are flat buttons (`:894`). `style.css:570` `.new-drawing-dialog popover button.selected { background: alpha(currentColor,.10) }`. The popover surface is `--popover-bg-color: var(--capy-panel)` (`style.css:7`).
- Fix direction: add a shared popup style helper (presenter background `brush(L"panel")`, line border, control-radius corner, compact padding) and apply it to the ComboBox dropdown resources, the `Flyout` (`FlyoutPresenterStyle`) and `MenuFlyout`s, in both themes.

##### HEAD-DLG-16: The picker title is smaller than GTK's title-4
- Class: METRIC   Confidence: LIKELY (Adwaita value)   Severity: low   Reported bug: none
- Windows: `PickerDialog.h:23` `title.FontSize(data->textSize()*1.05)`, Bold.
- GTK reference: `new_drawing.rs:167` `title.add_css_class("title-4")`. Adwaita title-4 is about 13pt at weight 700, about 1.18x the 11pt UI text.
- Fix direction: take the title scale from the shared catalog, or use ~1.18 to match.

##### HEAD-DLG-28: The New drawing dialog is shorter than GTK's
- Class: METRIC   Confidence: LIKELY   Severity: low   Reported bug: none
- Windows: `PickerDialog.h:6` `PickerDialogHeight=486.67`, used at `NewDrawingDialog.cpp:292`.
- GTK reference: `new_drawing.rs:321` `adw::Dialog::builder().content_width(720).content_height(500)`.
- Fix direction: get the content size from one shared constant; 500 unless 486.67 deliberately compensates for chrome (document why in the commit, not in a comment).

##### HEAD-DLG-29: Canvas and Image size dialogs use unstyled WinUI controls and default text
- Class: DEFAULT-CONTROL, BORDER/COLOR   Confidence: CERTAIN (code); visual LIKELY   Severity: medium   Reported bug: none
- Windows: `SizeDialog.cpp:71` `option.MinWidth(0);option.MinHeight(34)`: a default CheckBox template with no FontFamily, FontSize or Foreground, so it renders in WinUI's default font (Segoe UI Variable, 14px) and theme text colour, not the palette `text` at textSize. `:78`, `:91`: the `caption` and `message` TextBlocks have no font, size or foreground. `:46` combos: `choiceColors` only, no Height or font (WinUI default 32px min-height; GTK button 34). `:177` `dialog.Title(box_value(...))` uses the default ContentDialog title template (20px SemiBold, left-aligned). `UiControls.h:424` `label()` and `:431` `compactCheck()` exist and aren't used here.
- GTK reference: `canvas_size.rs:41-104` / `image_size.rs:39-80`: an `adw::AlertDialog` (centred heading, theme font) with a `gtk::CheckButton`, `gtk::DropDown`, plain labels, and `message.add_css_class("dim-label")`.
- Fix direction: build SizeDialog's labels with `label()`, the check with `compactCheck()`, and combos with the dialog field helper (HEAD-DLG-1/10). Give it a styled title (`pickerTitle`), or move it onto the picker dialog frame so titles and footers match the New drawing dialog.

##### HEAD-DLG-32: Size dialog response buttons keep the default ContentDialog button geometry
- Class: DEFAULT-CONTROL   Confidence: CHECK   Severity: low   Reported bug: none
- Windows: `ChoiceFeedback.cpp:60` `footer()` restyles PrimaryButton/CloseButton Background, Foreground, BorderThickness and CornerRadius only. Height, padding and FontWeight (Normal) stay at WinUI defaults.
- GTK reference: `canvas_size.rs:39-43` AlertDialog responses with `ResponseAppearance::Suggested` for Apply. The New drawing picker footer uses 34 / 0 16 / weight 500 (`style.css:511`).
- Fix direction: in `footer()` set `MinHeight 34`, `Padding{16,0,16,0}`, `FontWeight Medium` (the `pickerButton` metrics) so every dialog footer matches.

##### HEAD-DLG-33: The export form is entirely default WinUI controls in a flat list, not GTK's grouped rows
- Class: DEFAULT-CONTROL, LAYOUT-COLLAPSE, BORDER/COLOR   Confidence: CERTAIN   Severity: high   Reported bug: none (same class as #9, #11)
- Windows: `ExportForm.h:95` `add()` uses `ComboBox.Header(box_value(label))` (default header template and font); `:102`, `:104` `NumberBox` quality, width, height and `:108` ppi are default NumberBox/TextBox templates (default background, border and focus underline; no palette brush or font); `:11` (declared), `:103`, `:105`: `resize`, `enlarge` and `removeLocation` are default `CheckBox`es; `:111` `metadataNote` and `:113` `validation` are TextBlocks with no font or foreground. `:92` `root.Spacing(8)`. The only styling is `choiceColors` applied from `DocumentView.cpp:513`, which gives an `input` fill that is invisible on the dialog (HEAD-DLG-1).
- GTK reference: `files/export.rs:119-135` `adw::ComboRow` inside `adw::PreferencesGroup` (boxed-list cards, `style.css:15` shadows); `:208`, `:403`, `:452` `adw::SwitchRow` for enlarge, remove-location and dither; bodies use `gtk::Box::new(Vertical, 12)` (`:999-1026`); `:239`, `:408` notes are `dim-label`.
- Fix direction: build the form from the shared row helpers (label + combo/number/switch rows on a card surface, as SettingsView does), with `label()` text, the dialog field fill, the HEAD-DLG-4 TextBox resources for NumberBox, and spacing 12.


---

#### Root causes / systemic patterns

1. Dialog fields and default controls still need Agent A's palette and entry helpers (HEAD-DLG-1, 2, 4, 29, 33).
2. Caption centring, joined controls and selected surfaces need the shared label and segment helpers (HEAD-DLG-5, 8, 9, 10).
3. Popup, picker-frame and footer adoption remains in M4 (HEAD-DLG-11, 16, 28, 32).
