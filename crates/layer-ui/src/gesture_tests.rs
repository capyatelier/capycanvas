fn finger(s: &mut UiSession<Recorder>, id: u64, phase: ContactPhase, position: [f32; 2], ms: u64) -> InputReply {
    s.input(pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, position, 1_000_000_000 + ms * 1_000_000))
    .unwrap()
}

fn tap(s: &mut UiSession<Recorder>, fingers: u64, start: u64, hold: u64) -> InputReply {
    for id in 0..fingers {
        finger(s, 10 + id, ContactPhase::Down, [300. + 80. * id as f32, 400.], start + id * 20);
    }
    for id in 0..fingers {
        finger(s, 10 + id, ContactPhase::Move, [302. + 80. * id as f32, 401.], start + 60);
    }
    let mut reply = InputReply::default();
    for id in 0..fingers {
        reply = finger(s, 10 + id, ContactPhase::Up, [302. + 80. * id as f32, 401.], start + hold + id * 10);
    }
    reply
}

fn painted_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Android);
    s.pen(event(&s, 1, PenPhase::Down, 1.));
    s.pen(event(&s, 2, PenPhase::Move, 1.));
    s.pen(event(&s, 3, PenPhase::Up, 1.));
    s.frame(4, 4).unwrap();
    assert!(s.command(CommandId::Undo).enabled);
    s
}

fn quick_pinch(s: &mut UiSession<Recorder>, outward: bool, start: u64, duration: u64) {
    let (from, to) = if outward { ([350., 450.], [300., 500.]) } else { ([300., 500.], [350., 450.]) };
    for id in 0..2 { finger(s, 10 + id, ContactPhase::Down, [from[id as usize], 400.], start + id * 10); }
    for id in 0..2 { finger(s, 10 + id, ContactPhase::Move, [to[id as usize], 400.], start + duration + id * 10); }
    for id in 0..2 { finger(s, 10 + id, ContactPhase::Up, [to[id as usize], 400.], start + duration + 30 + id * 10); }
}

#[test]
fn quick_pinch_fits_upright_and_reverse_restores_the_view_on_every_host() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = painted_session();
        s.state.platform = platform;
        s.gesture([400., 300.], [350., 280.], 2., 0.7).unwrap();
        s.state.camera.flipped = [true, false];
        let before = s.state.camera.view();
        let checkpoint = s.engine.checkpoint();
        quick_pinch(&mut s, false, 0, 80);
        assert_eq!(s.state.camera.rotation, 0., "{platform:?}");
        assert_eq!(s.state.camera.flipped, [false; 2]);
        let mut fitted = s.state.camera.clone();
        fitted.fit(s.engine.document().composition().size);
        assert_eq!(s.state.camera.view(), fitted.view());
        quick_pinch(&mut s, true, 500, 80);
        for (actual, expected) in s.state.camera.view().document_to_surface.into_iter().zip(before.document_to_surface) {
            assert!((actual - expected).abs() < 0.001, "{platform:?}: {actual} != {expected}");
        }
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
}

#[test]
fn slow_pinches_and_outward_pinches_without_a_fitted_view_keep_navigation() {
    for (outward, duration) in [(false, 500), (true, 80)] {
        let mut s = painted_session();
        s.state.camera.rotate_to(0.7).unwrap();
        let zoom = s.state.camera.zoom;
        quick_pinch(&mut s, outward, 0, duration);
        assert!((s.state.camera.rotation - 0.7).abs() < 0.001);
        assert!((s.state.camera.zoom / zoom - if outward { 2. } else { 0.5 }).abs() < 0.001);
    }
    let mut s = painted_session();
    quick_pinch(&mut s, false, 0, 80);
    s.gesture([400., 300.], [450., 300.], 1., 0.).unwrap();
    let zoom = s.state.camera.zoom;
    quick_pinch(&mut s, true, 500, 80);
    assert!((s.state.camera.zoom / zoom - 2.).abs() < 0.001, "navigation invalidates quick-fit restoration");
}

#[test]
fn quick_pinch_rejects_cancellation_extra_contacts_pan_and_staggered_release() {
    for case in 0..4 {
        let mut s = painted_session();
        s.state.camera.rotate_to(0.7).unwrap();
        finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
        finger(&mut s, 11, ContactPhase::Down, [500., 400.], 10);
        if case == 1 { finger(&mut s, 12, ContactPhase::Down, [600., 400.], 20); }
        let delta = if case == 2 { 200. } else { 0. };
        finger(&mut s, 10, ContactPhase::Move, [350. + delta, 400.], 80);
        finger(&mut s, 11, ContactPhase::Move, [450. + delta, 400.], 90);
        finger(&mut s, 10, if case == 0 { ContactPhase::Cancel } else { ContactPhase::Up }, [350. + delta, 400.], 110);
        finger(&mut s, 11, ContactPhase::Up, [450. + delta, 400.], if case == 3 { 250 } else { 120 });
        assert!((s.state.camera.rotation - 0.7).abs() < 0.001, "case {case}");
    }
}

#[test]
fn held_fingers_repeat_history_and_release_never_adds_a_tap() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(platform);
        invoke(&mut s, CommandId::AddLayer);
        invoke(&mut s, CommandId::AddLayer);
        let painted = s.engine.document().clone();
        let count = s.state.layers.len();
        finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
        let down = finger(&mut s, 11, ContactPhase::Down, [400., 400.], 10);
        assert!(down.change.canvas_wake && s.wants_continuous_frames());
        for time in [1_020_000_000, 1_519_999_999] { s.frame(time, time).unwrap(); }
        assert_eq!(s.state.layers.len(), count);
        s.frame(1_520_000_000, 1_520_000_000).unwrap();
        assert_eq!(s.state.layers.len(), count - 1, "{platform:?}");
        s.frame(1_600_000_000, 1_600_000_000).unwrap();
        assert_eq!(s.state.layers.len(), count - 2);
        finger(&mut s, 10, ContactPhase::Up, [300., 400.], 700);
        finger(&mut s, 11, ContactPhase::Up, [400., 400.], 710);
        let checkpoint = s.engine.checkpoint();
        s.frame(1_900_000_000, 1_900_000_000).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint);
        for id in 0..3 { finger(&mut s, 10 + id, ContactPhase::Down, [300. + id as f32 * 80., 400.], 1000 + id * 10); }
        for time in [2_030_000_000, 2_530_000_000, 2_610_000_000] { s.frame(time, time).unwrap(); }
        for id in 0..3 { finger(&mut s, 10 + id, ContactPhase::Up, [300. + id as f32 * 80., 400.], 1700 + id * 10); }
        assert_live_artwork_eq(s.engine.document(), &painted);
    }
}

#[test]
fn held_history_stops_for_motion_blur_and_custom_tap_actions() {
    for case in 0..4 {
        let mut s = painted_session();
        if case == 3 { bind(&mut s, "touch.tap.2", "command.AddLayer"); }
        let painted = s.engine.document().clone();
        finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
        finger(&mut s, 11, ContactPhase::Down, [400., 400.], 10);
        s.frame(1_020_000_000, 1_020_000_000).unwrap();
        match case {
            0 => { finger(&mut s, 11, ContactPhase::Move, [450., 400.], 100); }
            1 => { s.input(UiInput::Blur).unwrap(); }
            2 => { finger(&mut s, 11, ContactPhase::Cancel, [400., 400.], 100); }
            _ => (),
        }
        s.frame(1_800_000_000, 1_800_000_000).unwrap();
        assert_live_artwork_eq(s.engine.document(), &painted);
    }
}

#[test]
fn trackpad_quick_pinches_and_history_holds_keep_taps_native() {
    let mut s = session(Platform::Mac);
    invoke(&mut s, CommandId::AddLayer);
    invoke(&mut s, CommandId::AddLayer);
    let count = s.state.layers.len();
    let raw = |s: &mut UiSession<Recorder>, id, phase, position, ms| {
        s.input(UiInput::TrackpadTouch { id, phase, position, time_ns: 1_000_000_000 + ms * 1_000_000 }).unwrap()
    };
    for (id, x) in [(1, 300.), (2, 500.)] { raw(&mut s, id, ContactPhase::Down, [x, 400.], id * 10); }
    for time in [1_030_000_000, 1_530_000_000, 1_610_000_000] { s.frame(time, time).unwrap(); }
    for (id, x) in [(1, 300.), (2, 500.)] { raw(&mut s, id, ContactPhase::Up, [x, 400.], 700 + id * 10); }
    assert_eq!(s.state.layers.len(), count - 2);
    invoke(&mut s, CommandId::Redo);
    let checkpoint = s.engine.checkpoint();
    for (id, x) in [(1, 300.), (2, 500.)] { raw(&mut s, id, ContactPhase::Down, [x, 400.], 1000 + id * 10); }
    for (id, x) in [(1, 300.), (2, 500.)] { raw(&mut s, id, ContactPhase::Up, [x, 400.], 1100 + id * 10); }
    assert_eq!(s.engine.checkpoint(), checkpoint, "trackpad taps remain native");
    s.gesture([400., 300.], [350., 280.], 2., 0.7).unwrap();
    let previous = s.state.camera.view();
    for (outward, start) in [(false, 2000), (true, 2500)] {
        let (from, to) = if outward { ([350., 450.], [300., 500.]) } else { ([300., 500.], [350., 450.]) };
        for i in 0..2 { raw(&mut s, 10 + i, ContactPhase::Down, [from[i as usize], 400.], start + i * 10); }
        for i in 0..2 { raw(&mut s, 10 + i, ContactPhase::Move, [to[i as usize], 400.], start + 80 + i * 10); }
        for i in 0..2 { raw(&mut s, 10 + i, ContactPhase::Up, [to[i as usize], 400.], start + 110 + i * 10); }
        if !outward { assert_eq!(s.state.camera.rotation, 0.); }
    }
    for (actual, expected) in s.state.camera.view().document_to_surface.into_iter().zip(previous.document_to_surface) {
        assert!((actual - expected).abs() < 0.001);
    }
}

#[test]
fn coalesced_finger_histories_keep_taps_and_history_holds_eligible() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        for held in [false, true] {
            let mut s = painted_session();
            s.state.platform = platform;
            let painted = s.engine.document().clone();
            finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
            finger(&mut s, 11, ContactPhase::Down, [500., 400.], 10);
            for (id, x, ms) in [(10, 300., 80), (10, 300., 120), (11, 500., 60), (11, 500., 100)] {
                finger(&mut s, id, ContactPhase::Move, [x, 400.], ms);
            }
            if held {
                s.frame(1_130_000_000, 1_130_000_000).unwrap();
                s.frame(1_700_000_000, 1_700_000_000).unwrap();
                assert_ne!(s.engine.document().artwork.paint, painted.artwork.paint, "{platform:?}: hold runs before release");
            }
            let end = if held { 800 } else { 150 };
            finger(&mut s, 10, ContactPhase::Up, [300., 400.], end);
            finger(&mut s, 11, ContactPhase::Up, [500., 400.], end + 10);
            assert_ne!(s.engine.document().artwork.paint, painted.artwork.paint, "{platform:?} held={held}");
        }
    }
}

#[test]
fn coalesced_finger_histories_keep_quick_fit_and_reverse_eligible() {
    let mut s = session(Platform::Windows);
    s.gesture([400., 300.], [350., 280.], 2., 0.7).unwrap();
    let previous = s.state.camera.view();
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
    finger(&mut s, 11, ContactPhase::Down, [500., 400.], 10);
    for (id, x, ms) in [(10, 330., 60), (10, 350., 100), (11, 470., 50), (11, 450., 90)] {
        finger(&mut s, id, ContactPhase::Move, [x, 400.], ms);
    }
    finger(&mut s, 10, ContactPhase::Up, [350., 400.], 110);
    finger(&mut s, 11, ContactPhase::Up, [450., 400.], 120);
    assert_eq!(s.state.camera.rotation, 0.);
    quick_pinch(&mut s, true, 500, 80);
    for (actual, expected) in s.state.camera.view().document_to_surface.into_iter().zip(previous.document_to_surface) {
        assert!((actual - expected).abs() < 0.001);
    }
}

#[test]
fn coalesced_finger_history_reversal_within_one_contact_still_cancels() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
    finger(&mut s, 11, ContactPhase::Down, [500., 400.], 10);
    finger(&mut s, 10, ContactPhase::Move, [300., 400.], 100);
    finger(&mut s, 10, ContactPhase::Move, [300., 400.], 90);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 150);
    finger(&mut s, 11, ContactPhase::Up, [500., 400.], 160);
    assert_live_artwork_eq(s.engine.document(), &painted);
}

fn scrub_contacts(s: &mut UiSession<Recorder>, phase: ContactPhase, offset: [f32; 2], ms: u64, count: u64) {
    for id in 0..count {
        finger(s, 10 + id, phase, [300. + 80. * id as f32 + offset[0], 400. + offset[1]], ms);
    }
}

#[test]
fn three_finger_scrub_clears_once_at_every_angle_and_undo_restores_the_layer() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        for degrees in (0..360).step_by(15) {
            let mut s = painted_session();
            s.state.platform = platform;
            let painted = s.engine.document().clone();
            let camera = s.state.camera.clone();
            let angle = (degrees as f32).to_radians();
            let offset = |length: f32| [angle.cos() * length, angle.sin() * length];
            scrub_contacts(&mut s, ContactPhase::Down, [0.; 2], 0, 3);
            for (step, length) in [60., 0., 60.].into_iter().enumerate() {
                scrub_contacts(&mut s, ContactPhase::Move, offset(length), 100 + step as u64 * 100, 3);
                if step < 2 { assert_live_artwork_eq(s.engine.document(), &painted); }
            }
            let cleared = s.engine.document().clone();
            assert!(cleared.target_operations(cleared.working.target.unwrap()).unwrap().is_empty(), "{platform:?} {degrees}°");
            assert_eq!(s.state.camera.view(), camera.view());
            let checkpoint = s.engine.checkpoint();
            for (step, length) in [0., 60., 0., 60.].into_iter().enumerate() {
                scrub_contacts(&mut s, ContactPhase::Move, offset(length), 400 + step as u64 * 100, 3);
            }
            finger(&mut s, 10, ContactPhase::Up, offset(60.), 800);
            finger(&mut s, 11, ContactPhase::Move, [600., 600.], 810);
            finger(&mut s, 11, ContactPhase::Up, [600., 600.], 820);
            finger(&mut s, 12, ContactPhase::Up, offset(60.), 830);
            assert_eq!(s.engine.checkpoint(), checkpoint, "only one clear per contact group");
            assert_eq!(s.state.camera.view(), camera.view(), "lifting cannot pan the view");
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &painted);
            invoke(&mut s, CommandId::Redo);
            assert_live_artwork_eq(s.engine.document(), &cleared);
        }
    }
}

#[test]
fn scrub_requires_three_fingers_coherent_travel_and_two_full_reversals() {
    for (count, path) in [
        (2, vec![[60., 0.], [0., 0.], [60., 0.]]),
        (4, vec![[60., 0.], [0., 0.], [60., 0.]]),
        (3, vec![[60., 0.], [120., 0.], [180., 0.]]),
        (3, vec![[60., 0.], [0., 0.]]),
        (3, vec![[10., 0.], [-10., 0.], [10., 0.]]),
        (3, vec![[60., 0.], [57., 0.], [63., 0.], [60., 0.]]),
        (3, vec![[60., 0.], [60., 60.], [0., 60.], [0., 0.], [60., 0.]]),
    ] {
        let mut s = painted_session();
        let painted = s.engine.document().clone();
        scrub_contacts(&mut s, ContactPhase::Down, [0.; 2], 0, count);
        for (step, offset) in path.into_iter().enumerate() {
            scrub_contacts(&mut s, ContactPhase::Move, offset, 100 + step as u64 * 100, count);
        }
        scrub_contacts(&mut s, ContactPhase::Up, [0.; 2], 900, count);
        assert_live_artwork_eq(s.engine.document(), &painted);
    }
    for moving in [1, 2] {
        let mut s = painted_session();
        let painted = s.engine.document().clone();
        scrub_contacts(&mut s, ContactPhase::Down, [0.; 2], 0, 3);
        for (step, x) in [180., -180., 180.].into_iter().enumerate() {
            scrub_contacts(&mut s, ContactPhase::Move, [x, 0.], 100 + step as u64 * 100, moving);
        }
        scrub_contacts(&mut s, ContactPhase::Up, [0.; 2], 400, 3);
        assert_live_artwork_eq(s.engine.document(), &painted);
    }
}

#[test]
fn scrub_cancellation_and_ineligible_contacts_never_clear() {
    for case in 0..10 {
        let mut s = painted_session();
        let painted = s.engine.document().clone();
        scrub_contacts(&mut s, ContactPhase::Down, [0.; 2], 0, 3);
        scrub_contacts(&mut s, ContactPhase::Move, [60., 0.], 100, 3);
        match case {
            0 => { finger(&mut s, 10, ContactPhase::Cancel, [360., 400.], 110); }
            1 => { finger(&mut s, 10, ContactPhase::Up, [360., 400.], 110); }
            2 => { s.input(UiInput::Blur).unwrap(); }
            3 => {
                finger(&mut s, 10, ContactPhase::Up, [360., 400.], 110);
                finger(&mut s, 10, ContactPhase::Down, [360., 400.], 120);
            }
            4 => {
                finger(&mut s, 13, ContactPhase::Down, [540., 400.], 110);
                finger(&mut s, 13, ContactPhase::Up, [540., 400.], 120);
            }
            5 => { s.interaction.facts.popup_open = true; }
            6 => { invoke(&mut s, CommandId::KeyboardShortcuts); }
            7 => { s.input(UiInput::ColorPickerHold { id: 10, position: [360., 400.], offset: 40. }).unwrap(); }
            8 => { scrub_contacts(&mut s, ContactPhase::Move, [60., 0.], 90, 3); }
            _ => { scrub_contacts(&mut s, ContactPhase::Move, [60., 0.], 2100, 3); }
        }
        let start = if case == 9 { 2100 } else { 200 };
        scrub_contacts(&mut s, ContactPhase::Move, [0., 0.], start + 100, 3);
        scrub_contacts(&mut s, ContactPhase::Move, [60., 0.], start + 200, 3);
        scrub_contacts(&mut s, ContactPhase::Up, [60., 0.], start + 300, 3);
        assert_live_artwork_eq(s.engine.document(), &painted);
    }
}

#[test]
fn scrub_respects_layer_locks_and_native_touch_slop() {
    let mut s = painted_session();
    layer(&mut s, LayerAction::Lock { id: 1, value: true });
    let painted = s.engine.document().clone();
    scrub_contacts(&mut s, ContactPhase::Down, [0.; 2], 0, 3);
    for (step, x) in [60., 0., 60.].into_iter().enumerate() {
        scrub_contacts(&mut s, ContactPhase::Move, [x, 0.], 100 + step as u64 * 100, 3);
    }
    scrub_contacts(&mut s, ContactPhase::Up, [60., 0.], 400, 3);
    assert_eq!(s.engine.document(), &painted);
    layer(&mut s, LayerAction::Lock { id: 1, value: false });
    s.set_touch_policy(TouchPolicy { tap_ms: 500, slop: 40. }).unwrap();
    let painted = s.engine.document().clone();
    for (start, span) in [(1000, 60.), (2000, 100.)] {
        scrub_contacts(&mut s, ContactPhase::Down, [0.; 2], start, 3);
        for (step, x) in [span, 0., span].into_iter().enumerate() {
            scrub_contacts(&mut s, ContactPhase::Move, [x, 0.], start + 100 + step as u64 * 100, 3);
        }
        scrub_contacts(&mut s, ContactPhase::Up, [span, 0.], start + 400, 3);
        if start == 1000 { assert_live_artwork_eq(s.engine.document(), &painted); }
    }
    let doc = s.engine.document();
    assert!(doc.target_operations(doc.working.target.unwrap()).unwrap().is_empty());
}

#[test]
fn trackpad_scrub_preserves_native_navigation_and_does_not_dispatch_taps() {
    let mut s = painted_session();
    s.state.platform = Platform::Mac;
    let contacts = |s: &mut UiSession<Recorder>, phase, offset: f32, ms: u64, count| {
        for id in 0..count {
            let input = serde_json::json!({"type": "trackpad_touch", "id": id, "phase": phase,
                "position": [100. + 30. * id as f32 + offset, 100. + offset], "time_ns": 1_000_000_000 + ms * 1_000_000});
            s.input(serde_json::from_value(input).unwrap()).unwrap();
        }
    };
    let painted = s.engine.document().clone();
    contacts(&mut s, "down", 0., 0, 2);
    let camera = s.state.camera.view();
    s.scroll([400., 400.], [20., 30.], 2., false, true).unwrap();
    assert_ne!(s.state.camera.view(), camera, "two-finger trackpad scrolling stays native");
    contacts(&mut s, "up", 0., 100, 2);
    assert_live_artwork_eq(s.engine.document(), &painted,);
    let camera = s.state.camera.view();
    contacts(&mut s, "down", 0., 1000, 3);
    for (step, offset) in [60., 0., 60.].into_iter().enumerate() {
        contacts(&mut s, "move", offset, 1100 + step as u64 * 100, 3);
        s.scroll([400., 400.], [20., 30.], 2., false, true).unwrap();
        s.multi_touch_gesture([400., 400.], [400., 400.], 1.2, 0.1).unwrap();
        assert_eq!(s.state.camera.view(), camera, "native navigation yields to three-finger contacts");
    }
    contacts(&mut s, "up", 60., 1400, 3);
    let cleared = s.engine.document();
    assert!(cleared.target_operations(cleared.working.target.unwrap()).unwrap().is_empty());
    invoke(&mut s, CommandId::Undo);
    assert_live_artwork_eq(s.engine.document(), &painted);
    contacts(&mut s, "down", 0., 2000, 3);
    contacts(&mut s, "up", 0., 2100, 3);
    assert_live_artwork_eq(s.engine.document(), &painted,);
    s.multi_touch_gesture([400., 400.], [400., 400.], 1.2, 0.1).unwrap();
    assert_ne!(s.state.camera.view(), camera, "native gestures resume after release");
}

#[test]
fn two_and_three_finger_taps_undo_and_redo() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    let camera = s.state.camera.clone();
    tap(&mut s, 2, 0, 150);
    assert_ne!(s.engine.document().artwork.paint, painted.artwork.paint);
    let undone = s.engine.document().clone();
    assert_eq!(s.state.camera.view(), camera.view(), "tap jitter never moves the view");
    assert!(s.state.camera.revision > camera.revision);
    tap(&mut s, 3, 1000, 150);
    assert_live_artwork_eq(s.engine.document(), &painted);
    tap(&mut s, 2, 2000, 150);
    tap(&mut s, 4, 3000, 150);
    assert_live_artwork_eq(s.engine.document(), &undone);
}

#[test]
fn taps_fail_when_held_moved_staggered_or_untimed() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    tap(&mut s, 2, 0, 900);
    assert_eq!(s.engine.document(), &painted, "a long press is not a tap");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 2000);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 2010);
    finger(&mut s, 11, ContactPhase::Move, [460., 400.], 2050);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 2100);
    finger(&mut s, 11, ContactPhase::Up, [460., 400.], 2110);
    assert_eq!(s.engine.document(), &painted, "a pinch is not a tap");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 3000);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 3010);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 3050);
    finger(&mut s, 12, ContactPhase::Down, [350., 400.], 3060);
    finger(&mut s, 11, ContactPhase::Up, [400., 400.], 3100);
    finger(&mut s, 12, ContactPhase::Up, [350., 400.], 3120);
    assert_eq!(s.engine.document(), &painted, "rolling contacts are not a tap");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 4000);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 4010);
    finger(&mut s, 11, ContactPhase::Cancel, [400., 400.], 4020);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 4050);
    assert_eq!(s.engine.document(), &painted, "cancellation is not a tap");
    for (id, phase) in [(10, ContactPhase::Down), (11, ContactPhase::Down), (10, ContactPhase::Up), (11, ContactPhase::Up)] {
        s.input(pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, [300., 400.], 0))
        .unwrap();
    }
    assert_eq!(s.engine.document(), &painted);
}

#[test]
fn taps_yield_to_pens_strokes_and_the_picker() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 10);
    s.input(pointer_input(1, ContactPhase::Down, PointerKind::Pen, PointerButton::Primary, [500., 500.], 1))
    .unwrap();
    s.input(pointer_input(1, ContactPhase::Up, PointerKind::Pen, PointerButton::Primary, [500., 500.], 2))
    .unwrap();
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 50);
    finger(&mut s, 11, ContactPhase::Up, [400., 400.], 60);
    assert_live_artwork_eq(s.engine.document(), &painted);
    s.pen(event(&s, 10, PenPhase::Down, 1.));
    tap(&mut s, 2, 1000, 100);
    s.pen(event(&s, 11, PenPhase::Up, 1.));
    s.frame(12, 12).unwrap();
    let stroked = s.engine.document().clone();
    assert_ne!(stroked.artwork.paint, painted.artwork.paint, "a stroke owns the canvas");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 3000);
    s.input(UiInput::ColorPickerHold { id: 10, position: [300., 400.], offset: 40. }).unwrap();
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 3010);
    finger(&mut s, 11, ContactPhase::Up, [400., 400.], 3050);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 3060);
    s.frame(13, 13).unwrap();
    assert_live_artwork_eq(s.engine.document(), &stroked);
}

fn bind(s: &mut UiSession<Recorder>, trigger: &str, id: &str) {
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(s, PreferenceAction::OpenActionPicker { trigger: trigger.into() });
    preference(s, PreferenceAction::ChooseAction { id: id.into() });
    assert!(s.preferences().unwrap().error.is_none());
    s.dispatch(UiAction::CloseSettings).unwrap();
}

#[test]
fn tap_bindings_are_configurable_and_validated() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    bind(&mut s, "touch.tap.2", "");
    assert_eq!(s.state.settings.gestures.get("touch.tap.2").map(String::as_str), Some(""));
    tap(&mut s, 2, 0, 100);
    assert_eq!(s.engine.document(), &painted);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::OpenActionPicker { trigger: "touch.tap.3".into() });
    let picker = s.preferences().unwrap().shortcut_page.picker.unwrap();
    assert_eq!(picker.title, "Three-finger tap");
    let actions: Vec<_> = picker.sections.iter().flat_map(|s| &s.actions).collect();
    assert!(actions.iter().any(|a| a.id == "command.Redo" && a.selected));
    assert!(!actions.iter().any(|a| a.id.starts_with("hold.") || a.id == "canvas.pan"), "taps cannot hold");
    assert!(actions.len() > 100, "every instant action is available");
    preference(&mut s, PreferenceAction::SearchActionPicker { query: "search com".into() });
    let picker = s.preferences().unwrap().shortcut_page.picker.unwrap();
    assert_eq!(picker.sections.iter().flat_map(|s| &s.actions).map(|a| a.id.as_str()).collect::<Vec<_>>(), ["command.SearchCommands"]);
    preference(&mut s, PreferenceAction::ChooseAction { id: "hold.eyedropper".into() });
    assert!(s.preferences().unwrap().error.unwrap().contains("cannot hold"));
    preference(&mut s, PreferenceAction::ChooseAction { id: "command.SearchCommands".into() });
    assert!(s.preferences().unwrap().shortcut_page.picker.is_none());
    s.dispatch(UiAction::CloseSettings).unwrap();
    tap(&mut s, 3, 1000, 100);
    assert!(s.state.command_search.is_some());
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close }).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::ResetTrigger { trigger: "touch.tap.2".into() });
    assert!(!s.state.settings.gestures.contains_key("touch.tap.2"));
    let mut settings = Settings::default();
    settings.gestures.insert("touch.tap.2".into(), "hold.eyedropper".into());
    assert!(settings.validate().is_err());
    settings.gestures.insert("touch.tap.2".into(), "command.Nope".into());
    assert!(settings.validate().is_err());
    settings.gestures = [("touch.tap.9".to_string(), String::new())].into();
    assert!(settings.validate().is_err());
    settings.gestures = [("pen.button.primary".to_string(), "hold.eyedropper".to_string())].into();
    settings.validate().unwrap();
    let saved = serde_json::to_value(&settings).unwrap();
    assert_eq!(serde_json::from_value::<Settings>(saved).unwrap(), settings);
    assert!(serde_json::to_value(Settings::default()).unwrap().get("gestures").is_none());
    let triggers = |platform| crate::shortcut_page::triggers(platform).map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(triggers(Platform::Mac), ["pen.button.primary", "pen.button.secondary"], "macOS reports tablet side buttons, not finger taps");
    assert_eq!(triggers(Platform::Ios), ["touch.tap.2", "touch.tap.3", "touch.tap.4"], "iPadOS reports finger taps, not pen buttons");
    assert_eq!(triggers(Platform::Android).len(), 5, "only Linux reports a third side button");
    assert_eq!(
        triggers(Platform::Windows),
        ["touch.tap.2", "touch.tap.3", "touch.tap.4", "pen.button.primary"],
        "Windows Ink reports one barrel button, and touchscreens report finger taps"
    );
    assert_eq!(triggers(Platform::Gtk).len(), 6);
}

#[test]
fn pen_buttons_are_opt_in_and_use_the_hold_lifecycle() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    let press = |s: &mut UiSession<Recorder>, button, pressed| {
        s.input(UiInput::PenButton { button, pressed }).unwrap()
    };
    assert!(!press(&mut s, PenButton::Primary, true).handled, "unbound buttons stay with the driver");
    assert!(!press(&mut s, PenButton::Primary, false).handled);
    assert!(painting_with(&s, preset));
    bind(&mut s, "pen.button.primary", "hold.eyedropper");
    assert!(press(&mut s, PenButton::Primary, true).handled);
    assert!(sampling(&s));
    assert!(press(&mut s, PenButton::Primary, false).handled);
    assert!(painting_with(&s, preset));

    bind(&mut s, "pen.button.secondary", "canvas.pan");
    assert!(press(&mut s, PenButton::Secondary, true).handled);
    assert!(!s.pointer_contact_paints(PointerKind::Pen, PointerButton::Primary, [500., 500.]), "a held pan button turns contacts into navigation");
    press(&mut s, PenButton::Secondary, false);
    assert!(s.pointer_contact_paints(PointerKind::Pen, PointerButton::Primary, [500., 500.]));

    s.pen(event(&s, 1, PenPhase::Down, 1.));
    press(&mut s, PenButton::Primary, true);
    assert!(painting_with(&s, preset), "a button change never splits the stroke");
    s.pen(event(&s, 2, PenPhase::Move, 1.));
    s.pen(event(&s, 3, PenPhase::Up, 1.));
    s.frame(4, 4).unwrap();
    assert!(sampling(&s));
    s.input(UiInput::Blur).unwrap();
    assert!(painting_with(&s, preset));
    assert!(!press(&mut s, PenButton::Primary, false).handled);

    bind(&mut s, "pen.button.primary", "command.Undo");
    let before = s.engine.document().clone();
    assert!(press(&mut s, PenButton::Primary, true).handled);
    press(&mut s, PenButton::Primary, false);
    assert_ne!(s.engine.document(), &before);
}

#[test]
fn a_new_drawing_keeps_the_hosts_tap_timing() {
    let mut s = painted_session();
    s.set_touch_policy(TouchPolicy { tap_ms: 900, slop: 24. }).unwrap();
    let mut next = painted_session();
    next.inherit_window_state(&s).unwrap();
    let painted = next.engine.document().clone();
    tap(&mut next, 2, 0, 700);
    assert_ne!(next.engine.document().artwork.paint, painted.artwork.paint, "the host's long-press time still applies");
}

#[test]
fn stylus_actions_switch_to_the_eraser_or_the_previous_tool() {
    let mut s = session(Platform::Ios);
    let pen = s.state.brush.preset;
    let stylus = |s: &mut UiSession<Recorder>, action| s.input(UiInput::StylusAction { action }).unwrap();
    assert!(!stylus(&mut s, StylusAction::SwitchPrevious).handled, "there is no previous tool yet");
    assert!(stylus(&mut s, StylusAction::SwitchEraser).handled);
    assert!(s.command(CommandId::Eraser).selected);
    stylus(&mut s, StylusAction::SwitchEraser);
    assert!(painting_with(&s, pen), "switching again returns to the brush");

    invoke(&mut s, CommandId::Lasso);
    stylus(&mut s, StylusAction::SwitchPrevious);
    assert!(painting_with(&s, pen));
    stylus(&mut s, StylusAction::SwitchPrevious);
    assert!(s.command(CommandId::Lasso).selected);

    bind(&mut s, "pen.button.primary", "hold.eyedropper");
    s.input(UiInput::PenButton { button: PenButton::Primary, pressed: true }).unwrap();
    s.input(UiInput::PenButton { button: PenButton::Primary, pressed: false }).unwrap();
    stylus(&mut s, StylusAction::SwitchPrevious);
    assert!(painting_with(&s, pen), "a held tool is not the previous tool");

    s.pen(event(&s, 1, PenPhase::Down, 1.));
    stylus(&mut s, StylusAction::SwitchEraser);
    assert!(painting_with(&s, pen), "a stroke keeps its tool");
    s.pen(event(&s, 2, PenPhase::Move, 1.));
    s.pen(event(&s, 3, PenPhase::Up, 1.));
    s.frame(4, 4).unwrap();
    assert!(s.command(CommandId::Eraser).selected, "the switch follows the stroke");

    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    assert!(!stylus(&mut s, StylusAction::SwitchEraser).handled);
    assert!(s.command(CommandId::Eraser).selected);
}

#[test]
fn remote_and_gamepad_keys_share_canonical_names() {
    for (native, canonical, label) in [
        ("AudioVolumeUp", "volumeup", "Volume Up"),
        ("AudioRaiseVolume", "volumeup", "Volume Up"),
        ("XF86AudioRaiseVolume", "volumeup", "Volume Up"),
        ("XF86AudioPlay", "mediaplaypause", "Play/Pause"),
        ("AudioLowerVolume", "volumedown", "Volume Down"),
        ("MediaPlayPause", "mediaplaypause", "Play/Pause"),
        ("AudioNext", "mediatracknext", "Next Track"),
        ("gamepad_a", "gamepad_a", "Gamepad A"),
        ("gamepad_l2", "gamepad_l2", "Gamepad L2"),
        ("gamepad_up", "gamepad_up", "Gamepad ↑"),
        ("gamepad_start", "gamepad_start", "Gamepad Start"),
        ("F13", "f13", "F13"),
    ] {
        let chord = KeyChord::new(native, Modifiers::default());
        assert_eq!(chord.key, canonical);
        assert_eq!(chord.label(Platform::Android), label);
        chord.validate().unwrap();
    }
    assert!(KeyChord::new("gamepad_z", Modifiers::default()).validate().is_err());
    let mut s = session(Platform::Android);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::BeginShortcut { id: CommandId::Undo.shortcut_id() });
    key(&mut s, "volumeup", true, false, false);
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    key(&mut s, "volumeup", false, false, false);
    preference(&mut s, PreferenceAction::BeginShortcut { id: "tool_setting.size.increase".into() });
    key(&mut s, "gamepad_r1", true, false, false);
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    key(&mut s, "gamepad_r1", false, false, false);
    s.dispatch(UiAction::CloseSettings).unwrap();
    s.pen(event(&s, 1, PenPhase::Down, 1.));
    s.pen(event(&s, 2, PenPhase::Up, 1.));
    s.frame(3, 3).unwrap();
    let size = s.state.brush.diameter;
    let press = |s: &mut UiSession<Recorder>, name: &str, repeat: bool| {
        s.input(UiInput::Key {
            key: name.into(),
            pressed: true,
            repeat,
            modifiers: Modifiers::default(),
            editing: false,
            divider: None,
        })
        .unwrap()
    };
    assert!(press(&mut s, "gamepad_r1", false).handled);
    assert!(press(&mut s, "gamepad_r1", true).handled);
    key(&mut s, "gamepad_r1", false, false, false);
    assert!(s.state.brush.diameter > size, "repeats step while the button is held");
    assert!(press(&mut s, "AudioVolumeUp", false).handled);
    assert!(!s.command(CommandId::Undo).enabled);
    assert!(!press(&mut s, "volumedown", false).handled, "unbound device keys stay with the system");
}

#[test]
fn stick_axes_navigate_with_dead_zones_and_stop_on_blur() {
    let mut s = session(Platform::Gtk);
    s.frame(1, 1).unwrap();
    let axes = |s: &mut UiSession<Recorder>, pan: [f32; 2], zoom: f32| {
        s.input(UiInput::Axes { pan, zoom }).unwrap()
    };
    let camera = s.state.camera.clone();
    assert!(!axes(&mut s, [0.1, -0.1], 0.12).change.canvas_wake, "drift inside the dead zone");
    assert!(!s.wants_continuous_frames());
    assert!(axes(&mut s, [1., 0.], 0.).change.canvas_wake);
    assert!(s.wants_continuous_frames());
    s.frame(1_000_000_000, 1_000_000_000).unwrap();
    let change = s.frame(1_016_000_000, 1_016_000_000).unwrap();
    assert!(change.regions & regions::CAMERA != 0 && change.canvas_wake);
    let moved = s.state.camera.clone();
    assert!(moved.translation[0] < camera.translation[0], "right stick deflection pans toward the right");
    assert_eq!(moved.zoom, camera.zoom);
    s.frame(1_500_000_000, 1_500_000_000).unwrap();
    let step = camera.translation[0] - moved.translation[0];
    assert!(moved.translation[0] - s.state.camera.translation[0] < step * 10., "long frame gaps are clamped");
    axes(&mut s, [0., 0.], 1.);
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    s.frame(2_100_000_000, 2_100_000_000).unwrap();
    assert!(s.state.camera.zoom > moved.zoom, "positive zoom deflection zooms in");
    s.input(UiInput::Blur).unwrap();
    assert!(!s.wants_continuous_frames());
    let stopped = s.state.camera.clone();
    s.frame(2_200_000_000, 2_200_000_000).unwrap();
    assert_eq!(s.state.camera, stopped);
    assert!(s.input(UiInput::Axes { pan: [2., 0.], zoom: 0. }).is_err());
    axes(&mut s, [1., 0.], 0.);
    s.pen(event(&s, 5, PenPhase::Down, 1.));
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    s.frame(3_050_000_000, 3_050_000_000).unwrap();
    assert_eq!(s.state.camera.translation, stopped.translation, "a stroke keeps the view still");
}

fn stroke(s: &mut UiSession<Recorder>, first: u64) {
    s.pen(event(s, first, PenPhase::Down, 1.));
    s.pen(event(s, first + 1, PenPhase::Move, 1.));
    s.pen(event(s, first + 2, PenPhase::Up, 1.));
    s.frame(first + 3, first + 3).unwrap();
}

#[test]
fn refused_brush_strokes_raise_a_notice_and_paint_nothing() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::Lock { id: 1, value: true });
    let revision = s.engine.document().revision;
    stroke(&mut s, 1);
    assert_eq!(notice_text(&s), Some("The active layer is locked"));
    assert_eq!(s.engine.document().revision, revision, "no stroke and no history");
    let first = s.state.notice.as_ref().unwrap().id;
    stroke(&mut s, 10);
    assert!(s.state.notice.as_ref().unwrap().id > first, "a repeated refusal is shown again");
    layer(&mut s, LayerAction::Lock { id: 1, value: false });
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    stroke(&mut s, 30);
    assert_eq!(notice_text(&s), Some("A group has no pixels of its own. Select a layer inside it."));
    s.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
    let revision = s.engine.document().revision;
    stroke(&mut s, 40);
    assert_eq!(s.state.notice, None, "a stroke that paints clears the notice");
    assert_ne!(s.engine.document().revision, revision);
}

#[test]
fn erasing_under_alpha_lock_is_refused_with_a_notice() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::AlphaLock { id: 1, value: true });
    invoke(&mut s, CommandId::Eraser);
    let revision = s.engine.document().revision;
    stroke(&mut s, 1);
    assert_eq!(notice_text(&s), Some("Alpha lock keeps this layer's transparency, so erasing has no effect"));
    assert_eq!(s.engine.document().revision, revision);
    invoke(&mut s, CommandId::Pen);
    let mut down = event(&s, 10, PenPhase::Down, 1.);
    down.tool = ToolKind::Eraser;
    s.pen(down);
    assert_eq!(s.engine.stroke_refusal(&down), Some(layer_engine::StrokeRefusal::AlphaLocked));
    s.pen(PenEvent { phase: PenPhase::Up, ..down });
    s.frame(11, 11).unwrap();
    assert_eq!(s.engine.document().revision, revision, "a pen's eraser end is refused too");
    stroke(&mut s, 20);
    assert_eq!(s.state.notice, None);
    assert_ne!(s.engine.document().revision, revision, "painting keeps working under alpha lock");
}

#[test]
fn mask_strokes_explain_dry_coverage_once_per_mask_session() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::AddMask { id: 1, replace: false });
    layer(&mut s, LayerAction::Select { id: 1, mask: true });
    stroke(&mut s, 1);
    assert_eq!(s.state.notice, None, "a dry brush paints masks as configured");
    let mut brush = s.engine.configured_brush().clone();
    brush.execution = layer_core::BrushExecution::Wet;
    s.engine.set_brush(brush).unwrap();
    let revision = s.engine.document().revision;
    stroke(&mut s, 10);
    assert_eq!(
        notice_text(&s),
        Some("Masks take dry coverage, so this brush paints without its wet or blending behavior")
    );
    assert_ne!(s.engine.document().revision, revision, "the mask stroke still paints");
    stroke(&mut s, 20);
    assert_eq!(s.state.notice, None, "shown once per mask-editing session");
    layer(&mut s, LayerAction::Select { id: 1, mask: false });
    layer(&mut s, LayerAction::Select { id: 1, mask: true });
    stroke(&mut s, 30);
    assert!(notice_text(&s).is_some_and(|t| t.starts_with("Masks take dry coverage")));
}

#[test]
fn retouch_strokes_refuse_masks_and_offer_a_reference_for_an_empty_layer() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::Clone);
    invoke(&mut s, CommandId::SelectionEditing);
    let revision = s.engine.document().revision;
    stroke(&mut s, 1);
    assert_eq!(notice_text(&s), Some("This layer is empty, so there's nothing to copy"));
    invoke(&mut s, CommandId::SelectionReference);
    stroke(&mut s, 10);
    assert_eq!(notice_text(&s), Some("This layer is empty, and there's no layer below it to copy from"));
    assert_eq!(s.engine.document().revision, revision, "refused retouch strokes paint nothing");

    layer(&mut s, LayerAction::New { group: false, clipped: false });
    stroke(&mut s, 20);
    let notice = s.state.notice.clone().unwrap();
    assert_eq!(notice.text, "This layer is empty, and no reference layer below it is marked");
    assert_eq!(notice.actions[0].label, "Use Current ink as Reference");
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: None }).unwrap();
    assert_eq!(s.engine.document().scene().references(), [OccurrenceHandle::from_index(0)].into());
    let revision = s.engine.document().revision;
    stroke(&mut s, 30);
    assert_eq!(s.state.notice, None, "the reference below is the source");
    assert_ne!(s.engine.document().revision, revision);

    let top = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::AddMask { id: top, replace: false });
    layer(&mut s, LayerAction::Select { id: top, mask: true });
    let revision = s.engine.document().revision;
    stroke(&mut s, 40);
    assert_eq!(notice_text(&s), Some("Return to the layer's artwork first"));
    assert_eq!(s.engine.document().revision, revision, "retouching never paints a mask");
}

#[test]
fn move_and_content_tools_explain_what_they_cannot_change() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::Move);
    layer(&mut s, LayerAction::Lock { id: 1, value: true });
    stroke(&mut s, 1);
    assert_eq!(notice_text(&s), Some("The active layer is locked"));
    assert!(s.layer_interaction.path.is_empty());
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    stroke(&mut s, 10);
    assert_eq!(notice_text(&s), Some("The selected layers cannot be moved together"));
    s.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
    for (command, text) in [
        (CommandId::Gradient, "The active layer is locked"),
        (CommandId::Figure, "The active layer is locked"),
    ] {
        invoke(&mut s, command);
        s.dismiss_notice();
        stroke(&mut s, 20);
        assert_eq!(notice_text(&s), Some(text), "{command:?}");
    }
    layer(&mut s, LayerAction::Lock { id: 1, value: false });
    layer(&mut s, LayerAction::Tool { tool: LayerCanvasTool::LassoFill });
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    stroke(&mut s, 30);
    assert_eq!(notice_text(&s), Some("A group has no pixels of its own. Select a layer inside it."));
    s.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
    layer(&mut s, LayerAction::AddMask { id: 1, replace: false });
    layer(&mut s, LayerAction::Select { id: 1, mask: true });
    invoke(&mut s, CommandId::Gradient);
    stroke(&mut s, 40);
    assert_eq!(notice_text(&s), Some("Return to the layer's artwork first"));
}

#[test]
fn a_fill_click_without_content_raises_a_notice_at_release_but_not_outside_the_canvas() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::Fill);
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    s.pen(event(&s, 1, PenPhase::Down, 1.));
    assert_eq!(s.state.notice, None);
    s.pen(event(&s, 1, PenPhase::Up, 1.));
    let change = s.frame(2, 2).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    assert_eq!(notice_text(&s), Some("A group has no pixels of its own. Select a layer inside it."));
    assert!(s.renderer_mut().region_requests.is_empty());
    let outside = PenEvent { surface_position: Point { x: -500., y: -500. }, ..event(&s, 3, PenPhase::Down, 1.) };
    s.pen(outside);
    s.pen(PenEvent { phase: PenPhase::Up, ..outside });
    s.frame(4, 4).unwrap();
    assert_eq!(s.state.notice, None, "a click outside the canvas stays silent");
}

#[test]
fn a_fill_larger_than_the_renderer_can_label_is_refused_with_a_notice() {
    let mut s = session(Platform::Gtk);
    s.renderer_mut().region_pixels = Some(0);
    invoke(&mut s, CommandId::Fill);
    let layers = s.engine.document().scene().order().len();
    s.pen(event(&s, 1, PenPhase::Down, 1.));
    s.pen(event(&s, 1, PenPhase::Up, 1.));
    s.frame(2, 2).unwrap();
    let refusal = s.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY);
    assert_eq!(notice_text(&s), Some(refusal.as_ref()));
    assert!(s.renderer_mut().region_requests.is_empty() && s.state.host_error.is_none());
    assert_eq!(s.engine.document().scene().order().len(), layers);
    assert!(!s.engine.can_undo(), "the refused fill adds no undo step");
    s.renderer_mut().region_pixels = None;
    s.pen(event(&s, 3, PenPhase::Down, 1.));
    s.pen(event(&s, 3, PenPhase::Up, 1.));
    s.frame(4, 4).unwrap();
    assert_eq!(s.renderer_mut().region_requests.len(), 1, "a fill that fits is requested");
}

#[test]
fn notices_reject_stale_answers_and_clear_at_the_next_contact() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::Move);
    layer(&mut s, LayerAction::Lock { id: 1, value: true });
    stroke(&mut s, 1);
    let id = s.state.notice.as_ref().unwrap().id;
    assert!(s.state.notice.as_ref().unwrap().actions.is_empty());
    assert!(s.dispatch(UiAction::Notice { id: id + 1, accept: false, action: None }).is_err());
    assert!(s.dispatch(UiAction::Notice { id: id - 1, accept: true, action: None }).is_err());
    assert!(s.state.notice.is_some());
    let change = s.dispatch(UiAction::Notice { id, accept: false, action: None }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    assert_eq!(s.state.notice, None);
    assert!(s.dispatch(UiAction::Notice { id, accept: false, action: None }).is_err(), "a dismissed notice is stale");

    stroke(&mut s, 10);
    assert!(s.state.notice.is_some());
    let reply = s
        .input(pointer_input(7, ContactPhase::Down, PointerKind::Touch, PointerButton::Primary, [300., 300.], 0))
        .unwrap();
    assert_ne!(reply.change.regions & regions::HOST, 0);
    assert_eq!(s.state.notice, None, "a canvas contact clears the notice");
    finger(&mut s, 7, ContactPhase::Up, [300., 300.], 1);

    stroke(&mut s, 20);
    layer(&mut s, LayerAction::Lock { id: 1, value: false });
    assert!(s.state.notice.is_some(), "an unrelated edit keeps the notice");
    stroke(&mut s, 30);
    assert_eq!(s.state.notice, None, "a pen-down that raises nothing new clears it");

    layer(&mut s, LayerAction::Lock { id: 1, value: true });
    stroke(&mut s, 40);
    assert!(s.state.notice.is_some());
    s.state.document_file.epoch += 1;
    let change = s.frame(50, 50).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    assert_eq!(s.state.notice, None, "a document switch clears the notice");
}

#[test]
fn row_alpha_lock_toggle_preserves_target_and_has_one_undo_step() {
    let mut s = session(Platform::Android);
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let target = s.engine.document().working.occurrence.unwrap();
    let selected = s.engine.document().working.layer_selection.clone();
    assert_eq!(s.state.layers.iter().find(|l| l.id == 1).unwrap().right_swipe, Some(LayerAction::ToggleAlphaLock { id: 1 }));
    layer(&mut s, LayerAction::ToggleAlphaLock { id: 1 });
    assert!(s.engine.document().scene().occurrence(OccurrenceHandle::from_index(0)).unwrap().alpha_locked);
    assert_eq!(s.engine.document().working.occurrence.unwrap(), target);
    assert_eq!(s.engine.document().working.layer_selection, selected);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert!(!s.engine.document().scene().occurrence(OccurrenceHandle::from_index(0)).unwrap().alpha_locked);
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert!(s.engine.document().scene().occurrence(OccurrenceHandle::from_index(0)).unwrap().alpha_locked);
    layer(&mut s, LayerAction::ToggleAlphaLock { id: 1 });
    assert!(!s.engine.document().scene().occurrence(OccurrenceHandle::from_index(0)).unwrap().alpha_locked);
    layer(&mut s, LayerAction::Lock { id: 1, value: true });
    for id in [1, 2, u64::MAX] {
        let before = s.engine.document().clone();
        assert!(s.dispatch(UiAction::Layer { action: LayerAction::ToggleAlphaLock { id } }).is_err());
        assert_eq!(s.engine.document(), &before);
    }
    assert!(s.state.layers.iter().find(|l| l.id == 1).unwrap().right_swipe.is_none());
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
    assert_eq!(s.state.layers.iter().find(|l| l.id == id).unwrap().right_swipe, Some(LayerAction::TogglePassThrough { id }));
    assert!(s.dispatch(UiAction::Layer { action: LayerAction::ToggleAlphaLock { id } }).is_err());
}
