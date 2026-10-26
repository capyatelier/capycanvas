use super::*;
use crate::session::test_support::*;

const VIEW: [f32; 2] = [1600., 1000.];

fn paint() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(crate::WorkspaceState {
        layout: WorkspacePreset::Illustrator.layout(Platform::Gtk), ..Default::default()
    }) }).unwrap();
    s
}

fn edge(s: &mut UiSession<Recorder>, id: u64, phase: ContactPhase, kind: PointerKind, position: [f32; 2], ms: u64) -> InputReply {
    s.input(UiInput::EdgeContact { id, phase, kind, contact: EdgeContact::Free, position, viewport: VIEW, time_ns: ms * 1_000_000 }).unwrap()
}

fn column_edge(s: &UiSession<Recorder>) -> Divider {
    let r = s.layout(VIEW);
    let column = r.groups.iter().find(|g| g.panels.contains(&Panel::Brushes)).unwrap().bounds;
    r.dividers.into_iter().find(|d| (d.bounds.x - column.x - column.width).abs() < 0.5).unwrap()
}

#[test]
fn a_claimed_contact_drives_its_edge_and_catches_up_on_frames() {
    let mut s = paint();
    let d = column_edge(&s);
    let start = [d.bounds.x + d.bounds.width + 20., 500.];
    let down = edge(&mut s, 1, ContactPhase::Down, PointerKind::Touch, start, 0);
    assert!(!down.handled && down.edge_cursor.is_none() && !down.edge_select_on_release);
    let pending = edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, [start[0] + 6., start[1]], 10);
    assert!(pending.edge_cursor.is_none() && !pending.handled && !pending.edge_hold, "sideways reach never holds motion back");
    let claim = edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, [start[0] + 13., start[1]], 20);
    assert_eq!(claim.edge_cursor, Some("col-resize"));
    assert!(claim.handled && claim.change.canvas_wake && s.wants_continuous_frames());
    let line = |s: &UiSession<Recorder>| column_edge(s).bounds.x;
    assert_eq!(line(&s), d.bounds.x, "the edge starts where it was");
    let target = start[0] + 13. - d.bounds.width * 0.5;
    s.frame(20_000_000 + 75_000_000, 0).unwrap();
    let easing = line(&s);
    assert!(easing > d.bounds.x && easing < target, "the edge eases toward the contact: {easing}");
    s.frame(20_000_000 + 160_000_000, 0).unwrap();
    assert!((line(&s) - target).abs() < 0.5, "and reaches it");
    assert!(!s.wants_continuous_frames());
    let moved = edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, [start[0] + 40., start[1]], 300);
    assert_eq!(moved.edge_cursor, Some("col-resize"));
    assert!((line(&s) - (start[0] + 40. - d.bounds.width * 0.5)).abs() < 0.5);
    edge(&mut s, 1, ContactPhase::Up, PointerKind::Touch, [start[0] + 40., start[1]], 310);
    assert!(s.command(CommandId::UndoWorkspace).enabled, "one workspace step");
    assert!(!edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, start, 320).handled);
}

#[test]
fn a_gap_contact_holds_motion_back_and_selects_tabs_on_release_until_it_decides() {
    let mut s = paint();
    let r = s.layout(VIEW);
    let column = r.groups.iter().find(|g| g.panels.contains(&Panel::Brushes)).unwrap().bounds;
    let gap = r.dividers.iter().find(|d| d.axis == Axis::Vertical && !d.band && (d.bounds.y - column.y - column.height).abs() < 0.5).unwrap().clone();
    let start = [gap.bounds.x + gap.bounds.width * 0.5, gap.bounds.y + gap.bounds.height + 3.];
    assert!(edge(&mut s, 2, ContactPhase::Down, PointerKind::Touch, start, 0).edge_select_on_release);
    assert!(edge(&mut s, 2, ContactPhase::Move, PointerKind::Touch, [start[0], start[1] + 5.], 10).edge_hold);
    let abandoned = edge(&mut s, 2, ContactPhase::Move, PointerKind::Touch, [start[0] + 13., start[1] + 5.], 20);
    assert!(!abandoned.edge_hold && abandoned.edge_cursor.is_none(), "a sideways first movement leaves the contact to the tabs");
    edge(&mut s, 2, ContactPhase::Up, PointerKind::Touch, [start[0] + 13., start[1] + 5.], 30);
    edge(&mut s, 3, ContactPhase::Down, PointerKind::Touch, start, 40);
    assert_eq!(edge(&mut s, 3, ContactPhase::Move, PointerKind::Touch, [start[0], start[1] + 13.], 50).edge_cursor, Some("row-resize"));
}

#[test]
fn a_second_finger_or_blur_retires_the_edge() {
    let mut s = paint();
    let d = column_edge(&s);
    let start = [d.bounds.x - 20., 500.];
    edge(&mut s, 1, ContactPhase::Down, PointerKind::Touch, start, 0);
    edge(&mut s, 2, ContactPhase::Down, PointerKind::Touch, [start[0], start[1] + 100.], 5);
    assert!(edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, [start[0] - 20., start[1]], 10).edge_cursor.is_none(), "two fingers are not an edge swipe");
    let before = crate::durable_layout(&s.state.workspace.layout);
    edge(&mut s, 3, ContactPhase::Down, PointerKind::Pen, start, 100);
    edge(&mut s, 3, ContactPhase::Move, PointerKind::Pen, [start[0] - 30., start[1]], 110);
    s.frame(400_000_000, 0).unwrap();
    assert_ne!(crate::durable_layout(&s.state.workspace.layout), before);
    s.input(UiInput::Blur).unwrap();
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before, "blur cancels the resize");
    assert!(!edge(&mut s, 3, ContactPhase::Move, PointerKind::Pen, [start[0] - 60., start[1]], 120).handled);
}

#[test]
fn a_double_press_in_a_column_reach_resets_it_except_on_controls() {
    let mut s = paint();
    let d = column_edge(&s);
    let width = |s: &UiSession<Recorder>| s.layout(VIEW).groups.iter().find(|g| g.panels.contains(&Panel::Brushes)).unwrap().bounds.width;
    let initial = width(&s);
    s.dispatch(UiAction::DragDivider { id: d.id, phase: ContactPhase::Down, position: [d.bounds.x + 3., 500.], viewport: VIEW }).unwrap();
    s.dispatch(UiAction::DragDivider { id: d.id, phase: ContactPhase::Up, position: [d.bounds.x + 63., 500.], viewport: VIEW }).unwrap();
    assert!(width(&s) > initial + 30.);
    let d = column_edge(&s);
    let press = |contact| UiInput::EdgeDoublePress { kind: PointerKind::Mouse, contact, position: [d.bounds.x - 20., 500.], viewport: VIEW };
    assert!(!s.input(press(EdgeContact::Pickup)).unwrap().handled);
    assert!(s.input(press(EdgeContact::Free)).unwrap().handled);
    assert!((width(&s) - initial).abs() < 0.5);
}
