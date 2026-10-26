use super::*;
use crate::{Platform, PointerKind};

const VIEW: [f32; 2] = [1600., 1000.];
const KINDS: [PointerKind; 3] = [PointerKind::Mouse, PointerKind::Pen, PointerKind::Touch];

fn center(b: Bounds) -> [f32; 2] {
    [b.x + b.width * 0.5, b.y + b.height * 0.5]
}

fn grab(r: &ResolvedLayout, point: [f32; 2], kind: PointerKind) -> Option<u32> {
    axis_edge(r, point, kind, Axis::Horizontal)
}

fn axis_edge(r: &ResolvedLayout, point: [f32; 2], kind: PointerKind, axis: Axis) -> Option<u32> {
    contact_edge(r, point, kind, EdgeContact::Free, axis)
}

fn contact_edge(r: &ResolvedLayout, point: [f32; 2], kind: PointerKind, contact: EdgeContact, axis: Axis) -> Option<u32> {
    match r.edge_grab_at(point, kind, contact)?.edges[usize::from(axis == Axis::Vertical)]?.target {
        EdgeTarget::Divider { id, .. } => Some(id),
        EdgeTarget::Floating { .. } => None,
    }
}

fn divider(id: u32, axis: Axis, line: f32) -> Option<Reach> {
    let line = if axis == Axis::Horizontal { [line, 0.] } else { [0., line] };
    Some(Reach { target: EdgeTarget::Divider { id, axis }, line, distance: 0. })
}

fn grab_with(edges: [Option<Reach>; 2]) -> EdgeGrab {
    EdgeGrab { origin: [100., 100.], edges, corner: None, claimed: None, heading: [0.; 2], caught: [false; 2] }
}

fn body_point(r: &ResolvedLayout, x: f32) -> [f32; 2] {
    let group = r.groups.iter().filter(|g| !g.floating && g.bounds.x <= x && x <= g.bounds.x + g.bounds.width)
        .max_by(|a, b| a.body().height.total_cmp(&b.body().height)).unwrap();
    [x, center(group.body())[1]]
}

struct Strip {
    resolved: ResolvedLayout,
    strip: CollapsedColumnPlacement,
    canvas_edge: Divider,
    interior_edge: Divider,
    toward_canvas: f32,
}

fn photo_strip() -> Strip {
    let resolved = WorkspacePreset::Photographer.layout_for_device(Platform::Gtk, false).resolved(VIEW);
    let strip = resolved.collapsed.iter().find(|c| c.open.is_none()).unwrap().clone();
    let toward_canvas = if center(strip.bounds)[0] < center(resolved.work_area)[0] { 1. } else { -1. };
    let adjacent = |inner: bool| resolved.dividers.iter().find(|d| {
        d.axis == Axis::Horizontal && if (toward_canvas > 0.) == inner {
            (d.bounds.x - strip.bounds.x - strip.bounds.width).abs() < 0.5
        } else {
            (d.bounds.x + d.bounds.width - strip.bounds.x).abs() < 0.5
        }
    }).unwrap().clone();
    Strip { canvas_edge: adjacent(true), interior_edge: adjacent(false), strip, toward_canvas, resolved }
}

#[test]
fn a_closed_strip_and_one_strip_width_of_canvas_grab_its_canvas_edge() {
    let Strip { resolved: r, strip, canvas_edge, toward_canvas, .. } = photo_strip();
    let tile = center(strip.groups[0].icons[0].bounds);
    let far_side = [center(strip.bounds)[0] - toward_canvas * (strip.bounds.width * 0.5 - 1.), tile[1]];
    for kind in KINDS {
        assert_eq!(grab(&r, tile, kind), Some(canvas_edge.id), "{kind:?} on a strip tile");
        assert_eq!(grab(&r, far_side, kind), Some(canvas_edge.id), "{kind:?} beside the interior gap");
        assert_eq!(grab(&r, center(strip.grip), kind), None, "{kind:?} on the member grip");
    }
    let edge = center(canvas_edge.bounds)[0] + toward_canvas * canvas_edge.bounds.width * 0.5;
    let canvas = [edge + toward_canvas * (strip.bounds.width - 1.), tile[1]];
    assert!(r.work_area.contains(canvas[0], canvas[1]));
    assert_eq!(grab(&r, canvas, PointerKind::Touch), Some(canvas_edge.id));
    assert_eq!(grab(&r, canvas, PointerKind::Pen), None, "the canvas accepts the pen");
    assert_eq!(grab(&r, canvas, PointerKind::Mouse), None, "the canvas accepts the mouse");
    assert_eq!(grab(&r, [edge + toward_canvas * (strip.bounds.width + 1.), tile[1]], PointerKind::Touch), None);
}

#[test]
fn an_interior_gap_reaches_into_its_expanded_neighbour_but_not_the_strip_it_cannot_open_outward() {
    let Strip { resolved: r, interior_edge, toward_canvas, .. } = photo_strip();
    let edge = center(interior_edge.bounds)[0] - toward_canvas * interior_edge.bounds.width * 0.5;
    for kind in KINDS {
        let near = body_point(&r, edge - toward_canvas * (TILE_SIZE - 1.));
        assert_eq!(grab(&r, near, kind), Some(interior_edge.id), "{kind:?} inside the expanded column");
        let far = body_point(&r, edge - toward_canvas * (TILE_SIZE + 1.));
        assert_ne!(grab(&r, far, kind), Some(interior_edge.id), "{kind:?} beyond one tile");
    }
}

fn open_paint_stack(layout: &mut DockLayout) -> (ResolvedLayout, OpenColumn) {
    let root = layout.collapsed[0].root;
    layout.column_stack_mut(root).open_column = Some(root);
    let r = layout.resolved(VIEW);
    let open = r.collapsed.iter().find_map(|c| c.open.clone()).unwrap();
    (r, open)
}

#[test]
fn an_edge_reaches_into_the_column_it_resizes_but_not_its_neighbour() {
    let r = WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false).resolved(VIEW);
    let toolbar = r.groups.iter().find(|g| g.panels.contains(&Panel::Toolbar)).unwrap().bounds;
    let edge = r.dividers.iter().find(|d| d.axis == Axis::Horizontal && (d.bounds.x - toolbar.x - toolbar.width).abs() < 0.5).unwrap();
    assert_eq!(edge.owner_side, Some(Edge::Left));
    let y = center(toolbar)[1];
    for kind in KINDS {
        assert_eq!(grab(&r, [toolbar.x + 4., y], kind), Some(edge.id), "{kind:?} inside the toolbar it resizes");
        assert_ne!(grab(&r, [edge.bounds.x + edge.bounds.width + 2., y], kind), Some(edge.id), "{kind:?} inside the Tool Set column");
    }
}

#[test]
fn open_columns_reach_one_tile_inside_their_edge() {
    let (r, open) = open_paint_stack(&mut WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false));
    let edge = r.dividers.iter().find(|d| r.open_column_at_divider(d.id) == Some(open.column)).unwrap();
    let inward = if open.direction == Edge::Left { 1. } else { -1. };
    let line = center(edge.bounds)[0] + inward * edge.bounds.width * 0.5;
    assert_eq!(grab(&r, body_point(&r, line + inward * (TILE_SIZE - 1.)), PointerKind::Pen), Some(edge.id));
    assert_ne!(grab(&r, body_point(&r, line + inward * (TILE_SIZE + 1.)), PointerKind::Pen), Some(edge.id));
}

#[test]
fn group_gaps_take_the_foot_of_the_panel_above_the_gap_and_a_sliver_of_the_tabs_below() {
    let mut layout = WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false);
    let (r, open) = open_paint_stack(&mut layout);
    let gap = r.dividers.iter().find(|d| d.axis == Axis::Vertical && open.bounds.contains(center(d.bounds)[0], center(d.bounds)[1])).unwrap().clone();
    let [x, _] = center(gap.bounds);
    let [top, bottom] = [gap.bounds.y, gap.bounds.y + gap.bounds.height];
    let above = r.groups.iter().find(|g| g.bounds.contains(x, top - 1.)).unwrap().clone();
    let below = r.groups.iter().find(|g| g.bounds.contains(x, bottom + 1.)).unwrap().clone();
    assert!(above.scrolls, "an unmeasured panel counts as scrolling");
    assert!(below.tabs_visible);
    let vertical = |r: &ResolvedLayout, y: f32, contact: EdgeContact| contact_edge(r, [x, y], PointerKind::Touch, contact, Axis::Vertical);
    assert_eq!(vertical(&r, top - SCROLLING_EDGE_REACH + 1., EdgeContact::Free), Some(gap.id));
    assert_eq!(vertical(&r, top - SCROLLING_EDGE_REACH - 1., EdgeContact::Free), None, "the rest of a scrolling panel keeps its scroll");
    assert_eq!(vertical(&r, bottom + STRIP_EDGE_REACH - 1., EdgeContact::Pickup), Some(gap.id), "a sliver of the tabs below");
    assert_eq!(vertical(&r, bottom + STRIP_EDGE_REACH + 1., EdgeContact::Pickup), None, "the rest of the tab strip keeps its drag");
    let grip = [below.bounds.x + below.bounds.width - TILE_SIZE * 0.5, bottom + 2.];
    assert!(r.edge_grab_at(grip, PointerKind::Touch, EdgeContact::Pickup).is_none(), "the group grip keeps its drag");
    let row = [x, top - 10.];
    assert_eq!(contact_edge(&r, row, PointerKind::Touch, EdgeContact::Pickup, Axis::Vertical), Some(gap.id), "rows that tear off yield vertical swipes");
    assert_eq!(contact_edge(&r, row, PointerKind::Touch, EdgeContact::Pickup, Axis::Horizontal), None);
    assert!(r.edge_grab_at(row, PointerKind::Touch, EdgeContact::Input).is_none(), "sliders keep every swipe");

    layout.measurements.push(PanelMeasurement { panel: above.active, tab_width: 0., content_height: TILE_SIZE, scroll: None });
    let r = layout.resolved(VIEW);
    let gap = r.dividers.iter().find(|d| d.id == gap.id).unwrap();
    assert!(!r.groups.iter().find(|g| g.id == above.id).unwrap().scrolls);
    let top = gap.bounds.y;
    assert_eq!(vertical(&r, top - TILE_SIZE + 1., EdgeContact::Free), Some(gap.id), "a panel that fits gives a full tile");
    assert_eq!(vertical(&r, top - TILE_SIZE - 1., EdgeContact::Free), None);
}

#[test]
fn toolbar_grips_keep_their_drag() {
    let r = WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false).resolved(VIEW);
    let (group, grip) = r.groups.iter()
        .filter(|g| !g.floating)
        .find_map(|g| Some((g.clone(), g.tiles.as_ref()?.grip?)))
        .filter(|(g, grip)| grab(&r, [g.body().x + grip.x + grip.width * 0.5, g.body().y + grip.y - 4.], PointerKind::Pen).is_some())
        .expect("a docked toolbar grip beside a column edge");
    let body = group.body();
    let on_grip = [body.x + grip.x + grip.width * 0.5, body.y + grip.y + grip.height * 0.5];
    for kind in KINDS {
        assert!(r.edge_grab_at(on_grip, kind, EdgeContact::Free).is_none(), "{kind:?}");
    }
}

fn floating_target(r: &ResolvedLayout, point: [f32; 2], kind: PointerKind, contact: EdgeContact) -> Option<EdgeTarget> {
    let grab = r.edge_grab_at(point, kind, contact)?;
    grab.corner.or(grab.edges[0]).or(grab.edges[1]).map(|reach| reach.target)
}

#[test]
fn floating_panels_resize_from_their_sides_and_bottom_but_move_from_their_tabs() {
    let mut layout = WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false);
    layout.move_panel(VIEW, Panel::Navigator, DockTarget::Float { position: [700., 300.] }).unwrap();
    let r = layout.resolved(VIEW);
    let g = r.groups.iter().find(|g| g.floating).unwrap().clone();
    let b = g.bounds;
    let target = |edge| Some(EdgeTarget::Floating { group: g.id, edge });
    let middle = b.y + TAB_BAR_HEIGHT + 40.;
    let bottom_reach = if g.scrolls { SCROLLING_EDGE_REACH } else { TILE_SIZE };
    for kind in KINDS {
        assert_eq!(floating_target(&r, [b.x + TILE_SIZE - 1., middle], kind, EdgeContact::Free), target(ResizeEdge::Left), "{kind:?}");
        assert_eq!(floating_target(&r, [b.x + b.width - 4., middle], kind, EdgeContact::Free), target(ResizeEdge::Right), "{kind:?}");
        assert_eq!(floating_target(&r, [b.x + b.width * 0.5, b.y + b.height - bottom_reach + 1.], kind, EdgeContact::Free), target(ResizeEdge::Bottom));
        assert_eq!(floating_target(&r, [b.x + 4., b.y + b.height - 4.], kind, EdgeContact::Free), target(ResizeEdge::BottomLeft));
        assert_eq!(floating_target(&r, [b.x + b.width - 4., b.y + b.height - 4.], kind, EdgeContact::Free), target(ResizeEdge::BottomRight));
        assert_eq!(floating_target(&r, [b.x + 4., b.y + TAB_BAR_HEIGHT * 0.5], kind, EdgeContact::Free), None, "the tab strip moves the panel");
        assert_eq!(floating_target(&r, [b.x + b.width * 0.5, b.y + b.height - bottom_reach - 1.], kind, EdgeContact::Free), None);
        assert_eq!(floating_target(&r, [b.x + 4., middle], kind, EdgeContact::Pickup), None, "rows keep sideways swipes");
        assert_eq!(floating_target(&r, [b.x + 4., b.y + b.height - 4.], kind, EdgeContact::Pickup), target(ResizeEdge::Bottom));
    }
    let outside = [b.x - TILE_SIZE + 1., middle];
    assert_eq!(floating_target(&r, outside, PointerKind::Touch, EdgeContact::Free), target(ResizeEdge::Left));
    assert_eq!(floating_target(&r, outside, PointerKind::Pen, EdgeContact::Free), None, "the canvas accepts the pen");
    assert_eq!(floating_target(&r, [b.x - 10., b.y + b.height + 10.], PointerKind::Touch, EdgeContact::Free), target(ResizeEdge::BottomLeft));
    assert_eq!(floating_target(&r, [b.x - 10., b.y + 4.], PointerKind::Touch, EdgeContact::Free), None, "beside the tabs stays canvas");
}

#[test]
fn toolbar_bands_reach_a_tile_into_the_toolbar_and_touch_reaches_the_canvas() {
    let r = WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false).resolved(VIEW);
    let band = r.dividers.iter().find(|d| d.band && d.axis == Axis::Vertical).unwrap();
    let x = center(band.bounds)[0];
    let [top, bottom] = [band.bounds.y, band.bounds.y + band.bounds.height];
    for kind in KINDS {
        assert_eq!(axis_edge(&r, [x, top - TILE_SIZE + 1.], kind, Axis::Vertical), Some(band.id), "{kind:?} inside the toolbar");
    }
    assert!(r.work_area.contains(x, bottom + 20.));
    assert_eq!(axis_edge(&r, [x, bottom + 20.], PointerKind::Touch, Axis::Vertical), Some(band.id));
    assert_eq!(axis_edge(&r, [x, bottom + 20.], PointerKind::Pen, Axis::Vertical), None);
}

#[test]
fn double_presses_reset_column_widths_from_the_whole_column_edge_reach() {
    let (r, open) = open_paint_stack(&mut WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, false));
    let column = r.dividers.iter().find(|d| d.band && d.axis == Axis::Horizontal && r.open_column_at_divider(d.id).is_none() && d.bounds.x > 100.).unwrap();
    let line = column.bounds.x;
    let inside = body_point(&r, line - TILE_SIZE + 2.);
    assert_eq!(r.edge_reset_at(inside, PointerKind::Mouse), Some(column.id));
    assert_eq!(r.edge_reset_at(body_point(&r, line - TILE_SIZE - 2.), PointerKind::Mouse), None);
    let gap = r.dividers.iter().find(|d| d.axis == Axis::Vertical && open.bounds.contains(center(d.bounds)[0], center(d.bounds)[1])).unwrap();
    assert_eq!(r.edge_reset_at([center(gap.bounds)[0], gap.bounds.y - 4.], PointerKind::Mouse), None, "group gaps have no width to reset");
}

#[test]
fn the_swipe_direction_chooses_between_a_column_edge_and_a_group_gap() {
    let grab = grab_with([divider(1, Axis::Horizontal, 60.), divider(2, Axis::Vertical, 80.)]);
    let column = EdgeGrabProgress::Claimed(EdgeTarget::Divider { id: 1, axis: Axis::Horizontal });
    let mut horizontal = grab;
    assert_eq!(horizontal.track([100. + EDGE_GRAB_DISTANCE, 102.]), column);
    assert_eq!(horizontal.track([100., 100. + EDGE_GRAB_DISTANCE * 4.]), column, "a claimed grab keeps its edge");
    let mut vertical = grab;
    assert_eq!(vertical.track([98., 100. - EDGE_GRAB_DISTANCE]), EdgeGrabProgress::Claimed(EdgeTarget::Divider { id: 2, axis: Axis::Vertical }));
    let mut column_only = grab_with([divider(1, Axis::Horizontal, 60.), None]);
    assert_eq!(column_only.track([100., 100. + EDGE_GRAB_DISTANCE]), EdgeGrabProgress::Abandoned, "a vertical swipe keeps scrolling");
}

#[test]
fn a_grab_claims_after_a_third_of_a_tile_along_its_axis_and_yields_to_other_directions() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let along = usize::from(axis == Axis::Vertical);
        let mut edges = [None; 2];
        edges[along] = divider(1, axis, 60.);
        let grab = grab_with(edges);
        let at = |travel: f32, drift: f32| {
            let mut p = [100.; 2];
            p[along] += travel;
            p[1 - along] += drift;
            grab.clone().track(p)
        };
        assert_eq!(at(EDGE_GRAB_DISTANCE - 0.5, 0.), EdgeGrabProgress::Pending);
        assert_eq!(at(-EDGE_GRAB_DISTANCE, EDGE_GRAB_DISTANCE * 0.5), EdgeGrabProgress::Claimed(EdgeTarget::Divider { id: 1, axis }));
        assert_eq!(at(EDGE_GRAB_DISTANCE, EDGE_GRAB_DISTANCE * 0.5 + 0.5), EdgeGrabProgress::Abandoned);
        assert_eq!(at(2., EDGE_GRAB_DISTANCE), EdgeGrabProgress::Abandoned);
    }
}

#[test]
fn a_corner_claims_in_any_direction_and_eases_both_axes() {
    let corner = Reach { target: EdgeTarget::Floating { group: 3, edge: ResizeEdge::BottomRight }, line: [60., 80.], distance: 0. };
    let mut grab = EdgeGrab { corner: Some(corner), ..grab_with([None; 2]) };
    assert_eq!(grab.track([100. + EDGE_GRAB_DISTANCE * 0.75, 100. + EDGE_GRAB_DISTANCE * 0.75]), EdgeGrabProgress::Claimed(corner.target));
    assert_eq!(grab.anchor([120., 140.]), [60., 80.]);
    assert_eq!(grab.position([120., 140.], EDGE_GRAB_CATCH_UP_MS), [120., 140.]);
}

#[test]
fn a_claimed_edge_eases_after_a_contact_moving_away_from_it() {
    let mut grab = grab_with([divider(1, Axis::Horizontal, 60.), None]);
    assert!(matches!(grab.track([112., 100.]), EdgeGrabProgress::Claimed(_)));
    let contact = [120., 140.];
    assert_eq!(grab.anchor(contact), [60., 140.]);
    let mut previous = 60.;
    for elapsed in [30., 60., 90., 120.] {
        let [x, y] = grab.position(contact, elapsed);
        assert!(x > previous && x < contact[0] && y == contact[1], "{elapsed}: {x}");
        previous = x;
    }
    assert_eq!(grab.position(contact, EDGE_GRAB_CATCH_UP_MS), contact);
    assert_eq!(grab.position([90., 140.], EDGE_GRAB_CATCH_UP_MS * 4.), [90., 140.], "a caught edge follows both ways");
}

#[test]
fn an_edge_waits_for_a_contact_moving_toward_it_and_never_moves_against_the_motion() {
    let mut grab = grab_with([divider(1, Axis::Horizontal, 160.), None]);
    assert!(matches!(grab.track([112., 100.]), EdgeGrabProgress::Claimed(_)));
    for x in [112., 130., 159.] {
        assert_eq!(grab.position([x, 100.], EDGE_GRAB_CATCH_UP_MS * 2.), [160., 100.], "the edge stays until the contact reaches it");
    }
    assert_eq!(grab.position([175., 100.], EDGE_GRAB_CATCH_UP_MS * 2.), [175., 100.], "then it follows");
    assert_eq!(grab.position([150., 100.], EDGE_GRAB_CATCH_UP_MS * 2.), [150., 100.], "and keeps following after reversing");
}
