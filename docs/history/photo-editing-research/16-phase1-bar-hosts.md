# Phase 1 audit: canvas action bar on each host

[Photo editing research](../photo-editing-research.md) · [Phase 1 plan](../../development/canvas-action-bar-transforms.md) · source report, 2026-09-26 · baseline `5eb45a47`

Read-only implementation audit made by an agent against baseline `5eb45a47` to prepare Phase 1. For GTK, Web, Android, Apple and Windows it covers panel-layer placement, glass registration, Zen, reuse of the Tool Options rows, measurement, chrome input, the placement bar to delete and the test harnesses. The [Phase 1 plan](../../development/canvas-action-bar-transforms.md) incorporates the findings and resolves the points where the audits differ. Line numbers can drift in later commits; verify before relying on one.

---

I ran one audit per host and checked the load-bearing lines myself. Nothing was edited. Line numbers can drift in later commits.

## Findings that change the plan

1. **The spec's surface rule is superseded.** "Opaque panel fill" (`docs/history/photo-editing-research.md`, Appearance and settings) no longer holds. Every host already has a glass path for panels, and the bar can join it. The lag concern it cited goes away because the bar is stationary whenever it is visible.
2. **Apple placement differs from the spec.** The bar belongs in the `WorkspacePanels` ZStack, not the root ZStack.
   - In the root ZStack it would cover the drawers (`WorkspaceDrawers.swift:91`) and the contact menu at z 400 (`WorkspacePanels.swift:56`).
   - Its `WorkspaceControlSurface` source is only collected inside `WorkspacePanels` (`:65`).
3. **Existing Windows bug.** `WorkspaceView.cpp:771` returns no glass regions while `chrome_hidden`, so floating panels lose their blur in Zen today. The line predates f587e1f2. It must be fixed for the bar.
4. **Renderer cost.** Any change to the region list repaints *all* old and new glass bounds (`crates/layer-render-wgpu/src/backdrop_blur.rs:510-514`). Hiding the bar at pen-down therefore repaints every panel's glass area in the stroke's first frame. Repaint only the symmetric difference, or measure the cost.
5. **Menus opened from the bar can cancel transforms on Android.** The chain:
   - `WorkspaceMenu` ties `focusable` to `preserveContact`, so it is focusable by default (`WorkspaceMenus.kt:38-45`).
   - The `ToolbarNumber` and `ToolbarChoice` menus are default, focusable `DropdownMenu`s (`ToolbarComponents.kt:234,291`).
   - Losing window focus sends `blur` (`MainActivity.kt:100-106`). `UiInput::Blur` then calls `cancel_layer_gesture` (`session.rs:1184-1191`).
6. **A tap on the bar closes the drawer it opened.** A bar tap is a `canvas:false` chrome contact, which closes `OutsideContact` drawers (`session.rs:859-906`). So More → Tool Options drawer cannot toggle closed. This needs a bar anchor in Rust (`DrawerAnchor`, `drawers.rs:30-43`) and bar bounds in `ChromeFacts` (`interaction.rs:31-51`).
7. **More in Zen reveals docked chrome.** `popup_open` pins chrome visible (`session.rs:1247-1250`). Existing menus already behave this way; accept it and document it.
8. **The fitter cannot keep completion actions.** `tool_options_layout` stops at the first field that does not fit and pins More bottom-right (`toolbar_components.rs:367-431`). Trailing Cancel/Apply that "never overflow" need a bar-specific fitter.

**Recommended z-order, the same on every host:** docked groups, dividers and collapsed columns < floating groups < **bar** < drawers < header < menus and popovers.
- A drawer opened from More then covers the bar.
- Rust keeps the bar clear of floating panels. It should also hide the bar during workspace (float) drags, so a dragged float never crosses a stationary bar.

---

## GTK (`apps/layer-linux/src`), the reference host

**1. Placement**
- Add `Slot::CanvasBar` to the enum (`workspace.rs:541-554`).
- In `size_allocate`, skip it in pass 1 (`:218-301`), as `Drawer(0)` is skipped (`:219-224`). Place it in pass 2 (`:302-321`) with `allocate_at` or `set_child_visible(false)`.
- `clear_docks` (`:637-650`) must keep the bar, or every dock rebuild deletes it.
- Stacking follows Vec order (`:343-368`, `add` at `:630-636`). Raise the bar just before the drawer raise (`raise_drawer` `:571-613`, called at `:2627` and `workspace_drawer.rs:884`). Floating groups re-raise on drag (`:3018-3022`), and this call order keeps floats < bar < drawers.
- Header and Status are added first (`:1041-1043`). Measure against the allocated HUD rect, which grows to its measured height (`:243-252`).

**2. Glass**
- Give the root `gtk::Box` the `dock-panel` class. The rule `window.capy-workspace.glass .dock-panel { background: var(--glass-panel) }` (`style.css:325`) uses the colour `apply_palette` writes (`workspace.rs:2663-2686`).
- Collection is automatic: `DockSurface::snapshot` (`:372-378`) → `glass::collect` (`glass.rs:64-160`).
- Republication happens only when the list changes (`:382`): one idle `wake()` (`:384-391`), copied to the renderer at `:2220-2226`. Glass is held during strokes (`:2219`).
- The cost is the node walk per snapshot, which `native_backdrop_blur_capture` prints (`tests.rs:10967-10981`).

**3. Zen**
- `set_chrome_hidden` rewrites `zen-hidden` and `can_target` on every non-Canvas child, exempting only the `floating-panel` class (`:1837-1854`). Exempt `Slot::CanvasBar` **by slot**, so the bar's own hidden state is not overwritten.
- Update the tests that assume every non-canvas child hides: `tests.rs:6862-6866` and `:14141-14145`.
- Edge reveal is decided in Rust (`session.rs:1235-1285`). Bar contacts are `canvas:false`, so they are never swallowed as a reveal (`:933-935`).

**4. Rows and menus**
- Extract an `OptionRows` type from `Component::new` (`toolbar_components.rs:400-540`), taking over the refresh diff (`:578-654`), `add_option` (`:1047-1270`) and sizing (`:157-191`).
- Add a name prefix. The fixed `toolbar-setting-*`/`toolbar-action-*` names (`:1079-1248`) would collide in `find_named` tests.
- More: a `gtk::PopoverMenu` filled by `populate_workspace_menu` (`workspace_customization.rs:1476-1560`), as `chrome_menu` does (`workspace.rs:1540-1570`). Register it with `watch_popover` (`:1572-1580`), which sets `popup_open` (`:1675-1679`).
- GTK has no Selection Actions today (T-20), so the bar is the first GTK caller of `selection_menu` (`selection_masks.rs:226-231`).
- GTK sends blur from the toplevel's `is_active` (`input.rs:357-366`). A popover should not deactivate the toplevel; verify this in the native test.

**5. Measure and place**
- Follow the drawer route: call a pure Rust placement directly (precedent `workspace_drawer.rs:610-638`); no dispatch is needed.
- GTK has no camera-only patch. The CAMERA refresh already clones `UiState` (`workspace.rs:2487-2493,2604-2610`). The bar is hidden during camera gestures, so it re-places only when it becomes visible again.

**6. Input**
- The window capture controller sends `Contact{canvas:false}` for any press that does not pick `area` (`:1469-1518`). Canvas controllers live only on `area` (`input.rs:92-345`).
- The brush cursor clears on leave (`input.rs:195-199`).
- Pen tooltips need `has_tooltip` (`tooltips.rs:47-111`) plus `bind_action_tooltip` (`workspace.rs:1407-1430`).
- Set `focus_on_click(false)` on every bar control (only `documents.rs:553` does this today). A focused DropDown counts as `editing` (`:1364-1370`) and changes Enter/Escape.

**7. Delete**
- `tool_panels.rs:14-58`; `workspace.rs:913,1113-1114,1173,1210,2521`. There is no CSS.
- Migrate `photo_drop_tests.rs:221-236` (callers `:301,436,507`) and `photo_workflow_tests.rs:377,439-440,571,579,601`.

**8. Tests**
- Native tests are `#[ignore] native_*` (`tests.rs:4-73`, helpers `:80-117`).
- Run one with `workspace-motion.sh gtk --native-test=NAME` (`bench/native-input.js:37-38,78-92`).
- Mouse and touch go through RemoteDesktop; pen through `--tablet` (`native-input.js:98-103,215-218`).
- Glass capture: `native_backdrop_blur_capture` with `LAYER_GLASS_THEME`/`LEVEL` (`tests.rs:10770-10983`).

**Files:** new `canvas_bar.rs`; `workspace.rs`; `toolbar_components.rs`; `tool_panels.rs` (delete); `style.css` (padding and radius only); `tests.rs`, `photo_*_tests.rs`, and a new `native_canvas_bar_input` test.

---

## Web (`apps/layer-web`)

**1. Placement**
- A `section.canvas-action-bar` (`role=toolbar`), appended to `#workspace` after the Capy button (`app.js:1662-1670`).
- Current bands: docked groups 1, floating `100+2i` (`app.js:557`), header 1000 (`style.css:148`), drawers 1800/1900 (`workspace-chrome.js:190-201`), expanded panel 2000 (`customization.js:481`).
- Use **z-index 900**. The bar will cover the `#status` toast in bottom-edge placement.

**2. Glass**
- Add `.canvas-action-bar` to the selector list (`glass.js:1-11`) and fill it with `var(--glass-panel)` (`style.css:3`; `app.js:82-84`). Also add it to the `--glass-selection` rule (`style.css:199`).
- It must not carry `.chrome`, or the Zen exclusion (`glass.js:12`) drops it.
- `flush()` measures `getBoundingClientRect` and computed radii (`glass.js:18-32`) at the start of the frame (`app.js:354`).
- `place()` queues a re-measure (`:453-461`); `hidden` toggles and `transform` moves do not. Call `glass.queue()` on show and hide.
- Cost: `querySelectorAll`, then `getBoundingClientRect` and `getComputedStyle` per surface (a forced layout if the DOM is dirty), then `set_glass` (`src/editor.rs:157-175`) and one frame.

**3. Zen**
- The bar survives `style.css:461` as a plain `#workspace` child.
- `updateZen` does not re-lay out (`app.js:1243-1251`). Keep the placement based on the non-Zen layout so the bar does not jump on reveal.

**4. Rows and menus**
- Extract a field factory from `createToolbarComponent` (`toolbar-components.js:7-313`). The tile couplings are at `:8-18,34,188,280,297-301`, and per-row `place()` queues glass (`:284,290`).
- More reuses the synthetic-`contextmenu` path (`selection-masks.js:3-11` → `customization.js:86-108`, manual popover).
- `positionPopup` only clamps (`:95-99`). Add a flip-above so a menu does not cover a bottom bar.
- The open popover sets `popup_open` (`app.js:1237-1239`). That is acceptable for explicitly opened menus.

**5. Measure and place**
- Add a bar export beside `drawer` (`src/editor.rs:177-201`), or a dispatch.
- Read hide/show next to `chrome_hidden` in `input()` (`app.js:1200`), not through `state_update`.
- Receive the anchor and placement in the camera branch (`:286-290`).

**6. Input**
- The capture pointerdown sends `canvas: e.target===canvas` (`app.js:1289-1317`). The cursor clears through `elementFromPoint` (`:298-324`).
- The bar needs:
  - `touch-action:none` and `user-select:none`;
  - its own `contextmenu` `preventDefault`;
  - `mousedown` `preventDefault` except on inputs, so taps do not move focus;
  - `tabIndex=-1` until the focus command lands;
  - `aria-disabled` instead of `disabled`, because disabled buttons get no pointer events (`style.css:307`) and so could show no reason.
- Only window blur cancels transforms (`app.js:1560-1566`).

**7. Delete**
- `image-import.js:6-9,42-46`; `documents.js:310`; `style.css` lines 4, 6, 8, 9 and `902-910`.
- Port the tests:
  - `image-placement.test.mjs:72-160` (the Zen + 360px check is at `:113-116`);
  - `image-placement-device.test.mjs:21-25,36,38,48`;
  - `test.mjs:271-273`; `device.test.mjs:151-153`.

**8. Tests**
- `node --test` units. `pointer.test.mjs` slices `app.js` source (`:21,58-59`), so new calls need stubs.
- `test.mjs` drives Chrome over a pipe, with one flag per journey (`:252-470`):
  - pen: `pointerType:'pen'` (`:738-753`);
  - touch: `dispatchTouchEvent` (`:772-783`);
  - light/dark screenshots: `drawers.test.mjs:149,164-165`;
  - glass: a `set_glass` spy exists (`workspace-motion.test.mjs:121-131`).
- Real OS input: `workspace-motion.sh web`. Android Chrome: `device.test.mjs`. Pen latency: `tools/performance/web-pen.mjs`.

**Files:** new `canvas-bar.js`; `toolbar-components.js`; `app.js`; `glass.js`; `style.css`; `customization.js`; `workspace-chrome.js` (drawer source for the bar anchor, `:188-189`); `src/editor.rs`/`lib.rs`; packaging (`package.mjs:42`); new `canvas-bar.test.mjs`.

---

## Android (`apps/layer-android/.../art/capycanvas`)

**1. Placement**
- A sibling right after the groups loop (`Workspace.kt:294-344`), outside every `!hidden` gate.
- Bands: docked `100+i`, collapsed columns 160, dividers 170, floating `180+i`, drawer source 199 (`:294-298,354`); drawers 200/220 (`WorkspaceChrome.kt:222,239`); header 300 (`Workspace.kt:280`).
- Use **zIndex 198** (199 when the bar is a drawer source).

**2. Glass**
- `Modifier.glass` (`UiStyle.kt:114-127`) → `glassBox` → one coalesced post per looper turn → `Native.glassRegions` + wake (`CanvasHost.kt:136-158`). It unregisters on dispose (`UiStyle.kt:118`).
- An `offset{}` move re-fires `onGloballyPositioned`, which re-registers.
- Remove the bar from composition when hidden. Leaving it composed but unplaced leaves stale glass and chrome regions.

**3. Zen**
- Floating groups and drawers ignore `hidden` (`Workspace.kt:273-294`). Render the bar ungated as well.

**4. Rows and menus**
- Extract `toolOptionSizes` (`ToolbarComponents.kt:137-165`) and `ToolOptionField` (`:176-198`). Add a height parameter to `ToolbarNumber` (`:211`).
- Make the `ToolbarNumber`/`ToolbarChoice` menus non-focusable.
- Split `focusable` from `preserveContact` in `WorkspaceMenu`, and set `dock.popupOpen` while the bar's menu is open. `popup_open` is computed at `WorkspaceInput.kt:159-163`; today `WorkspaceMenu` never sets it. `popupOpen` is a single Boolean, so make it an owner count.

**5. Measure and place**
- Keep the bar model in its own `mutableStateOf`, like `cameraReadout` (`CanvasHost.kt:124,826-829`), filled from the camera-only packet (`:728-741`).
- Keep it out of the panel-content lists (`:655,770`), or every change republishes all panels.
- Dispatch the measurement, deduplicated like `measure_panels` (`WorkspaceInput.kt:143-155`).

**6. Input**
- `chromeRegion` (`WorkspaceInput.kt:429-433`) drives `canvas:false` contacts (`:322-327`) and the pointer-icon hand-off (`CanvasSurfaceView.kt:112-117`).
- Wrap the bar in a `Surface` so it consumes input (`Workspace.kt:493`).
- `dock.hit()` picks by bounds only (`WorkspaceInput.kt:187-188`), so a grip under the bar would win. Rely on Rust clearance.
- `HOVER_EXIT` sends `cursor_leave` (`CanvasSurfaceView.kt:190-204`). The bar must never call `requestFocus`.

**7. Delete**
- `ImageImport.kt:205-215`, keeping the progress `Popup` (`:196-203`) and `SourceProfileDialog` (`:204`). Update the caller at `Documents.kt:353`.
- Port the `AndroidRasterTest` uses: `:1453-1480`, `:1566-1721`, `:2312-2313`.

**8. Tests**
- `AndroidInteractionTest` builds finger, mouse and stylus `MotionEvent`s (`:109-160`), with optional `systemInput` injection (`:48,128-131`).
- Light/dark screenshot checks: `:997-1048`, `:1129-1160`; `AndroidPanelShadowTest:31-98`.
- `glass_regions` is reported in the status (`native/src/android.rs:117`).
- Runners: `run.sh:40-42`, or `am instrument` (`docs/development/android.md:104-113`).

**Files:** new `CanvasBar.kt`; `Workspace.kt`; `ToolbarComponents.kt`; `WorkspaceMenus.kt`; `CanvasHost.kt`; `WorkspaceInput.kt`; `ImageImport.kt`/`Documents.kt`; tests.

---

## Apple (`apps/layer-apple`)

**1. Placement**
- Inside the `WorkspacePanels` ZStack after the collapsed columns (`WorkspacePanels.swift:51`), with `.zIndex(180)` and `workspaceLayer` 180.
- Groups there have no zIndex (`:18-35`); collapsed columns are 160 and drawers 200/220 (`WorkspaceDrawers.swift:15,91`).
- Place with `.placed` (`EditorStyle.swift:63-66`).

**2. Glass**
- `glassSurface` (`GlassSurface.swift:67-71`) filled with `palette.glassPanel` (`EditorStyle.swift:24`), content under `palette.glassy`, and `OutsideShadow`, as floating groups do (`WorkspacePanels.swift:103-109`).
- `GlassRegistration` follows the frame through `onGeometryChange` and unregisters in `onDisappear` (`GlassSurface.swift:42-55`).
- `GlassRegistry` sends one main-queue flush per run-loop turn (`:18-22`) through `capy_apple_glass_regions`. Rust marks dirty only on a real change (`native/src/previews.rs:39-57`).
- Hide the bar by removing the view, not by lowering opacity.

**3. Zen**
- Groups are gated at `WorkspacePanels.swift:19`; drawers are not gated (`:52`). The bar must not read `chrome_hidden`.

**4. Rows and menus**
- Make `ToolOptionField` (`ToolbarComponents.swift:437-471`) internal, taking `iconSize`/`idPrefix` instead of `panel`. Lift `fieldSize` (`:394-410`).
- Add a `ToolOptionsRow<More>`; `ToolOptionsComponent` (`:358-411`) becomes that row plus its drag and tile parts.
- Edits go through `toolbarEdit` (`:32-34`), so stale-context rejection carries over.
- Selection Actions uses `editorPopover` (`SelectionControls.swift:57-78`), which is modal with a tap-catcher (`EditorPopover.swift:76,85`) and does **not** set `popup_open`.
- For bar menus, use the `EditorMenuButton` pattern (`EditorActionMenu.swift:3-19`), which registers `workspace.popover` → `popup_open` (`WorkspacePresentation.swift:140-149`).
- `dismissTransients` closes only the slider preview (`:150`). Keep the bar out of it.

**5. Measure and place**
- Stage a `canvas_bar` key in the partial snapshot branches (`Shared/Bridge/EditorSnapshotState.swift:38,46-51,58-65`).
- Dispatch the measurement, deduplicated like `measureTiles` (`ContentDrawersPresentation.swift:111-121`).
- Do not bump `revision` on hide/show; that triggers the heavy full path (`EditorStore.swift:130-152`).

**6. Input**
- iPad chrome contacts come from a recognizer that never recognizes (`iOS/Platform/ChromeContact.swift:36-41`; `EditorView.swift:94-97`). Pencil hover ends over chrome (`PencilInput.swift:199-213`).
- On macOS, a `SpatialTapGesture` sends the contact (`macOS/Platform/ChromeContact.swift:4-8`), and hover hit-testing clears the brush (`MacInput.swift:113-119`).
- Resigning key status sends blur (`MacMetalCanvas.swift:64-67`), so never use `.popover`, `.sheet` or a window from the bar.
- The bar needs `contentShape`, `WorkspaceControlSurface`, `.focusEffectDisabled()`, no `FocusState`, and `.allowsHitTesting(expansion.isNull)`.

**7. Delete**
- `EditorView.swift:49-51,171-196`.
- Port the macOS XCUITests in `Shared/Tests/DocumentControlChecks.swift:206-292,448-461` (Zen hit check at `:450-455`).
- Update docs: `README.md:765`, `command-coverage.json:44-45`.

**8. Tests**
- XCUITest targets are generated by `scripts/project.py:91-121`. Relevant entries: `testPanelTransparency` (`PanelTransparencyChecks.swift:28-54`), the toolbar-components tests and the Zen tests.
- XCUITest cannot produce Pencil input. Use the UIKit fixtures (`tests/canvas-modifiers.swift`, `canvas-hover.swift`, `canvas-native-input.swift`) and `tests/snapshot-projection.swift`.
- Rust: `cargo test -p layer-apple`, following `glass_tests.rs` and the popup-consumption test (`workspace_tests.rs:66-90`).

**Files:** new `Shared/Editor/CanvasActionBar.swift`; `WorkspacePanels.swift`; `ToolbarComponents.swift`; `EditorView.swift`; `EditorSnapshotState.swift`; new `CanvasActionBarChecks.swift` registered in both `EditorLaunchTests`; new `native/src/canvas_bar_tests.rs`.

---

## Windows (`apps/layer-windows`)

**1. Placement**
- Create the bar in `init()` after the drawers (`WorkspaceView.cpp:149`). Re-append it in the theme reset (`:318`) and apply it after `drawers->Apply()` (`:383`).
- Bands: docked 0, dividers 10, floating `100+2·order` (`:352`), collapsed columns 159/160, drawers 200/220 (`WorkspaceDrawers.cpp:44,57`), `zenCapy` 1001.
- The header is a later root-Grid sibling (`CanvasWindow.cpp:360`), so it is always above the bar.
- Use **ZIndex 180**.

**2. Glass**
- Append the bar in `glass()` (`WorkspaceView.cpp:770-775`) and **delete the `chrome_hidden` early return at `:771`**.
- Fill a squircle `Path` with `data->glass(L"panel")` (`UiControls.h:157-160`), as drawers do (`WorkspaceDrawers.cpp:55`).
- Draw the shadow with `WorkspaceShadow` Shape+Cut (`WorkspaceShadow.cpp:25-57`), not `ThemeShadow`, which would show through a translucent surface.
- `PublishGlass` sends only on change (`CanvasWindow.cpp:302-310`). `appendGlass` skips Collapsed elements (`WorkspaceGeometry.h:43-50`), so call `glassChanged()` right after collapsing the bar.
- Test hook: `windows_glass.regions` (`native/src/host.rs:1013-1016`).

**3. Zen**
- Groups hide only when docked (`:350`). The bar is ungated and must stay clear of `zenCapy` (`:737-750`).

**4. Rows and menus**
- Extract a `ToolOptionsRow` from `ToolbarComponent`. The tile couplings are at `ToolbarComponents.cpp:108-157,207-211,224`; the builders at `:473-724` and `options_layout` at `:236-249` are reusable.
- Choice menus and the numeric editor already go through `TrackPopup` → `menuOpen` (`NativeMenus.h:42-46`; `CanvasWindow.cpp:1192-1195`).
- Move Selection Actions (`ToolView.cpp:211-229`) into a `NativeMenus.h` helper and open it with `ShowAt(moreButton)`. Owned flyouts do not cancel transforms (`CanvasWindow.cpp:194-206`).

**5. Measure and place**
- A transient `measure_canvas_bar` action, allowed while the workspace is blocked (`native/src/actions.rs:50-58`).
- Forward `canvas_bar` through the camera-patch copy (`CanvasWindow.cpp:1119-1120`) and the mailbox (`CanvasSnapshotMailbox.h:11-17`).

**6. Input**
- `WorkspaceGestures` sends `canvas:false` contacts (`WorkspaceGestures.cpp:242-252`).
- The bar needs a non-null Background so empty space hit-tests, no `gestures->Source`, and `AllowFocusOnInteraction(false)`/`IsTabStop(false)`. Otherwise `Key()` treats Enter and Space as button keys (`CanvasWindow.cpp:690-704`).

**7. Delete**
- `WorkspaceView.cpp:105,135-146,318,386-391,732-736`; `README.md:442-446`. No test uses its IDs (checked by grep).

**8. Tests**
- `cargo test -p layer-windows`.
- UI Automation scripts, with `RowPointerDriver.cs` for mouse (SendInput), touch (InjectTouchInput) and pen (InjectSyntheticPointerInput).
- `exercise-transparency.ps1` never switches theme; add light and dark.

**Files:** new `CanvasActionBar.h/.cpp` (add to the vcxproj); `ToolbarComponents.*`; `NativeMenus.h`; `ToolView.cpp`; `WorkspaceView.cpp`; `CanvasWindow.cpp`; `CanvasSnapshotMailbox.h`; `native/src/actions.rs`; new `exercise-canvas-bar.ps1`.

---

## Cross-host summary

**Order of work**
1. **Shared Rust**, with the placement context only:
   - `CanvasBarView`;
   - the fitter and placement;
   - the hide rules, adding workspace drags;
   - the bar `DrawerAnchor` and `ChromeFacts` bounds;
   - the renderer repaint diff.
2. **GTK**, deleting `PlacementActions` in the same change (`docs/COMMIT_GUIDE.md:35`).
3. **Web and Android.**
4. **Apple, then Windows**, following `docs/APPLE_PORTING_GUIDE.md:12-16` and `docs/WINDOWS_PORTING_GUIDE.md:4`.

Each host deletes its placement bar in the same change that adds the bar.

**What every host needs from Rust**
1. **`CanvasBarView` in state:** context token, items (`ToolOption`), `visible`, and placement `{bounds, fields[], more}` in logical pixels. It rides the camera-only patch (`snapshot.rs:102-107`). Hide/show should also go out in `InputReply` next to `chrome_hidden` (`interaction.rs:104-113`), so pen-down needs no snapshot.
2. **`UiAction::MeasureCanvasBar{context, sizes, more, gap}`:** transient, deduplicated, allowed while blocked. GTK can call the pure function directly, as it does for drawers.
3. **A bar fitter:** one row, trailing completion items that never overflow, More before them.
4. **Placement against the non-Zen layout,** so the bar does not jump when chrome reveals.
5. **`ChromeFacts.canvas_bar` plus a bar `DrawerAnchor`,** so More can toggle its drawer.
6. **Menu models:** `selection_menu` (`selection_masks.rs:226-231`) plus an overflow menu with Hide Bar and Placement ▸.

**Risks**
- **Glass republication cost:**
  - Hiding at pen-down forces a region change, which repaints all glass bounds (`backdrop_blur.rs:510-514`).
  - Reappearing recomputes one new region (`docs/ui/panel-transparency.md` Cost).
  - Host costs on each change: a snapshot walk and wake (GTK), a forced layout (Web), a JSON send and wake (Android), an FFI call (Apple), a LayoutUpdated pass and JSON (Windows).
  - While the bar is stationary, none of these runs.
- **Z-order:** drawers, the header and popovers stay above the bar. The Web status toast and the Windows `zenCapy` can collide with it.
- **Zen:** More pins chrome visible. Fix the Windows `:771` bug first. The bar ignores edge reveal, but a bottom-edge bar can trigger the hover reveal.
- **Focus:** Android's focusable menus cancel transforms. GTK needs `focus_on_click(false)`; Windows needs `AllowFocusOnInteraction(false)`; Web needs `mousedown` `preventDefault`; Apple must not change key-window status.
- **Pen latency:**
  - Windows Independent Flip was never shown to engage (`docs/development/windows-pen-latency-20260920.md:147-150,209-210`; `docs/history/windows-vulkan-presentation-20260922.md:213-214`). A bar over otherwise uncovered canvas in Zen or fullscreen, plus the collapse at pen-down, is the untested case.
  - Android shared-buffer composition with an overlay has not been measured.
  - Measure with the Android front-buffer benchmark, `web-pen.mjs`, the Windows pen-latency workload and Apple `CAPY_WORKLOAD` before enabling the bar by default.

**Tests to add on every host** (mouse, touch and pen)
- A tap on the bar never paints.
- A canvas contact never dismisses it.
- A transform survives a bar tap and an opened More menu, with window focus checked.
- The bar hides during strokes, handle drags and camera gestures, and returns once.
- The glass region count rises by one while shown and falls back when hidden.
- Screenshots in light and dark at Off and High.
- Zen visibility, and a narrow width.
- The migrated Original Size / Cancel / Apply journeys.
