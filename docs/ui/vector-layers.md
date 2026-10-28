# Vector layers

[Workspace and UI](README.md)

A vector layer stores paths instead of pixels
([model](../reference/authored-model.md), [rendering](../internals/rendering.md#vector-layers)).
**New vector layer** is the second button in the Layers panel footer, the item
after New layer in **Layer › New**, and a command with Ctrl+Alt+Shift+N that
command search also finds by words such as ink, line art and linework. Krita's
Shift+Insert stays Paste, as in every preset. The New submenu is there in every layer state,
including while a mask or selection layer is edited and with no layer. Vector
rows carry a path badge on their thumbnail, and Properties names the layer
type "Vector layer" under its name.

A vector layer's mask takes brush strokes like any mask, and the vector tools
leave the paths alone while it is edited. Rotate, Flip and Image Size move and
scale the paths with the canvas, scaling outline widths and keeping rectangles
and ellipses live. Clear Layer removes every path in one undo step; Fill
Selection is unavailable. Copy with a pixel selection, and the Magic Wand and
Select Color set to Editing, read what the layer shows.

## Drawing

Plain round dry brushes draw on a vector layer: G-Pen, whose pressure sets the
width, and Monoline, which keeps one width. The line follows the same Smoothing,
pressure curve, rulers and late stylus pressure updates as the brush does on a
paint layer. While the pen is down it shows as a provisional path whose tip
reaches ahead of the stylus with the paint layer's prediction; at pen-up it
becomes one path in one undo step, made of as few cubic segments as stay within
half a screen pixel of the drawn line, with smooth anchors where curves meet and
corners where the line turns sharply. Its width profile keeps pressure changes
to the same half pixel with the fewest width points. Pressure updates that
arrive after pen-up reshape that path within its undo step.

Making or selecting a vector layer while painting with a brush that cannot
draw paths switches to the last pen that drew paths, G-Pen at first; going back
to a paint layer while still painting brings that brush back, unless a brush or
tool was chosen meanwhile.
On a vector layer the Tool panel dims brushes and brush groups that cannot draw
paths, including a pen whose settings stop it: size jitter, grain, or hardness
or flow below the pen's own. Their tooltips say why, and they stay choosable. Pens leave out Spacing, Tip angle and Rotation
variation there, and the eraser shows only its size, because nothing else
shapes paths.
A refused stroke shows the canvas cursor's refusal before contact and a notice
naming the brush, such as "Pencil doesn't work on vector layers.", with **Use G-Pen** (the last pen that drew paths), **New Paint
Layer** above it and **Rasterize Layer**. Choosing a brush, Sculpt tool or other
tool that cannot act on the vector layer keeps the choice and shows the same
notice at once, before any stroke; choosing one that can closes it. When a setting keeps a pen from
drawing paths, the notice names the setting and offers to reset it instead.
Tools that need pixels, such as Gradient, offer **New Paint Layer Below** and
Rasterize Layer; the new layer sits directly under the vector layer and, when
the vector layer is clipped, clips to the same base.

The line is fitted while it is drawn, a bounded amount each frame, so pen-up
fits only its last segments. Each segment reaches as far as one cubic stays
within the tolerance, with a minimax refinement when least squares narrowly
misses it. The tolerance holds between samples too: each chord's farthest
point from the curve is solved exactly, and chords beside a sharp turn are
also checked for overshoot. Each smooth anchor is placed by trying the last few points before
that reach and a few tangents between the arriving and leaving directions, and
keeping the one that lets the next segment reach farthest. A decision that
would depend on input not yet drawn waits, and late pressure updates undo only
the decisions that saw the changed samples, so the result matches fitting the
whole line at pen-up. Width points are the fewest that keep every sample within
the tolerance, found as a shortest path.

## Figures

Figure offers Line, Rectangle, Ellipse, Polygon, Star and Path. Its tool
options add **Corner radius** for Rectangle, **Corners** for Polygon, and
**Points** and **Inner radius** for Star (3 to 100 corners or points; 5 and 50%
by default). On a vector layer each figure adds one path, selected and in front
of the others, instead of painting pixels. Line draws with round caps.
Rectangle, Ellipse, Polygon and Star keep their frame and options as a live
shape, so later moves, rotations, flips and uniform scaling keep them editable
shapes; the first corner or point of a polygon or star sits at the top of its
frame. Outline strokes with the foreground color at the line width; Fill fills
with it; Outline and Fill fills with the background and strokes with the
foreground above it. Rectangles, polygons and stars get sharp corners. Shift
makes squares, circles, regular shapes and 45° lines, and brush opacity sets
the color's alpha. Transparent paint is refused.

On a paint layer, Line, square-cornered Rectangle and Ellipse paint as before.
Rounded rectangles, polygons, stars and Path paint the same path the vector
layer would hold into the layer's pixels, in one undo step that re-bakes the
layer; an alpha-locked layer keeps its transparency, and the pixel selection
does not limit them. While drawing, they show their outline.

## Properties

With paths selected, the layer's Properties add Fill, Fill color, Outline,
Outline color, Outline width, Line ends (Flat, Round, Square) and Corners
(Sharp, Round, Beveled), with **Overlaps** (Filled, Holes) for filled paths and
**Sharp corner limit** for sharp corners. They show the front-most selected path, and a label
ends in "(mixed)" when the selected paths differ. An edit applies
to every selected path: colors change the first fill and the top outline, a path
whose style holds more fills or outlines keeps the others, and paths that shared
a style keep sharing one. Turning Fill or Outline on adds the background or
foreground color. Each change, or each drag of a number or color, is one undo
step.

## Edit Points

**Edit Points** (A), beside Move and Scale-Rotate, edits the anchors of the
selected paths; pressing another path selects it. Squares mark the anchors,
filled when selected, and selected anchors show their handles.

- Click an anchor to select it, Shift-click to add or remove it, or drag a
  marquee on empty canvas to select the anchors inside.
- Drag selected anchors to move them (Shift keeps 45° steps). Drag a handle to
  turn the curve; a smooth anchor keeps its handles in line and a symmetric one
  mirrors them, unless Alt breaks them into a corner.
- Drag a segment to bend it; click a segment to add an anchor without changing
  the curve. The new anchor lands at the click, including on straight segments.
- Double-click an anchor to switch it between corner and smooth.
- **Clear Selected** (Delete) removes the selected anchors and joins their
  neighbors; a path left with one anchor goes. Width profiles keep the widths
  at the remaining anchors.

Editing an anchor turns a rectangle or ellipse into a plain path. Anchor and
handle selection belongs to the selected paths in that layer; a history change
cannot transfer it to a different anchor. A drag that returns to the original
geometry adds no undo step. Each other drag, click or deletion is one undo step.
The Edit Points tool options offer Split at
point, Join paths, Close path and Reverse direction.

## Path commands

**Edit › Path** acts on the selected paths, each command in one undo step, and
keeps outline widths and width profiles along the result:

- **Join paths** connects the nearest ends of two open paths, merging ends
  within 6 screen pixels and otherwise adding a straight segment; the front
  path keeps its identity and style. With one open path selected it closes it.
- **Close path** adds a segment from each selected open path's end to its
  start.
- **Split at point** cuts the selected paths at the anchors selected in Edit
  Points; a closed path opens there, and the pieces stay selected.
- **Reverse direction** runs each path the other way.
- **Make compound path** combines the selected paths into one that cuts holes
  where they overlap, with the back-most path's style and place. **Release
  compound path** makes each part its own path again.

When a command can't act on the selected paths, it says what to select, such
as one or two open paths to join or a compound path to release. Joining or
combining onto a path whose outline has no width keeps the other paths'
widths, and a command that would leave the paths as they are, such as
reversing or turning a single dot, adds no undo step.

Starting G-Pen or Monoline within 6 screen pixels of an open end of a selected
path continues that end, even when the new line finishes nearer its other end,
in the same undo step as the line; the Path tool
does the same with its first click, and clicking the path's other end closes
it.

## Path

Figure's **Path** draws a path anchor by anchor. A click adds
a corner; a press and drag pulls out symmetric handles for a smooth anchor, and
Alt drags only the outgoing handle. Shift keeps the new anchor at 45° steps from
the last. Dragging a handle back to its anchor retracts it. The canvas bar's Remove Point (Backspace), Cancel (Escape) and Finish
(Enter) work as for polygon selections; Undo also removes the last anchor, and
with none left cancels the draft, without undoing earlier steps. Forward Delete
keeps its Clear Selected binding and never removes a draft anchor. Clicking the
first anchor closes the path. The finished path uses Figure's paint mode, line width and colors, with
sharp corners and flat ends, and is selected, in one undo step. On a paint
layer the draft shows as an outline and Finish paints it into the layer.

## Erasing

The Eraser, transparent paint and the pen's eraser end erase paths on a vector
layer. On a vector layer the Tools list starts with the eraser's modes, above
the eraser brushes:

- **Erase touched part**, the default, cuts paths where the eraser touches
  visible ink. The cut leaves a gap even when the eraser is narrower than the
  outline; the rest keeps its exact curve and widths, and a cut closed path
  becomes open.
- **Erase whole paths** removes every path the eraser touches.
- **Erase up to intersection** removes the touched part of a path out to the
  nearest crossings with other paths, or with itself, on either side; a path
  that crosses nothing goes entirely. Crossing positions do not depend on
  eraser size, and new caps end at the crossing.

The chosen mode is saved with the preferences. The first piece keeps the path's
identity and the others get new ones. The eraser's width is the brush size, and
one stroke is one undo step.
Picking and selection frames follow the visible caps, joins and pressure
widths. Whole-path erasing uses the width at the contact, including along a
fast sweep between input points.
Solid fills stay attached to cut paths; erasing does not subtract filled areas.

## Coloring line art

With **Fill** source **Visible** or **Editing** on a vector layer, a click inside
a filled path, or inside a path whose parts are all closed, fills it with the
foreground color below its outline, in one undo step; where such paths nest or overlap, the smallest one
around the click fills. Transparent paint removes the fill. A click
anywhere else, or using source **Reference**, adds a paint layer directly below
the vector layer, clipped to the same base when the vector layer is clipped,
and fills it there from the chosen source: all visible artwork, the edited vector layer's
lines, or the marked reference layers. Once the fill lands, a notice says it
went on a new layer and offers **Back to “Vector 1”**. Missing references are resolved before
adding a paint layer. Fill keeps its tolerance, gap closing,
expansion and smoothing; the new layer and the fill are two undo steps.
**Lasso Fill** on a vector layer adds a filled closed path, with holes where
the outline crosses itself, and selects it. Gradient and Enclose Fill offer
New Paint Layer Below.

## Moving paths

**Scale/rotate** on a vector layer chooses Move, whether or not paths are
selected, and shows as the active tool while Move or Edit Points is. Sketch,
which has no Move button, reaches paths this way, and its Select drawer on a
vector layer starts with **Select paths** (Move) and **Edit points**.

With **Move** on a vector layer, whether or not pixels are selected:

- Pressing a path selects the front-most visible, unlocked path under the pen
  within 6 screen pixels and drags it. Pressing a selected path drags the whole
  selection. Shift adds a path to the selection or removes it.
- Pressing empty canvas and dragging draws a marquee that selects the paths it
  fully encloses; Shift adds them. A click on empty canvas clears the selection.
- A dashed box marks the selection's bounds, with the eight scale handles and
  the rotate handle image layers use. Dragging a side or corner scales the
  selected paths and their outline widths from the opposite edge (Shift keeps
  the proportions, Alt scales about the centre); the rotate handle turns them
  about their centre (Shift snaps to 15°). Each drag previews live and commits
  one undo step. Rectangles and ellipses stay live shapes while the transform
  keeps them rectangles and ellipses.
- Arrow keys nudge the selected paths by one pixel, ten with Shift; a held key
  previews the move and commits it as one undo step on release.
- Alt-dragging a path drags copies of the selected paths, placed in front, and
  leaves the originals; the copies become the selection.
- **Select All** (Ctrl+A) on a vector layer with paths selects every path and
  switches to Move; on an empty vector layer it selects pixels as usual.
  **Deselect** and Escape clear the path selection, along with any pixel
  selection. **Edit › Transform** (Ctrl+T) switches to Move,
  whose frame transforms the selected paths.
- With paths selected, the Move tool options add Flip Horizontal, Flip
  Vertical, Rotate Left and Rotate Right, which turn the selection about its
  centre in one undo step, and **Scale outline widths**, on by default and
  saved with the preferences. Turned off, dragging the frame scales the paths
  but keeps their outline widths.
- Enter, or a double-click on a selected path, opens **Edit Points** for the
  whole selection; a double-click on another path opens it for that path.
  Escape in Edit Points returns to Move.
- **Copy** and **Cut** with selected paths and no pixel selection copy those
  paths. **Paste** onto an unlocked vector layer adds them in front with new
  identities and selects them; elsewhere it adds a vector layer holding them.
  Other applications receive a PNG of the copied paths.
- **Clear Selected** (Delete), labelled **Delete** while paths or anchors are
  selected, removes them.
- **Edit > Arrange** and the Move tool options restack the selected paths within
  their layer: Bring to front, Bring forward, Send backward and Send to back.
  Forward and backward step a run of selected paths past one neighboring path;
  unknown content from a newer app version is never stepped over. These commands
  have no default keys.

A drag commits one translation edit at pen-up. During the drag the moving paths
draw above the rest of their layer; they return to their place in the layer's
order when the move commits. Path selections are not undo steps and are not
restored with a session.

Scaling and rotating with the Move frame shows the selected paths' image
through the frame's transform, as a drag shows it moved, and derives the paths
once at release, so outline widths under uneven scaling, or with Scale outline
widths off, settle only then. Edit Points drags and scrubs of the path
Properties draw the selection's edited paths in place of the selection and
leave the layer as it was until release. These, and eraser strokes, keep only
the latest pen sample or value between frames, including plain sets sent during
a scrub, and preview it once in the next frame, and once per presentation
target when a host draws several frames for the same display refresh; the eraser still
cuts along every sample's segment, in order. Release
applies the last sample, so the committed paths match a preview after every
sample. Edits replace paths in place without revalidating the layer. While a drag moves, the menus, layer rows and Properties
are published again only when a preview changes the selected paths' Properties
values. Command checks for the selected paths are computed once per selection
and layer version, and trial layer edits behind menu and command availability
leave the path selection out.

## Rasterize Layer

**Rasterize Layer** turns a vector layer into a paint layer holding what its
paths show, at the document grid, in one undo step.

Merge Down of a vector layer onto another vector layer keeps paths when both
have full opacity, no mask, no clipping and no attached filters, and the upper
layer holds nothing from a newer version: the upper
layer's paths move in front of the lower layer's at their place on the canvas,
in one undo step. Other merges that would turn vector layers into pixels,
including any other Merge Down, Merge Visible, Merge Group and Stamp Visible,
first show a notice naming those layers with the merge as its action; Flatten Image
names them in the notice that counts the hidden layers it discards. The layer keeps its opacity,
blend, mask, clipping and attached effects, which still apply once, as for image
layers ([Object layers](image-objects.md)).

## Layers from a newer version

A vector layer saved by a newer version that uses paths this version cannot
read opens locked, showing the picture the newer version saved with it, and
its paths are written back unchanged when the drawing is saved.

## Tests

`vector_strokes_smooth_weigh_and_predict_input_like_raster_strokes` in
[`canvas.rs`](../../crates/layer-engine/src/canvas.rs) checks that a vector line
takes the raster stroke's dabs and shows its predicted preview on every frame,
`vector_line_previews_reach_predicted_samples_without_drawing_them` checks the
session forwards platform predictions to the preview only, and
`hand_drawn_lines_keep_few_anchors_within_tolerance`,
`recorded_lines_stay_within_tolerance_between_samples`,
`fitting_while_drawing_matches_fitting_at_pen_up` and
`late_pressure_corrections_replay_the_line` in
[`vector_stroke.rs`](../../crates/layer-engine/src/vector_stroke.rs) bound the
fitted anchors, widths and corrections, check recorded Movink strokes against
the drawn line between samples, and check that fitting while drawing gives the
same path as fitting at pen-up.
`vector_lines_land_under_the_pen_on_moved_layers`,
`late_pressure_corrections_reshape_the_drawn_line_in_its_undo_step`,
`move_tool_selects_drags_and_deletes_vector_paths`,
`arrange_commands_restack_selected_paths_one_step_at_a_time`,
`figure_shapes_on_vector_layers_become_selected_editable_paths`,
`move_tool_nudges_duplicates_and_copies_paths`,
`properties_restyle_every_selected_path_in_one_undo_step`,
`rasterize_layer_turns_a_vector_layer_into_paint_in_one_undo_step`,
`monoline_draws_constant_width_vector_lines`,
`move_frame_scales_and_rotates_selected_paths_in_one_undo_step`,
`path_tool_builds_open_and_closed_paths_anchor_by_anchor`,
`edit_points_moves_adds_deletes_and_switches_anchors`,
`vector_layer_masks_take_brush_strokes_while_paths_stay`,
`move_and_edit_points_reach_paths_while_pixels_are_selected`,
`clear_layer_removes_every_path_in_one_undo_step`,
`copy_and_the_wand_read_what_a_vector_layer_shows`,
`new_vector_layers_switch_to_a_pen_and_paint_layers_switch_back`,
`the_layer_menu_offers_new_vector_layer_in_every_state`,
`painting_brushes_on_vector_layers_offer_a_pen_a_paint_layer_or_rasterizing`,
`a_pen_setting_that_stops_paths_is_named_and_reset_from_the_refusal`,
`fill_tools_on_vector_layers_offer_a_paint_layer_below`,
`select_all_transform_flip_escape_and_delete_act_on_paths`,
`path_commands_join_split_close_reverse_and_compound_selected_paths`,
`pens_and_the_path_tool_continue_a_selected_paths_open_end`,
`figure_options_shape_rectangles_polygons_and_stars_on_both_layer_kinds`,
`erasing_up_to_intersection_stops_at_crossing_lines`,
`fill_colors_paths_or_fills_a_new_paint_layer_below_from_the_lines`,
`lasso_fill_adds_a_filled_path_on_vector_layers`,
`merging_vector_layers_keeps_paths_and_baking_them_asks_first`,
`fill_ignores_clicks_off_the_canvas_and_fills_that_change_nothing`,
`select_all_takes_only_the_paths_the_move_tool_can_pick`,
`delete_removes_paths_only_while_their_unlocked_layer_is_edited`,
`undo_waits_for_vector_lines_erasing_and_drags_to_finish`,
`cancelled_vector_gestures_leave_the_paths_and_history_alone`,
`split_needs_an_anchor_inside_a_path_and_reverse_takes_a_dot`,
`join_paths_merges_ends_within_six_screen_pixels`,
`a_setting_refusal_closes_when_another_brush_is_chosen`,
`vector_refusal_actions_check_the_layer_when_pressed`,
`every_tool_shows_the_refusal_cursor_on_a_locked_vector_layer`,
`finishing_a_path_whose_layer_changed_explains_why_and_adds_nothing`,
`path_tools_with_transparent_paint_ask_for_a_color`,
`the_pens_eraser_end_erases_paths_and_hands_back_the_pen`,
`one_flatten_notice_names_hidden_layers_and_vector_layers`,
`paint_layers_under_a_clipped_vector_layer_stay_in_its_clip`,
`fill_takes_the_innermost_path_around_the_click`,
`soft_or_light_pens_name_hardness_or_flow_in_the_refusal_and_other_settings_still_draw`,
`choosing_a_tool_that_cannot_change_a_vector_layer_explains_at_once`,
`choosing_pen_after_leaving_a_vector_layer_keeps_the_pen`,
`filling_outside_paths_names_the_new_paint_layer_and_leads_back`,
`sketch_reaches_paths_from_select_and_scale_rotate`,
`pen_and_eraser_settings_on_vector_layers_show_only_what_shapes_paths`,
`dimmed_tools_rows_tiles_and_title_bar_items_say_why`,
`undo_while_drafting_a_path_removes_its_last_point_without_touching_history`,
`path_commands_say_what_to_select_when_they_cannot_apply`,
`reversing_flipping_or_turning_a_lone_dot_adds_no_undo_step`,
`double_clicking_a_selected_path_edits_the_points_of_the_whole_selection`,
`polygon_and_star_options_hold_three_to_a_hundred_corners_and_inner_radii_up_to_one`,
`the_eraser_mode_and_outline_width_choice_come_back_with_the_preferences`,
`every_vector_tool_result_reopens_from_a_package_unchanged`,
`edit_transform_shows_the_move_frame_and_outline_widths_can_stay` and
`the_eraser_cuts_or_removes_vector_paths_in_one_undo_step` in
[`vector_line_tests.rs`](../../crates/layer-ui/src/vector_line_tests.rs) cover
the session rules. There,
`sample_bursts_preview_once_per_frame_and_commit_what_frame_by_frame_previews_commit`
checks that bursts of samples or width updates preview once per frame and
commit the same paths as a frame after every sample,
`scaling_and_turning_paths_preview_through_their_motion_and_commit_at_release`
checks that the frame previews through the motion transform and commits the
paths that transform showed,
`path_scrubs_and_anchor_drags_preview_through_motion_paths_until_release`
checks that width scrubs and anchor drags draw replacement paths at most once a
frame and edit the layer only at release,
`scrub_values_rebuild_the_preview_once_per_presentation_target` checks a 120 Hz
scrub with jittered frames and the Android benchmark's scrub with a frame after
every update, and
`moving_or_turning_a_large_path_selection_republishes_nothing_until_release`
checks that a drag of 2,000 selected paths publishes nothing until release; the
ignored `crowded_vector_gesture_costs` times those gestures, panning and the
eraser modes on 20,000 paths. `vector_layer_shortcuts_hold_in_every_keymap_preset` in
[`keymap_tests.rs`](../../crates/layer-ui/src/keymap_tests.rs) checks their keys. `vector_paths_follow_rotation_flip_and_resize` in
[`canvas_geometry_tests.rs`](../../crates/layer-core/src/canvas_geometry_tests.rs),
`merging_a_vector_layer_down_bakes_its_paths` and
`merge_down_keeps_paths_only_when_the_upper_vector_layer_holds_nothing_unknown` in
[`merge_tests.rs`](../../crates/layer-core/src/merge_tests.rs) and the GPU tests
`rasterizing_a_vector_layer_bakes_what_it_shows`,
`merging_trimmed_crossings_preserves_ink_across_raster_pages` and
`baking_crowded_paths_derives_them_off_the_canvas_thread_and_keeps_the_picture` in
[`vector_tests.rs`](../../crates/layer-render-wgpu/src/scene/vector_tests.rs)
cover canvas edits and baking; there
`pressing_selected_paths_draws_nothing_their_selection_prepared_and_deselecting_releases_it`
and `scaled_and_rotated_paths_preview_from_their_moving_image_near_the_committed_transform`
cover drags of selected paths and Scale and Rotate previews; `paths_paint_over_a_paint_layer_through_a_bake_of_the_layer_and_a_reserved_path_layer`
and the GPU test `paths_painted_onto_paint_layers_match_the_path_drawn_above_them`
cover paths painted onto paint layers,
`masks_hide_vector_layers_on_screen_and_once_rasterized_or_merged` covers layer
masks on screen and through Rasterize Layer, Apply Mask and Merge Down, and
[`ops_tests.rs`](../../crates/layer-core/src/authored/vector/ops_tests.rs) covers
the path commands' geometry and width profiles, including
`path_commands_keep_single_anchor_dots`,
`erasing_up_to_intersection_stops_at_locked_lines_and_passes_hidden_ones`,
`mirroring_a_path_keeps_each_side_width_on_its_mirrored_side` and
`joining_or_combining_onto_a_zero_width_outline_keeps_the_other_widths` and
`replacing_paths_in_place_matches_a_full_replace_and_needs_no_revalidation`;
`deleting_anchors_keeps_the_widths_and_closing_of_untouched_parts` in
[`points.rs`](../../crates/layer-core/src/authored/vector/points.rs) covers
deleting anchors. The GTK journey `native_vector_draw_move_and_delete` draws two
G-Pen lines with tablet input, drags one, undoes, selects the other with a
marquee and deletes it. `native_vector_tools` draws a Figure rectangle and a
Path, drags an anchor with Edit Points, restyles the path, scales the rectangle
with the Move frame and cuts a G-Pen line with the eraser.
`native_vector_discover_and_wrong_brush` makes a vector layer from the Layers
footer, checks the badge and the switch to G-Pen, inks a line, then answers the
vector refusal with Use G-Pen and New Paint Layer. `native_vector_color_and_canvas_changes`
fills inside an ellipse outline and outside it on a new paint layer below, then
rotates the canvas. `native_vector_select_erase_mask_and_merge` selects all paths
with Ctrl+A, flips them, clears the selection with Escape, erases up to
intersection from the eraser's drawer, hides part of a line by erasing the
layer's mask, and merges the layer into paint through the notice with that part
still hidden. `native_vector_sketch_paths_and_explanations` picks a line in Sketch
through Scale/rotate and through Select paths in the Select drawer, answers the
notice Sculpt shows at once, follows a fill outside the line back to the vector
layer, and checks that Paint's Brush tile explains itself, that a dimmed pen's tooltip says
why, that greyed Scale/rotate names the lock, and that each notice keeps the
canvas bar's height and controls. Each checks presented pixels. Run them with `--tablet` once per theme through
`CAPY_NATIVE_TEST_THEME` ([Linux](../development/linux.md)); each writes its
captures to the run's `input` directory. On a tablet,
`AndroidVectorLayerTest#gPenLinesDrawMoveAndUndoOnVectorLayers` draws, drags and
undoes a G-Pen line with stylus events in both themes and checks the screen, and
`vectorToolsDrawEditRestyleScaleAndErase` walks the same tools as
`native_vector_tools`, and `vectorMilestoneDiscoverColorEraseAndMerge` walks the
footer button, the refusal actions, coloring, Erase up to intersection and the
merge notice, and `vectorSketchExplainsDimmedToolsAndNoticesKeepTheBarShape`
chooses a dimmed pen in Sketch, follows the Fill notice and a locked layer's
greyed Scale/rotate, and checks each notice's bar shape in both themes
([Android](../development/android.md#device-tests)). On
Web, `tools/performance/workspace-motion.sh web --vector-tools` walks them with
pen events in both themes and checks presented pixels, and `--vector-milestone`
walks the same discovery, coloring, erasing and merge steps as Android, plus
Sketch's path tools, a dimmed pen that stays choosable, the reasons on greyed
tiles of a locked layer and the Fill notice's Back action, as do
the Apple `EditorLaunchTests/testVectorLayers` and `testVectorLayersDark`
([Apple](../development/apple.md)); headless Chrome omits
WebGPU pixels from screenshots, so run it headed ([Web](../development/web.md)).

The edge journeys walk what painters do around these tools. On GTK,
`native_vector_input_kinds` (`--tablet`) draws with pen and mouse, checks that one
finger neither draws nor pans, cuts lines with the pen's eraser end, checks every
tool's refusal cursor by pen and mouse, and moves paths by pen, mouse and touch,
with two fingers landing mid-stroke and mid-drag.
`native_vector_keys_and_touch_points` (`--tablet`) covers Ctrl+A, Escape, Enter,
Delete, Ctrl+E and Ctrl+Z, including Escape and Ctrl+Z during a drag or stroke,
and Edit Points by touch. `native_vector_notices_and_german_labels` switches to
German and checks the Layers footer, the refusal notice across a theme switch, a
stale notice action and the eraser modes in drawers and docked panels; run it at
the default size and with `LAYER_MOTION_VIEWPORT=1280x900` and `640x900`.
`native_vector_layer_rows` renames, duplicates, hides, locks, reorders and deletes
a vector layer from its row and menu, and `native_vector_save_reopen_and_resize`
(`--native-storage`) drags a path across a window resize, then saves, reopens and
restarts with the eraser mode kept. On Web, `--vector-edges` and `--vector-files`
cover the same ground at 1440, 1280 and 640 px, including the footer's actions
menu, and check that each notice keeps the canvas bar's shape. On Android, `vectorEdgeInputCursorsGesturesAndBackground`,
`vectorEdgeKeysTouchPointsNoticesAndGermanLabels` and
`vectorEdgeRowsFilesAndRecreate` do so in landscape and portrait, place their
points on the fitted paper, background the app mid-stroke and recreate the
activity. On Windows, the `vector-edges` and `vector-edges:light` fixtures cover
input, rows, keys, a drag across a window resize, save and reopen, and German
labels in a maximized and a narrow window; the synthetic pen there has no eraser
end.
