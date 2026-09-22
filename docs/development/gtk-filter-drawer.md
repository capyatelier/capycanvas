# Sketch filters and layer interaction validation

The Brush/Sculpt/Eraser cleanup was pushed to `origin/main` as `16bb86c1`.
Following the GTK review, the filter and layer changes also cover Web and Android.

The Sketch Filters drawer uses Filter Type → Filters → Properties. Selecting a
filter inserts above the selected layer, or replaces the selected filter while
retaining its identity, mask and clipping. Closing/reopening preserves that
selection. Cancel deletes the selected filter and closes the drawer. Undo/redo,
property editing and drawing-target resolution live in shared Rust.

Drawing through filters stops at the first ordinary layer below (or the clipping
base). Only unlocked paint content is eligible; groups are not entered. The
selected filter's own mask wins, but lower masks do not. Paper has a contrasting
thumbnail icon, a color property with a current-color bucket, and editing lock.
Blocked drawing targets show the red prohibited cursor.

Paper and the final remaining layer can be deleted. An empty document remains
valid for save, load, undo and adding a layer. Without paper, the canvas clear is
transparent for alpha storage and white for opaque storage. Current document
storage is RGBA; opaque export already composites against white by default.
Animated filters integrate playback speed without seeking when speed changes,
including zero and restored speeds. Canvas, exact queries and export share that
clock. Animated markers precede filter icons in the picker.

GTK layer rows track touch/pen swipes to reveal Delete. Reverse swipes and outside
clicks close it; deletion uses the existing shared validation and undo action.
The common pen scroller covers all 25 application-created scrollers, including
retained drawers, tools, filters, layers, preferences and picker dialogs. It
preserves direct controls, grips, native slop and hold arbitration. Mouse does
not pan lists. Preference hold menus and workspace-manager rows now classify
tablet tools as pen even when GTK reports the logical pointer as mouse.

Android uses Compose scrolling for both touch and pen. Web has one pen panning
adapter for native scrolling containers, preserving direct controls, handles,
hold arbitration and ordinary clicks. Each host owns swipe tracking and input
capture; filter replacement/cancellation, deletion validation, drawing targets,
paper color and history remain in Rust.

## Validation

- Shared layer-ui, layer-core and layer-engine suites passed. Focused filter
  tests cover replacement/reopening/cancellation, target/mask rules, actual
  strokes and undo, paper color persistence/lock/history, and blocked cursors.
- Native workspace migration tests passed, including untouched-default upgrades
  and preservation of customizations.
- GTK native journeys passed: `native_filter_drawer_input`,
  `native_panel_pen_input`, `native_layer_preview_selection`,
  `native_layer_hold_input`, `native_drag_pickup_input` and
  `native_brush_drawer_input`.
- Light/dark drawer, paper property and swipe screenshots are in
  `artifacts/filters-gtk/`. The GTK application build passed.

Run a native journey with a freshly built test executable:

```sh
cargo test --locked -p layer-linux --no-run
LAYER_NATIVE_TEST_EXECUTABLE=/absolute/path/to/the/reported/test/executable \
LAYER_TEST_ARTIFACTS="$PWD/artifacts/filters-gtk" \
bash tools/performance/workspace-motion.sh gtk --native-test=native_filter_drawer_input
```

For `native_panel_pen_input`, add `--tablet` and set
`LAYER_MOTION_VIEWPORT=1100x650` so the tool catalog overflows. Its isolated Wayland tablet-v2
fixture produces actual GDK tablet events and checks tool-list scrolling and
swipe/delete, alongside mouse/touch behavior. It does not qualify physical pen
sensors or compositor drag-and-drop serials. Existing Mutter journeys cover
the compositor reorder paths with mouse/touch.

## Huion device journeys

The Android instrumentation journey is
`art.capycanvas.AndroidTitleBarTest#filterDrawerLayersAndPenScrolling`.
The Web journey is `node apps/layer-web/device.test.mjs --filter-drawer`, with
`LAYER_DEVICE_CDP` pointing to the tablet's forwarded Chrome endpoint and
`LAYER_WEB_URL` to the test server. Both exercise filter replacement, reopening,
cancellation, paper properties, square swipe deletion, reverse swipes, deletion
of the empty stack and undo, and mouse/touch/pen tool scrolling. The fixture
constrains the tool viewport so the catalog overflows. Web also verifies that a
completed pen hold opens the layer menu and retains reorder ownership in an
overflowing list, including Escape cancellation.

Device screenshots are retained in `artifacts/filters-android/` and
`artifacts/filters-web/`. Input is injected through Android MotionEvent and
Chrome DevTools on Huion hardware; these journeys do not qualify pen sensors.
