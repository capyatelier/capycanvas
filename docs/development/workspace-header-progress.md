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

Choose Painter, then Window → Customize Workspace UI… or Ctrl+Shift+U.
Keep the printed/assigned profile path to reopen the same review workspace.
Existing customized Painter histories deliberately do not receive the new
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
- Overlay visibility at bottom right, menu-label toggle under Window, true Zen return.
- Caption drag, double/secondary click, maximized/fullscreen, light/dark, 1x/2x.
- Shared unit/bridge tests, GTK isolated real-input tests, Web regressions,
  actual release executable and screenshot inspection. Report physical-device
  and untested-host limitations explicitly.
