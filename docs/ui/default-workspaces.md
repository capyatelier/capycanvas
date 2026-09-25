# Default workspaces

The included profiles are **Sketch**, **Paint** and **Photo** (formerly
Painter, Illustrator and Photographer). Sketch is the minimal drawing workspace;
Paint is the panel-heavy workspace. Their stable internal IDs are unchanged.
**Workspaces are the only saved product item**.
There is no Save Layout or Load Layout UI. Workspace changes save automatically.

## Research and choices

[Procreate's interface handbook](https://help.procreate.com/procreate/handbook/interface-gestures/interface)
puts paint, smudge, eraser, layers, and color together, with size, opacity, and
undo/redo on the side. Selection and transform remain readily accessible.
[Clip Studio Paint's Simple Mode](https://help.clip-studio.com/en-us/manual_en/090_tablet/Tablet_interface.htm)
similarly emphasizes drawing tools, color, layers, brush size/opacity, and undo/redo.
The GTK/Web Sketch arrangement uses those common essentials as individual
[window-bar items](window-bar.md). Drawers replace persistent panels; file
operations remain in the menu button.

[Affinity Photo's interface reference](https://affinity.help/photo2/English.lproj/pages/Workspace/interface.html)
separates editing tools from the settings panels in its right Studio.
[Photoshop's panel documentation](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/collapse-expand-icons.html)
supports keeping frequently used panels expanded and secondary panels as icons.
Photographer adapts those patterns to the panels and commands CapyCanvas already
implements; it does not expose placeholders for clone, healing, crop, histogram,
or other missing photography features.

The header follows the grouped shape and clear active state of the
[user's switcher reference](https://content-management-files.canva.com/8a24f050-ceea-4ef9-9a44-ece1ae982d09/UI.png),
using CapyCanvas theme colors, type, and compact spacing.

## Arrangements

| Workspace | Left | Top | Right |
| --- | --- | --- | --- |
| Sketch (GTK/Web title bar) | Capy, Menu, Filters, Select, Scale/rotate | Centered workspace switcher | Brush, Sculpt, Eraser, Layers, Color (plus Full Screen on Web) |
| Paint | Tools toolbar and expanded Tool Set/Tool/Brush size/Color column | Commands toolbar | Open collapsed stack for Navigator/Diagnostics, Properties/Filters and Layers |
| Photo | Tools toolbar | Commands toolbar | Expanded Color/Diagnostics, Properties/Filters, Layers; inner collapsed strip for Tool Set, Tool/Brush size, Navigator |

The GTK, Android and Web Sketch Brush button now opens **Brushes → Tools → Tool**. Brushes is a
narrow list of drawing sets (Paint, Pencil, Pastel, and the other brush media),
Tools contains only the selected set's tools and stroke previews, and Tool keeps
the existing settings. The previous Brush class is named **Paint Brush**. The new
Brush class restores the most recently selected drawing tool; clicking it while
already selected opens or closes its drawer. Choosing a set keeps the drawer
open and restores that set's last tool and settings.

Brushes, Sculpting and Tools are independent panels in the panel menus. Brushes starts
at 160 logical pixels wide, can shrink to a 104-pixel minimum, and uses rows at
least 44 pixels tall with an icon and name on the same line. Tools uses the usual
tool-list sizing; Android set rows are at least 48 dp tall.

Sculpt replaces Sketch’s Blend entry and opens **Sculpting → Tools → Tool**.
Sculpting contains Blend and Liquify; neither appears under Brush, and Eraser
remains a separate tool with a two-panel **Tools → Tool** drawer. Brush and Sculpt
each restore their own last selection and settings, including after switching
workspaces or restarting. Other hosts
retain their previous entry points until their native projections are added.

The GTK/Web/Android/macOS/iPadOS **Select** button replaces the Sketch Lasso entry and remembers the last
selection tool. Its two-panel **Tools → Tool** drawer contains Rectangle select,
Ellipse select, Lasso selection, Polygonal lasso, Auto select (the contiguous
magic wand), and Select by color. Choosing a tool retains the drawer; its right
panel shows the chosen tool's settings. Rectangle/ellipse support free sizing,
fixed aspect ratio, exact image-pixel dimensions, and drawing from the center.
Shift constrains a square/circle and Alt draws from the center. Polygonal lasso
uses successive clicks; click the first point, press Enter, or use Finish
selection to close it. Backspace removes a point and Escape cancels the outline.
Select by color uses the existing GPU color tolerance and sampling sources,
including disconnected matching regions. Expansion and edge smoothing apply to
all matching regions; gap closing remains specific to the contiguous tool.

Photo on these hosts adds the four new tools to the existing selection group in the Tools
toolbar. Untouched included Sketch/Photo layouts upgrade automatically;
customized workspaces retain their layout and can use Restore Starting Layout.
Other platforms retain their existing selection defaults. The SVGs use the shared
icon bank. See [selection behavior and checks](../development/selection-tools.md).

Untouched included GTK, Android and Web Sketch workspaces receive the new default automatically.
Customized layouts keep their arrangement; Restore Starting Layout adopts the
new default. Saved brush choices, settings, and color are preserved.

Drawer regression coverage lives in GTK's `native_brush_drawer_input`, Android's
`AndroidTitleBarTest#brushAndSculptDrawersKeepIndependentSelections`, and Web's
`--brush-drawers` journey (desktop and device runners). On 2026-09-21 the GTK
mouse/touch journey and Huion KP1202 Android/Chrome mouse/touch/stylus journeys
passed, with light/dark captures in `artifacts/sculpt-gtk`,
`artifacts/sculpt-android`, and `artifacts/brush-sculpt-web`. Tablet contacts were
automated native MotionEvents / Chrome input events, not physical pen strokes.

GTK/Web Sketch uses Medium window-bar icons, a transparent canvas overlay, no menu
labels and no zoom/rotation bubble. Other hosts retain the earlier two-toolbar
Painter arrangement until their window-bar projection is implemented.
Photo uses Small toolbar tiles and starts with Operation selected. Sketch starts
with Brush selected and no docked content panels. Photo uses a
permanently expanded far-right column and an adjacent collapsed strip toward
the canvas on all six hosts. The strip starts closed, with Auto-hide and Open
individual panels off. Paint retains its original expanded left panels and
initially open right stack.

On every host, Paint's Color and Navigator groups take their content
height. Color follows its SDR or HDR wheel and footer; Navigator follows the
document shape, from a 4:1 strip up to a square. Tool Set and Tool share the
rest of the left column evenly, and Properties and Layers keep their 30:45 split.
Untouched Paint workspaces upgrade; customized ones keep their arrangement until
Restore Starting Layout. Windows keeps the proportional columns. Check with
`bash tools/performance/workspace-motion.sh gtk --native-test=native_paint_fitted_columns`,
`node apps/layer-web/device.test.mjs --paint-columns` on a tablet origin, and
`AndroidTitleBarTest#paintColorAndNavigatorFitTheirContent`.

## Workspace behavior

- Paint and Photo title-bar defaults include Settings at the right. Sketch ends
  with Color; Web adds a Full Screen button. Native fullscreen commands/shortcuts
  remain available.
  GTK/Web status components show only in fullscreen; the builder retains editable
  Clock/Battery placeholders when their values are hidden. Workspace-specific
  tools and panels remain distinct.
- Startup refreshes included names to Sketch, Paint and Photo, retaining normal
  collision suffixes and preserving saved contents, working tools and history.
  A name swap is atomic, so the two included names do not collide with each
  other. If either participant is open elsewhere, both names wait for release.
  Custom names and workspaces owned by another live window are not modified.
- Seed exactly three default workspaces with stable IDs:
  `builtin:workspace:painter`, `builtin:workspace:illustrator`, and
  `builtin:workspace:photographer`. Fresh installations open Illustrator.
- Startup resumes the saved workspace when available. If another window owns it,
  reuse an available built-in (Illustrator, Painter, then Photographer), then an
  existing user workspace. Only when every workspace is in use does another
  window create a copy, named after its source. Deleting the active workspace
  also reuses an available built-in; it never creates a replacement workspace.
  Legacy imports preserve existing settings once, including a previously saved
  "My Workspace". Deleting an imported workspace does not import it again.
- All three save tool and layout edits normally. They cannot be renamed or deleted.
  The header initially shows these three, follows workspace identities, and
  displays their current names. Its entries can be changed in Manage Workspaces.
- The pill defaults to the right of the document title and left of the clock;
  GTK/Web Sketch centers it, and the window-bar builder can reposition it. Its
  rounded track stays 34 px high with 26 px choices, vertically centered at every
  title-bar size. It uses
  normal workspace switching, including outgoing saves and ownership checks.
  Selecting a workspace restores its latest settings and arrangement. It never
  reapplies the shipped preset to a healthy workspace. An unpinned active workspace is temporarily
  prepended and selected until the user switches away.
- If another window owns a default workspace, focus that window through the normal
  ownership path. Do not take it over or reset its contents just to switch modes.
- If an included workspace cannot decode or validate, restore only that workspace
  from its current platform default. This covers startup, header switching and
  manager previews, including incompatible development fields such as
  `colors.shape`. Its layout history and working settings are reset; its stable ID,
  pins, ordering, document and all other workspaces remain unchanged. Healthy
  customized defaults and user-created workspaces are never reset this way.
  Repair checks the live owner and advances generations/fencing in the same
  transaction as the replacement. A preview releases its temporary claim.
  Disk errors, newer database schemas and damaged ownership/counter records are
  errors, not reasons to reset. Low-level storage reads remain non-mutating.
- New Workspace copies the current settings and arrangement, asks only for a
  name, and pins the new workspace. Manage Workspaces retains selection preview, explicit Switch to Workspace,
  and Cancel. Layout History remains a history of arrangements within a workspace.
- Restore Starting Layout loads the latest shipped platform layout for the
  built-in Sketch, Paint and Photo workspaces, including customized defaults
  created by older versions. Custom workspaces and copies restore their saved
  starting arrangement. The dialog previews exactly what Restore will apply;
  Cancel keeps the current layout. Restore preserves working tool settings and
  document edits, and adds one undoable workspace layout change. Restore remains
  available when a built-in workspace still matches an older default.
  Reset All Brushes resets all brush-setting overrides in the current workspace,
  including inactive presets. It preserves color, selected tool, arrangement,
  document edits, and other workspaces. Resetting brushes creates no layout event.
- Seed idempotently. Upgrades retain existing workspaces and resume the previous
  active one. Existing user names win collisions: the seeded workspace receives
  a numeric suffix, which the pill also shows. Never overwrite user content.
- The first Photographer arrangement used Medium tiles. On switching to an
  untouched copy of that arrangement, update it and its starting layout to Small.
  Keep renamed workspaces and brush edits; leave customized layout histories alone.
- Untouched older Photo workspaces upgrade to the two-column right arrangement.
  Untouched Paint workspaces using that temporary arrangement return to Paint's
  original default. Customized defaults retain their arrangement until the user
  chooses Restore Starting Layout. Ordinary strip open/close remains transient
  and adds no layout history entry.
- Untouched GTK/Web Sketch workspaces upgrade from the shipped two-toolbar layout
  or the earlier title bar with Settings to the current title bar. Working
  brush/color values remain intact. Any edited history, custom baseline or
  independent copy is left alone.

## Configurable switcher

Manage Workspaces keeps a single ordered list. Every row has a narrow, dimmed
left grip. Workspaces shown in the top bar have a separate pin icon with the
tooltip **Shown in top bar**. The current workspace retains its checkmark.

Each row's **⋮** menu includes **Show in top bar**. Checking it shows that
workspace in its list position; unchecking removes its pin without moving the row.
The switcher has no border and a darker, recessed background like a slider track
(80% theme background, 20% black), with a subtle blue active choice.
New workspaces start pinned. Creation and pinning are saved together, so the
workspace stays in the top bar after switching away or restarting. Existing
hidden workspaces keep their preferences. The top bar follows the list order, skipping
unchecked entries. If the current workspace is unchecked, temporarily prepend it
until the user switches away. This keeps the current workspace visible even when
all entries are unchecked, without changing saved pins or order. Dialog previews
do not change this temporary entry. The pill scrolls horizontally when its choices
exceed the available width.

Drag any row to change its order, including unchecked rows. Moving a row never
changes its visibility. Mouse can drag any non-button part of the row. Touch and
pen can drag the handle immediately; the rest of the row requires a hold,
following the [app-wide drag convention](drag-and-reorder.md). Ordinary touch
swipes scroll the list. Right-click or touch/pen hold opens the row menu; mouse
holds never open menus. Moving with the
same held contact closes the menu and starts dragging; release without movement
leaves the menu open. An insertion line shows the destination. Escape cancels the
drag. **Move Up / Move Down** is available for every row and supports keyboard
access. Clicking still previews a workspace; menus, scrolling, and dragging
preserve the selected row and its preview.

Pinning and ordering save immediately, independently of the preview. Cancel closes
the manager and cancels the layout preview; it does not undo these app preferences.
All workspaces and windows share the switcher configuration.

## Implementation and host integration

`layer-ui/src/layout_presets.rs` defines shared geometry and initial working state.
`layer-workspace::DEFAULT_WORKSPACES` supplies stable identities. Initialization
creates workspaces directly; no reusable layout records are seeded. Older layout
records and storage APIs remain compatible with existing data, without UI routes.

`metadata.builtin` protects an included item's name and prevents deletion.
Reusable items with this flag are read-only. Included workspaces still allow
layout, working-state, and lifecycle metadata updates. SQLite and the browser
store both enforce the distinction, including direct metadata writes.

Included-workspace repair and initial seeding share one definition. SQLite replaces
the failed row with self-contained content, without editing shared resources.
The browser stores the same entity JSON shape but decodes entities individually,
so an incompatible item cannot prevent opening the catalog. Neither backend adds
support for obsolete workspace fields.

The shared manager exposes `workspace_ids` (complete dialog order), `switcher_ids`
(visible subset), `refresh_switcher`, and `edit_switcher(SwitcherEdit::{Show, Move})`.
Read preferences at startup, on focus, and when refreshing the manager.
`StoreRequest::Switcher` returns optional visible IDs; `None` means the three
defaults, while `Some([])` hides the bar. `WorkspaceOrder` returns the optional
complete order. Without a saved order, existing switcher preferences seed it;
new workspaces follow alphabetically. Visibility edits preserve this order.
`UpdateSwitcher` and `UpdateWorkspaceOrder` atomically compare their respective
previously read list before replacement. These requests need no workspace claim
and change no workspace generations, settings, layout, or history. Invalid/deleted
IDs are rejected; duplicate deliveries are idempotent. SQLite schema 4 adds row
ordering alongside schema 3's visibility preference. Upgrades preserve previous
workspaces and pins. The browser reducer supports the same operations and upgrades
schema 2 and 3 snapshots.

GTK's `workspace_switcher.rs` renders the pill and uses the shared manager.
`UiSession::reset_workspace_brushes` performs the brush reset; hosts provide the
confirmation and save the resulting capture. `WorkspaceCommand::ResetBrushes`
routes the shared menu entry. Saved-layout menu entries and GTK flows were removed.
Legacy enum variants stay decodable for hosts updating concurrently, but must not
be exposed as new UI.

See the [host handoff](workspace-manager-host-handoff.md) for preview, ownership,
save failure, lifecycle, and accessibility requirements.

## GTK review

Native captures from the real-pointer interaction test:

![Painter](default-workspaces/painter.png)
![Illustrator](default-workspaces/illustrator.png)
![Photographer](default-workspaces/photographer.png)
![Reset All Brushes](default-workspaces/reset-brushes.png)

Run `cargo run --locked --release -p layer-linux` from the repository root.
Use the three header choices, change a brush size, and switch away and back to
check that each workspace keeps its changes. Window → Workspaces contains
New Workspace, Manage Workspaces, Layout History, Restore Starting Layout, and
Reset All Brushes. The manager's plus button copies the current workspace.

Initial preset validation: 314 shared tests passed (273 `layer-ui`, 41 `layer-workspace`). Six
native GTK tests pass with isolated storage and a private Wayland compositor:
real-pointer menus/pill/drawers/reset, manager previews/history, database restart
and independent windows, ownership takeover, unavailable-storage close recovery,
and fullscreen title/clock placement. Shared host, Apple, and Windows bridge
compilation passes. Other platforms' native UI acceptance remains with their
host implementations.

The pill-color and Small-toolbar refinement reran the shared suite and native
pointer test. The upgrade test also covers brush edits, renaming, preservation of
customized layouts, durable publication, and repeated initialization.

## Switcher acceptance

The integrated shared suite passes 331 tests (278 `layer-ui`, 53 `layer-workspace`).

Run `bash apps/layer-linux/bench/workspace-switcher.sh` for isolated real mouse,
touch, and keyboard verification. The test covers whole-row and handle dragging,
narrow grips on every row, immediate touch handles, held touch rows, right-click
and hold menus, same-contact menu-to-drag continuation, long-list scrolling, keyboard Move Up,
reordering hidden rows without pinning, pinning custom workspaces, hiding every
entry, drag cancellation, preview
preservation, and persisted choices after reopening. The shared suite covers
cross-window preference updates, idempotency, conflict rejection, and SQLite/browser
parity. A focused Wasm/IndexedDB run passes 39 storage contract cases. Native
pen timing still needs a hardware check; the automated native driver covers
mouse, touch, and keyboard. Shared, Android, Apple, and Windows bridges compile.

![Switcher configuration in Manage Workspaces](default-workspaces/switcher-manager.png)
![Workspace row options](default-workspaces/switcher-options.png)


## Web switcher acceptance

Web now uses the same persisted visibility and row order as GTK. The shared
`WorkspaceController` exposes `switcher`, `order`, and independent preference
status; `EditSwitcher` acknowledgements preserve pending and visible previews.
Web refreshes on focus and uses BroadcastChannel to update other tabs after a
successful preference edit. The fixed `defaults` field remains for compatibility;
new host switchers should render `switcher_display`; `switcher` is the saved pin selection.

`node apps/layer-web/test.mjs --workspace-switcher` passes in desktop Chrome on an
isolated Wayland display. It covers native mouse/touch and CDP pen input: every-row
grips, hold/secondary menus, same-contact dragging, rejection before touch/pen
holds, unpinned reordering, keyboard moves, scrolling, cancellation/blur, live
preview preservation, multiple tabs, overflow, and restart. The existing
`--workspace-manager` regression passes too. Physical stylus testing remains a
separate hardware check. The headless GPU environment emits an existing external
Instance warning; desktop Chrome acceptance has no page errors and renders the
canvas previews correctly. Shared controller tests and 12 packaging tests pass.

Other platforms can use the [minimal implementation handoff](workspace-switcher-platform-handoff.md).

![Web workspace switcher configuration](default-workspaces/web-switcher.png)
![Web workspace row menu](default-workspaces/web-switcher-menu.png)

## Invalid-default recovery acceptance (September 13)

- 396 shared tests pass (`layer-ui`, `layer-host`, and 72 `layer-workspace` tests).
  Recovery fixtures corrupt actual stored JSON for all three included workspaces
  on SQLite and the browser reducer. They cover unknown fields, invalid working
  versions, absent working state, invalid metadata/content, missing resources,
  healthy/custom preservation, live leases, stale writes, rollback and reopen.
- GTK's `native_default_workspace_recovery_input` passes with private SQLite
  storage and real pointer input. It covers header switching, broken resumed
  startup, manager preview/Cancel, claim release, preservation of another
  workspace's edited brush settings, and using the recovered Painter color
  drawer. Evidence: `/tmp/capy-workspace-motion.FbGJD2`.
- Chrome passes 49 SQLite/IndexedDB contract cases plus default recovery,
  transaction abort after replacement, ownership rejection and durable reopen:
  `/tmp/capy-workspace-motion.OKMFPg`. The existing complete workspace-manager
  interaction regression also passes: `/tmp/capy-workspace-motion.W0b1pp`.

Run the recovery journey with
`bash tools/performance/workspace-motion.sh gtk --native-test=native_default_workspace_recovery_input --native-storage`.
For the browser contract, first generate its fixture with
`CAPY_STORE_CONTRACT_FIXTURE=/tmp/capy-workspace-store-contract.json cargo test --locked -p layer-workspace --features native browser_transactions_match_sqlite_contract`,
then run `bash tools/performance/workspace-motion.sh web --workspace-store`.
These tests use isolated stores; they do not wipe normal app workspaces.
