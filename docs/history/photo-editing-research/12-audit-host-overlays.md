# Audit: host support for a floating canvas bar

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-26 · baseline `5eb45a47`

Read-only implementation audit made by an agent against baseline `5eb45a47`. It checks how GTK, Web, Android, Apple and Windows layer widgets over the canvas, which existing floating widgets are precedents, how input and camera updates reach them, and the risks for pen latency. The report calls the component a "contextual canvas bar"; the [research record](../photo-editing-research.md) names it the **canvas action bar** and incorporates the findings in section 5. Line numbers can drift in later commits; verify before relying on one.

---

Paths are relative to `the repository root`. I ran one read-only audit per host in parallel and spot-checked the lines that matter most myself. Nothing was edited.

**Bottom line:**
- **Every host already has a non-modal bar over the canvas.** It is the image-placement Original Size / Cancel / Apply bar. It shows while `placement_original_size` is enabled, sits bottom-centre, uses an opaque fill, is not hidden by Zen, and is **not anchored** to the object. Generalizing it (an anchor plus Tool Options rows) is the cheapest route.
- **Popup trap (all hosts):** any host popup that sets `ChromeFacts.popup_open` makes the next canvas contact dismiss everything and be consumed instead of drawn (`crates/layer-ui/src/session.rs:933-934,1222-1225`). It also hides the brush cursor (`session.rs:634`).
- **Camera lag (all hosts):** the camera reaches host UI asynchronously everywhere, so a bar that tracks the camera live will trail the canvas by 1–2 frames.

## What shared Rust already provides
- **Form data:** `ToolbarComponentView{context, options: Vec<ToolOption>}` (`crates/layer-ui/src/toolbar_components.rs:64-69,117-135`). Stale edits are rejected by the `ToolbarContext` guard (`:51-59`). The fitter `tool_options_layout` always reserves a **More** button (`:363-431`).
- **Transform box, handles and selection outline** are renderer `CursorSegment`s in logical viewport pixels (`crates/layer-render/src/lib.rs:31-47`). They are computed from `camera.view().document_to_surface` (`operation.rs:532-572`, `art_layers.rs:2000-2010`, `camera.rs:100,159`), so Rust can compute an anchor rect with the same math.
- **Things to avoid:** `ResolvedLayout.work_area`, `status` (the HUD) and `groups` (`layout.rs:1366-1379`).
- **"Host measures, Rust places" pattern:**
  - `drawer_placement` (`drawers.rs:780-791`).
  - `DrawerDismissal::Explicit` vs `OutsideContact` (`drawers.rs:70-73`; `session.rs:859-862`). The compact colour-picker drawer is already Explicit (`drawers.rs:428-441`).
- **Selection menu model:** `selection_masks.rs:226-231`. An optional floating selection action bar is already specified as "Next" work: drag-handle rules, a View toggle, and "do not move it during a stroke" (`docs/ui/selection-command-inventory.md:142-151`).
- **Zen:** hides the header and docked chrome but keeps floating panels (`docs/ui/shared-ui.md:250-252`).
- **Glass:** hosts publish rects and the renderer draws the blur behind them (`docs/ui/panel-transparency.md:22-66`). Popovers and menus stay opaque (`:243`).

## GTK (`apps/layer-linux/src`)
1. **Embedding.**
   - The canvas widget is an empty `gtk::Picture` (`workspace.rs:1005-1013`). The pixels come from an app-owned `wl_subsurface` with an empty input region, `place_below` and `set_desync` (`wayland.rs:370-377`).
   - `DockSurface` allocates absolute `Slot`s for Canvas, Header, Status, Group, Drawer and so on from the shared layout (`workspace.rs:542-554,226-232,295-298`).
   - A `gtk::Overlay` wraps it (`workspace.rs:1081-1084`). Its overlay children are the Zen Capy button, notices, the drop label and `PlacementActions` (`:1085-1114`).
   - So there are two places a bar could go: a child of that Overlay, or a new `DockSurface` Slot.
2. **Precedents.**
   - `PlacementActions`: a toolbar card, centred at the bottom with a 40px margin. Shared state drives its visibility and it has no dismissal (`tool_panels.rs:14-58`).
   - Slider stamp preview: a Popover with `set_autohide(false)` (`toolbar_components.rs:703-705`). A window-level capture controller closes it on an outside press, Escape or focus loss (`:874-935`).
   - Pen tooltip: a Popover with autohide off and `can_target(false)` (`tooltips.rs:277-286`).
   - Command bar: a `gtk::Popover` with default autohide, so it grabs input (`command_bar.rs:29-33`). It is registered with `watch_popover` (`:94-95`). Watched popovers are closed on `dismiss_popups` (`workspace.rs:1786-1796`) and the stroke is denied (`input.rs:126-128`).
   - The colour-picker loupe is drawn by the GPU; only the hold timing is native (`color_picker.rs:197-255`).
3. **Tool Options renderer.**
   - `ComponentBody::measure` returns `(0,0,-1,-1)` (`toolbar_components.rs:116-118`), so a floating host must size it explicitly.
   - It calls `tool_options_layout` directly (`:193-200`); GTK does not use `toolbar_transport`.
   - Rows are built in `add_option` (`:1047-1270`) and are reusable.
   - Dock coupling:
     - `Component::new` needs a `Panel` and a `ToolbarTile` (`:400`).
     - It installs drag and context handlers (`:422-429,455`).
     - More dispatches `ActivateTile` (`:479`).
     - Orientation comes from `set_presentation` (`:265-295`).
4. **Input.**
   - All canvas controllers sit on `workspace.area` (`input.rs:97-345`).
   - Pen goes through normal GDK picking; `tablet_input.rs:1-35` only reads device metadata. A widget above the canvas therefore receives pen, touch and mouse inside its bounds.
   - An active stroke keeps its implicit grab (`input.rs:132`).
   - Hover-leave clears the GPU cursor (`input.rs:195-199`). Pen hover tooltips already use a window-level capture controller (`tooltips.rs:47-111`).
5. **Camera.**
   - The camera lives on the main thread in `UiSession`. The HUD updates on `regions::CAMERA` (`workspace.rs:2605-2609`).
   - The render worker copies `packet.view` on the main thread (`render_thread.rs:736-745`).
   - Surface coordinates are device pixels, so divide by the scale factor (`input.rs:173-178`).
6. **Selection Actions.** GTK has no equivalent. This is gap T-20 (`docs/history/photo-editing-research.md:1563`).
7. **Zen and glass.**
   - Zen fades `DockSurface` children except those with the `floating-panel` class (`workspace.rs:1837-1854`).
   - Glass regions are collected only from `DockSurface` children (`workspace.rs:370-377`; `glass.rs:139-150`).
   - So an Overlay child gets neither Zen hiding nor glass, while a Slot gets both.
8. **Risk.**
   - The toplevel already composites above the desynchronized subsurface, so one more widget leaves the pen path unchanged.
   - Because the subsurface presents independently (`wayland.rs:376-377`), the bar lags by at least one frame.
   - Changing glass regions wakes the renderer (`workspace.rs:379-390`).
   - A Popover is a separate `xdg_popup` surface.

## Web (`apps/layer-web`)
1. **Embedding.**
   - Structure: `main#workspace > #center > canvas#canvas`, with the header and `#canvas-status` as siblings (`index.html:18-37`).
   - `#workspace` is position:relative with overflow:clip (`style.css:113`). The canvas has `touch-action:none` (`:147`). The header is at z-index 1000 (`:148`).
   - WebGPU uses the default configuration with no desynchronized context (`src/lib.rs:660-683`).
   - There is no dedicated overlay layer. `#workspace` already hosts absolutely placed children: floating groups at z-index 100+2i (`app.js:557`) and drawers at 1800/1900 (`workspace-chrome.js:201`).
   - The `place()` helper always queues a glass re-measure (`app.js:453-461`).
2. **Precedents.**
   - Image placement controls: an `aside` fixed to `body`, bottom-centred (`image-import.js:6-9`; `style.css:902-910`). Not glass, not Zen-aware.
   - Slider preview: `popover='manual'`. It flips and clamps to the viewport (`toolbar-components.js:60-99,78-82`) and closes on a capture-phase outside pointerdown (`:127-129`).
   - Context menu: a manual popover with the `positionPopup` clamp (`customization.js:23-28,95-99`).
   - Command bar: a modal `<dialog>` (`command-bar.js:3`).
   - **Trap:** any open `dialog`, `details` or `:popover-open` other than `.hover-tooltip` sets `popup_open` (`app.js:1237-1238`). A canvas contact is then handled and stopped (`app.js:1308-1312`).
3. **Tool Options renderer.**
   - `createToolbarComponent` (`toolbar-components.js:7`) is created only from `customization.js:221`.
   - It depends on the tile id and `tile_style` (`:13-15`), the More → `activate_tile` dispatch (`:16`), and the `target`/`draggable` hooks (`:11,18`).
   - `layoutComponent(bounds, axis)` (`:308-312`) measures `scrollWidth` and asks Rust for `options_layout` (`:271-291`).
   - It can be reused with a synthetic tile, no-op drag hooks, and a `place` that skips glass.
4. **Input.**
   - Canvas listeners and `setPointerCapture` are on the canvas itself (`app.js:1454-1457,1476-1490`).
   - A window capture-phase pointerdown sends `canvas: e.target===canvas` (`:1289-1317`).
   - Overlays block the canvas purely by hit-testing. The GPU cursor clears when `elementFromPoint` is not the canvas (`:300-308`).
   - Pen drags over scrollable DOM become scrolling (`pen-scroll.js:5-17`).
5. **Camera.**
   - Camera region 32 triggers `state.camera=app.camera()` (`app.js:286-290`).
   - JS never sees `document_to_surface`, so Rust must send the anchor.
6. **Selection Actions.** `editor-panels.js:116` calls `selectionUi.menuButton` (`selection-masks.js:3-11`). That queries `app.selection_menu` (`src/lib.rs:361`) and renders the result as a manual popover (`customization.js:86-94`).
7. **Zen and glass.**
   - Zen CSS hides `.chrome` and non-floating groups (`style.css:461-463`).
   - Glass is a fixed selector list measured with `getBoundingClientRect` (`glass.js:1-11,18-31`).
8. **Risk.** Moving a DOM node on every camera change costs style and layout work, plus a glass re-measure when done through `place()`. Use `transform: translate` instead. The web pen doc notes that DOM overlays matter for latency (`docs/development/web-pen-huion-2026-09-20.md:121-135`).

## Android (`apps/layer-android/app/src/main/java/art/capycanvas`)
1. **Embedding.**
   - The canvas is a `SurfaceView` (`CanvasSurfaceView.kt:22-24`), hosted in an `AndroidView` inside the workspace `BoxWithConstraints` (`Workspace.kt:236-251`).
   - Presentation uses Vulkan `SharedDemandRefresh` with frame latency 1 (`native/src/android.rs:315-320`), switching to Fifo while the view changes (`:430-448`).
   - Chrome elements are absolutely placed siblings (`Modifier.placed`, `Workspace.kt:63-65`). Z bands: docked 100+, collapsed columns 160, dividers 170, floating 180, drawers 200/220, header 300, Zen button 1000 (`Workspace.kt:279-298,354`; `WorkspaceChrome.kt:105,222`).
2. **Precedents.**
   - Image placement bar: a non-modal `Popup` at BottomCenter with no dismiss callback. A BackHandler cancels the transform (`ImageImport.kt:205-215`).
   - Slider preview: a `Popup` with `focusable=false` and a flip/clamp position provider (`ToolbarComponents.kt:379-387`).
   - `WorkspaceMenu` uses `focusable=!preserveContact` because a focusable popup cancels the contact (`WorkspaceMenus.kt:40-45`).
   - Command search is a `Dialog` (`CommandSearch.kt:75`).
3. **Tool Options renderer.**
   - It is the else-branch of `ToolbarComponent` (`ToolbarComponents.kt:133-207`). It measures natural sizes (`:146-165`) and queries `options_layout` (`:167`).
   - More dispatches `activate_tile` (`:201-205`); the tile context menu is wired at `:170-175`.
   - The `ToolbarNumber` (`:211`) and `ToolbarChoice` (`:260`) row composables are reusable.
4. **Input.**
   - `onTouchEvent` with `requestUnbufferedDispatch` (`CanvasSurfaceView.kt:136-189,147`).
   - Compose siblings hit-test before the SurfaceView.
   - `chromeRegion` bounds (`WorkspaceInput.kt:429-433`) drive the cursor hand-off (`CanvasSurfaceView.kt:112-117`) and `canvas:false` contacts (`WorkspaceInput.kt:322-327`).
   - HOVER_EXIT sends `cursor_leave` (`CanvasSurfaceView.kt:190-205`).
5. **Camera.** There is no per-frame camera callback. `publish()` pulls updates at most every 33 ms unless the frame is idle (`CanvasHost.kt:573,592,642-648`), then posts them to the main thread (`:728-740`). The precedent for layout-only repositioning is the `offset{}` lambda (`WorkspaceUpdate.kt:46-58`).
6. **Selection Actions.** A `TextButton` (`SelectionMasks.kt:15-26`) queries `selection_menu` (`crates/layer-host/src/lib.rs:842`) and shows a focusable DropdownMenu (`WorkspaceMenus.kt:38-48`).
7. **Zen and glass.**
   - `chrome_hidden` keeps floating groups and drawers (`Workspace.kt:273-294`).
   - `Modifier.glass` registers a rect (`UiStyle.kt:114-127` → `CanvasHost.kt:139-158`).
8. **Risk.**
   - The window composites above the SurfaceView at default z-order (`Workspace.kt:252-254`), so an in-tree Compose element adds no layer.
   - A `Popup` adds a new window, and a focusable one cancels contacts (`CanvasSurfaceView.kt:95-99`; `MainActivity.kt:100-106`).
   - Unverified: an animated overlay over the shared-buffer layer might force GPU composition.

## Apple (`apps/layer-apple`)
1. **Embedding.**
   - iPadOS: `MetalCanvas` is a `UIViewRepresentable` whose layer class is `ObservedMetalLayer` (`iOS/Canvas/MetalCanvas.swift:5,13`).
   - macOS: an `NSViewRepresentable` (`macOS/Canvas/MacMetalCanvas.swift:5,30-35`).
   - Both share one root `ZStack` (`Shared/Editor/EditorView.swift:20-59`) holding, in order: canvas, header (`:27`), camera HUD at `layout.status` (`:28-34`), `WorkspacePanels` (`:35`), `PhotoPlacementControls` (`:49-51`), and indicators placed at `work_area` (`:54-59`).
   - `.placed(bounds)` in the `editor-workspace` coordinate space (`EditorStyle.swift:63-66`) is the existing absolute-placement pattern.
2. **Precedents.**
   - `PhotoPlacementControls` (`EditorView.swift:171-196`): non-modal and opaque.
   - Slider preview: anchored and clamped (`ToolbarComponents.swift:304-339`), but every canvas touch calls `dismissTransients(at:nil)` (`iOS/Input/PencilInput.swift:67`; `macOS/Input/MacInput.swift:56,88`).
   - `editorPopover` is effectively modal: a full-viewport tap-catcher plus `.isModal` (`EditorPopover.swift:76,85`).
3. **Tool Options renderer.**
   - `ToolOptionsComponent` (`ToolbarComponents.swift:358-411`) depends on the dock tile size and style, `options_layout` (`:373-375`), More → `activate_tile` (`:423-425`), and `WorkspaceDrag` sources (`:378,433`).
   - `ToolOptionField` (`:437-471`) is private but needs only the store, the component, the icon size and orientation.
4. **Input.**
   - iPadOS: touches arrive on `CanvasView` (`MetalCanvas.swift:202-206`). Pencil hover uses `UIHoverGestureRecognizer` (`PencilInput.swift:199-224`). SwiftUI overlays win hit-testing; non-canvas touches are only observed (`iOS/Platform/ChromeContact.swift:36-41`).
   - macOS: NSView mouse and tablet overrides (`MacMetalCanvas.swift:148-167`). Hover hit-tests so brush hover stops under chrome (`MacInput.swift:113-119`).
5. **Camera.** Rust sends camera-only patches (`crates/layer-host/src/snapshot.rs:102-107`), applied at `EditorSnapshotState.swift:58-65`. They are published asynchronously on the main queue (`EditorStore.swift:103,157-162`).
6. **Selection Actions.** A plain `Button` (`SelectionControls.swift:57-78`) queries the menu and shows it through `editorPopover` (`:64,73-75`).
7. **Zen and glass.**
   - Zen hides docked chrome and keeps floating groups (`WorkspacePanels.swift:19,36,51`).
   - Glass is a palette fill plus `GlassRegistration`, flushed through an async hop (`GlassSurface.swift:18-31,42-55`).
   - Popups use the opaque `EditorPopupSurface` (`EditorPopupSurface.swift:3-17`).
8. **Risk.** The Metal layer is opaque with `presentsWithTransaction=false` (`MetalCanvas.swift:45,47`) and presents Fifo at latency 2 (`native/src/metal.rs:292-293`). SwiftUI chrome already covers the canvas, so the bar adds no new presentation path.

## Windows (`apps/layer-windows`)
1. **Embedding.**
   - WinUI 3 with C++/WinRT, XAML built in code (`CapyCanvas.vcxproj:14`).
   - The D3D12 swap chain lives in a `SwapChainPanel` (`native/src/host.rs:124`) and prefers Immediate present mode (`:291-297`).
   - Root `Grid` order: `SwapChainPanel` at index 0 (`CanvasWindow.cpp:128,133`), then the WorkspaceView XAML `Canvas` (`:344`; `WorkspaceView.h:14`), then the header (`:360`).
   - The workspace Canvas has no background, so empty areas pass input through to the canvas (`WorkspaceView.cpp:48`). It is the overlay layer.
   - Z-indices: floating groups 100+2·order (`:352`), drawers 200/220 (`WorkspaceDrawers.cpp:44,57`), `placementBar` 1000 (`WorkspaceView.cpp:142`).
2. **Precedents.**
   - `placementBar`: a `Border` with the opaque panel brush and a ThemeShadow (`WorkspaceView.cpp:135-146,391`), placed bottom-centre 48px above the edge (`:732-736`), with no light-dismiss.
   - Command search: a `Popup` with light-dismiss enabled (`CommandSearch.h:73,167-171`).
   - **Trap:** `TrackPopup` flyouts increment the popup count (`NativeMenus.h:42-45`), which sets `menuOpen` (`CanvasWindow.cpp:1192-1195`). The next canvas contact is then consumed (`:746-750`).
3. **Tool Options renderer.**
   - `ToolbarComponent` takes panel and tile JSON plus gestures (`ToolbarComponents.h:7-8`). `Layout` calls `options_layout` (`ToolbarComponents.cpp:219-253`).
   - Dock coupling: tile identity (`:109-113`), gesture registration (`:117-126`), More → `activate_tile` (`:146-149`).
   - Edits go through `context`, not the tile (`:150-152`), so the field builders are reusable.
4. **Input.**
   - Canvas pointer input comes from `CreateCoreIndependentInputSource` on a separate dispatcher (`CanvasWindow.cpp:475-476`). XAML above it wins hit-testing.
   - `PointerExited` sends `cursor_leave` (`:518-521`). Chrome hover goes through `root.PointerMoved` (`:151-156`).
5. **Camera.** Camera patches are merged in a mailbox and posted to the UI thread (`CanvasWindow.cpp:1120,1140-1144`). Today they only drive the zoom label (`WorkspaceView.cpp:777-779`).
6. **Selection Actions.** Present: a `MenuFlyout` filled from application menu `select` (`ToolView.cpp:211-229`), added when a selection tool is active (`:354`).
7. **Zen and glass.**
   - Zen hides only non-floating groups (`WorkspaceView.cpp:350`).
   - Glass rects go out through `PublishGlass` (`CanvasWindow.cpp:302-310`). To get glass, the bar would have to add its rect in `glass()` (`WorkspaceView.cpp:770-775`).
8. **Risk.** Independent Flip/MPO depends on overlapping content (`docs/history/windows-vulkan-presentation-20260922.md:213-217`) and was never proven for pen (`docs/development/windows-pen-latency-20260920.md:149,209-210`). Chrome already overlaps the canvas, but the effect of a moving overlay has not been measured.

## Cross-host summary

**Recommended implementation per host (none of these use a popup):**
- **GTK:** a new `DockSurface` Slot, which inherits Zen and glass handling and shares the layout allocator. Not a Popover.
- **Web:** an absolutely positioned child of `#workspace`, without the `popover` attribute, moved with `transform: translate` from the region-32 handler.
- **Android:** an in-tree Compose `Box` using an `offset{}` lambda and `chromeRegion`. Not a `Popup`.
- **Apple:** a view in the root `ZStack` using `.placed`. It must stay outside `dismissTransients` and not use `editorPopover`.
- **Windows:** a `Border` child of the workspace `Canvas`, like `placementBar`.
- **All hosts:** add a Tool Options constructor that needs no tile, drag hooks or panel. Its More button needs a non-tile drawer anchor or a menu.

**Shared data the hosts need from Rust (a proposed `CanvasBarView`):**
- Kind (selection, transform, placement or crop) plus the `ToolbarContext`.
- `Vec<ToolOption>` and command actions.
- The anchor rect in logical surface pixels, using the same math as `append_transform_overlay` (the axis-aligned bounds of a rotated quad).
- The final position, computed by Rust from host-measured size in the `drawer_placement` style, avoiding `work_area`, HUD, groups and the contact point.
- A frozen/hidden flag while a contact or handle drag is active (Rust already tracks `interaction.pointer`), optionally also during camera gestures.
- Zen and glass policy.
- The anchor should ride in the camera-only patch (`snapshot.rs:102-107`, and Web region 32).

**Main risks:**
1. The popup and transient dismissal paths consume canvas taps on every host, so the bar must never set `popup_open`.
2. There is a 1–2 frame camera lag on every host. Hiding the bar during navigation, or accepting the lag, avoids jitter.
3. Glass makes the lag worse: glass rects re-register asynchronously and trigger re-measures or renderer wakes. Prefer opaque, popup-style surfaces.
4. Pen-latency impact on Windows (Independent Flip) and Android (shared buffer) is unmeasured.
5. A movable bar must follow the drag convention: an explicit handle, and a hold before dragging tile bodies.
6. GTK still lacks a Selection Actions button (gap T-20).
