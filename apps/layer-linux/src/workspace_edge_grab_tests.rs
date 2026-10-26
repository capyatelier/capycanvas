use super::*;

type Scenario = fn(&Rc<Workspace>, &mut RemoteInput, &str);

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_edge_grab_columns_input --tablet"]
fn native_edge_grab_columns_input() {
    edge_grab_journey("art.capycanvas.EdgeGrabColumns", &[closed_strip, closed_stack, open_stack, left_stack, toolbar_beside_column]);
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_edge_grab_panels_input --tablet"]
fn native_edge_grab_panels_input() {
    edge_grab_journey("art.capycanvas.EdgeGrabPanels", &[group_gap, toolbar_band, floating_panel, column_reset]);
}

fn edge_grab_journey(name: &str, scenarios: &[Scenario]) {
    let app = native_test_app(name);
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    wait_workspaces(&w);
    let mut input = RemoteInput::new().settle_ms(250).timeout_secs(10);
    input.ready();
    input.click(screen_point(w.header.root.upcast_ref(), &w.window, [0.5, 0.5]));
    until(|| w.window.is_active(), "edge grab window activation");
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for device in ["mouse", "touch", "pen"] {
            println!("Checking {theme:?} {device}");
            for scenario in scenarios {
                scenario(&w, &mut input, device);
            }
        }
    }
    input.finish();
}

fn kind(device: &str) -> PointerKind {
    match device {
        "touch" => PointerKind::Touch,
        "pen" => PointerKind::Pen,
        _ => PointerKind::Mouse,
    }
}

fn middle(b: Bounds) -> [f32; 2] {
    [b.x + b.width * 0.5, b.y + b.height * 0.5]
}

fn screen(w: &Workspace, p: [f32; 2]) -> [f32; 2] {
    let origin = w.surface.compute_point(&w.window, &gtk::graphene::Point::new(p[0], p[1])).unwrap();
    [origin.x(), origin.y()]
}

fn path(w: &Workspace, device: &str, points: &[[f32; 2]], down: bool, up: bool) -> serde_json::Value {
    let mut events = Vec::new();
    if down {
        if device == "mouse" {
            events.push(contact(device, "move", screen(w, points[0])));
        }
        events.push(contact(device, "down", screen(w, points[0])));
    }
    for pair in points.windows(2) {
        for step in 1..=8 {
            let f = step as f32 / 8.;
            events.push(contact(device, "move", screen(w, [pair[0][0] + (pair[1][0] - pair[0][0]) * f, pair[0][1] + (pair[1][1] - pair[0][1]) * f])));
        }
    }
    if up {
        events.push(contact(device, "up", screen(w, *points.last().unwrap())));
    }
    serde_json::Value::Array(events)
}

fn swipe(w: &Workspace, input: &mut RemoteInput, device: &str, points: &[[f32; 2]]) {
    input.perform(path(w, device, points, true, true));
    pump(250);
}

fn open_member(w: &Workspace) -> Option<u32> {
    w.resolved().collapsed.iter().find_map(|c| c.open.as_ref().map(|o| o.column))
}

fn restore(w: &Rc<Workspace>, layout: DockLayout) {
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout, ..WorkspaceState::default() }) });
    pump(400);
}

fn durable(w: &Workspace) -> DockLayout {
    layer_ui::durable_layout(&state(w).workspace.layout)
}

fn undo(w: &Rc<Workspace>) {
    assert!(state(w).commands.iter().any(|c| c.id == CommandId::UndoWorkspace && c.enabled), "the gesture is one workspace step");
    w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    pump(250);
}

fn eligible(w: &Workspace, device: &str, xs: impl Iterator<Item = f32> + Clone, ys: impl Iterator<Item = f32>) -> [f32; 2] {
    ys.flat_map(|y| xs.clone().map(move |x| [x, y]))
        .find(|p| w.edge_grab_candidate(kind(device), *p, None).is_some())
        .unwrap_or_else(|| panic!("no {device} grab point in the edge reach"))
}

fn closed_strip(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspacePreset::Photographer.layout(Platform::Gtk));
    let r = w.resolved();
    let strip = r.collapsed.iter().find(|c| c.open.is_none()).unwrap().clone();
    let toward = if middle(strip.bounds)[0] < middle(r.work_area)[0] { 1. } else { -1. };
    let icon = strip.groups.iter().flat_map(|g| g.icons.iter()).nth(1).unwrap().clone();
    let tile = middle(icon.bounds);
    let drawers = |w: &Workspace| state(w).customization.column_drawers.len();
    swipe(w, input, device, &[tile]);
    assert_eq!(drawers(w), 1, "{device}: a tap still opens the tile's panel");
    swipe(w, input, device, &[tile]);
    assert_eq!(drawers(w), 0, "{device}: a second tap closes it");
    let before = durable(w);
    swipe(w, input, device, &[tile, [tile[0], tile[1] + 60.]]);
    assert_eq!((durable(w), open_member(w)), (before.clone(), None), "{device}: a vertical swipe never grabs the column edge");
    let settle = std::mem::replace(&mut input.settle_ms, 0);
    input.perform(path(w, device, &[tile, [tile[0] + toward * 90., tile[1]]], true, false));
    pump(150);
    assert!(!state(w).workspace.layout.is_collapsed(strip.id), "{device}: an outward swipe from a tile opens the column");
    if device != "touch" {
        assert_eq!(drag_cursor(w).as_deref(), Some("col-resize"), "{device}: opening keeps the resize pointer");
    }
    input.perform(path(w, device, &[[tile[0] + toward * 90., tile[1]]], false, true));
    input.settle_ms = settle;
    pump(250);
    undo(w);
    assert_eq!(durable(w), before);
    let edge = r.dividers.iter().filter(|d| d.axis == Axis::Horizontal)
        .min_by(|a, b| (middle(a.bounds)[0] - middle(strip.bounds)[0]).abs().total_cmp(&(middle(b.bounds)[0] - middle(strip.bounds)[0]).abs())
            .then_with(|| (toward * (middle(b.bounds)[0] - middle(a.bounds)[0])).total_cmp(&0.)))
        .unwrap();
    let line = middle(edge.bounds)[0] + toward * edge.bounds.width * 0.5;
    let canvas = [line + toward * 20., tile[1]];
    assert!(r.work_area.contains(canvas[0], canvas[1]));
    swipe(w, input, device, &[canvas, [canvas[0] + toward * 90., canvas[1]]]);
    if device == "touch" {
        assert!(!state(w).workspace.layout.is_collapsed(strip.id), "touch grabs the strip edge from the canvas");
        undo(w);
    } else {
        assert_eq!(durable(w), before, "{device}: the canvas keeps the contact");
    }
}

fn closed_stack(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspaceState::default().layout);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let columns: Vec<_> = [Panel::Brushes, Panel::Layers].into_iter().map(|panel| {
        let group = state(w).workspace.layout.panel_group(panel).unwrap();
        w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnCollapsed { group, collapsed: true } });
        state(w).workspace.layout.collapsed_column_for_group(group).unwrap()
    }).collect();
    w.dispatch(UiAction::MoveColumn { column: columns[1], target: DockTarget::StackColumn { column: columns[0], before: false }, viewport });
    pump(400);
    let r = w.resolved();
    let stack = state(w).workspace.layout.column_stack(columns[0]).column;
    let members: Vec<_> = r.collapsed.iter().filter(|c| c.stack == stack).cloned().collect();
    assert_eq!(members.len(), 2);
    let member = &members[1];
    let toward = if middle(member.bounds)[0] < middle(r.work_area)[0] { 1. } else { -1. };
    let tile = middle(member.groups[0].icons[0].bounds);
    let before = durable(w);
    swipe(w, input, device, &[tile, [tile[0] + toward * 90., tile[1]]]);
    assert_eq!(open_member(w), Some(member.id), "{device}: an outward swipe opens the member beside the contact");
    assert_eq!(durable(w), before, "{device}: opening a member is transient");
}

fn open_paint_stack(w: &Rc<Workspace>) -> (ResolvedLayout, OpenColumn, Divider) {
    restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
    if open_member(w).is_none() {
        let icon = w.resolved().collapsed[0].groups[0].icons[0].clone();
        let group = state(w).workspace.layout.panel_group(icon.panel).unwrap();
        w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel: icon.panel } });
        pump(400);
    }
    let r = w.resolved();
    let open = r.collapsed.iter().find_map(|c| c.open.clone()).expect("the Paint stack opens");
    let divider = r.dividers.iter().find(|d| r.open_column_at_divider(d.id) == Some(open.column)).unwrap().clone();
    (r, open, divider)
}

fn open_stack(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    let (r, open, divider) = open_paint_stack(w);
    let inward = if open.direction == Edge::Left { 1. } else { -1. };
    let line = middle(divider.bounds)[0] + inward * divider.bounds.width * 0.5;
    let is_open = |w: &Workspace| open_member(w) == Some(open.column);
    let navigator = r.groups.iter().find(|g| g.active == Panel::Navigator && open.bounds.contains(middle(g.bounds)[0], middle(g.bounds)[1]));
    if let Some(navigator) = navigator {
        let on_view = [line + inward * 20., middle(navigator.body())[1]];
        assert!(w.edge_grab_candidate(kind(device), on_view, None).is_none(), "{device}: the Navigator keeps its drag");
    }
    let start = eligible(w, device, (16..=32).step_by(4).map(|d| line + inward * d as f32),
        (0..(open.bounds.height / 8.) as usize).map(|i| open.bounds.y + 60. + i as f32 * 8.));
    let before = durable(w);
    let undo_before = state(w).commands.iter().any(|c| c.id == CommandId::UndoWorkspace && c.enabled);
    let settle = std::mem::replace(&mut input.settle_ms, 0);
    let claim = [start[0] + inward * 16., start[1]];
    input.perform(path(w, device, &[start, claim], true, false));
    assert!(w.workspace_drag.borrow().as_ref().is_some_and(|d| d.started && d.grab.is_some() && d.target == DragTarget::Divider(divider.id)),
        "{device}: travel along the axis grabs the open edge");
    pump(300);
    let edge = |w: &Workspace| w.resolved().dividers.iter().find(|d| d.id == divider.id).map(|d| middle(d.bounds)[0] + inward * d.bounds.width * 0.5);
    let caught = edge(w).unwrap();
    assert!((caught - claim[0]).abs() <= divider.bounds.width, "{device}: the edge catches up to the contact: {caught} vs {}", claim[0]);
    let far = [line + inward * open.bounds.width, start[1]];
    input.perform(path(w, device, &[claim, far], false, false));
    pump(100);
    assert!(!is_open(w), "{device}: dragging inward past the collapse distance closes the member");
    input.perform(path(w, device, &[far, start], false, false));
    pump(100);
    assert!(is_open(w), "{device}: reversing in the same drag reopens it");
    input.perform(path(w, device, &[start, far], false, true));
    input.settle_ms = settle;
    pump(250);
    assert!(!is_open(w));
    assert_eq!(durable(w), before, "{device}: closing is transient");
    assert_eq!(state(w).commands.iter().any(|c| c.id == CommandId::UndoWorkspace && c.enabled), undo_before);
    if device == "touch" {
        let (_, open, divider) = open_paint_stack(w);
        let line = middle(divider.bounds)[0] + inward * divider.bounds.width * 0.5;
        let canvas = [line - inward * 24., middle(open.bounds)[1]];
        let width = open.bounds.width;
        swipe(w, input, device, &[canvas, [canvas[0] - inward * 30., canvas[1]]]);
        let widened = w.resolved().collapsed.iter().find_map(|c| c.open.clone()).unwrap().bounds.width;
        assert!(widened > width + 40., "touch from the canvas grabs the open edge and widens it: {width} -> {widened}");
        undo(w);
    }
}

fn group_gap(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
    let r = w.resolved();
    let tool_set = r.groups.iter().find(|g| !g.floating && g.panels.contains(&Panel::Brushes)).unwrap().clone();
    let top = tool_set.bounds.y + tool_set.bounds.height;
    let gap = r.dividers.iter().find(|d| d.axis == Axis::Vertical && !d.band && (d.bounds.y - top).abs() < 0.5).unwrap().clone();
    let bottom = gap.bounds.y + gap.bounds.height;
    let below = r.groups.iter().find(|g| (g.bounds.y - bottom).abs() < 0.5).unwrap().clone();
    let gap_y = |w: &Workspace| w.resolved().dividers.iter().find(|d| d.id == gap.id).unwrap().bounds.y;
    let scroller = find_css(&w.groups.borrow().iter().find(|g| g.id == tool_set.id).unwrap().root.clone().upcast(), "capy-scroller")
        .and_downcast::<gtk::ScrolledWindow>().unwrap();
    let reach = if tool_set.scrolls { 24. } else { TILE_SIZE };
    let row = [middle(gap.bounds)[0], top - reach + 4.];
    assert!(w.edge_grab_candidate(kind(device), row, None).is_some(), "{device}: the foot of Tool Set starts a gap swipe");
    let before = durable(w);
    let scrolled = scroller.vadjustment().value();
    swipe(w, input, device, &[row, [row[0], row[1] + 60.]]);
    assert!(gap_y(w) - gap.bounds.y > 20., "{device}: a vertical swipe from a Tool Set row moves the gap");
    assert_eq!(scroller.vadjustment().value(), scrolled, "{device}: the swipe does not scroll Tool Set");
    undo(w);
    assert_eq!(durable(w), before);
    let tabs: Vec<_> = w.tab_hits().into_iter().filter(|t| t.group == below.id).collect();
    let tab = tabs.iter().find(|t| below.panels[t.index] != below.active).expect("an unselected tab below the gap");
    let sliver = [middle(tab.bounds)[0], bottom + 3.];
    assert!(sliver[0] < below.bounds.x + below.bounds.width - TILE_SIZE);
    swipe(w, input, device, &[sliver, [sliver[0], sliver[1] - 60.]]);
    assert!(gap.bounds.y - gap_y(w) > 10., "{device}: a vertical swipe from the top of the tabs moves the gap");
    let active = |w: &Workspace| w.resolved().groups.iter().find(|g| g.id == below.id).unwrap().active;
    assert_eq!(active(w), below.active, "{device}: a gap swipe does not select the tab under it");
    undo(w);
    assert_eq!(durable(w), before);
    swipe(w, input, device, &[sliver]);
    assert_eq!(active(w), below.panels[tab.index], "{device}: a tap on the top of a tab still selects it");
    let grip = [below.bounds.x + below.bounds.width - TILE_SIZE * 0.5, bottom + 3.];
    assert!(w.edge_grab_candidate(kind(device), grip, None).is_none(), "{device}: the group grip keeps its drag");
}

fn drag_cursor(w: &Workspace) -> Option<String> {
    let device = w.workspace_drag.borrow().as_ref()?.device.clone()?;
    w.window.surface()?.device_cursor(&device)?.name().map(|name| name.to_string())
}

fn left_stack(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
    let group = state(w).workspace.layout.panel_group(Panel::Color).unwrap();
    w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnCollapsed { group, collapsed: true } });
    pump(250);
    let column = state(w).workspace.layout.collapsed_column_for_group(group).unwrap();
    w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel: Panel::Color } });
    pump(400);
    let left_open = |w: &Workspace| w.resolved().collapsed.iter().any(|c| c.open.as_ref().is_some_and(|o| o.column == column));
    assert!(left_open(w), "the left stack opens beside the toolbar");
    let r = w.resolved();
    let open = r.collapsed.iter().find_map(|c| c.open.clone().filter(|o| o.column == column)).unwrap();
    let divider = r.dividers.iter().find(|d| r.open_column_at_divider(d.id) == Some(column)).unwrap().clone();
    let inward = if open.direction == Edge::Left { 1. } else { -1. };
    let line = middle(divider.bounds)[0] + inward * divider.bounds.width * 0.5;
    let toolbar = |w: &Workspace| w.resolved().groups.iter().find(|g| g.panels.contains(&Panel::Toolbar)).unwrap().bounds;
    let toolbar_before = toolbar(w);
    let before = durable(w);
    let start = eligible(w, device, (16..=32).step_by(4).map(|d| line + inward * d as f32),
        (0..(open.bounds.height / 8.) as usize).map(|i| open.bounds.y + 60. + i as f32 * 8.));
    swipe(w, input, device, &[start, [line + inward * open.bounds.width, start[1]]]);
    assert!(!left_open(w), "{device}: dragging the left stack's edge inward closes it");
    assert_eq!(toolbar(w), toolbar_before, "{device}: the toolbar beside the stack keeps its width");
    assert_eq!(durable(w), before);
}

fn toolbar_band(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
    let r = w.resolved();
    let band = r.dividers.iter().find(|d| d.band && d.axis == Axis::Vertical).unwrap().clone();
    let bar = r.groups.iter().find(|g| g.panels.contains(&Panel::Commands)).unwrap().bounds;
    let start = eligible(w, device, (0..bar.width as usize).map(|i| bar.x + i as f32),
        [band.bounds.y - 6., band.bounds.y - 14.].into_iter());
    let before = durable(w);
    swipe(w, input, device, &[start, [start[0], start[1] + 50.]]);
    assert_ne!(durable(w), before, "{device}: a vertical swipe from the Commands bar resizes it");
    undo(w);
    assert_eq!(durable(w), before);
    if device == "touch" {
        let canvas = [middle(band.bounds)[0], band.bounds.y + band.bounds.height + 20.];
        swipe(w, input, device, &[canvas, [canvas[0], canvas[1] + 50.]]);
        assert_ne!(durable(w), before, "touch grabs the bar's edge from the canvas");
        undo(w);
    }
}

fn floating_panel(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    w.dispatch(UiAction::MovePanel { panel: Panel::Stats, target: DockTarget::Float { position: [700., 300.] }, viewport });
    pump(400);
    let bounds = |w: &Workspace| w.resolved().groups.iter().find(|g| g.floating).unwrap().bounds;
    let b = bounds(w);
    let before = durable(w);
    let tabs = [b.x + 10., b.y + layer_ui::TAB_BAR_HEIGHT * 0.5];
    assert!(w.edge_grab_candidate(kind(device), tabs, None).is_none(), "{device}: the tab strip moves the panel");
    let left = eligible(w, device, (6..=30).step_by(4).map(|d| b.x + d as f32),
        (0..((b.height - layer_ui::TAB_BAR_HEIGHT - 60.) / 8.) as usize).map(|i| b.y + layer_ui::TAB_BAR_HEIGHT + 20. + i as f32 * 8.));
    swipe(w, input, device, &[left, [left[0] - 40., left[1]]]);
    let after = bounds(w);
    assert!(after.x < b.x - 20. && after.width > b.width + 20., "{device}: the left edge follows: {b:?} -> {after:?}");
    undo(w);
    assert_eq!(durable(w), before);
    let corner = eligible(w, device, (4..=16).step_by(4).map(|d| b.x + d as f32), (4..=16).step_by(4).map(|d| b.y + b.height - d as f32));
    swipe(w, input, device, &[corner, [corner[0] - 40., corner[1] + 40.]]);
    let after = bounds(w);
    assert!(after.x < b.x - 20. && after.width > b.width + 20. && after.height > b.height + 20., "{device}: the corner resizes both ways: {b:?} -> {after:?}");
    undo(w);
    if device == "touch" {
        let outside = [b.x + b.width + 20., b.y + b.height * 0.5];
        swipe(w, input, device, &[outside, [outside[0] + 40., outside[1]]]);
        assert!(bounds(w).width > b.width + 20., "touch grabs the right edge from the canvas");
        undo(w);
    }
    assert_eq!(durable(w), before);
}

fn column_reset(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
    let width = |w: &Workspace| w.resolved().groups.iter().find(|g| g.panels.contains(&Panel::Brushes)).unwrap().bounds.width;
    let initial = width(w);
    let divider = w.resolved().dividers.into_iter().find(|d| d.band && d.axis == Axis::Horizontal && d.bounds.x > 200.).unwrap();
    drag_divider(w, divider.id, [middle(divider.bounds)[0] + 60., middle(divider.bounds)[1]]);
    pump(250);
    assert!(width(w) > initial + 30.);
    let r = w.resolved();
    let divider = r.dividers.iter().find(|d| d.id == divider.id).unwrap().clone();
    let point = (6..=32).step_by(2).flat_map(|d| (0..60).map(move |i| [divider.bounds.x - d as f32, 120. + i as f32 * 12.]))
        .find(|p| w.edge_reset_divider(*p, kind(device)) == Some(divider.id))
        .unwrap_or_else(|| panic!("{device}: no double-press point in the column edge reach"));
    let tap = |device: &str| vec![contact(device, "down", screen(w, point)), contact(device, "up", screen(w, point))];
    let mut events = if device == "mouse" { vec![contact(device, "move", screen(w, point))] } else { Vec::new() };
    events.extend(tap(device));
    events.extend(tap(device));
    input.perform(serde_json::Value::Array(events));
    pump(300);
    assert!((width(w) - initial).abs() < 1., "{device}: a double press in the edge reach resets the column: {initial} vs {}", width(w));
}

fn toolbar_beside_column(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str) {
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    for shared in [false, true] {
        restore(w, WorkspacePreset::Illustrator.layout(Platform::Gtk));
        if shared {
            let toolbar = state(w).workspace.layout.panel_group(Panel::Toolbar).unwrap();
            w.dispatch(UiAction::MovePanel { panel: Panel::Navigator, target: DockTarget::Split { group: toolbar, edge: Edge::Right }, viewport });
            pump(400);
        }
        let neighbour = if shared { Panel::Navigator } else { Panel::Brushes };
        let bounds = |w: &Workspace, panel: Panel| w.resolved().groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds;
        let toolbar = bounds(w, Panel::Toolbar);
        let column = bounds(w, neighbour);
        let r = w.resolved();
        let gap = r.dividers.iter().find(|d| d.axis == Axis::Horizontal && (d.bounds.x - toolbar.x - toolbar.width).abs() < 0.5).unwrap().clone();
        assert_eq!(gap.owner_side, Some(Edge::Left), "shared={shared}: the gap belongs to the toolbar");
        let y = column.y + column.height * 0.5;
        for dx in [2., 10., 20.] {
            let inside = [gap.bounds.x + gap.bounds.width + dx, y];
            let claimed = w.resolved().edge_grab_at(inside, kind(device), EdgeContact::Free)
                .map(|mut grab| grab.track([inside[0] + 20., inside[1]]));
            assert!(!matches!(claimed, Some(EdgeGrabProgress::Claimed(EdgeTarget::Divider { id, .. })) if id == gap.id),
                "{device} shared={shared}: touching the column never grabs the toolbar's edge");
        }
        let before = durable(w);
        let handle = middle(gap.bounds);
        swipe(w, input, device, &[[handle[0], y], [handle[0] + 40., y]]);
        assert!(bounds(w, Panel::Toolbar).width > toolbar.width + 30., "{device} shared={shared}: dragging the gap outward grows the toolbar");
        assert!((bounds(w, neighbour).width - column.width).abs() < 1., "{device} shared={shared}: the column keeps its width");
        assert!(!state(w).workspace.layout.is_collapsed(state(w).workspace.layout.panel_group(neighbour).unwrap()));
        swipe(w, input, device, &[[handle[0] + 40., y], [0., y]]);
        assert!((bounds(w, Panel::Toolbar).width - layer_ui::TILE_SIZE).abs() < 1., "{device} shared={shared}: the toolbar shrinks back to one tile");
        assert!((bounds(w, neighbour).width - column.width).abs() < 1.);
        undo(w);
        undo(w);
        assert_eq!(durable(w), before);
    }
    restore(w, WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, true));
    let toolbar = w.resolved().groups.iter().find(|g| g.panels.contains(&Panel::Toolbar)).unwrap().bounds;
    let gap = w.resolved().dividers.into_iter().find(|d| d.axis == Axis::Horizontal && (d.bounds.x - toolbar.x - toolbar.width).abs() < 0.5).unwrap();
    let handle = middle(gap.bounds);
    swipe(w, input, device, &[[handle[0], toolbar.y + 100.], [0., toolbar.y + 100.]]);
    let after = w.resolved().groups.iter().find(|g| g.panels.contains(&Panel::Toolbar)).map(|g| g.bounds);
    assert_eq!(after.map(|b| b.width), Some(layer_ui::TILE_SIZE), "{device}: a column of toolbars shrinks to one tile without collapsing");
}
