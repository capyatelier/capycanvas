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
    assert!(!down.handled && down.edge_cursor.is_none() && !down.edge_select_on_release && down.edge_pending);
    let pending = edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, [start[0] + 6., start[1]], 10);
    assert!(pending.edge_cursor.is_none() && !pending.handled && !pending.edge_hold && pending.edge_pending, "sideways reach never holds motion back");
    let claim = edge(&mut s, 1, ContactPhase::Move, PointerKind::Touch, [start[0] + 13., start[1]], 20);
    assert_eq!(claim.edge_cursor, Some("col-resize"));
    assert!(!claim.edge_pending);
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
    assert!(!edge(&mut s, 4, ContactPhase::Down, PointerKind::Pen, start, 400).edge_pending, "the canvas keeps the pen");
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
    assert!(!abandoned.edge_hold && abandoned.edge_cursor.is_none() && !abandoned.edge_pending, "a sideways first movement leaves the contact to the tabs");
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
    let press = |contact, previous_position| UiInput::EdgeDoublePress { kind: PointerKind::Mouse, contact, previous_position, position: [d.bounds.x - 20., 500.], viewport: VIEW };
    assert!(!s.input(press(EdgeContact::Pickup, None)).unwrap().handled);
    assert!(!s.input(press(EdgeContact::Free, Some([800., 500.]))).unwrap().handled);
    assert!(width(&s) > initial + 30.);
    assert!(s.input(press(EdgeContact::Free, Some([d.bounds.x + d.bounds.width * 0.5, 400.]))).unwrap().handled);
    assert!((width(&s) - initial).abs() < 0.5);
}

#[test]
fn a_repeated_pending_down_uses_the_new_origin_and_kind() {
    let mut s = paint();
    let d = column_edge(&s);
    let start = [d.bounds.x - 20., 500.];
    assert!(edge(&mut s, 7, ContactPhase::Down, PointerKind::Touch, start, 0).edge_pending);
    let replacement = [start[0], start[1] + 80.];
    assert!(edge(&mut s, 7, ContactPhase::Down, PointerKind::Pen, replacement, 10).edge_pending);
    let reply = edge(&mut s, 7, ContactPhase::Move, PointerKind::Touch, start, 20);
    assert!(!reply.handled && !reply.edge_pending);
    let reply = edge(&mut s, 7, ContactPhase::Move, PointerKind::Pen, [replacement[0] - 6., replacement[1]], 30);
    assert!(reply.edge_pending && !reply.handled);
    assert!(edge(&mut s, 7, ContactPhase::Move, PointerKind::Pen, [replacement[0] - 20., replacement[1]], 40).handled);
}

#[test]
fn a_reused_active_id_cancels_the_lost_contact_before_starting_again() {
    for kind in [PointerKind::Mouse, PointerKind::Pen, PointerKind::Touch] {
        for on_edge in [false, true] {
            let mut s = paint();
            let d = column_edge(&s);
            let start = [d.bounds.x - 20., 500.];
            let before = crate::durable_layout(&s.state.workspace.layout);
            edge(&mut s, 7, ContactPhase::Down, kind, start, 0);
            assert!(edge(&mut s, 7, ContactPhase::Move, kind, [start[0] - 40., start[1]], 10).handled);
            s.frame(200_000_000, 0).unwrap();
            assert_ne!(crate::durable_layout(&s.state.workspace.layout), before);
            let replacement = if on_edge { start } else { [800., 500.] };
            let reply = edge(&mut s, 7, ContactPhase::Down, kind, replacement, 300);
            assert!(!reply.handled && reply.edge_cursor.is_none());
            assert_eq!(reply.edge_pending, on_edge, "{kind:?}");
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
            assert!(!s.command(CommandId::UndoWorkspace).enabled);
            if on_edge {
                assert!(edge(&mut s, 7, ContactPhase::Move, kind, [start[0] - 6., start[1]], 310).edge_pending);
                assert!(edge(&mut s, 7, ContactPhase::Move, kind, [start[0] - 20., start[1]], 320).handled);
                edge(&mut s, 7, ContactPhase::Cancel, kind, start, 330);
                assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
            }
        }
    }
}

#[test]
fn only_another_touch_retires_a_pending_contact() {
    for kind in [PointerKind::Mouse, PointerKind::Pen, PointerKind::Touch] {
        for second in [PointerKind::Mouse, PointerKind::Pen, PointerKind::Touch] {
            let mut s = paint();
            let d = column_edge(&s);
            let start = [d.bounds.x - 20., 500.];
            assert!(edge(&mut s, 7, ContactPhase::Down, kind, start, 0).edge_pending);
            let reply = edge(&mut s, 8, ContactPhase::Down, second, start, 10);
            assert!(!reply.handled && !reply.edge_pending);
            assert_eq!(reply.edge_retired, second == PointerKind::Touch);
            assert!(!edge(&mut s, 8, ContactPhase::Move, second, [start[0] - 20., start[1]], 20).handled);
            edge(&mut s, 8, ContactPhase::Cancel, second, start, 30);
            assert_eq!(edge(&mut s, 7, ContactPhase::Move, kind, [start[0] - 20., start[1]], 40).handled,
                second != PointerKind::Touch, "{kind:?} then {second:?}");
        }
    }
}

#[test]
fn blocked_workspace_samples_cancel_pending_and_active_edges() {
    for transition in [false, true] {
        for claimed in [false, true] {
            let mut s = paint();
            let d = column_edge(&s);
            let start = [d.bounds.x - 20., 500.];
            let before = crate::durable_layout(&s.state.workspace.layout);
            assert!(edge(&mut s, 7, ContactPhase::Down, PointerKind::Touch, start, 0).edge_pending);
            if claimed {
                assert!(edge(&mut s, 7, ContactPhase::Move, PointerKind::Touch, [start[0] - 40., start[1]], 10).handled);
                s.frame(200_000_000, 0).unwrap();
                assert_ne!(crate::durable_layout(&s.state.workspace.layout), before);
            }
            if transition { s.workspace_transition = true; } else { s.set_workspace_read_only(true); }
            let reply = edge(&mut s, 7, ContactPhase::Move, PointerKind::Touch, [start[0] - 80., start[1]], 300);
            assert!(!reply.handled && !reply.edge_pending && reply.edge_cursor.is_none());
            assert!(s.edge_contacts.active.is_none() && s.edge_contacts.pending.is_none());
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
            assert!(!edge(&mut s, 7, ContactPhase::Down, PointerKind::Touch, start, 310).edge_pending);
            s.workspace_transition = false;
            s.set_workspace_read_only(false);
            assert!(!edge(&mut s, 7, ContactPhase::Move, PointerKind::Touch, [start[0] - 20., start[1]], 320).handled);
            assert!(edge(&mut s, 7, ContactPhase::Down, PointerKind::Touch, start, 330).edge_pending);
        }
    }
}

#[test]
fn a_pending_contact_cannot_resume_after_a_workspace_transition() {
    let mut s = paint();
    let d = column_edge(&s);
    let start = [d.bounds.x - 20., 500.];
    assert!(edge(&mut s, 7, ContactPhase::Down, PointerKind::Touch, start, 0).edge_pending);
    s.begin_workspace_transition().unwrap();
    s.end_workspace_transition();
    assert!(!edge(&mut s, 7, ContactPhase::Move, PointerKind::Touch, [start[0] - 20., start[1]], 10).handled);
    assert!(edge(&mut s, 7, ContactPhase::Down, PointerKind::Touch, start, 20).edge_pending);
}
