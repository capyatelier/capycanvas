# Tool bubbles

[Workspace and UI](README.md) · [Drag and reorder](drag-and-reorder.md) · [Panel transparency](panel-transparency.md)

Tool bubbles keep tools on the canvas, like brushes laid on a desk. Dragging a
title-bar button, a toolbar tool or a brush row out onto the canvas takes a
bubble of it; dropping the bubble back on a toolbar or the title bar puts it
away. Bubbles stay while painting and in Zen mode, push each other aside and
return to their places when there is room.

Shared Rust owns everything a host would otherwise decide: `bubbles.rs` holds
sources, sizes and the geometry of every part (halo arcs, color beads, the
radial chooser, brush columns, the Layers card, the count badge and readout
spots, the widget reach), which part a point lands on (`BubbleView::hit`, which
hosts use for input, hover and pressed states alike), `bubble_desk.rs` holds how
bubbles push each other, and `session_bubbles.rs` holds taps, choices, beads,
strokes, menus, history and UI text. The workspace keeps each bubble's place and
options and each brush's ring settings (`BubbleLayout`). Hosts draw the parts
at the offsets in `BubbleView`, run the frame clock and forward contacts
relative to the bubble center. GTK is the reference presenter; Web, Android, Apple and Windows
use the same views, hit testing and actions.

## What becomes a bubble

| Source | Bubble | Face |
| --- | --- | --- |
| A tool group in the title bar or a toolbar (Brush, Eraser, Sculpt, a tool slot…) | Group dial, 56 px | The tool's current icon and the title bar's drawer mark |
| A brush row in Brushes or a tool drawer | Brush, 56 px | The brush icon over its own stroke |
| A medium (Pencil, Oil…) in Brushes or a drawer | Medium, 56 px | The medium icon and one dot per brush along the lower edge; the current brush's dot is darker |
| The color picker | Brush controls, 134 px | A 56 px circle with the color picker icon, Strength on a left arc and Size on a right arc, both shown by default; **Swap sliders** in its hold menu swaps them |
| Undo or Redo | Undo and redo, 104 × 56 px | A capsule with Undo on the left and Redo on the right; each is a full 56 px circle out to the capsule's end, which answers taps and shows its hover and pressed highlight, and a side is dimmed when there is nothing to undo or redo |
| Color | Color ring, 136, 216 or 344 px (pinch: 128–352) | The Color panel's wheel on a glass plate, with beads around it |
| Layers | Layers card, opened or folded to a 56 px squircle | The current layer's thumbnail with the layer count |

Other commands, panels, sliders, Tool Options, size chips and dividers stay in
their toolbars. `BubbleSource::for_control` decides; Redo is stored as Undo, so
both make the same bubble. There is one bubble per source: taking a tool again
moves the existing bubble.

## Taking and putting back

- **Title bar and drawer rows:** drag without a hold. The contact keeps scrolling
  its list or toolbar until it leaves the bar, drawer or panel; from then on the
  bubble follows it. Releasing within 80 px (`WORKSPACE_PROXIMITY`) of where it
  came from takes nothing.
- **Toolbar tiles:** a plain drag takes a bubble the same way. Hold, then drag
  still rearranges toolbars; releasing such a drag on the canvas past the 80 px
  line takes a bubble.
- **Putting back:** drop a bubble on a toolbar, a panel or the title bar, or choose
  **Put back** in its hold menu. While the contact is over a place it would go
  back to, the bubble shrinks and fades. `BubbleAction::PutBackAll` empties the
  desk in one step.
- **Coming out:** a brush, medium or group dial comes out selected, with its
  settings halo open when **Show Sliders** is checked and its brush has settings, and Layers opens into its
  card as soon as it leaves the toolbar or title bar. Pulling out a bubble closes
  the open tool drawer; a tile pulled from a column drawer closes that drawer too.
  The drag keeps its contact as the drawer disappears. Moving a bubble already on the canvas leaves it as it was.
- Each take, move or put back is one layout undo step. A drag with no net change
  records nothing; cancelling restores the arrangement.

## Moving and pushing

Only the drawn parts of a bubble take a pen, finger or mouse; everything else,
such as the gaps between halo arcs, the space between the color plate and its
beads, and the corners around round parts, belongs to the canvas. Each halo arc
answers on its band, from the round cap behind its icon to the round cap past
its end; undo and redo answer inside their circles, and the rest of the
capsule moves it.

Bubbles move without a hold: drag the face of a tool bubble or Brush controls,
the undo and redo capsule, the folded Layers squircle, an open card anywhere but
its layer rows and controls that drag (sliders, number fields, curves), or the
color plate's rim. Two fingers move any
bubble from anywhere on it, the wheel included; while one finger is on a bubble,
the second may land up to 90 px outside it. Pinching the color ring sets its
size between 128 and 352 px in one undo step. A finger on the hue ring or disc
picks a color at once; once it moves more than a radius outside the wheel, the
ring lets go of the pick, restores the color and follows the finger by the
nearest point of its rim, easing to it instead of jumping.

A dragged bubble pushes the bubbles it meets, keeping a 6 px gap. Pushed bubbles
take 12% of the push as speed and slide with friction (0.86 per 60 Hz frame);
they keep their new places. Letting go glides a short way, at most 18 px per
frame, about 130 px in all; with reduced motion there is no glide. An open halo,
card, palette pill or chooser pushes neighbours only while it is open, each part pushing where
it is (the chooser's media, the shown medium's name tag and the brush pills, not
the empty space between them), and they return when it closes. A neighbour
caught among a chooser's parts is pushed outward from the dial, never into the
ring, and a neighbour pinned against a wall makes the open bubble give way
instead, so open parts and other bubbles never overlap; every bubble kind is
checked against an open halo and ring in open space and against walls.
Every open part, including the palette pill below a color ring, stays inside the
walls. Walls sit 8 px inside the canvas area, below the title bar and clear of docked
toolbars; bounces off them keep a quarter of their speed. Saved places keep
their distance to the nearest window corner, so a bubble near the bottom right
stays there when the window shrinks.

Motion runs in fixed 240 Hz steps, so it looks the same at any display rate; the
host steps it from its frame clock while anything moves and stops ticking when
everything rests.

## Taps, halos and the chooser

- **Show Sliders:** each tool, group, medium and Brush controls bubble has this
  shared option checked by default in its hold menu. Changing it in any bubble's
  menu changes it for every tool, group and medium in the workspace, including
  bubbles taken out later. It controls opening sliders when
  dragging the source out, picking a tool from its chooser or clicking its face.
  Unchecking it closes existing bubble sliders; checking it shows the selected
  tool's sliders. The workspace remembers the shared choice even after all bubbles
  are put back.
- **Brush:** the first tap selects the brush and, with **Show Sliders** checked,
  opens its settings halo; the next tap closes it. If the brush is already
  selected, a tap toggles its halo. With the option unchecked, taps only select.
- **Group dial and medium:** select with halo, then chooser, then close. With
  **Show Sliders** unchecked or no settings, select, then chooser, then close.
- **Halo:** the selected brush's settings sit on arcs around the bubble, within
  134 px for one to three settings and 204 px for four to six (2+2, 3+2, 3+3).
  Each arc is a 4 px track on a 28 px matte band that covers only the arc, with
  the setting's icon on the band 21 px before the track starts, clear of the
  thumb. Each band, caps included, is centred on its slot, so a single slider
  is a symmetric gauge over the top, a pair mirrors left and right, and
  neighbouring bands keep an even gap. The set part of the track is grey like the panels' sliders and the
  thumb is a 14 px circle in the toolbars' thumb color with a thin border;
  side arcs grow upward and top and bottom arcs rightward. Drag along an arc to
  set the value; its name and value show below the halo while dragging. A
  double click or double tap on an arc resets its setting to the default
  (`BubbleAction::ResetRing`, the toolbar sliders' `ResetToolSetting`); hosts
  detect it with the platform's double-click time and distance. A halo
  stays open while painting and until the bubble is tapped or another tool is
  chosen.
- **Which settings:** each brush starts with its primary settings. Its hold menu
  lists every Tool Settings row as a check item, at most six checked. The choice
  belongs to the brush and is part of the workspace.
  Blend presets share one control profile with Tool Settings: the controls
  immediately below Strength are also their default arcs, in the same order.
  See the [blender control table](../internals/brushes.md#blender-controls).
  Dither starts with Spread and Cell size; Scatter adds Grit as a third arc.
  Their Cell size uses whole pixels from 1 to 50 and starts at 4 px for Dither
  and 3 px for Scatter. Spread and Cell size follow Smoothing's normalized
  logarithmic curve.
  Customized arc selections keep their saved order. A hidden conditional
  setting keeps its saved selection and value and returns when applicable;
  it still occupies one of the six saved slots. An explicitly empty selection
  stays empty rather than returning to the defaults.
- **Chooser:** a dial rings its media (or, for tools like Select, its tools) in
  44 px circles 84 px out, from the top clockwise. The current medium is
  highlighted with its name, set radially outward beside its icon (as wide as
  the name, ellipsized only past 96 px) so it never covers a neighbour, and its
  brushes curve beside it as pills with their strokes, just past the name; when that side lacks
  room they open on the other side without the name's offset. Tapping a medium fans its brushes, even a single one; a press can slide through a medium
  onto one of its brushes. Picking a brush or tool closes the chooser and opens
  its settings halo when **Show Sliders** is checked and it has sliders; letting
  go on nothing keeps the chooser.
  A medium bubble fans its own brushes, with space for the full pill bounds
  around its face, including short fans such as Watercolor's. Near a window edge
  the media gather on the open side, in a second arc when one doesn't hold them.
  Their layout uses the saved bubble position, so wall displacement cannot
  repeatedly switch the chooser between gathered and circular arrangements.
- **Brush controls:** drag along the Size and Strength arcs, or tap a point on
  them. While a contact is on an arc, a dab of the
  current brush in the current color sits under the finger or pen at the
  canvas's zoom, behind every bubble, and follows each change; the pointer
  hides while it shows, and the setting's name and value show below. Halo arcs
  for the other settings that shape a single dab (opacity, hardness, angle,
  grain depth, material hardness, bristle scale and load;
  `tool_settings::shapes_dab`) show the same dab.
  Tapping its center starts or stops the color picker, like its tile.
- **Undo and redo:** tapping either half undoes or redoes; dragging anywhere on
  it moves it.
- **Hold** always opens the bubble's menu: a heading that can't be chosen
  (`ContextMenuItem::heading`) naming the tool, which for a dial or medium is its
  current brush or tool rather than the group, its ring settings or options and
  **Put back**. Checking or unchecking an option keeps the
  menu open; each item changes only its own option (`BubbleAction::Option`), so
  several changes in one visit never undo each other. On the color ring a hold anywhere opens it;
  on the wheel's ring and disc it takes twice the usual delay, so a slow pick
  stays a pick. A hold
  restores the color or setting its press changed; the menu stays open when the
  contact lifts, and dragging instead closes it and carries on the drag.
- **Showing and hiding:** halo arcs grow and fade in, chooser items arrive in a
  short stagger and fade out when it closes, and beads slide to new places; with
  reduced motion they appear at once.

## Color ring

The wheel is the Color panel's bare wheel, with the same shapes, snaps and color
management, on a glass plate; the plate's rim moves the bubble. The beads are
the Color panel's own swatch buttons (`WheelButton::swatch`) with its styles,
outline and selection ring. 24 px beads orbit it on up to two orbits 28 px
apart:

- **Recent colors** along the top, newest on the left, eight at most (fewer on a
  small ring). Slide along them to reach older colors; beads shrink and fade as
  they turn past either end, so the strip never ends in a jump.
- **Primary, secondary and transparent.** In SDR these beads have the Color drawer's
  sizes and sit its gap (about 10 px) off the grey rim, and the primary and
  secondary overlap like the drawer's pair, the chosen one in front. Tapping
  the secondary chooses it and brings it to the front; nothing swaps. They sit
  lower left, and move up the side when the palette is shown.
- **Palette** in two orbits wrapping the lower half, filled column by column;
  sliding either orbit turns both, and they settle on whole columns. Columns
  past the ends shrink and fade continuously instead of waiting behind other
  beads. The glass palette bead ends the orbits on the right, as far from the
  wheel as from the outer orbit, and the transparent bead keeps at least 9 px
  from it. While a whole column of colors is hidden past either end of a strip,
  one faint dot per orbit marks where they will emerge. Tapping it opens a fixed-width ‹ name › pill
  below; the arrows switch palettes and the name lists them with **Edit
  palettes…**. Picking a color or a contact elsewhere closes it. No bead
  overlaps another at any size or turn.

A new ring is the Medium circle alone. The hold menu turns **Recent colors**,
**Palette** and **Primary and secondary** on or off and chooses Small (136 px),
Medium (216 px) or Large (344 px). Putting the ring back remembers its options
and size for the next time it is taken.

In HDR drawings, GTK, Web, Android, Apple and Windows keep the wheel's size and add the Color
panel's intensity arc below it. Every point on the plate's outer edge is the
same distance from the nearest hue-ring or intensity-track edge, including
around the arc's caps.
The offset outlines meet in inward corners beside the lower bump. Primary,
secondary and transparent use the panel's swatch proportions; the palette follows
the lower outline at its normal bead spacing. The palette name sits below those
rows. Mask colors keep the circular SDR plate.

`ColorBubbleLayout` publishes the contour, wheel stage, expanded bounds, palette
placement and collision circles. Shared Rust also owns intensity picking,
rollback and color-context retirement. `BubblePresentation::color` carries the
outline, glass geometry, intensity range, ramp, marker and zero-EV tick for native presenters.
The intensity marker stays opaque for translucent paint. Adjusting intensity
preserves the paint's alpha.
`UiSession::hdr_color_bubbles` keeps that availability decision in shared Rust.

## Layers card

Layers comes out open. Tapping the folded squircle unfolds the card in place,
pushing neighbours aside; taps inside the open card change nothing, and a hold
anywhere but a layer row or a control opens the bubble's menu, with **Close**
while the card is open.
The card shares the Layers drawer's code and styles: it hosts the drawer's
`LayerInspector` (layer list, rows and properties page) in the drawer's
`dock-panel content-drawer` container, at the drawer's width and height
(`LayerDrawerSizing`, from the heights GTK measures and reports with
`BubbleAction::MeasureCard`), with the toolbars' shadow like the drawers. **Full** is the drawer
exactly: blend, opacity and lock header, full rows, layer actions and the Layer
properties footer. **Simple** keeps the approved design with the same widgets:
rows show only each layer's eye, thumbnail and name, and one bottom row in the
drawer footer's style holds the New layer icon on the left and **Properties**
with an up chevron on the right. The properties slide up over the rows as in the
drawer. There is no grab bar: dragging anywhere on the open card moves it,
except on the layer list, whose rows keep their own drag, and on controls that
drag. Contacts elsewhere fold the card, which shrinks away before the squircle
returns; turning off **Fold when not in use** in the hold menu keeps it open
until **Close**. The folded
squircle shows the current layer's thumbnail with the layer count in a badge
centered on its upper right corner: a 22 px circle that widens into a pill for
more digits.

## Look

Bubbles look like the toolbars: faces, glass beads, pills, the readout, tags and
the count badge take the panel color (the panel glass in glass mode) with no
border, a selected part layers the toolbars' selection over it exactly as a
selected toolbar tile does, and halo bands are panel glass too, blurring the
canvas under their arc. Round parts cast the toolbars' 2 px, 8 px shadow, and beads a smaller one, each
drawn as one smooth gradient around its own shape and clipped outside it, so it never
bands, never darkens the glass and never renders offscreen when scaling or
pinching. Faces, the undo and redo halves and chooser items show hover and
pressed states. Bubble, readout, tag, count and chooser text uses the standard
UI text size; brush pills are 152 x 48 px to fit it. A
pen shows no system pointer over bubbles, as over the canvas; the mouse keeps
its pointer.

## Painting

A stroke that comes within 48 px of a bubble fades it to 20% (`FADED_BUBBLE`)
over 140 ms and passes under it, its glass blur fading with it so the artwork
shows through; 180 ms after the pen lifts it fades back over 140 ms. Both fades
are one smoothstep ramp timed from the first frame they draw, and the tint, the
blur and the slider bands move together: glass changes that only fade never
wait for the canvas to confirm them. Painting closes the chooser, the palette
name and folding cards; a halo stays and fades with its bubble.

## Zen mode

Bubbles stay visible in Zen mode, like floating panels, and their drags, halos,
choosers, cards and menus keep working while other chrome stays hidden. **Show in Zen
mode** in a toolbar's menu keeps that toolbar (off by default), and a
one-finger swipe in from an edge peeks the toolbars docked there until the next
contact elsewhere. Dropping a bubble on a toolbar kept or peeked in Zen puts it
back.

Resolved groups publish `zen_kept` for saved toolbar choices and temporary edge
peeks. Every host keeps those groups visible and interactive while chrome is
hidden. Starting or dismissing a peek publishes a layout change.

## Host contract

Everything a port needs from shared Rust is on `UiSession` and in `bubbles.rs`;
a host only presents it and captures native input. GTK
(`apps/layer-linux/src/workspace_bubbles.rs`) is the reference.

### Shared API

- **Views:** `bubbles()` returns a `BubbleView` per bubble: kind, face size,
  icon and label, preview brush, dots, selected and enabled, open parts (halo
  disc, `rings` with arcs, values and readout, `chooser` items with tags,
  `beads`, `hints`, the palette `pill`, the Layers `card`), the dab `stamp`,
  undo and redo `buttons`, the layer thumbnail id and count, and options.
  `bubble_placements()` gives each center and whether it is lifted or homing.
  `bubble_presentations()` adds derived ring points, dot centers, history and
  readout positions, the palette pill and display-mapped bead and stamp colors
  for hosts crossing a serialization boundary. Native model snapshots include
  these presentations under `bubbles`; the `bubble_motion` query carries only
  placements and stroke fade state. `BubbleHitGeometry` reads a serialized view
  and uses the same hit test without accessing the render-owned session.
- **Motion:** call `step_bubbles(now_ns)` from the frame clock while it returns
  true; `reduce_bubble_motion` follows the platform's reduced-motion setting.
- **Geometry:** `BubbleView::hit` (which part a point lands on), `reach`
  (how far the widget must extend), `readout_center`, `count_badge`, `dot_centers`,
  `ChooserItem::{contains, size, tag_center}`, `chooser_tag`, `history_center`,
  `palette_pill`, `RingArc::{point, fraction, icon_point, covers}` and the
  constants (`TOOL_BUBBLE`, `HISTORY_BUBBLE`, `HALO_BANDS`, `RING_ICON`,
  `RING_TRACK`, `RING_RIM`, `CHOOSER_ITEM`, `CHOOSER_TAG`, `BRUSH_PILL`,
  `PALETTE_PILL`, `FADED_BUBBLE`, `SECOND_FINGER_REACH`, `STROKE_FADE`).
  Every part uses the placement's center, including the always-visible Controls
  arcs when `halo` is absent. Native layout wrappers may exceed the viewport;
  constraining those wrappers must not recenter their contents. Hosts measure
  text with their UI font and pass its width to `chooser_tag` and `count_badge`.
- **Actions:** `UiAction::Bubble` with `BubbleAction::Drag` (grabs `Bubble`,
  `Rim`, `Tile`, `Header`, `Variant`; phases Down, Move, Up, Cancel), `Tap`,
  `Ring`, `ResetRing`, `Wheel`, `Intensity`, `SetIntensity`, `Beads`, `Choose`, `Palette`, `Resize`, `Properties`,
  `Close`, `MeasureCard`, `Option`, `SetOptions`, `ToggleRingSetting`,
  `PutBack` and `PutBackAll`. Drag positions use workspace coordinates; other
  contact positions are relative to the bubble center.
  `Wheel` retains the initial color and pressed wheel part in Rust. Down and
  Move pick; Up keeps the last sampled color, and Cancel restores the initial
  color only in the original document and color context. Removing its owner
  retires the contact before layout undo can reuse the bubble ID. Late wheel
  and intensity events with no current owner also retire while a drawing's
  renderer is starting; they do not report a painting failure or edit that drawing.
- **Menus:** `context_menu(ContextTarget::Bubble { id })`; the first item of a
  section may be a heading (`ContextMenuItem::is_heading`). Serialized items
  expose `is_heading` and `keep_open` for the same presentation across hosts.
- **Sources:** `bubble_grab` resolves eligible toolbar and header sources;
  their native projections expose `tears_bubble` for immediate capture without
  a render-owner round trip. `can_take_bubble` checks a held toolbar drop against
  the shared home regions.
- **Painting:** `bubble_fade()` reports whether a stroke is down and which
  bubbles it faded. **Dab:** `toolbar_stamp` returns the alpha mask of the
  current brush for `BubbleView::stamp`.

### Input

GTK forwards wheel contacts through `BubbleAction::Wheel` from the bubble’s
native drag gesture and HDR contacts through `BubbleAction::Intensity`.
Double-clicking or double-tapping the intensity arc resets it to zero EV;
keyboard edits use `SetIntensity`. An intensity gesture keeps its starting range
until release, so repeated input at one point cannot change the selected value.
A hold or second touch restores the color and
intensity from before the contact. Shared Rust rejects stale contacts after a
document, paint slot or mask changes.

`native_hdr_color_bubble` checks all three sizes, optional bead groups, both
themes, mouse and touch intensity edits, reset, keyboard input, cancellation,
pinching and mask transitions. `native_hdr_color_bubble_motion` measures three
warmed six-second drags and intensity gestures per input type. Run both on the
private display; capture the first with `LAYER_NATIVE_CAPTURE_DIR`, and run the
second with `LAYER_NATIVE_EVENT_MS=4` and `LAYER_TEST_ARTIFACTS`.
`native_bubble_pickup_closes_drawers` checks drawer dismissal during pickup and
continued mouse/touch dragging in both themes. Shared pickup tests cover every
platform, including cancellation and tiles pulled from column drawers.

Web captures pointer events before canvas input and retains DOM widgets for the
wheel, swatches and Layers inspector. Mouse resets use the browser's native
double-click event. Browsers expose no system hold or double-tap thresholds;
Web uses a 500 ms hold (1000 ms on the wheel) and a 400 ms, 8 px double tap.
Web, Android and Windows forward ordered `Wheel` actions; captured color,
restoration and context validation stay in Rust. Android uses its native hold
and double-tap timing and distance, captures at the workspace, and lets the
card's native controls retain their contacts. Windows uses the native gesture
recognizer for hold timing, doubled over the wheel. Its retained overlay shares
the ordinary panel wheel renderer, swatches, brush previews, dab conversion,
layer rows and Layers inspector. Native card controls keep their own captures;
a second touch transfers capture and cancels their edit. Only while a touch is
down does the overlay admit another within the shared reach plus 90 px, keeping
that contact out of the independent canvas input source. Over an open card,
the extra surface leaves the native card exposed; its controls offer a second
contact to the bubble before starting another edit.
Canvas contacts wake the bubble frame clock even when the retained
view is unchanged, so stroke fade and its delayed return continue to run.
Native contact delivery checks the latest published bubble identities before
forwarding an event or hold timeout. Removed or replaced sources retire their
native contacts without forwarding cancellation to the replacement.
Shared workspace restoration cancels ring and wheel edits and clears transient bubble
state before adopting the replacement, including when it reuses bubble IDs.
An outside contact advances the model revision when it dismisses an open part,
even when the contact does not paint or move the camera.
An open Layers card publishes both Layers and Properties panel projections,
including the hidden page, so hosts retain ordinary panel components when those
panels are absent from the dock.

- **Targets:** a point belongs to a bubble only when `hit` returns a part;
  everything else goes to the canvas. While a contact or finger is down on a
  bubble, the bubble also takes points within `reach() +
  SECOND_FINGER_REACH` so a second finger can land.
- **Press and hold:** track the first contact (mouse button, pen or finger) in
  a capture phase. A hold needs a hit, not a layer row or card control, and
  the platform's long-press delay, doubled on the wheel's ring and disc. Moving
  more than 8 px first cancels the hold. A hold restores any color or setting
  the press changed (`Ring` Cancel) and opens the bubble's menu; moving after
  the hold closes the menu and continues as a drag from the pressed part.
- **Contacts:** on drag start map the hit: `Ring` sends `Ring` Down, Move, Up
  (or `ResetRing` when it is the second press within the platform's
  double-click time and distance, at least 8 px); `Item` sends `Choose` and
  enlarges the item under the contact; `Bead` sends `Beads`; `Face`, `Half`
  and handled `Card` points wait: a release without moving is `Tap`, moving is
  `Drag` with the `Bubble` grab. `Wheel` and `Pill` go to their own widgets; a
  wheel pick dragged past `color_drag_out` restores the color and becomes a
  `Rim` drag. In the open card, the layer list and controls that drag
  (sliders, number fields, curves, text) keep their contacts.
- **Two fingers:** the second finger cancels the first contact (restoring its
  color or value) and drags the bubble by the midpoint; on a color ring the
  finger spread pinches its size (`Resize` on release).
- **Pointer:** a pen hides the system pointer over bubbles, as over the canvas,
  and the pointer hides while the dab shows.
- **Hover and pressed:** the part under the pointer or first contact is
  `Item`, `Half` or `Face` (not on the color ring or undo and redo capsule);
  draw it with the states below.

### Presentation

- **Surfaces:** faces, undo and redo, chooser items, tags, pills, the readout
  and the count badge use the toolbar panel color (the panel glass in glass
  mode) with no border; selected parts add the toolbar selection; hovered adds
  7% and pressed 14% of the text color. Text uses the standard UI size; icons
  are 20 px on faces and 16 px on arcs. Disabled faces and chooser items draw
  at 45%, including their glass strength; stroke previews, dots, count badges
  and outside shadows retain their own opacity. Unavailable Undo and Redo icons draw
  at 35%; their circular hover highlights keep their normal opacity.
  The drawer mark is a 16 px icon inset 9 px from the face's bottom and right.
  Medium dots are 3.5 px circles at 90% text color for the current brush and
  30% for the others.
- **Shadows:** round parts cast one smooth gradient, 8 px spread, 2 px drop,
  8% black at the edge, fading quadratically and clipped outside the part; a
  lifted bubble grows it to 12 px, 4.5 px and 16%. Beads use 3 px, 1 px and
  13%. The glass palette button uses the ordinary resting and lifted shadow,
  with selection fill and no swatch outline. The Layers card uses the same
  gradients with an 18 px radius; the folded Layers face has no shadow. Disabled
  surfaces keep their shadow strength. Ordinary drawers retain the toolbars' shadow.
- **Halo arcs:** a band `arc.band` wide from `icon_point` to the end with round
  caps in the panel glass, a 4 px track at 18% text color, the set part at
  50%, a 14 px thumb (15 px while dragged) in the toolbar thumb color with a
  1 px border at 60%, and the setting's 16 px icon at `icon_point`; an
  unavailable arc draws at 40%. The name and value show at `readout_center`,
  fitting their text with 7 px horizontal and 2 px vertical padding and tabular
  digits; there is no fixed minimum width or height.
- **Dab:** draw the stamp mask in the stamp color, `stamp.diameter` times the
  stamp's extent, centered at `stamp.center`, behind every bubble.
- **Chooser:** 44 px media circles and 152 × 48 px brush pills with the brush
  stroke (68% of the pill wide, 14 px tall, 6 px from the top) over its name;
  18 px icons on media and tool choices; brush names at 80% opacity, 8 px from
  the sides and 5 px from the bottom. A shown medium or the choice under an active
  contact shows its name at `tag_center`; passive hover does not reveal hidden names.
  The tag fits its measured text width plus 18 px, at most 96 px, a 9 px corner radius
  and 3 px vertical padding.
  Items arrive scaled from 60% and fading in over 180 ms, 14 ms apart (brushes
  16 ms), and leave over 120 ms; the item under a contact grows 12% (brushes
  6%).
  Each item grows about its own center. Closing items remain until their fade
  completes, including when the chooser reopens. Changing an item's kind or label
  restarts its entry; changing its preview alone does not.
- **Halo:** grows from 85% and fades in over 160 ms, and out over 120 ms.
- **Beads:** the Color panel's swatch widget and styles, tweening 220 ms
  between layouts from their current positions, sizes and rendered alpha.
  Crossing drag slop suppresses tweening until that contact ends, including
  after returning near its origin. Hints are `HINT_DOT` dots at 35% text color.
  The palette bead has a 13 px icon. Its 184 × 30 px pill has 32 px Previous and
  Next buttons (24 px content plus 4 px padding on each side), 2 px gaps, the shared
  `go-previous`/`go-next` icons and a centered bold name in the remaining 116 px.
- **Layers card:** host the Layers drawer's own inspector in the drawer's
  container and styles; report measured page heights with `MeasureCard`.
  Unfold over 200 ms from 60%; fold over 150 ms, then the squircle returns over
  180 ms. The Layers page retains the inspector's 8 px inset, including its
  16 px contribution to measured fixed height. The simple footer has a 48 px
  Add cell with a 16 px icon and a right-aligned Properties label. The folded
  face uses a 20 px squircle radius; its 46 px thumbnail uses a 13 px radius,
  white backing and a 1 px, 28% grey outline. The count is semibold and stays
  centered at `count_badge` while its measured text width changes, with a
  minimum height of 22 px.
- **Easing:** halo, chooser, bead and card transitions use cubic ease-out
  (`1 - (1 - t)³`). Stroke fading uses the separate smoothstep below.
- **Undo and redo:** each button is a circle as tall as the capsule centered at
  `history_center`, and its highlight fills that circle.
- **Lift and home:** a lifted bubble draws at 104%; one about to be put back at
  82% and 55% opacity.
- **Fade:** follow `bubble_fade` as in [Painting](#painting): fade listed
  bubbles to `FADED_BUBBLE` and make them ignore input while the stroke is
  down; 180 ms after it ends, fade every bubble back. Use one smoothstep over
  140 ms, timed from the first frame drawn.
- **Glass:** with transparency on, every glass part blurs the canvas beneath it
  as strongly as it is drawn (`BackdropRegion::with_strength`), halo bands as
  `BackdropRegion::arc`, and a change that only fades glass must not hold the
  frame while the canvas catches up.

## Validation

- Material spacing: `bash tools/performance/workspace-motion.sh gtk --native-test=native_watercolor_chooser_spacing`
  checks Watercolor's pills at the center and a window corner, including clicking
  the material face beside a pill, with mouse and touch in both themes.
- Chooser picks: `bash tools/performance/workspace-motion.sh gtk --native-test=native_bubble_chooser_sliders`
  selects group and medium bubbles with one click, picks their brushes, then
  adjusts their newly shown sliders, toggles **Show Sliders** in the hold menu
  and checks it after putting back and dragging out again, with mouse and touch
  in both themes.
- Shared: `cargo test --locked -p layer-ui bubble`, which covers geometry,
  pushing, tear-off and put back, tap states, halos, the chooser and its slides,
  beads, the card, stroke fading, menus, history and workspace round trips
  without a window.
- GTK: `LAYER_NATIVE_TEST_TIMEOUT_MS=420000 bash tools/performance/workspace-motion.sh gtk --native-test=native_tool_bubbles`,
  with `LAYER_NATIVE_CAPTURE_DIR` for compositor screenshots. With mouse and
  touch in both themes it takes a title-bar tool, walks the taps, adjusts a halo
  arc, paints under it, chooses a medium and slides to a brush, pushes one
  bubble with another, opens the hold menu, drags the color picker out and sets
  Size and Strength on its arcs with the dab under the contact while the gaps
  around them stay canvas, double-clicks Size back to its default, undoes and redoes from the bubble taken from Redo and
  then Undo, takes the Layers card open, taps and holds inside it for the menu
  and its heading, opens its properties, adds a layer from its footer, folds it
  and closes it from the menu, picks on the wheel and
  moves the ring by its rim while the space around the plate stays canvas,
  holds it for its menu, uses the color beads and
  palette pill, drags out of the wheel, pinches the ring with fingers starting
  on the wheel, moves a dial with two fingers, checks Zen mode and puts
  everything back.
- Frames: `LAYER_NATIVE_CAPTURE_DIR=... bash tools/performance/workspace-motion.sh gtk --native-test=native_tool_bubble_frames`
  records tearing out, pinching and putting back the ring in both themes and
  fails on dark pixels outside the ring in any frame.
- Motion: `LAYER_NATIVE_EVENT_MS=4 bash tools/performance/workspace-motion.sh gtk --native-test=native_tool_bubble_motion`
  drags one bubble through four others with 250 Hz mouse and touch input and
  reports presentation rate, frame gaps and canvas GPU time for steady motion;
  it fails if steady motion refreshes any UI model. Compare it with
  `--workspace-motion` on the same machine.
- Web: `bash tools/performance/workspace-motion.sh web --tool-bubbles` runs
  mouse, touch and pen journeys in both themes. The same flag in
  `apps/layer-web/device.test.mjs` exercises tablet Chrome. The journey covers
  shared hit regions, source scrolling and tear-out, cancellation, ring reset,
  chooser selection, color, two-finger motion, Layers, glass fade, Zen, layout
  history and window edges. `tools/performance/web-bubbles.mjs` drives three
  six-second touch drags through four neighbours on a prepared tier photo;
  pair its changing DOM positions with native presentation measurements.
- **Rendered geometry:** the Web journey compares actual DOM/SVG bounds with
  shared placements, arc points, chooser bounds and text measurements, including
  multi-digit badges. Android's `AndroidToolBubblesTest` checks the same origin
  for faces and arc icons, swapped controls, a narrow viewport, intrinsic
  readouts and count badges in both themes. These assertions supplement input
  journeys: hitting a wide arc alone does not establish visual alignment.
- Apple: the mounted `tests/tool-bubbles.swift` fixture checks retained models,
  contact rollback, pinch, menus, Layers, glass strength, fades and idle work in
  both themes and Apple policies. The native XCTest journeys run separately on
  Mac and iPad. See [Apple tool bubbles](../development/apple.md#tool-bubbles)
  for builds, native input limits and motion measurement.
