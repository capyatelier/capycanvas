# Workspace window-bar builder

Base: `008b646` (fetched origin/main). Previous prototype is preserved on
`recovery/unified-header-39e1daa`. No pushes. GTK implementation and acceptance
checks are complete; ready for user review within the tested scope below.

## Design

- The window bar owns individual, stable-ID items in left/center/right regions.
  It is not a dock group and cannot swallow, hide, or float toolbar containers.
- Shared Rust owns configuration, validation, editing, tool semantics, drawer
  origins, overflow policy and workspace history. GTK owns widgets, native
  measurements and input arbitration.
- Fixed native window controls and a window-drag reservation sit outside the
  editable items. Empty caption space uses the native window handle; controls
  own their own pointer/context-menu events.
- Inline editing: visible region targets, grips, Add Items, size, canvas-info
  settings, defaults and Done. Drop outside cancels. Context menus provide
  move/order/remove alternatives. Tile bodies require hold-drag; explicit
  grips drag immediately, with native movement slop.
- One size for the whole bar. Plain tool icons, transparent backing over the
  full-window GPU canvas. Narrow windows compact the switcher and overflow
  whole items; no invisible hit targets or overlapping native controls.
- Menu labels are an independent workspace setting under Window. A menu
  button always exposes the complete application menu. Recovery remains
  accessible if every user-configurable navigation item is removed.
- Canvas-info visibility is a workspace setting, fixed at bottom right, separate from
  header settings. Painter hides zoom/rotation. Total Zen has one meaning and
  no partial-Zen preference.
- GTK is the first native projection. Other hosts retain working existing
  controls until their projection is implemented; portable data remains valid.

## Milestones

- [x] Preserve old work; reset to fetched main.
- [x] Shared configuration, history, validation, drawer/picker integration.
- [x] GTK header and inline editor, Painter defaults, full-canvas/overlay layout.
- [x] Total-Zen cleanup and host compatibility.
- [x] End-to-end GTK acceptance and release build.

## Review follow-up (September 13)

Replaced the header-only Add popup with an inline catalog reusing the toolbar
picker's GTK search/list/row component. Catalog rows and grips can add items by
dragging into any bar region; + buttons remain a keyboard/click alternative.
Native device classification preserves touch/pen list scrolling before hold.
The editor has Done/Cancel with a single shared baseline; previews are excluded
from saved captures and history. Done records the complete customization once.
Removed the canvas-info corner setting; visibility remains workspace-owned.

Restored the workspace-selector background and baseline text-menu padding/height.
Fixed hover radius, centered native icon grips, 6px inter-item gaps, square
drawer-facing tile corners and equally sized native close hit targets/padding.

Follow-up acceptance:

- Shared UI/host/native storage: 299 / 25 / 65 pass (one host GPU case ignored).
- Catalog drag/add/cancel/Done with real mouse/touch: 1×
  `/tmp/capy-workspace-motion.h6TLs9`; real 2× `/tmp/capy-workspace-motion.A1ZHgf`.
- All sizes/both themes, measured menu heights/padding, close-button centering,
  selector centering, grips, drawer state and inspected screenshots:
  `/tmp/capy-workspace-motion.utKlDc` (final panel); earlier pass
  `/tmp/capy-workspace-motion.X7AsCt`.
- 640×600 overflow/editor: `/tmp/capy-workspace-motion.JoDVFi`.
- Save/switch/restart, including closing with an uncommitted preview:
  `/tmp/capy-workspace-motion.Cq0n3J`.
- Reorder holds/context menus: `/tmp/capy-workspace-motion.DuV7Is`;
  editor controls: `/tmp/capy-workspace-motion.hSCOOf`;
  caption/cancellation/Zen: `/tmp/capy-workspace-motion.tVyMWW`;
  all header tool drawers: `/tmp/capy-workspace-motion.HEuMRo`.
- Ordinary toolbar customization and reused picker checkboxes, both themes:
  `/tmp/capy-workspace-motion.PAIkZj`. Updated stale test assumptions about
  hard-coded group IDs and the first Pencil search result being a preset.
- Native fullscreen/clock/battery: `/tmp/capy-workspace-motion.0FBgVC`;
  restored/maximized/fullscreen window movement: `/tmp/capy-workspace-motion.xDb04g`.
- Release build, five native non-GUI utility tests, and actual release launch
  with inspected 1200×900 capture: `/tmp/capy-unified-release.oFksX7`.

No physical-pen or other-host GUI acceptance is inferred from these GTK tests.

## Initial implementation acceptance (September 13)

Implemented the first shared model and GTK projection. Header items now have
their own identity/zone/size, separate from dock panels. Header tools use the
same activation and validated drawer handlers as toolbar tiles. Added native
grips and reused workspace drag arbitration. Shared layout/history strips
native header measurements. GTK has a searchable Add picker, context actions,
canvas-info options and navigation recovery. Total-Zen cleanup is in place;
legacy wire fields remain false/empty for older hosts.

Verified so far (each GTK GUI test runs in its own private Mutter/Vulkan process):

| Check | Result / artifacts |
| --- | --- |
| Shared UI / host / native workspace storage | 299 / 25 / 65 pass; one separate host GPU test remains ignored |
| Hardware-GPU ABI, serialized release run | All 11 pass (a parallel sandboxed attempt crashed; serialized hardware run is the acceptance result) |
| GTK non-GUI utilities | All 5 pass; native ignored tests are run separately below |
| Painter tools, cross-zone grip move, Undo, outside cancellation | `native_header_builder_input`: `/tmp/capy-workspace-motion.GZgxAp` |
| Mouse/touch body holds, immediate grips, held release, context/keyboard actions at real 2× | `native_header_hold_context_input`: `/tmp/capy-workspace-motion.4Agt4i` |
| All sizes, Add/search/reopen, singleton prevention, canvas-info options, defaults, remove-all recovery | `native_header_editor_controls_input`: `/tmp/capy-workspace-motion.7HoWBt` |
| Narrow 640px overflow, all sizes, hidden tool activation and persistent drawer origins | `native_header_overflow_input`: `/tmp/capy-workspace-motion.w8NWdJ` |
| White canvas under the bar, both themes, contrast inspection | `native_header_canvas_visual`: `/tmp/capy-workspace-motion.1owAVF` |
| Actual brush numeric editing, color swap, Layers menu, every content-panel drawer family | `native_header_drawer_controls_input`: `/tmp/capy-workspace-motion.6gOXDv` |
| Caption double-click, native close, Escape/blur/source removal, Tab, menu toggle, same-frame publication, Zen | `native_header_cancel_caption_input`: `/tmp/capy-workspace-motion.NEF1KX` |
| Centered visible switcher, edits, keyboard remove/focus, switch away/back, close/reopen persistence | `native_header_managed_input`: `/tmp/capy-workspace-motion.CIbV4e` |
| Ordinary toolbar/drawer regression, both themes and all edges | `native_tool_drawers`: `/tmp/capy-workspace-motion.vNzeTb` |
| Windowed/restored caption movement, maximized/fullscreen docking | `native_window_drag_input`: `/tmp/capy-workspace-motion.SywmDL` |
| Workspace manager, library, history, save/reset/duplicate workflows | `native_named_workspace_manager_library_and_history`: `/tmp/capy-workspace-motion.obj5wo` |
| Total Zen and all Capy icon choices | `native_zen_behaviors`: `/tmp/capy-workspace-motion.bDTIKJ`; `native_zen_icons`: `/tmp/capy-workspace-motion.rogGYZ` |
| Fullscreen, F11/native WM state, locale clock formats, component remove/re-add, simulated battery sizes/states | `native_fullscreen_header_clock_and_battery`: `/tmp/capy-workspace-motion.WIdi1n` |
| Web build and focused total-Zen browser regression | Build passes; `--zen`: `/tmp/capy-workspace-motion.eTui8O` |
| Regular release executable, new profile, GPU canvas and desktop menu capture | `/tmp/capy-unified-release.LeLeA1` |

The native input harness converts logical touch positions to ScreenCast physical
pixels for real 2× monitor tests. A test-only touch-coordinate error was caught
and corrected; setting GDK_SCALE alone is not the acceptance method.

The older all-preferences Web suite currently fails before its Zen checks on a
missing panel typography selector in the current default workspace. It is not
counted as passing. The independent total-Zen suite exercises both themes,
legacy-setting removal, native browser mouse/touch recovery and layout invariance.
No physical pen, Windows, macOS or iPadOS GUI acceptance is claimed.

The fullscreen battery test initially raced the machine's UPower response:
its synthetic battery disappeared while waiting for an allocation. The fixture
now explicitly disables that observer, while production still observes UPower.
The complete test passes, including medium/large indicator scaling. Desktop
menu-label padding was also verified in the regular release screenshot: File,
Edit, Layer, Select, Filter, View, Window and Help fit at the default 1200px size.

Rust formatting and diff checks are complete. The release executable is built
and smoke-tested. This feature retires the two partial-Zen implementation files;
the previous code remains in Git and on the recovery branch.

## Try without touching existing profiles

From the repository root:

```bash
preview_dir=$(mktemp -d /tmp/capy-header-review.XXXXXX)
mkdir "$preview_dir/workspaces"
CAPY_WORKSPACE_DIR="$preview_dir/workspaces" LAYER_SETTINGS_FILE="$preview_dir/settings.json" \
  dbus-run-session -- target/release/layer-linux
```

Choose Paint, then Window → Customize Window Bar….
Keep the printed/assigned profile path to reopen the same review workspace.
Existing customized workspace histories deliberately do not receive the new
defaults automatically, so a fresh profile is useful when reviewing the design.

## Acceptance (must be tested, not inferred from serialized state)

- Fresh Painter: requested left/center/right order, medium plain icons, no menu
  labels/clock/battery/zoom bubble, canvas visible behind header.
- Every header tool: activation, repeat activation/drawers, enabled/selected
  state, working drawer controls, close/reopen, menu/secondary-click behavior.
- Editor: enter/exit, all sizes, add all item types, reorder within/across
  zones, remove/re-add, context alternatives, keyboard access, defaults.
- Native mouse/touch gestures: hold vs immediate grips, cancellation, outside
  drops, release without movement, no accidental action after drag.
- Overflow: narrow/large, center positioning, accessible hidden items, no
  overlap or disappearing tools, resize with a drawer/editor open.
- Recovery with menu/Capy/switcher removed; native close remains protected.
- Workspace switching, duplicate/saved layouts, restart persistence;
  preview/cancel produce no saved revisions and Done applies the edit once.
- Overlay visibility at bottom right, menu-label component add/remove, true Zen return.
- Caption drag, double/secondary click, maximized/fullscreen, light/dark, 1x/2x.
- Shared unit/bridge tests, GTK isolated real-input tests, Web regressions,
  actual release executable and screenshot inspection. Report physical-device
  and untested-host limitations explicitly.

## Main integration — 2026-09-13

Merged `origin/main` at `6ff2201` into the GTK window-bar branch at `69bbea6`.
The repository's upstream branch is named `main`, not `master`. The customizable
header, nested shared tool picker, total-Zen policy and drawer dismissal are
retained alongside the incoming compact color picker and shared SVG icon bank.
Header controls, palette grips and tool-picker rows now use the shared GTK SVG
renderer; the header's color tile uses its live foreground/background paintable.
The customization command also has a packaged toolbar icon.

Validation passed: release GTK executable and Web/Wasm build; 434 shared tests
(337 UI, 25 host, 72 workspace; one hardware-only host test remains ignored);
and these 16 isolated native GTK cases:

- Picker journey, all drawer controls, outside drawer dismissal, caption/cancel,
  and editor controls: `/tmp/capy-workspace-motion.medFeO`, `DIcal8`, `Fz2pM4`,
  `mXj9GZ`, `6s0MI5` (later IDs share the same directory prefix).
- Both-theme/all-size spacing and SVG checks, persistence, held context menus,
  640px overflow, and 2× mouse/touch palette drag: `vhR8dn`, `nNFK25`, `Ct9AS6`,
  `EbQcvA`, `i6lhdC`.
- Compact color input, full SVG bank and 280 production-control icon checks,
  drawing/default layout, Zen icon choices, invalid-default recovery, and canvas
  behind the header: `ok94G4`, `W7Ypa3`, `YABflV`, `7U6tbi`, `x276nW`, `CYKNQT`.

Reviewed header, editor and color-drawer captures in both themes. Tests were
updated to use the new wheel's rotated hue geometry and Okhsv coordinates, and
to expect a valid saved `shape` after workspace recovery rather than no field.
The native runner now resolves a short test name to exactly one full Rust test
name: Cargo substring filtering had accidentally selected both the default
workspace and default-recovery tests. Physical pen and other native platforms
were not revalidated by this integration. The Web build is a merge check, not a
port of the GTK window-bar editor.

## GTK editor ergonomics — 2026-09-13

Replaced the full-width region strip and mixed Options/action rows with a
460px-wide inline panel. The component palette and Add Tools are at the top,
with insertion region/position immediately beside them. Selected-item actions
are separate from adding; size and canvas-info visibility follow, then Reset
Bar and adjacent Cancel/Done. Size is a labeled segmented choice, not a popup;
the former cryptic footer is replaced by explicit help on demand. The panel
measures its content and scrolls at the available window height.

Editing context menus omit redundant Customize and global Capy preferences,
disable the current region and impossible moves, and offer Done/Cancel.
Empty editable bar space has its own menu; native caption menus outside editing
and native window controls retain ownership. Context selection follows the
clicked item. Shared context popovers restore their keyboard invoker on close.

Keyboard entry establishes panel focus. F6 switches between panel and bar;
arrows/Home/End select items, Alt+arrows reorder, Alt+Shift+arrows move regions,
Delete removes, and Menu/Shift+F10 opens actions. Tab/Shift+Tab stay within the
customization surface, with native popovers and nested dialogs retaining their
own navigation. Rebuilds preserve focus; an overflowing item retains selection
in the panel. Key releases still reach shared shortcut bookkeeping so repeated
Ctrl+Shift+U entry works after Done/Cancel. Escape cancels drag/menu before preview.

Validation: release GTK build; 435 shared tests (338 UI, 25 host, 72 native
workspace; one hardware-GPU host case ignored). Thirteen isolated native runs
passed, with no invalid header measurements or GTK criticals in their test logs:

- Component hold/grip mouse/touch input: `wiS86W`; same at 2× scale: `4KhT60`.
- Picker and nested cancellation: `TAdGx5`; held context menus: `DQwBmA`;
  caption/Zen/cancellation: `aUJID8`; editor controls/defaults: `AcwdtB`.
- Extended keyboard/context/focus/reopen journey: `iXLHze` (including the final
  labeled Help button); minimum 640×480
  window with every component available and keyboard Done access: `9dLgN4`.
- Managed workspace persistence/restart: `uDtngz`; 640×600 overflow: `jpT0iQ`.
- Drawer dismissal on bars/toolbars: `w9nKCP`; actual drawer controls: `x5G7At`.
- Both themes/all sizes, shared SVGs and spacing: `PHJDke`.

IDs above are under `/tmp/capy-workspace-motion.<ID>`. Reviewed the actual
light/dark editor captures, narrow overflow layout, and minimum-window Done
capture. The app's existing minimum window height is 480px; the short-window
test uses that supported minimum. These checks use real GTK mouse, virtual
touch and keyboard input under private Mutter, not physical pen hardware or
other hosts. No Web editor port, workspace reset, or remote push in this change.

## Simplified window-bar palette and live drag — 2026-09-13

Supersedes the editor controls and shortcuts described in the ergonomics entry
above. The 420px inline panel now contains only the wrapping component palette,
bar size, bottom-right canvas-readout visibility, and Cancel/Done. Add Tools is
a palette component: clicking or dropping it opens the existing modal toolbar
tool picker. There are no region buttons, selected-item action row, Help, Reset,
or dedicated opening/focus/movement shortcut combinations. Left/Right moves the
selected bar item, including across region boundaries; Delete/Backspace removes
it. Native Tab, button and context-menu navigation remain available.

Removed the separate Show Menu Bar state/action as well as its menu entry;
Menu Labels is simply a component to add/remove. All visible entry points say
Customize Window Bar. The Window menu puts this entry first so recovery remains
reachable at the minimum window height. Space has exactly one tile of width,
with the normal 6px inter-item spacing and an additional grip only while editing.
Defaults now use Sketch/Paint/Photo and include Full Screen and Settings on the
right. Existing saved names/layouts are not reset or migrated.

The shared HeaderDrag policy uses the existing TabDrag frozen-slot algorithm
for horizontal movement. GTK retains/snapshots the actual item widgets, animates
neighbors, and preserves the original grab offset. Half a tile beyond the bar
detaches the item; an outside release removes it, returning singleton components
to the palette. Re-entering reattaches the same contact. Motion never publishes
session changes; release produces one edit, and editor Cancel restores its full
starting state. Hidden overflow neighbors are captured without reparenting and
remain intact after removal/cancellation. Native hold/slop/device arbitration
still follows the application drag convention.

Native testing also found and fixed missing keyboard focus when reopening the
retained Settings dialog. Constrained-popover test coordinates now use the
actual GdkPopup position rather than its unpositioned widget allocation.

Validation passed: release GTK executable and isolated executable smoke capture
(`/tmp/capy-header-release.957uAV/release.png`); 438 shared tests (341 UI, 25 host,
72 native workspace; one hardware-specific host case ignored); and 17 isolated
native runs with no GTK criticals or rejected header measurements:

- Live slide/tear-off/re-entry/removal with mouse and touch: `o5i2xv`; 2×: `c5oMQb`.
- Palette Tools drop/modal handoff: `XI7iwa`; click/search/multi-select: `qxPWQG`.
- Catalog hold/grip/cancel: `V9JzY5`; context holds: `zXtnXD`.
- Keyboard/region boundaries/removal: `fL9BGu`; all editor controls: `Qb4HEu`.
- Managed save/switch/reopen: `d39qsy`; caption/Zen/cancel/blur: `nIQbyS`.
- Settings/fullscreen/reopen at all sizes: `DuhDkM`.
- 640×600 overflow activation/picker: `vK3AHb`; overflow drag at all sizes with
  mouse/touch, hidden entries, removal and Cancel: `bnEUHn`.
- 640×480 full-palette/footer/recovery entry: `4K3CLH`.
- Both themes/all sizes, one-tile Space, gaps, corners and grips: `LbOqpz`.
- Drawer dismissal on bar and toolbars: `QcOA9o`; drawer controls: `HFbVjG`.

Native IDs are under `/tmp/capy-workspace-motion.<ID>`. Inspected the actual
default bar, light/dark editor, sliding and detached ghosts, narrow overflow
drag and minimum-window captures. These tests deliver real GTK mouse, virtual
touch and keyboard input under private Mutter; physical pen and other hosts
were not revalidated. Nothing was pushed and no existing user profile was wiped.
