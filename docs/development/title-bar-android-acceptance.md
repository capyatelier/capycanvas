# Android title-bar acceptance

2026-09-13 port of the reviewed [title-bar handoff](title-bar-web-handoff.md).
Kotlin measures and renders native controls and owns focus, contact capture,
timing and slop. Shared Rust supplies the view, geometry, frozen drag previews,
validation, tool activation, picker, workspace publication and history. Web now
serializes the same shared header view.

## Device and reproduction

Wacom MovinkPad 14 (DTHA140), Android 15, arm64, landscape 2880 × 1800,
existing density override 362 dpi (2.2625 scale). The device was used after its
other agent released it. Installation used `adb install -r`; no app storage was
cleared. Tests use isolated SQLite and raster-recovery directories and restore
the original preferences. An isolated x86_64 tablet emulator also exercised
bank/body/grip pickup and menu compaction during implementation.

Build/install instructions are in [Android development](android.md#focused-device-tests-and-debugging).
Run the suite with:

```sh
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w \
  -e class art.capycanvas.AndroidTitleBarTest \
  art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

## Journeys

The final attached-device run reports **OK (6 tests)** in 240.771 seconds.
UI-action waits synchronize with native snapshot publication, and consecutive
popup/held-contact journeys wait for native window focus to settle.

- Mouse, finger and stylus × light/dark × Small/Medium/Large: inert bank taps,
  immediate body/bank/grip pickup, empty-center placement, Tools drop and picker
  Cancel, live preview without publication, detach with original grab offset,
  re-entry, outside removal, singleton return, ACTION_CANCEL, Done/Cancel and
  one-step undo/redo. Workspace captures retain the committed baseline.
- Menu Labels compact after actual component-bank drops beside Capy. The same
  native item retains its grip and identity; both the compact menu and following
  neighbors remain visible and movable with every device. Whole-item overflow
  exposes draggable hidden items in a chooser.
- The actual Window menu enters Customize Title Bar. The existing tool picker
  inserts Brush color, its native drawer opens, the workspace pill remains 34 dp
  high and centered in the large bar, footer visibility and full Zen work, and
  closing/reopening during a preview restores the committed header.
- System-dispatched keys move a pointer-selected item on the first arrow press,
  cross regions, delete and cancel. Mouse holds never open menus; secondary
  click does. Touch/stylus holds retain contact through context-menu dismissal
  and dragging. A real Android dialog taking focus cancels capture.
- The built-in Sketch workspace has individual header tools and no painter
  toolbars or footer. All seven available default drawers open and dismiss from
  unused bar space. Pixel checks retain selected blue during press and keep
  action presses neutral in both themes. Clock/battery use native status, and
  switching away during customization discards the temporary header.

The tablet tests deliver typed native mouse/touch/stylus `MotionEvent`s through
Compose's actual Android views and system keyboard dispatch. They establish
native UI behavior, not hand-held pen accuracy or digitizer latency.

The layout-feedback regression checks the final glyph of every menu label at
all three sizes and the native screen coordinates of all eight dropdowns.
Menu metrics include individually pixel-rounded padding, each label owns its
popup anchor, and the size/footer/Cancel/Done group stays at the editor's right
edge. Its native layout wraps below the bank when both groups cannot fit beside
each other. Captures are named `anchored-*` and `aligned-editor-*`.
The feedback run and its captures are retained under ignored
`artifacts/android/title-bar/feedback-2026-09-13/`.

The title bar's empty areas are transparent over the full canvas. Its native
background hit region no longer paints an opaque surround-colored strip.
On the attached tablet, zooming the paper behind the bar confirms that it shows
through the gaps and behind unselected controls. Debug builds and lint pass;
the menu/size/alignment and Sketch drawer/theme journeys report **OK (2 tests)**
in 38.959 seconds on the final build. Before/after captures and logs are retained under ignored
`artifacts/android/title-bar/transparency-2026-09-13/`.

## Regressions and artifacts

The port fixes two native ownership problems found on the tablet: pointer
selection now requests native keyboard focus, and empty title-bar space owns
its contact so it cannot also draw on the underlying SurfaceView. Compaction
updates retained item nodes and keeps prior geometry until replacement metrics
arrive. Native durable workspace snapshots now substitute the editor baseline;
untouched legacy Android Sketch defaults migrate conservatively.

Shared validation: 479 tests passed across `layer-host`, `layer-ui` and native
`layer-workspace`; one unrelated hardware-GPU test was ignored. Android debug
app/instrumentation builds and lint pass. Web Wasm builds, and real Chromium's
`--header-controls` regression passes all full/compact/overflow menus, Zoom In,
resize closure and focus restoration.

Captures are generated under the app's external-files `validation/title-bar/`
directory and pulled into ignored `artifacts/android/title-bar/accepted-2026-09-13/`, including
theme/size/device, compact menus, all Sketch drawers, feedback, status and restart.
The final instrumentation report is retained with these artifacts.


## Compact workspace selector — September 13, 2026

The compact workspace button and its overflow entry now project only the pill's
`switcher_display` choices and use the normal shared workspace-switch command.
The existing hamburger sizing is confirmed at 20/28/36dp, centered in its button.

Debug app/instrumentation builds and lint pass. On the connected Wacom tablet,
`AndroidTitleBarTest#compactWorkspaceChoicesAndOverflowIconsFollowTheTitleBar`
reports **OK (1 test)** in 19.114 seconds. It covers configured choice order,
workspace switching with typed native mouse/finger/stylus events, both themes,
all sizes, and selection when the whole item is in overflow. Captures were
inspected under `artifacts/android-compact-workspaces/title-bar`.
Validation used the separate `art.capycanvas.overflowreview` application ID and
isolated workspace/recovery stores; the normal app's data was preserved.
