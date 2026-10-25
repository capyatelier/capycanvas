use super::*;
use crate::session::test_support::*;

fn contact(t: &mut LayerTouches, id: u64, phase: ContactPhase, point: [f32; 2], row: u64, ms: u64) -> (bool, Option<LayerGesture>) {
    t.contact(&UiInput::LayerTouch { surface: 1, id, phase, position: point, row: Some(row), time_ns: 1_000_000_000 + ms * 1_000_000 }, 1, TouchPolicy::default())
}

#[test]
fn layer_two_finger_gestures_distinguish_actions_from_swipes() {
    for case in 0..4 {
        let mut t = LayerTouches::default();
        let row = if case == 3 { 20 } else { 10 };
        assert!(!contact(&mut t, 1, ContactPhase::Down, [50., 50.], 10, 0).0);
        assert!(contact(&mut t, 2, ContactPhase::Down, [100., if case == 3 { 250. } else { 50. }], row, 10).0);
        let expected = match case {
            0 => Some(LayerGesture::Opacity(10)),
            1 => {
                assert!(t.advance(1, TouchPolicy::default()).is_none());
                assert_eq!(t.advance(500_000_001, TouchPolicy::default()), Some(LayerGesture::Contents(10)));
                assert!(t.advance(900_000_001, TouchPolicy::default()).is_none());
                contact(&mut t, 1, ContactPhase::Up, [50., 50.], 10, 700);
                assert!(contact(&mut t, 2, ContactPhase::Up, [100., 50.], row, 710).1.is_none());
                continue;
            }
            2 => {
                contact(&mut t, 1, ContactPhase::Move, [110., 50.], 10, 80);
                contact(&mut t, 2, ContactPhase::Move, [160., 50.], row, 90);
                None
            }
            _ => {
                contact(&mut t, 1, ContactPhase::Move, [65., 130.], 10, 80);
                contact(&mut t, 2, ContactPhase::Move, [85., 170.], row, 90);
                Some(LayerGesture::Merge(10, 20))
            }
        };
        let points = match case { 2 => [[110., 50.], [160., 50.]], 3 => [[65., 130.], [85., 170.]], _ => [[50., 50.], [100., 50.]] };
        assert!(contact(&mut t, 1, ContactPhase::Up, points[0], 10, 110).1.is_none());
        assert_eq!(contact(&mut t, 2, ContactPhase::Up, points[1], row, 120).1, expected);
    }
}

#[test]
fn layer_gestures_reject_cancel_extra_contacts_surfaces_and_drag_pickup() {
    for case in 0..5 {
        let mut t = LayerTouches::default();
        contact(&mut t, 1, ContactPhase::Down, [50., 50.], 10, 0);
        if case == 4 { contact(&mut t, 1, ContactPhase::Move, [50., 100.], 10, 5); }
        contact(&mut t, 2, ContactPhase::Down, [100., 50.], 10, 10);
        match case {
            0 => { contact(&mut t, 1, ContactPhase::Cancel, [50., 50.], 10, 20); }
            1 => { contact(&mut t, 3, ContactPhase::Down, [150., 50.], 10, 20); }
            2 => { t.contact(&UiInput::LayerTouch { surface: 2, id: 3, phase: ContactPhase::Down, position: [150., 50.], row: Some(10), time_ns: 1_020_000_000 }, 1, TouchPolicy::default()); }
            3 => { t.contact(&UiInput::LayerTouch { surface: 1, id: 2, phase: ContactPhase::Move, position: [100., 50.], row: Some(10), time_ns: 1_020_000_000 }, 2, TouchPolicy::default()); }
            _ => (),
        }
        assert!(t.advance(1, TouchPolicy::default()).is_none());
        assert!(t.advance(1_000_000_001, TouchPolicy::default()).is_none());
        assert!(contact(&mut t, 1, ContactPhase::Up, [50., 50.], 10, 100).1.is_none());
        assert!(contact(&mut t, 2, ContactPhase::Up, [100., 50.], 10, 110).1.is_none());
    }
}

fn input(s: &mut UiSession<Recorder>, id: u64, phase: ContactPhase, point: [f32; 2], row: u64, ms: u64) -> InputReply {
    s.input(UiInput::LayerTouch { surface: 1, id, phase, position: point, row: Some(row), time_ns: 1_000_000_000 + ms * 1_000_000 }).unwrap()
}

#[test]
fn layer_tap_opens_opacity_and_two_finger_swipes_leave_layers_unchanged_on_every_host() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(platform);
        let id = s.state.layers.iter().find(|l| l.editing).unwrap().id;
        let revision = s.engine.document().revision;
        for (finger, x) in [(1, 50.), (2, 100.)] { input(&mut s, finger, ContactPhase::Down, [x, 50.], id, finger * 10); }
        for (finger, x) in [(1, 50.), (2, 100.)] { input(&mut s, finger, ContactPhase::Up, [x, 50.], id, 100 + finger * 10); }
        assert_eq!(s.state.layer_tools.opacity_popup, Some(id), "{platform:?}");
        assert_eq!(s.engine.document().revision, revision);
        layer(&mut s, LayerAction::CloseOpacity);
        for start in [500, 1000] {
            layer(&mut s, LayerAction::ToggleSelection { id });
            let selected = s.state.layers.iter().find(|l| l.id == id).unwrap().selected;
            let checkpoint = s.engine.checkpoint();
            for (finger, x) in [(1, 50.), (2, 100.)] { input(&mut s, finger, ContactPhase::Down, [x, 50.], id, start + finger * 10); }
            for (finger, x) in [(1, 110.), (2, 160.)] { input(&mut s, finger, ContactPhase::Move, [x, 50.], id, start + 80 + finger * 10); }
            for (finger, x) in [(1, 110.), (2, 160.)] { input(&mut s, finger, ContactPhase::Up, [x, 50.], id, start + 120 + finger * 10); }
            assert_eq!(s.state.layers.iter().find(|l| l.id == id).unwrap().selected, selected);
            assert!(!s.state.layers.iter().find(|l| l.id == id).unwrap().alpha_locked);
            assert!(s.state.layer_tools.opacity_popup.is_none());
            assert_eq!(s.engine.checkpoint(), checkpoint, "{platform:?}: swiping leaves history unchanged");
        }
    }
}

#[test]
fn layer_focus_loss_retires_contacts_before_the_next_gesture() {
    let mut s = session(Platform::Gtk);
    let id = s.state.layers.iter().find(|layer| layer.editing).unwrap().id;
    input(&mut s, 1, ContactPhase::Down, [50., 50.], id, 10);
    input(&mut s, 2, ContactPhase::Down, [100., 50.], id, 20);
    s.input(UiInput::Blur).unwrap();
    s.frame(1_900_000_000, 1_900_000_000).unwrap();
    assert!(s.state.layer_tools.opacity_popup.is_none());
    for (finger, x) in [(3, 50.), (4, 100.)] { input(&mut s, finger, ContactPhase::Down, [x, 50.], id, 1000 + finger * 10); }
    for (finger, x) in [(3, 50.), (4, 100.)] { input(&mut s, finger, ContactPhase::Up, [x, 50.], id, 1100 + finger * 10); }
    assert_eq!(s.state.layer_tools.opacity_popup, Some(id));
}

#[test]
fn coalesced_layer_contacts_validate_time_per_finger() {
    for reversed in [false, true] {
        let mut t = LayerTouches::default();
        contact(&mut t, 1, ContactPhase::Down, [50., 50.], 10, 0);
        contact(&mut t, 2, ContactPhase::Down, [100., 50.], 10, 10);
        for (id, x, ms) in [(1, 50., 80), (1, 50., 120), (2, 100., 60), (2, 100., 100)] {
            contact(&mut t, id, ContactPhase::Move, [x, 50.], 10, ms);
        }
        if reversed { contact(&mut t, 1, ContactPhase::Move, [50., 50.], 10, 90); }
        contact(&mut t, 1, ContactPhase::Up, [50., 50.], 10, 150);
        assert_eq!(contact(&mut t, 2, ContactPhase::Up, [100., 50.], 10, 160).1,
            (!reversed).then_some(LayerGesture::Opacity(10)));
    }
}
