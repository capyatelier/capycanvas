# iPhone phone layout

[Developer guide](README.md) · [Phone layout](phone-layout.md) · [Apple development](apple.md) · [Apple porting guide](../APPLE_PORTING_GUIDE.md)

This plan covers the iPhone part of [phone layout](phone-layout.md) milestone 4:
the Apple client presents the shared phone layout on iPhone, and in narrow iPad
windows. GTK, Web and Android already present it. Delete this plan when the work
lands, after moving any lasting decisions into the guides.

The design is settled in [phone layout](phone-layout.md). This plan does not
restate it, and it leaves the Swift and UIKit structure to you. What it fixes is
the bar for calling the port done, the references you compare against, and the
process and pitfalls that kept the Web and Android ports correct.

## What already exists

- **Shared Rust decides everything.** `crates/layer-ui/src/phone.rs` covers the
  frame, contacts, the Capy menu routes and rows, and the menu geometry.
  `dialog_presentation.rs` covers pages, alerts, canvas panels, the header and
  escape. Drawer placement, colour fit and presented rows live in `drawers.rs`
  and `drawer_geometry.rs`; notices in `notice_placement.rs`; Draw with Finger in
  `finger_input.rs`.
- **The native transport is in `crates/layer-host`. Apple uses it already:**
  - **Snapshot fields:** `layout.phone`, `state.phone`, `phone_tools`,
    `phone_settings`, `phone_sliders`, `capy_caption`, `presented_layer_rows`.
  - **Stateless calls:** `layer_ui::phone_ui`, holding serializable bar and menu
    contacts plus menu width, geometry and placement; and `layer_ui::dialog_ui`,
    for header, escape, alert layout, floating width, pane fit, menu
    presentation and `phone_window`.
  - **Host queries:** drawer frames and measurement, dialog presentation and
    bounds, command search placement and the tool panel projection.
  - **Document window:** `TabRequest::PhoneMenu`.
  - **Device class:** `NativeHost::set_device_class`.
  - **Rounded corners:** `PhoneAction::Metrics` carries `corners`. iOS publishes
    no corner radius, so send zeros and let the safe area do the work.
- **The capability gate** is `Platform::presents_phone_layout`
  (`crates/layer-ui/src/settings.rs`). Add `Ios` in the same commit that first
  draws the frame, never earlier; without the frame, a phone-size window
  resolves an empty layout.
- **iPad only today.** The Xcode targets build for iPad only
  (`TARGETED_DEVICE_FAMILY = 2`). Adding the iPhone device family is part of
  this work.

### Reference implementations

| Area | GTK (behaviour authority) | Web (visual reference) | Android (closest native host) |
| --- | --- | --- | --- |
| Frame, bars, contacts | `workspace_phone.rs` | `phone.js` | `Phone.kt` |
| Capy menu and swipe back | `workspace_phone_menu.rs`, `phone_menu_swipe.rs` | `menu.js` | `PhoneMenu.kt` |
| Drawers | `workspace_drawer.rs`, `tool_panels.rs` | `workspace-chrome.js`, `editor-panels.js` | `WorkspaceChrome.kt`, `ToolPanels.kt` |
| Dialog pages, alerts, canvas panels | `page_dialog.rs`, `page_dialog_metadata.rs` | `dialog-host.js`, `page-kit.js` | `DialogShell.kt` |
| Capture visitor | `phone_tests.rs`, `page_dialog_tests.rs` | `phone-audit.mjs` (`--phone-*` journeys) | `AndroidPhoneAuditTest.kt`, `tools/devices/android-emulators.py` |

GTK paths are under `apps/layer-linux/src/`, Web under `apps/layer-web/` and
Android under `apps/layer-android/app/src/`.

## References and how to compare

- **Web is the primary visual reference.** It was audited sheet by sheet
  against GTK. It renders at any viewport, so it can be captured at exactly
  the iPhone sizes you test. It also runs on the Mac you build on; GTK capture
  needs Linux.
- **GTK is the authority on behaviour and content.** When Web and GTK
  disagree, follow GTK and record the Web difference.
- **Android is the reference for native system integration:** insets, gesture
  strips, keyboard avoidance and modality.

Capture Web references with the phone audit journeys
(`docs/development/web.md`, `--phone-audit`, `--phone-frame`, `--phone-menus`,
`--phone-drawers`, `--phone-input`, `--phone-dialogs`). Add the iPhone sizes
to `phoneAuditMatrix` in `apps/layer-web/phone-audit.mjs`. Pair the images with
`tools/visual/phone_audit.py --reference <manifest> --candidate <captures>`,
which wraps `tools/visual/compare.py`. Key the reference manifest by family,
state, size, theme, text size and fixture, never by file name alone.

### Comparison classes

Every sheet gets exactly one class, recorded after you open it:

| Class | Meaning |
| --- | --- |
| SAME | No visible difference beyond rasterization noise. |
| N | An intentional native difference with a stated reason. |
| R | Rendering only: font rasterization or text metrics, with geometry and content right. |
| STATE | The fixture or state differs. Fix the visitor, not the product. |
| DEFECT | Anything else. It is fixed or explicitly deferred with a reason. |
| INHERITED | A shared limit that every host has, such as alerts at least 300 wide. |
| MISSING | No reference exists. Open and review the iPhone image anyway. |

Geometry, content, order, state, reachability and behaviour must match the
reference. Native control rendering may differ, and is N:
- UIKit and SwiftUI controls, SF fonts and the system accent colour;
- native menus and pickers where the iPad app already uses them;
- the uniform shift of the bars and bar-anchored chrome by the reported safe
  area. Web references can carry the same insets if your Chrome build supports
  safe-area emulation over CDP; otherwise this shift is N.

Never restyle the existing iPad UI to match. "Paired" means opened and
classified, never just captured.

## Definition of done

### Surfaces and journeys

Every surface below is reachable on iPhone, captured, paired where a
reference exists, and classified. These families mirror the GTK, Web and
Android visitors:
- **Frame:**
  - rest in Sketch, Paint and Photo;
  - swapped sides;
  - hidden bars, which keep a visible Capy chip;
  - Size and Strength sliders held, pulled away and returned, with their
    preview;
  - Capy held and dragged to swap;
  - empty, ten-tool and divider bars;
  - the startup cover, GPU failure and Restart.
- **Capy's menu:**
  - every route `PhoneMenuNavigation` enumerates, both swapped and not;
  - the Drawings page with several drawings, the current check, unsaved dots,
    close buttons and New drawing;
  - the strip toggles: Preferences, Search, Draw with Finger, Zen and Swap;
  - back swipe cancelled, halfway and completed;
  - a scroll that cancels a row press;
  - a release that activates exactly one row.
- **Drawers:**
  - every tool set in all three presets, plus their other-set states;
  - settings-only and mixed tool drawers;
  - the Color drawer at its budget top and bottom, with the wheel floor at 128
    and Palettes visible;
  - Layers with 2, 5 and 12 rows, capped, after a return, and with the
    properties disclosure;
  - every Window › panel drawer;
  - the bridge and selected opener on both bars, swapped and not;
  - dismissal by an outside contact and by the next stroke.
- **Customization:**
  - the tile and empty-bar hold menus;
  - Insert Tools as a page, up to and at the ten-tool capacity, including with
    dividers;
  - hold-drag reorder and off-bar cancel;
  - Restore Starting Layout, its alert, then the restored bar;
  - one layout undo step per edit.
- **Pages and alerts.** Every editor dialog is a page and every short
  confirmation an alert:
  - Preferences: every page and subpage, choice rows, shortcuts with their
    category, the modifier and pen editors, keymap details and import;
  - New Drawing, with its preset, paper and paper-colour subpages and Save
    preset;
  - Edit Color, with All Swatches and the eyedropper hiding the page while
    picking;
  - Export, with its Size, Color and Presets subpages, the preset name and the
    progress alert;
  - Canvas Size and Image Size;
  - Document Properties;
  - Assign Profile, Convert Color and Change Bit Depth;
  - the source repair, rasterize and interpret dialogs;
  - the profile library;
  - the paper picker and paper rename;
  - palette name and delete;
  - the workspace manager with its prompts and not-saved alert;
  - the toolbar manager and prompt;
  - the unsaved changes, cannot-open, recovery, preserved-package and progress
    alerts;
  - command search;
  - Refine and Frequency Separation as canvas panels, with the keyboard open;
  - pressure and tilt calibration;
  - the workspace save status beside a timed notice.
- **Canvas:** the canvas action bar and its menus, notices, the finger-hold
  loupe, and the overlays (transform, crop, selection, vectors, clone source)
  inside the phone work area.

### Sizes, themes and text

Capture the families above in both themes at:
- one current iPhone size in portrait and landscape, for example 393×852 pt;
- iPhone SE at 375×667;
- iPhone SE with Display Zoom at 320×568;
- one large Dynamic Type size, the iOS counterpart of the 320×320 16 pt case.
  Pick the content size category whose measured workspace text is closest to
  16 pt, and record the measurement.

Then run journeys, with assertions rather than full sheet pairing:
- 479↔480 in both axes on an iPad window, including transitions mid-stroke,
  mid-slider, mid-reorder and with a drawer open;
- right-to-left with and without Swap Sides;
- a resized open page;
- rotation during a stroke;
- iPad Slide Over and Split View widths, which get the phone layout with the
  device class still Tablet.

### Behaviour that images cannot show

Assert these in Rust or XCTest:
- **Draw with Finger:**
  - provisional finger strokes withdrawn by a second finger and by a hold,
    leaving no undo step;
  - Auto paints with a finger until a pen draws, survives a restart, and
    unknown or hover-only pointers never set `pen_seen`;
  - pen wins, and fingers it rejected stay ignored until they lift.
- **Undo and layout:** one layout undo step per toolbar edit; phone
  customization and panel reveal leave the desktop arrangement unchanged.
- **Pages:**
  - a page is modal: shortcuts and canvas contacts don't reach the canvas, and
    focus stays inside;
  - closing returns focus to the opener;
  - Escape and back go through `dialog_escape`, then shared Escape;
  - no page keeps a stale route or contact after it is rebuilt.
- **Keyboard:** the visible area excludes the keyboard exactly once, and a
  focused field scrolls into view without the page reserving the keyboard
  twice.

### iPad must not change

Run the existing iPad UI tests (`apps/layer-apple/iOS/Tests`) and the Apple
scripts that cover editors and workspaces on a control build of the commit
before your change. Diff the failure sets. Any new iPad failure is yours until a
control run proves otherwise.

### Gates per milestone

- The normal Rust suites: `cargo test --locked -p layer-ui -p layer-host`.
- The Apple build and tests in [Apple development](apple.md).
- The Web normal Node batches and build, plus `cargo check -p layer-linux --tests`,
  whenever shared Rust changes.
- The Windows `smoke` family for shared host or UI changes, following
  [testing](testing.md#windows-validation-scope). Attribute failures against
  main runs.

### Physical devices

The Simulator stands in for neither a pen nor edge gestures nor frame rates.
On a physical iPhone:
- no tile sits under the Dynamic Island, notch or home indicator;
- the bars' edges defer system gestures (`preferredScreenEdgesDeferringSystemGestures`);
- the slider handles and the bars don't trigger the home or Control Center
  gestures on the first swipe;
- finger painting works in Auto.

On an iPad with Apple Pencil, in a narrow Split View window:
- pen takeover;
- palm rejection until lift;
- Auto across two windows and a restart.

### Performance

The targets in [PERFORMANCE_TARGETS](../PERFORMANCE_TARGETS.md) apply to every
motion. iPhone is not a tier device. Measure the rows the phone layout touches
on the Apple reference iPad (13-inch iPad Pro, M4) in a narrow window that
presents the phone layout: painting with the bars visible, drawer open and
close, slider scrubs, tile reorder and menu swipe-back. Compare against a
matched control of main ([measuring](../performance/measuring.md)), and show
no regression. If a physical iPhone is available, record the same rows there.

## Process that worked

- **Plan, then build, in a few milestones.** For example:
  1. capture harness, frame, input and the capability flip;
  2. Capy's menu;
  3. drawers;
  4. customization, the dialog shell and its entry dialogs;
  5. the remaining dialogs;
  6. the audit, one fix round, and devices.

  Write each milestone's design against the current code with file:line
  references, and stop and report when the code contradicts the plan.
- **Shared first.** If a host needs logic that exists only in another host's
  bridge, move it into `UiSession` or a shared free function and call it from
  both. Don't copy it.
- **Separate capture from implementation.** A second agent builds the
  committed source cleanly, captures every family in every case, opens and
  classifies every sheet, and lists DEFECTs with file:line. The implementer
  fixes only the rows marked FIX. This found crashes and regressions that
  passing journeys missed.
- **Review every checkpoint diff yourself** before the next one builds on it.
  Look for:
  - host policy;
  - duplicated shared logic;
  - test-only switches;
  - explanatory comments;
  - stale contacts;
  - iPad behaviour changes.
- **Verify each code path once.** Drop a check when another passing check
  already covers the same code path, or when it needs new fixture machinery.
  Give each check two attempts, then record it and move on.
- **Run the control first.** On a product failure, run the same check on a
  clean main build before designing a fix. Several suspected regressions were
  main behaviour.
- **Bugs already on main ship alone,** each with its regression test, before
  the milestone that found them.
- **Clean builds for controls.** Incremental builds have produced broken
  binaries.

## Pitfalls seen on Web and Android

- **Text squeezed to zero at 280–320 pt.** Titles, search fields and values
  squeezed to zero width. Give titles and fields a usable minimum, and wrap or
  stack instead of shrinking to nothing.
- **Shared rules the host forgot to call.** The ports missed tool-set chip
  columns (`tool_set_columns`), preview row heights (`tool_preview`), presented
  layer rows and the context menu anchor (`menu_presentation`). Search the Web
  and Android hosts for every shared call they make, and make the same ones.
- **Nested dialogs in a scrolling container.** These measure an infinite
  height. Host page-owned sheets and alerts above the page, never inside its
  scroller.
- **A keyboard-aware floating card on a large screen.** It moves when the
  keyboard opens and breaks drags. Only phone pages and alerts follow the
  keyboard-reduced area.
- **Two cancel paths for system Back or edge-swipe dismissal.** Route it
  through the shared Escape order, and keep a single handler.
- **Click suppression after a bar hold.** It must end with that gesture, or
  VoiceOver activation stays dead.
- **Safe areas and system gesture areas.** These are native facts for the
  shared insets, not host margins. Neither Android navigation mode reports zero
  gesture insets in immersive mode. Expect the iPhone home indicator to behave
  the same way.
- **Default workspaces differ by platform.** Mobile platforms start in Sketch,
  desktops in Paint. Restore the reference fixture's workspace for every state.
- **Captures taken before the frame settles.** A capture taken before the
  brush is ready, or before a page transition settles, shows a stale state.
  Wait for the published state and an idle UI.
- **Large Dynamic Type sizes.** Expect the same clipping questions as the open
  narrow and large-text decisions in [phone layout](phone-layout.md). Report
  them; don't invent host rules.
