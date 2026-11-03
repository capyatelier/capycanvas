# Phone layout

[Developer guide](README.md) · [Workspace and UI](../ui/README.md) · [Default workspaces](../ui/default-workspaces.md) · [Title bar](../ui/window-bar.md)

This plan covers [issue #15](https://github.com/capyatelier/capycanvas/issues/15):
the workspace layouts don't fit phone screens. Today a docked column shrinks only
to its panels' minimum (usually 242 px), that minimum overrides the canvas
(`layout.rs`, band widths), and nothing in the layout reads the window's height.
When the work lands, move its decisions into the `docs/ui` guides and delete this
plan.

## Principles

- **A phone presentation, not a squeezed tablet.** Below a size, every workspace
  presents the same fixed frame: controls on the two short edges, the canvas on
  the long axis.
- **Nothing is derived from the tablet arrangement.** The phone keeps one small,
  explicit list per workspace. No rules fold columns, toolbars or the title bar.
- **Reuse what exists.** The bars look like the title bar, tiles edit like toolbar
  tiles, the workspace switcher and drawing selector are the existing ones, and
  drawers are the existing drawers. No edit modes, settings pages or menu items
  where behavior does the job.
- **Resize, never wrap or hide.** Tiles shrink to fit; windows below the
  supported minimum scale.
- **One shared function decides.** Shared Rust computes the presentation from the
  layout, viewport, insets and settings; hosts draw it and forward input.

## When it applies

- GTK and Web present the phone layout. Each host enables its shared
  `Platform::presents_phone_layout` capability when its bars and finger input
  are wired; saved phone toolbars remain available on every host.
- On enabled hosts, the phone layout replaces the arrangement whenever the window's
  short side is under 480 logical pixels (dp on Android, points on Apple). That
  covers phones, tablets in split screen, narrow iPad windows, and narrow desktop
  and browser windows.
- Crossing the line saves nothing. The workspace's arrangement and Zen setting
  stay as they are and return when the window grows.
- Crossing the line or rotating cancels chrome drags (toolbar reorders, slider
  drags, panel moves) without saving and closes drawers and menus whose openers
  move. A stroke in progress continues to its end. The test uses the window's size,
  never the space an on-screen keyboard leaves.
- The supported minimum short side is 320: Android's smallest phone width, an
  iPhone SE with Display Zoom (320 × 568 pt) and narrow iPad windows. Narrower
  windows (browsers, freely resized windows; the first Galaxy Fold's cover screen
  is 280 CSS px wide) get the 320 layout scaled down to fit. Hosts that can set a
  minimum window size (Android split screen and freeform, GTK, macOS, Windows)
  set 280 × 280. Fold cover screens can be about 280 dp or CSS px wide, and a
  360 × 640 phone in split screen can have a window about 280 tall.
- Linux phone shells see the app through its AppStream minimum display length
  and the Phosh and Plasma Mobile desktop FormFactor declarations.

## Layout

- **Capy bar** on the holding hand's short edge (left in landscape, bottom in
  portrait): Capy, which opens Capy's menu, brush Size, Strength and Tool settings
  share one chip. It is the same in
  every workspace and is neither saved nor customized.
- **Phone toolbar** on the other short edge: the workspace's phone toolbar in
  its saved order. All tools share one chip; dividers are lines inside it.
- **Style:** title bar chips at the Medium size. Each bar is one tile thick, 8
  from its edge, with 2 between tiles and 8 between chips. Chips are glass filled
  with the canvas surround, opaque at panel transparency Off; Capy is an in-bar
  tile. The canvas renders under the bars; Fit Canvas uses the space between them.
- **Tile size:** the largest up to 48 at which the phone toolbar fits its edge.
  The Capy bar uses the same size. Bars never wrap; the toolbar holds up to ten
  tools.

Examples without safe insets or native controls; slider lengths include their
end insets:

| Window | Tiles | Slider length | Fit area, landscape |
| --- | ---: | ---: | --- |
| 412 × 891 | 47.75 | 147.25 | 763.5 × 412 |
| 393 × 852 | 45.375 | 140.125 | 729.25 × 393 |
| 375 × 667 | 43.125 | 133.375 | 548.75 × 375 |
| 360 × 780 | 41.25 | 127.75 | 665.5 × 360 |
| 320 × 568 | 36.25 | 112.75 | 463.5 × 320 |

These use Sketch's eight tools. Native controls and additional tools participate
in the same fit calculation. Size and Strength have a 2-unit internal gap; their
chip includes Capy with a 2-unit gap. Hiding the bars keeps that same chip
allocated to Capy alone in the same pass, without moving or restyling Capy.
The shared bar contact areas disappear when hidden; Capy's own control keeps
reveal and hold input. The chip gains its own rounded corners when hidden. Slider
tracks fill their bounds without grips; tracks and handles scale from the desktop
Brush controls.

- **Native controls:** GTK keeps its normal circular close button and other
  configured window buttons outside the chips. They align to the bar's
  centerline, with symmetric space toward the edge and adjacent tools. Dragging tiles, Capy or
  unused space on either bar moves the window after native movement slop, without
  a hold. In movable landscape windows, the top 24 units between the bars also
  move the window, even with bars hidden. This input band scales below 320;
  drawers, menus and other overlays keep their input above it. Fullscreen and
  portrait have no band. Sliders scrub and native window buttons keep their actions.
- **Safe area:** each bar sits 8 from its edge, or past the platform's safe inset
  where that is larger, and its ends follow the same rule. Android reports a
  camera cutout only on its own edge and, from Android 12, the rounded corners.
  iPhone reports 59 pt on both sides in landscape and 59/34 top and bottom in
  portrait; the bars follow it as reported, since iOS publishes no corner radius.
  Web reads `env(safe-area-inset-*)`. The canvas still runs edge to edge.
- **Drawers** from the phone toolbar open along its edge and are only as deep as
  their content needs, up to the space between the bars. Their columns sit side by
  side when their minimum widths fit and stack otherwise. Brush and Sculpt show
  set chips above their tools in portrait and a separate scrolling sets column
  in landscape. A choice is a set only when selecting it opens its own list;
  Fill and Gradient choices are full-width tool rows. Color stacks in portrait.
  Drawers close on the next stroke
  as today. Tool drawers with other content omit Tool settings; its fixed tile
  on the Capy bar opens those settings alone. Settings-only drawers keep them.
- **Every drawer keeps its bridge**, as from Sketch's title bar: the opener tile
  takes the drawer's color and joins it across the gap with concave, tab-like
  corners, rounding only exposed corners. This works from either side bar, from the
  top or bottom bar, and from Capy to its menu, including mirrored layouts and glass
  chips.
- **Selected tools** stay blue while their drawer is open. Their tile uses the
  drawer's selected-row color over the panel surface, including glass panels.
- **Sliders** show their value and brush preview beside the handle, inside the
  window, including when the bars are swapped. On the track, input follows the
  handle. Moving away blends toward the whole window along the slider axis over
  two tiles. Away from the bar, the window's extent maps linearly to slider
  position; Size keeps its logarithmic numeric curve. Desktop toolbar sliders
  use the same mapping over four tiles.

### Color and Layers drawers

- **Color** keeps all of today's contents: the wheel (or chosen shape), the color
  slots and readout, Palettes (recent colors, saved swatches, the palette chooser
  and the color's name) and, for HDR drawings, the intensity ramp. In landscape the
  wheel and Palettes sit side by side.
  In portrait they stack. The wheel uses the height left after measured fixed
  contents, capped by its column width and floored at a diameter of 128. It has
  no inner scroller; the whole drawer scrolls only when that minimum cannot fit.
  Palettes fits its width without horizontal scrolling.
- **No Color Picker button is added**, so Color matches every other layout.
  Touch and hold picks a color. That hold works for fingers only today, so a pen
  picks through a pen-button binding or a Color Picker tile added to the phone
  toolbar.
- **Layers** keeps its single column and contents: the header rows (blend mode,
  opacity, locks), the list, the action bar, and the Layer properties footer that
  opens Properties in place. It keeps its own 280 width in landscape, leaving
  most of the canvas visible; in portrait it spans the phone toolbar.
- **Heights.** In the phone layout, drawer heights replace Layers' 85% and six-row
  budget (`LayerDrawerSizing` in `effects.rs`): in landscape a drawer is as tall as
  the edge, and in portrait it fits its content up to the Capy bar. Visible rows
  come from the measured fixed content (header rows, action row and the 49-unit
  footer), so the counts here are estimates: about six 40-unit rows at 396 with
  GTK's measurements, fewer on hosts with taller controls; the list scrolls.
  While open, adding, removing or expanding rows updates the height immediately,
  including Add Paper. At the available height it stays capped and scrolls the
  newly selected row into view.
- **When content still doesn't fit** once the wheel reaches its minimum (128),
  as in a 320 × 320 window or with enlarged text, the drawer scrolls rather than
  dropping controls.

## Phone toolbar

- `DockLayout` gains `phone: Option<Vec<ToolbarTile>>`. `None` means never saved;
  `Some([])` keeps an empty toolbar. As part of the arrangement, it is
  saved and validated with it, undone with Undo Layout Change, restored by Restore
  Starting Layout and copied by New Workspace. Following
  [saved-state rules](../ui/settings.md#saving-and-loading), a workspace saved
  without the field, including any 1.0.15 workspace, still loads. The workspace
  store fills it, since only the store knows the owner: a built-in workspace gets
  its preset's toolbar and a custom one Sketch's, in the current layout, its history
  and its baseline, before validation. A toolbar saved empty stays empty.
- Its catalog is the toolbar catalog without the Size and Strength sliders and Tool settings
  (fixed on the Capy bar), and Tool Options (in the Tool drawer).
- Editing uses the toolbar interactions: hold a tile for Remove Tool and Insert
  Tools…, hold then drag to move it, and hold empty bar space for the toolbar's
  menu. At ten tools the picker shows the toolbar as full. Each edit is one layout
  undo step.
- Title bar editing is unavailable in the phone layout, since no title bar shows.
  Window hides Customize Title Bar…, and command search, bindings and tiles that run
  it are disabled with a reason, so nothing opens an invisible editor or changes the
  saved Zen setting.
- Restore Starting Layout restores what the window presents: in the phone layout,
  only the phone toolbar, as one layout history step; in larger windows, the
  arrangement, leaving the phone toolbar alone. Today it resets the whole workspace
  (`ManagerAction::Reset`), so this narrows it.
- The tablet arrangement and the phone toolbar never mirror each other.

| Workspace | Phone toolbar |
| --- | --- |
| Sketch | Filters (`Panel::Adjustments`), Select, Scale/rotate, Brush, Sculpt, Eraser, Layers, Color: its title bar's tools |
| Paint | Select, Gradient, Brush, Sculpt, Eraser, Fill, Layers, Color |
| Photo | Crop, Select, Healing, Brush, Eraser, Filters, Layers, Color |

Brush, Sculpt and Eraser stay separate tiles: painters switch between them
mid-drawing, and each remembers its own tool.
Paint's Sculpt drawer offers Blend, Paint mixing, Effects, Liquify and Retouch.
An authored Blend tile keeps its Blend, Paint mixing and Effects choices.

## Capy's menu and the Main Menu

Capy's menu has one list and an icon strip parallel to the Capy bar, on the
menu's edge nearest that bar. The strip runs from Capy's end: Preferences,
Search Commands, Draw with Finger, Zen mode, Swap Sides. Square tiles match the
measured native File–Window row height, with the Medium tile's icon ratio and
standard hover, active and selected treatment. They pack from the left in
portrait (right when swapped), or from the top in landscape, with a 2-unit gap.
Keep 8 between the tiles and the drawer edges and 8 before the native separator
that divides the strip from the list. Spare length stays at the far end; row,
tile and icon sizes follow the native text size together.

```text
Portrait                         Landscape
┌─────────────────────────┐      ┌───┬──────────────────────┐
│ • Untitled 2       2   › │      │ ⚙ │ • Untitled 2   2   › │
│ ─────────────────────── │      │ ⌕ │ ──────────────────── │
│ File                  › │      │ ☝ │ File               › │
│ Edit                  › │      │ ◐ │ Edit               › │
│ …                       │      │ ⇄ │ …                    │
│ Window                › │      │   │ Window             › │
│ ─────────────────────── │      │   │                      │
│                         │      └───┴──────────────────────┘
│ ⚙ ⌕ ☝ ◐ ⇄               │       ↑ 8 to tiles at the top
│                         │
└─────────────────────────┘
  ↑ 8 from Capy's end; 8 above/below the strip tiles
```

Swapping sides mirrors the strip beside the Capy bar. Draw with Finger and Swap
Sides show the selected-tile highlight and accessible checked state and keep the
menu open. Preferences and Search open their existing page or bar and close the
menu. Zen closes it and hides the bars.

The list starts with the current drawing's unsaved dot and name, a dim count
when several drawings are open, and the submenu caret. It opens **Drawings** in
place. A separator precedes File through Window. There is no workspace pill;
Window → Workspaces contains the saved workspace checklist and manager. A panel
in Window opens that panel as a drawer from Capy, replacing the menu and joined
to Capy by the bridge.

Drawings lists documents in tab order, with the current check in the gutter,
unsaved dots, titles that ellipsize in the middle and a close button for each.
Closing keeps the normal save prompt. A separator precedes New drawing… and its
shortcut. Selecting a drawing closes the menu. The desktop title bar retains
its selector popover.

Every page keeps the root menu's outer bounds. Portrait fits the root content
up to the top bar and then scrolls; landscape uses the Capy bar's span. One
finger drags the list and every subpage with kinetic scrolling; pen uses the
shared pen scroller. Scrolling cancels the row press and activates no row.
A subpage's horizontal drag toward the trailing side reveals its parent beneath
it; vertical drags stay with scrolling after native slop locks the axis.
Releasing past 30 percent of the page width, or faster than half a page width per
second, completes Back. Shorter, slower drags settle back. The native Back row
and keyboard navigation remain.
Menu rows, check marks, separators, text and carets use the desktop GTK menu
presentation, with one text edge.

`PhoneMenuView` supplies typed page identities, the drawing-first row list,
check gutters, close slots and ellipsis intent. The session owns the route stack
in `PhoneState`; `PhoneAction::MenuSelect`, `MenuBack` and `MenuOpen` change it.
Closing clears it, and changing menu content returns to a surviving ancestor.
`MenuActivate` rejects actions from a retired page and applies the shared
close-or-keep-open rule after a successful command.
Hosts read the published route and parent and present the menu through their
common drawer owner, including its bridge and opener treatment.

Hosts retain only `PhoneMenuContact` and native widget/animation state. The
contact value decides release eligibility, swipe axis, velocity freshness and
settle timing. Pointer moves update the local presentation; completed outcomes
dispatch actions. Back names its starting route, so an old completion cannot
change another page. Rebuilding a page or changing routes cancels its contact.
GTK constructs public command/check rows with their accessible roles fixed at
creation; Web uses the same row builder as its desktop menus. Each phone row
shows its first available shortcut. Back titles fill the centered space between
matching arrow slots, keeping the title centered independently of text width.
Native scrolling cancels the shared press and resets descendant input
controllers, releasing GTK's button state when the scroller takes the contact.

Search uses `CommandSearchStyle::placement` on both hosts. Escape leaves a
parameter page first, clears a focused nonempty search next, and otherwise
closes it; `dialog_escape` owns that ordering.

Undo and Redo (finger taps and Edit), Save and Open (File) and toolbar editing
(on the toolbar) are not repeated there.

On larger layouts, the Main Menu adds what the title bar leaves out: the drawing
row when the title bar shows no drawing tabs, and the workspace switcher when it
shows no switcher. Sketch's title bar has a switcher but no drawing tabs, so its
Main Menu gains the drawing row. Paint and Photo show both and are unchanged. A
title bar with Menu Labels and no Main Menu keeps reaching the selector through
its overflow, as today.

## Draw with Finger

Shared Rust and GTK implement the modes below. Off retains gesture-only drawing
contacts: color-picker holds, handle/object/path drags and Hand navigation. Two
fingers navigate, and two to four fingers tap. Other hosts still need their own
native input integration and qualification.

| Mode | One finger |
| --- | --- |
| **Auto** (default) | On phones and tablets, paints until a pen first draws on this installation, then behaves as Off. On desktops, behaves as Off. |
| **On** | Does what a pen does, with every tool. |
| **Off** | Gesture-only behavior. |

Every platform starts in Auto. The switch in Capy's menu shows what the current
mode does now; choosing it sets On or Off. Settings → Input offers all three.
GTK counts as Desktop at every window size, so Auto stays Off. Choose On in
Settings → Input or the phone layout's Capy menu to test finger painting.

### When a finger paints

- **A finger does what the pen does** with every tool: strokes, erasing,
  selections, figures, gradients, vector paths and the clone source. One rule
  replaces today's per-tool finger exceptions. Fingers paint at full pressure on
  every host.
- **A finger stroke starts at once but stays provisional** for the multi-finger tap
  window. If a second finger lands in that window, the stroke is withdrawn without
  an undo step and the fingers navigate, tap or scrub as today. If the finger stays
  within touch slop for the long-press time, the stroke is withdrawn and the
  touch-and-hold picker starts. This reuses the held-contact path
  (`deferred_edits.rs`) and `InputReply.cancel_paint`. The gesture recognizer,
  which today refuses while a pointer or stroke is active (`gesture_input.rs`),
  admits the provisional contact with its original time and position, so taps,
  scrubs and navigation start as if it had never painted.
- **A pen wins.** A pen landing withdraws any finger stroke, and the fingers it
  rejected stay ignored until each lifts; only contacts that start after the pen
  lifts behave normally. This is the iPad host's rule today (`PencilInput.swift`),
  moved to shared Rust for every host. Without it a palm that lands first would
  block the pen, and Auto would never see it.
- With the Eyedropper active, a finger tap picks; today it cancels the picker.
- Finger strokes get no prediction, as now.

### Pen detection and device class

- Settings stores `finger_drawing` (Auto, On, Off) and internal `pen_seen`;
  the mode appears on the Input page. Both default (Auto, not seen) so older preferences read unchanged. Swap
  Sides adds a third preference with the same rule.
- A pen counts when its drawing is admitted, in the input routing every host
  shares: not when a locked or unsupported layer refuses it, and not when it is
  cancelled while held until painting is ready. The contact keeps its identity
  through that hold. Hover and `UiAction::PenInput` device reports, which hosts send
  for tilt calibration even on hover, don't count. Neither do pointers that hosts
  only guess are pens: Android's unknown tool type and Windows' unclassified
  pointers.
- `pen_seen` is saved like other settings and reaches every window; GTK, macOS,
  Windows and iPad scenes each run their own session.
- The session uses a device class independently of window geometry. GTK is
  Desktop; shared tests cover Phone and Tablet. The remaining host integration uses Apple device type,
  Android from the display's (not the window's) smallest width at 600 dp, Web from a
  coarse pointer and the screen size. GTK, macOS and Windows are desktops. An iPad in
  a narrow window gets the phone layout but stays a tablet here.

### Hit targets that reach into the canvas

These stay, and with finger drawing they decide what a finger starting near them
does.

| Target | Reach | With finger drawing |
| --- | --- | --- |
| Docked band and split dividers | The 6 px strip on the canvas side (`layout.rs`, band dividers); Android widens it to about 16 dp (`Workspace.kt`) | A stroke starting there resizes the column, and on a collapsed column expands it |
| Floating panel resize | 6 px outside each edge and corner; Android adds 6 dp | A stroke starting there resizes the panel |
| Open drawers, column drawers, expanded panels, Zen reveal | The first contact outside them closes them and is consumed | The first finger after opening a drawer only closes it, as a pen does today |
| Zen edge peek | Within 24 px of an edge, moving 40 px inward (`zen_peek.rs`) | Peeks the docked toolbars instead of painting. The phone layout has no edge peek; Capy brings the bars back |
| A held bubble | A second finger within 90 px (`bubbles.rs`) | Joins the bubble's gesture. Bubbles are hidden in the phone layout |
| Bubble take and put back | 80 px (`WORKSPACE_PROXIMITY`) | Unchanged |
| Phone bars | Tiles cover their 2 px gaps and the margin toward the screen edge, not toward the canvas | New |
| Compact toolbar edges, canvas action bar, grips, tab strips | Their own bounds, or only during a toolbar drag | Unaffected |

No host defers the system's edge gestures today. In the phone layout, the slider
handles are excluded from Android's back gesture (within its 200 dp per edge), and
iOS defers the edges the bars occupy.

### Code and tests

- Shared Rust: the touch branch in `session.rs` (`route_input`) gains a
  finger-paints predicate beside `transform_contact`, the provisional stroke and the
  pen-wins rule; `color_picker_session.rs` (hold acceptance, Eyedropper taps);
  `clone_source.rs` (a finger sets the source); `settings.rs` (the new fields).
- Hosts: Web sends fingers as Finger rather than Mouse (`app.js`, `queuePen`; wasm
  `pen()`); Android and Windows send full pressure for fingers; GTK and Web honor
  `cancel_paint` on pointer replies, not only on blur; the iPad's resting-finger
  rejection moves to Rust.
- Docs that state today's rule: `docs/ui/shared-ui.md`, `docs/internals/brushes.md`
  and `docs/ui/vector-layers.md`.
- Tests that assert fingers never paint keep passing under Off and gain On and Auto
  cases: Rust (`vector_edge_tests.rs`, `gesture_tests.rs`, `color_picker_tests.rs`,
  `clone_source_tests.rs`, `navigation_tests.rs`, `crop_tests.rs`), GTK, Android
  (`AndroidInteractionTest`, `AndroidHostTest`), Web (`pointer.test.mjs` and the
  touch journeys), Apple (`canvas-modifiers.swift`) and Windows
  (`exercise-canvas-touch.ps1`).

## Dialogs

GTK presents editor dialogs as pages whenever the shared phone layout is active,
using their existing bodies and actions. Navigation and page actions stay in
the header; compact confirmations remain native alerts. Settings, New Drawing,
Edit Color and the workspace manager adapt their panes during allocation, with
no tick callback while idle. Manage Workspaces and Layout History defer layout
adoption until confirmation. Other hosts still need the remaining dialog rules
below; Web Settings already fills windows below 480 px wide and Android Settings
always uses a full-screen page.

- In the phone layout, every dialog opens the way Android's Settings does: a
  full-window page that slides down from the top, inside the safe area.
- Its actions move into the page's header: close on the left, and an existing
  primary action as a filled button on the right. Pages without a primary action,
  such as Preferences and Document Properties, have a filled Done at the end
  and no start X. Neither a short landscape window nor the on-screen keyboard
  can hide them; the body scrolls and
  keeps a focused field above the keyboard. Close does what the current subpage's
  cancel or back does today, so leaving New Drawing's preset browser keeps the
  drawing's draft. Secondary actions, such as Save As or Export's Preview, stay in
  the body.
- Every page uses the dialog's opaque surface and one title. Dialogs declare
  their title, primary and cancel actions explicitly. Opening preserves the
  dialog's focus when present, without focusing close or selecting label text.
  Short forms keep their fields at natural height at the top of the scrolling body.
  Preferences and Export keep their own navigation headers, without an outer
  page header. Preferences has Done at the end on every level, with only search
  at the start of its root and Back at the start of subpages. A plain Settings
  request opens the page list; a named page or reveal opens that page directly.
  Two-pane windows retain Appearance as the initial selected page. Export keeps
  its root close and subpage Back; its declared primary follows the visible
  header. The workspace manager keeps one header above the tabs, with Back only
  on subpages and Done at the end when it has no declared primary action.
- A page is modal like the dialog it replaces: it owns focus, keyboard shortcuts
  and canvas contacts while open, except during Edit Color's eyedropper, and gives
  focus back to its opener when it closes.
- There are no separate portrait designs. Side-by-side panes follow the drawer
  rule: side by side when their minimum widths fit, stacked otherwise, preview
  first. A navigation pane, like Settings' sidebar, becomes a list that opens its
  pages. Edit Color, New Drawing and Settings already adapt this way on Web.
- Short confirmations (save before closing, delete, errors) stay compact alerts.
- Dialogs that work on the canvas stay over it. Refine and Frequency Separation
  keep their panel at the bottom of the canvas, and Edit Color's eyedropper hides
  the page while picking, as it does today. Manage Workspaces and Layout History
  skip their live preview in the phone layout, since the page would cover it.
- The 480 test for Web's Settings becomes the phone layout's test, so a landscape
  phone gets the full-screen page too.

### What changes for dialogs

- **One rule in shared Rust.** Shared Rust decides each dialog's presentation (page
  or floating dialog) and whether its panes sit side by side or stack, and hosts
  drop their own breakpoints. GTK uses measured pane minima. Other hosts still
  disagree: Settings collapses at 680 px on Web, 840 dp on Android, by
  the system on Apple and never on Windows; New Drawing and Edit Color stack at
  632–744 depending on the host.
- Supply content measurement to `DialogPresentation::bounds` as a callback;
  only canvas panels need it. Full-screen pages use their shared bounds directly.
- **Pages live inside the window**, as Android's Settings overlay does, rather than
  in a toolkit dialog. GTK's automatic bottom sheet (at most 450 wide or 360 tall)
  and Apple's fitted sheets don't apply in the phone layout.
- **Minimum sizes larger than a phone window go:** GTK removes its Settings,
  New Drawing and Edit Color requests and the open-summary picture minimum.
  Apple's sheet minimums (340–380 wide, 420–440 tall) and Windows' 360 px drawing
  selector still need the same treatment.
- **Side-by-side panes without a narrow form gain one:** Settings on Windows (a
  fixed 210 sidebar) and every workspace manager page on Windows, which leaves
  20 px of details at 320 wide.
  GTK's workspace manager reuses its list and details in list/page navigation
  when their measured widths do not fit.
- **Panels that work on the canvas stay panels**, placed in the space between the
  bars: Refine and Frequency Separation, and the pen response and tilt calibration
  panels. Their shared placement and the notice anchor (`NoticePlacement`) stay
  within that space in the phone layout. Their content scrolls when
  it doesn't fit. Canvas Size and Image Size become pages like other dialogs,
  including on Android, where they are a panel today.
- **Command search** keeps its narrow placement (16 from the top below 600 wide),
  moved into shared Rust; four hosts copy that rule today and GTK lacks it.

## Hiding the bars

- The Zen command hides both bars: a four-finger tap (its new default binding on
  every host except Windows, which keeps three- and four-finger taps), Tab, View →
  Zen Mode and Capy's menu.
- Capy stays as the glass Zen Capy in its corner, whatever Zen's Show Capy
  preference says. Tapping it, four fingers again, Tab or Escape brings the bars
  back.
- The phone's hidden state drives Zen Mode's check in Capy's menu. It never writes
  the workspace's saved Zen setting and isn't saved or recovered, so a restart
  shows the bars.
- The four-finger tap toggles Zen on tablets and desktops too, wherever the host
  delivers it.

## Left-handed use

- **Swap Sides** in Capy's menu, or holding Capy and dragging it to the other
  short edge, mirrors the layout: the bars trade edges in landscape, and in
  portrait the Capy bar reverses so Capy and Size sit under the right thumb. The
  phone toolbar keeps its order.
- It is saved per installation, not per workspace. There is no separate setting
  and no guessing from palm contacts. The side follows the hand, not the text
  direction.

## Bubbles

Bubbles stay hidden in the phone layout, and dragging off a phone tile doesn't
take one. The workspace keeps them and their places for larger windows. Their
parts (a 136 px Color ring at the smallest) would cover a large share of a phone
canvas, and a pen sliding off a tile beside the canvas would take one by accident.

## Not doing

- A 0.5× interface scale, starting in Zen, or fitting the tablet arrangement.
- Folding each workspace's controls by kind into the bars.
- Moving the bars, putting both on one edge, or a third bar or second row.
- Merging Brush, Sculpt and Eraser into one drawer.
- Bubbles on phones.

## Milestones

New UI is designed and validated on GTK and approved before the other hosts port
it.

1. **Sketch's phone layout on GTK.** Shared Rust: the threshold, the Capy bar, the
   phone toolbar with Sketch's default, tile sizing, safe insets, scaling below 320,
   the transition rule and Capy's menu.
   *Done when* a windowless test lays out Sketch at 412, 375, 360, 320 and 280
   short sides in both orientations, with and without iPhone and Android insets:
   tiles are 47, 42.4, 40.5 and 35.5, the 280 window scales to 0.875, and no tile
   falls inside an inset. Crossing 479 ↔ 480 and rotating during a stroke, a slider
   drag, a toolbar reorder and an open drawer leave the saved arrangement, Zen
   setting and bubble places unchanged. GTK presents it in a phone-sized window for
   review.
2. **Menus, finger drawing, hiding and handedness on GTK.** Capy's menu with its
   drawing row and icon strip; the desktop Main Menu rule; Draw with
   Finger and its input changes; the four-finger tap; Swap Sides; bubbles hidden;
   the phone drawer sizes; slider values.
   *Done when* Rust tests cover provisional strokes withdrawn by a second finger and
   by a hold, with artwork and history unchanged; two-, three- and four-finger taps,
   navigation and the scrub after a provisional contact; palm → pen → pen lift →
   palm lift under On and Auto; refused, cancelled-while-held and admitted pen
   contacts for `pen_seen`; and hiding from a saved Zen with Show Capy off. GTK
   journeys in both themes walk every Sketch drawer and menu switch at 412, 360 and
   320 in both orientations and at 320 × 320 with enlarged text, paint with a finger
   in Auto until a pen draws, hide and restore the bars, swap sides, and switch
   among four drawings, with every Color and Layers control reachable. Each drawer
   and Capy's menu joins its opener through the bridge on every edge, mirrored or
   not, including a panel opened from Window that isn't on the toolbar. Every
   dialog opens as a page with its actions in view at 412 × 891, 891 × 412 and
   320 × 568, with the on-screen keyboard open. While a page is open, Undo, Delete
   and tool shortcuts and pen contacts don't reach the canvas. Cancelling New
   Drawing's preset naming returns to its draft, and Export's Preview works before
   a file is chosen. Refine, Frequency Separation and the calibration panels keep
   their actions between the bars at 320 × 320 with enlarged text. A binding or tile
   for title bar editing can't open the editor or change the saved Zen setting.
3. **Customizing, Paint and Photo on GTK.** Tile menus, the ten-tool picker and
   dragging on the phone toolbar, and the Paint and Photo defaults.
   *Done when* a windowless test removes, inserts and moves tiles, undoes each, and
   copies the toolbar with New Workspace, and Restore Starting Layout in the phone
   layout restores only the phone toolbar while a customized tablet arrangement
   stays unchanged. Workspaces saved without the field by a released build (Sketch,
   Paint, Photo and a custom one) load with the right toolbars, including after
   restoring an older layout from history, and an empty toolbar survives a save
   and reload.
4. **The other hosts and real devices**, after GTK approval. Web, Android, Apple
   (adding the iPhone device family) and Windows present the same shared views.
   *Done when* the milestone 1–3 journeys pass on each host in both themes,
   including toolbar holds and their cancellation, enlarged text, a right-to-left
   language with Swap Sides, and Auto across two windows and a restart. Every
   dialog opens as a page with its actions in view at 412 × 891, 891 × 412 and
   320 × 568, with the on-screen keyboard open. New UI text
   passes the localization checks. The affected painting, drawer, slider and reorder
   rows are measured on the tiers' reference hardware. On a physical phone with a
   pen and one without, no tile sits under a cutout or corner, the slider handles
   don't trigger the back gesture, and finger painting and pen takeover behave as
   tested. Emulators don't stand in for the pen, edge-gesture or tier checks.
