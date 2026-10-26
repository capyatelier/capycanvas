# Stacked collapsed columns

[Workspace and UI](README.md) · [Panel customization](panel-customization.md) · [Drag convention](drag-and-reorder.md)

Every host presents collapsed side columns as stacks; the workspace model, drop
validation, geometry and history are shared Rust.

A stack contains one or more collapsed columns. Each member retains its dock
tree, tab groups, selected tabs, split ratios and expanded width. Each member
has an immediate drag handle. Dropping that handle on another member stacks
the columns; dropping beside a stack or at a side edge unstacks the member.
Tile bodies retain the application-wide hold-before-drag convention.

Only top-level side columns can collapse. Columns inside vertically stacked
groups remain expanded, including when their parent column opens. Side-by-side
top-level columns can collapse independently; a stack's members are the only
collapsed children of its root. Saved nested collapsed columns and nested stacks
reopen as ordinary groups, preserving their panels, selected tabs and split tree.

Dropping an individual panel, a whole toolbar, or a tab group into a
member's empty lower area or footer grip creates a new collapsed column after
that member. The new
column contains only the dropped content; a whole group retains its tab order,
active tab and tab style, and a toolbar retains its tools. Stack preferences
apply to the new member. Icon bodies still insert tabs and group dividers still
insert groups within a member. A 12 px target centered on the bottom edge of
the last tile appends a new group inside that member, with a blue insertion
line at the tile boundary. It includes the trailing gap above the footer grip,
so compact members earlier in a stack have the same target as its last member.
It only appears when the last group is scrolled into view and excludes the
grip itself. The grip and spacing between members continue creating stack
members. All hosts use the shared target geometry.

“Open individual panels” and Auto-hide belong to the stack and both default to
disabled for a new stack. Explicit saved preferences are retained. Enabling
“Open individual panels” opens the selected tab group in a compact popover.
With it disabled, a tile selects its tab and opens that member's complete
ordinary column toward the canvas. One member can be
open per stack; choosing another member switches the open column. Clicking an
already selected tile in the open member closes it.

Stacked members have the standard 6 px workspace gap between them. No separator
is drawn above the first group; separators only appear between groups within
a member. The first tile starts at the member's top edge. Dropping a panel or
group in the workspace gap immediately above it prepends a group within that
member while its first group remains unscrolled. Whole-column drags retain the
gap's stack insertion target. All five clients present this placement and
consume shared bridge geometry through their native rendering adapters.

An open member uses the full available workspace height, aligned with the
stack’s top and bottom. It keeps the normal outer workspace inset, without
additional vertical padding. One 6 px workspace gap separates it from the stack,
and one from the canvas and toolbars beside it, as for an expanded column.
Its normal tab groups and split dividers are rendered by the ordinary dock views. The sidebar highlights the active tile of every visible
tab group in the open column. Drawer-style connectors join those active tiles
to the expanded column.

GTK resolves selected tiles against allocated receiving panels through
`ResolvedLayout::panel_bridges`. It sends sidebar, panel, tab and tile bounds to
the existing GPU background renderer. The sidebar, receiving backgrounds and
bridges share one coverage calculation and one shadow. GTK retains the controls,
input targets, hover feedback and tab-bar content clip. The tab bar keeps its
own rounded upper outline. Corner backing belongs to an actual GPU attachment;
an unconnected native tab bar only clips its content and adds no corner filler.

Windows measures its retained native groups, strip viewport and tiles in logical
points and resolves them through the same shared bridge projection. The existing
glass transport carries receiver IDs, four corner radii and native tab bounds to
the render worker. It applies the surface offset and display scale once, prepares
the existing GPU chrome pipelines, and acknowledges the revision after presenting.
Only that matching revision suppresses the native backing and shadow; controls,
focus, scrolling and input targets remain native. Retired allocation IDs are ignored; duplicate, nonfinite or negative measurements
are rejected. Missing current allocations clear the join. Ordinary individual
panel drawers retain their shared drawer connection.

Apple uses the same allocation and presentation contract. Swift reports the
retained group, collapsed-column, content and icon frames in workspace points;
the shared projection supplies receiver and column radii to Metal. Native panel
and column controls keep their clips and input, and retire their inferred backing,
shadow and connector only after the matching Metal panel revision is presented.

GTK rounds absolute layout edges once to integer logical pixels, then derives
native sizes from those edges. A divider step grows one neighboring panel by
the same amount it shrinks the other, preserving the workspace gap and outer
edges at every display scale. GPU bridge geometry uses the allocated bounds.

Bridge receivers carry stable group IDs and all four corner radii. GTK derives
native-background suppression and GPU emission from those same facing receivers;
other groups keep their native backgrounds. Rebuilding panel groups or collapsed
strips clears their connection frame before the next allocation publishes one.
Missing native bounds never fall back to a different group's position.
Each tile contributes a bridge only to vertically adjacent receivers, using
each receiver's own upper and lower corner profiles. A distant tile cannot fill
an intervening group's rounded corner through a column-wide bridge envelope.
Attachment uses the vertical separation of the complete panel and tile
intervals. With tile bounds `[B,E]` and panel bounds `[T,P]`, separation is
`d = max(B-P, T-E, 0)`. The patch blends the original rounded panel field into
the attached field with `S(clamp(1-d/6,0,1))`, where
`S(t) = 6t^5 - 15t^4 + 10t^3`, before combining patches. Touching or overlapping
intervals retain the existing joined profile. The panel keeps its original
radius; a newly eligible patch cannot bypass the blend and fill the corner immediately.

For a standard 6 px gap with top `g`, moving between panels above and below a
tile has these attachment keyframes:

| Gap position | Receiving panels |
| --- | --- |
| `g <= B-6` | Lower connected; upper absent. |
| `B-6 < g < B` | Upper enters over 6 px; lower stays connected. |
| `B <= g <= E-6` | Both connected; the gap retains its full broad cap. |
| `E-6 < g < E` | Lower exits over 6 px; upper stays connected. |
| `g >= E` | Upper connected; lower absent. |

A gap just below the tile bottom cannot fade the upper panel's existing bridge.
Aligned panel/tile tops and aligned bottoms retain their flat outer boundaries.

Shared Rust generates the normalized outer-curve profiles at build time. GTK
uploads those bytes with its panel resources; native vector projection reads
the same table. A positive symmetric
smoothing kernel preserves an exactly straight boundary when the panel and tile
tops align; bottom alignment is its vertical reflection. Offset boundaries join
with continuous curvature. The panel and sidebar keep their separate corner
sizes in the published geometry.

Gap positions drive entry and exit. With gap top `g`, height `h`, and tile top
and bottom `B` and `E`, clamped quintic ramps use `(g+h-B)/h` and `(E-g)/h`.
The gap's broad cap translates toward the panel plane while retaining its shape.
Its implicit boundary is `abs((x-m)/D)^3 + ((y-c)/(h/2))^2 = 1`, where `c` is
the gap center. For the standard 6 px gap and bridge, `D` is 4 px, leaving 2 px
of connected bridge. The cap never widens the gap and meets its flat edges with
zero curvature. Facing background corners fill as their attachments form; the
tab bar's own rounding is independent of that backing.

GTK dragging updates compact GPU parameters. It does not sample a CPU shape field,
trace contours, rasterize bridge masks or generate glass bands. Tint, glass and
shadow use the same GPU shape, and unchanged artwork retains the existing blur
cache. Unchanged scenes retain their packed geometry; viewport and cached-blur
reprojection update only their sampling parameters. Changed native chrome and
its matching GPU background commit together;
the input thread never waits for GPU completion. The worker retains the matching
published buffer until GTK releases its revision, with the existing 100 ms
recovery limit.

GTK's common presenter factory schedules bridge and glass pipelines
on the background shader compiler at startup and for HDR/color replacements.
The profile table is uploaded before chrome becomes ready. Presentation waits
for that readiness without compiling pipelines or waiting for GPU completion in
the drag path; color candidates prepare before adoption.

Android projects the same field into cubic paths on its render worker. The
projection splits disconnected components and fits their boundaries to 0.025
logical pixels, with coordinates stored at 1/1024 pixel precision. Its bounded
cache retains at most 512 previous projections within 4 MiB. Only the current
receivers are published. Bulk JNI arrays carry the prepared geometry; unchanged
geometry retains its identity across publications.

Android fills each connected sidebar and its receiving panels as one native
path with one shadow. Separately antialiased pieces would
leave a faint line where they meet inside a gap. Native controls retain their
own panel clips, tab-bar rounding and input targets. The worker prepares
immutable paths before publishing matching layout and geometry together. Shared
corner arrays name physical sides; Android retains those sides under RTL layouts.
The merged path uses the authored source/receiver allocation as its origin;
cubic control-point bounds can extend slightly beyond the visible contour.
On Android 12 and later, a quarter-size native GPU blur mask supplies the shadow,
while the fill and control clips keep their original resolution. Earlier Android
versions use the existing native shadow renderer.
Only panel dimensions invalidate control measurement; unchanged content and
stationary columns retain their models.

Every facing panel remains in its column's native surface while the column is
open. A panel without an eligible anchor contributes only its original outline.
Corner entry and exit therefore change the fill without replacing the panel's
native parent or remembered controls.

Web uses the same projection on a dedicated, prewarmed Wasm worker. The worker
formats SVG cubic paths and transfers backdrop triangles. The browser publishes
panel bounds and the matching paths together, with one combined fill and shadow
per open column. Original panel outlines clip the native controls; cached native
tab contours cut selected faces out of the strip fill. A pending projection
retains only the newest layout, and closing or changing content retires stale
results. SVG and native controls share the browser's normal painting pipeline.

The existing GPU backdrop renderer consumes the projection's patch triangles.
Only exterior edges receive antialiasing; shared edges stay fully covered.
The normal panel and sidebar masks remain part of that same blur pass.

Vertical groups retain saved proportions when their measured scroll targets fit.
A scroll target includes fixed controls and four measured rows, or four tile
heights for continuous content, capped at the panel's natural content height.
When these targets exceed the available height, groups first retain fixed
controls and one measured row, or one tile height of continuous content, for
every tab. Palettes retains two rows, including empty palettes. Remaining space
grows groups toward their four-unit target. If the minimums cannot fit, each
group keeps a tile-height body viewport wherever space permits. The complete
body then scrolls at its measured minimum height, including fixed controls.

Fitted groups yield body space to neighboring groups while retaining their fixed
scroll controls and useful scroll viewport. Neighboring lists receive preferred
rows only after every group's minimum fits. Content
beyond the allocated scroll area remains reachable by scrolling. Hosts retain the
last measured row height while lazy rows are absent and replace it when a row
reports a new height.

There is no Expand button. Escape closes open columns once nothing else claims
it: a canvas drag or stroke, a popup, a drawer or what the canvas bar edits. Auto-hide consumes the outside canvas contact and
respects popups, nested drawers and active gestures. Open/close is transient;
membership, preferences, widths and ordinary split ratios are saved. Completed
stacking and resizing gestures each produce one workspace undo step, with
cancellation restoring the previous state.

A closed stack containing multiple columns has a fixed sidebar width. Dragging
its outer edge 36 logical pixels toward the canvas opens the member level with
the contact, without a history entry; reversing before the open edge closes it
again. Past the open edge, the same drag resizes that member's canvas-facing
edge, as it does after opening a member with a tile. That changes only the open
member's remembered width and stops at its panels' minimum width, preserving
the stack, its other members and the existing undo/cancel behavior. Nudging the
closed edge with the keyboard or resetting its width cannot expand the aggregate
tree. With “Open individual panels” enabled no member column opens, so the edge
has no resize affordance.

Dragging an open member's edge inward past 108 logical pixels beyond its
minimum width, or down to the icon-strip width, closes the member like its tile
does; reversing the same drag reopens it. Closing keeps the width from before
the drag and adds no layout history.

Old Group panel settings migrate to “Open individual panels” disabled. The
custom all-tabs renderer, per-panel height weights and dedicated resize actions
are retired.

The shared model does not serialize an open member; saved stacks start closed,
except that Paint opens its right stack on load. Photo and Paint's stacks are
described in [default workspaces](default-workspaces.md#arrangements).

Apple uses the ordinary SwiftUI dock groups, dividers and drawer connector shape
for full-column opening. Column grips expose the shared stack preferences.
Every host omits the resize source of a divider that Rust publishes as `fixed`.

On GTK, Web and Android, dropping a panel, toolbar, tab group or column in the menu/title bar
prepends it to the side column directly below the pointer, except in the
first-group prepend strip described above. An expanded column
receives new groups; a collapsed stack receives a new first member. A column
inserted among expanded groups keeps its tree and opens its contents. No panel
group reserves the upper portion of its body for a split insertion. Instead,
the upper 20% of each group's body extends its tab-strip target: the horizontal
pointer position chooses a tab insertion slot, with the preview line in the
tab strip. This applies to first and lower groups, floating groups and drawers.

Dropping into a GTK, Web or Android panel group's body prepends the incoming tabs and selects
the incoming content. A translucent filled rectangle with an outline covers
the target's content area. Existing tabs stay in the group. Tab strips continue
to offer precise insertion positions with a line preview.

The history comparison includes collapsed member boundaries. Moving the last
member into the preceding column can preserve the panel order while changing
the grouping; that remains a real move with one undo step.

## Checks

```sh
cargo test --locked --release -p layer-linux --lib workspace::allocation_tests
LAYER_TEST_ARTIFACTS=artifacts/ui/column-panel-bridges bash tools/performance/workspace-motion.sh gtk --native-test=native_column_panel_bridge_input --tablet
LAYER_TEST_ARTIFACTS=artifacts/ui/column-panel-configuration bash tools/performance/workspace-motion.sh gtk --native-test=native_column_panel_bridge_configuration
LAYER_TEST_ARTIFACTS=artifacts/ui/column-panel-bridges-motion bash tools/performance/workspace-motion.sh gtk --native-test=native_column_panel_bridge_motion
LAYER_TEST_ARTIFACTS=artifacts/ui/column-panel-bridge-timings bash tools/performance/workspace-motion.sh gtk --native-test=native_column_panel_bridge_motion_diagnostic
LAYER_TEST_ARTIFACTS=artifacts/ui/column-bridge-transitions LAYER_NATIVE_CAPTURE_DIR="$CAPY_BRIDGE_CAPTURE_DIR" bash tools/performance/workspace-motion.sh gtk --native-test=native_column_panel_bridge_transitions
bash tools/performance/workspace-motion.sh gtk --native-test=native_stack_member_drop_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_column_group_append_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_layout_drop_input
bash tools/performance/workspace-motion.sh web --column-stacks
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/column-stacks.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/column-stack-persistence.swift
./apps/layer-windows/scripts/exercise-column-stacks.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -Device mouse
```

The Windows journey covers both themes, native panel/tile bounds, all four
receiver radii and the presented GPU bridge revision, plus the existing drag,
resize, Undo/Redo, persistence and Zen checks. Repeat with `-Device pen` and
`-Device touch`; inspect the captured joined surfaces in both themes.

The panel-bridge journey checks GPU background pixels, partial curves, gap caps,
first/last physical tile rows, selected-tile seam pixels, tab-bar corner colors
and Undo/Redo with mouse, touch and the tablet proxy in both themes on both sides. Its divider starts on a half-pixel coordinate to exercise alignment.
The configuration journey checks that opening all controls expands the column,
retires its GPU connection and restores the ordinary native configuration
background, with clear space above it.
Set `CAPY_BRIDGE_CAPTURE_DIR` to an owned SSD scratch directory for the transition
journey. It records the actual compositor while switching tiles through eight
panel-height fixtures in both themes on both sides, without synchronous pixel
readback during recording. It requires upper-only and lower-only gap overlaps
in each theme/side pair, verifies exact native tile/bridge extents, and saves
the geometry beside its final screenshots; review the recorded frames for flashes.
The separate motion journey runs without the tablet proxy and records a priming
resize and three six-second mouse resize traces in each theme on each side,
using completed presentation timings only while geometry changes. Its custom
toolbars and default drawing provide a workstation diagnostic, not reference-tier
qualification. The path uses the allocated minimum panel height so that its
entire measured contact changes geometry; see the
[GTK diagnostic](../performance/top-tier.md#gtk-column-bridge-diagnostic).
The focused timing diagnostic runs the light-theme left column and exports
existing owner, worker, GPU and presentation counters after each contact.

Repeat the Windows run with `-Device pen` and `-Device touch`, serially on an
unlocked desktop; the pointer leaves the strip before the Zen assertion because
hovering visible chrome reveals it. `node apps/layer-web/test.mjs --headless --column-stacks`
runs against a development server where headless WebGPU works, and
`--column-drops` checks adjacent tile and divider boundaries. Android runs
`AndroidInteractionTest#stackedColumnsOpenAndResizeOrdinaryGroups` and
`#stackedColumnDropsKeepFooterAppendTargets`; Apple runs `testColumnStacks` and
`testColumnStacksDark`. Injected pen and touch do not establish physical-device
acceptance.
