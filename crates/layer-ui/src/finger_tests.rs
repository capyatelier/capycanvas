fn finger_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    edit_preference(&mut s, PreferenceId::FingerDrawing, PreferenceValue::Choice(1));
    s
}
fn finger_sample(s: &mut UiSession<Recorder>, id: u64, phase: ContactPhase, position: [f32; 2], ms: u64) -> InputReply {
    let time_ns = 1_000_000_000 + ms * 1_000_000;
    let reply = s.input(pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, position, time_ns)).unwrap();
    if reply.paint {
        let event = PenEvent { device_id: 1000 + id, sequence: 0, timestamp_ns: time_ns,
            surface_position: Point { x: position[0], y: position[1] }, tool: ToolKind::Finger,
            phase: pen_phase(phase), ..event(s, 0, pen_phase(phase), 1.) };
        s.pen(event);
    }
    reply
}
fn finger_frames(s: &mut UiSession<Recorder>, ms: u64) {
    for tick in 0..4 {
        let time = 1_000_000_000 + (ms + tick) * 1_000_000;
        s.frame(time, time).unwrap();
    }
}

#[test]
fn finger_policy_uses_device_class_independently_of_window_and_persists_modes() {
    for device in [DeviceClass::Desktop, DeviceClass::Phone, DeviceClass::Tablet] {
        for mode in FingerDrawing::ALL {
            for seen in [false, true] {
                let mut s = session(Platform::Gtk);
                s.set_device_class(device);
                s.state.settings.finger_drawing = mode;
                s.state.settings.pen_seen = seen;
                for viewport in [[320, 568], [891, 412], [1000, 1000]] {
                    s.set_viewport(viewport.map(|v| v as f32), viewport).unwrap();
                    assert_eq!(s.finger_drawing(), mode == FingerDrawing::On || (mode == FingerDrawing::Auto && device != DeviceClass::Desktop && !seen));
                }
                let restored = Settings::restore(&serde_json::to_string(&s.state.settings).unwrap());
                assert_eq!((restored.finger_drawing, restored.pen_seen), (mode, seen));
            }
        }
    }
    let mut gtk = session(Platform::Gtk);
    gtk.set_viewport([320., 568.], [320, 568]).unwrap();
    assert_eq!(gtk.state.settings.finger_drawing, FingerDrawing::Auto);
    assert!(!gtk.finger_drawing());
}

#[test]
fn drawing_activation_keeps_the_windows_device_class_without_contacts() {
    for platform in Platform::ALL {
        for device in [DeviceClass::Desktop, DeviceClass::Phone, DeviceClass::Tablet] {
            let mut previous = session(platform);
            previous.set_device_class(device);
            finger_sample(&mut previous, 1, ContactPhase::Down, [250., 300.], 10);
            for initial in [DeviceClass::Desktop, DeviceClass::Phone, DeviceClass::Tablet] {
                let mut next = session(platform);
                next.set_device_class(initial);
                next.inherit_window_state(&previous).unwrap();
                assert_eq!(next.finger.device, device, "{platform:?}/{device:?}/{initial:?}");
                assert_eq!(next.finger_drawing(), previous.finger_drawing());
                assert!(!next.finger.recognizing());
                assert!(next.finger.raw.is_none());
            }
        }
    }
}

#[test]
fn finger_on_starts_immediately_and_finishes_once_while_off_keeps_gestures() {
    for mode in [FingerDrawing::On, FingerDrawing::Off] {
        let mut s = finger_session();
        s.state.settings.finger_drawing = mode;
        let before = s.engine.checkpoint();
        let down = finger_sample(&mut s, 1, ContactPhase::Down, [250., 300.], 10);
        finger_frames(&mut s, 12);
        assert_eq!(down.paint, mode == FingerDrawing::On);
        assert_eq!(s.engine.has_active_stroke(), mode == FingerDrawing::On);
        finger_sample(&mut s, 1, ContactPhase::Move, [450., 300.], 30);
        finger_sample(&mut s, 1, ContactPhase::Up, [450., 300.], 40);
        finger_frames(&mut s, 42);
        assert_eq!(s.engine.metrics().committed_strokes, u64::from(mode == FingerDrawing::On));
        assert_eq!(s.engine.checkpoint() != before, mode == FingerDrawing::On);
        assert!(!s.state.settings.pen_seen);
        if mode == FingerDrawing::On {
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.checkpoint(), before);
        }
    }
}

#[test]
fn finger_second_contact_withdraws_before_navigation_without_touching_history() {
    let mut s = finger_session();
    let before = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    let camera = s.state.camera.view();
    finger_sample(&mut s, 1, ContactPhase::Down, [250., 300.], 10);
    finger_sample(&mut s, 1, ContactPhase::Move, [350., 300.], 30);
    finger_frames(&mut s, 32);
    assert!(s.engine.has_active_stroke());
    let second = finger_sample(&mut s, 2, ContactPhase::Down, [500., 400.], 60);
    assert!(second.cancel_paint && !second.paint);
    finger_sample(&mut s, 1, ContactPhase::Move, [400., 350.], 80);
    finger_sample(&mut s, 2, ContactPhase::Move, [550., 450.], 90);
    finger_sample(&mut s, 1, ContactPhase::Up, [400., 350.], 110);
    finger_sample(&mut s, 2, ContactPhase::Up, [550., 450.], 120);
    finger_frames(&mut s, 122);
    assert_eq!(s.engine.document().artwork, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert_eq!(s.engine.metrics().committed_strokes, 0);
    assert_ne!(s.state.camera.view(), camera);
}

#[test]
fn finger_hold_withdraws_the_stroke_before_starting_the_picker() {
    for ready in [false, true] {
        for rendered in [false, true] {
            let mut s = finger_session();
            s.renderer_mut().painting_unready = !ready;
            let checkpoint = s.engine.checkpoint();
            finger_sample(&mut s, 1, ContactPhase::Down, [250., 300.], 10);
            if rendered { finger_frames(&mut s, 12); }
            let reply = s.input(UiInput::ColorPickerHold { id: 1, position: [250., 300.], offset: 44. }).unwrap();
            assert!(reply.handled && reply.cancel_paint, "ready={ready}, rendered={rendered}");
            assert!(s.color_picker_overlay().is_some(), "ready={ready}, rendered={rendered}");
            finger_sample(&mut s, 1, ContactPhase::Up, [250., 300.], 600);
            s.renderer_mut().painting_unready = false;
            finger_frames(&mut s, 602);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.engine.metrics().committed_strokes, 0);
        }
    }
}

#[test]
fn withdrawn_finger_raw_samples_cannot_operate_the_held_picker() {
    for delivered in [false, true] {
        let mut s = finger_session();
        let checkpoint = s.engine.checkpoint();
        let position = [250., 300.];
        let raw = PenEvent { tool: ToolKind::Finger, timestamp_ns: 1_010_000_000,
            surface_position: Point { x: position[0], y: position[1] }, ..event(&s, 1, PenPhase::Down, 1.) };
        assert!(s.input(pointer_input(1, ContactPhase::Down, PointerKind::Touch, PointerButton::Primary, position, raw.timestamp_ns)).unwrap().paint);
        if delivered { s.pen(raw); finger_frames(&mut s, 12); }
        let reply = s.input(UiInput::ColorPickerHold { id: 1, position, offset: 44. }).unwrap();
        assert!(reply.cancel_paint && s.color_picker_overlay().is_some());
        for phase in [PenPhase::Move, PenPhase::Up, PenPhase::Cancel] {
            assert!(s.filter_pen(PenEvent { phase, ..raw }).is_some());
        }
        if !delivered { s.pen(raw); }
        for phase in [PenPhase::Move, PenPhase::Cancel] { s.pen(PenEvent { phase, ..raw }); }
        finger_frames(&mut s, 550);
        assert!(s.color_picker_overlay().is_some(), "delivered={delivered}");
        picker_reply(&mut s, [1.; 4]);
        finger_sample(&mut s, 1, ContactPhase::Up, position, 600);
        finger_frames(&mut s, 602);
        assert!(s.eyedropper.picking.previous.is_none());
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.engine.metrics().committed_strokes, 0);
        assert!(finger_sample(&mut s, 2, ContactPhase::Down, position, 650).paint);
        finger_sample(&mut s, 2, ContactPhase::Cancel, position, 660);
    }
}

#[test]
fn finger_pen_wins_and_rejected_fingers_wait_for_lift() {
    let mut s = finger_session();
    finger_sample(&mut s, 1, ContactPhase::Down, [250., 300.], 10);
    finger_frames(&mut s, 12);
    let reply = s.input(pointer_input(7, ContactPhase::Down, PointerKind::Pen, PointerButton::Primary, [400., 400.], 1_100_000_000)).unwrap();
    assert!(reply.paint && reply.cancel_paint);
    s.pen(PenEvent { device_id: 7, sequence: 0, timestamp_ns: 1_100_000_000, ..event(&s, 0, PenPhase::Down, 1.) });
    let palm = finger_sample(&mut s, 2, ContactPhase::Down, [550., 400.], 120);
    assert!(palm.handled && !palm.paint);
    finger_frames(&mut s, 122);
    assert!(s.state.settings.pen_seen);
    s.input(pointer_input(7, ContactPhase::Up, PointerKind::Pen, PointerButton::Primary, [450., 400.], 1_200_000_000)).unwrap();
    s.pen(PenEvent { device_id: 7, sequence: 0, timestamp_ns: 1_200_000_000, ..event(&s, 0, PenPhase::Up, 1.) });
    finger_frames(&mut s, 202);
    assert_eq!(s.engine.metrics().committed_strokes, 1);
    for id in [1, 2] {
        assert!(!finger_sample(&mut s, id, ContactPhase::Move, [650., 400.], 220).paint);
    }
    assert!(finger_sample(&mut s, 3, ContactPhase::Down, [300., 500.], 230).paint);
    finger_sample(&mut s, 3, ContactPhase::Up, [300., 500.], 240);
    for id in [1, 2] { finger_sample(&mut s, id, ContactPhase::Up, [650., 400.], 250); }
    finger_frames(&mut s, 252);
    assert_eq!(s.engine.metrics().committed_strokes, 2);
    assert!(finger_sample(&mut s, 1, ContactPhase::Down, [250., 300.], 260).paint);
    finger_sample(&mut s, 1, ContactPhase::Cancel, [250., 300.], 270);
}

#[test]
fn finger_pen_seen_requires_authentic_admission_and_survives_broadcast_and_restart() {
    for tool in [ToolKind::Pen, ToolKind::Eraser, ToolKind::Brush, ToolKind::Pencil, ToolKind::Airbrush, ToolKind::Unknown, ToolKind::Finger, ToolKind::Mouse] {
        for (phase, through_filter) in [(PenPhase::Up, true), (PenPhase::Cancel, true), (PenPhase::Cancel, false)] {
            let mut s = session(Platform::Gtk);
            s.set_device_class(DeviceClass::Tablet);
            s.renderer_mut().painting_unready = phase == PenPhase::Cancel;
            s.pen(PenEvent { tool, ..event(&s, 1, PenPhase::Hover, 0.) });
            finger_frames(&mut s, 10);
            assert!(!s.state.settings.pen_seen);
            s.pen(PenEvent { tool, ..event(&s, 2, PenPhase::Down, 1.) });
            let terminal = PenEvent { tool, ..event(&s, 3, phase, 1.) };
            if through_filter { s.pen(terminal); } else { s.admit_pen(terminal); }
            assert!(!s.state.settings.pen_seen);
            s.renderer_mut().painting_unready = false;
            finger_frames(&mut s, 40);
            let expected = tool.is_pen() && through_filter;
            assert_eq!(s.state.settings.pen_seen, expected, "{tool:?}/{phase:?}/{through_filter}");
            if expected {
                let saved = s.state.requests.iter().rev().find_map(|request| match &request.kind {
                    HostRequestKind::SaveSettings { settings } => Some(settings.clone()), _ => None,
                }).unwrap();
                for restarted in [false, true] {
                    let mut other = session(Platform::Gtk);
                    other.set_device_class(DeviceClass::Tablet);
                    let saved = if restarted { Box::new(Settings::restore(&serde_json::to_string(&saved).unwrap())) } else { saved.clone() };
                    other.dispatch(UiAction::RestoreSettings { settings: saved }).unwrap();
                    assert!(other.state.settings.pen_seen);
                    assert!(!other.finger_drawing());
                }
            }
        }
    }
    let mut locked = session(Platform::Gtk);
    layer(&mut locked, LayerAction::Lock { id: 1, value: true });
    locked.pen(event(&locked, 1, PenPhase::Down, 1.));
    locked.pen(event(&locked, 2, PenPhase::Up, 1.));
    finger_frames(&mut locked, 30);
    assert!(!locked.state.settings.pen_seen);
}

#[test]
fn finger_cancellation_preserves_artwork_for_drawing_discrete_and_navigation_tools() {
    for tool in [LayerCanvasTool::Paint, LayerCanvasTool::Region { fill: true, source: RegionSource::Visible },
        LayerCanvasTool::Gradient { shape: layer_core::GradientShape::default() }, LayerCanvasTool::LassoFill,
        LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Outline },
        LayerCanvasTool::Selection { kind: SelectionTool::Rectangle }, LayerCanvasTool::Selection { kind: SelectionTool::Ellipse },
        LayerCanvasTool::Selection { kind: SelectionTool::Polygon }] {
        let mut s = finger_session();
        s.object_tool(tool).unwrap();
        let before = s.engine.document().artwork.clone();
        let checkpoint = s.engine.checkpoint();
        finger_sample(&mut s, 1, ContactPhase::Down, [250., 300.], 10);
        finger_sample(&mut s, 1, ContactPhase::Move, [350., 350.], 30);
        finger_frames(&mut s, 32);
        assert!(!finger_sample(&mut s, 2, ContactPhase::Down, [500., 400.], 60).paint, "{tool:?}");
        finger_sample(&mut s, 1, ContactPhase::Up, [350., 350.], 90);
        finger_sample(&mut s, 2, ContactPhase::Up, [500., 400.], 110);
        finger_frames(&mut s, 112);
        assert_eq!(s.engine.document().artwork, before, "{tool:?}");
        assert_eq!(s.engine.checkpoint(), checkpoint, "{tool:?}");
    }
}

#[test]
fn finger_withdrawal_preserves_a_preexisting_polygon_or_vector_path_draft() {
    for vector in [false, true] {
        let mut s = if vector { vector_session().0 } else { finger_session() };
        s.state.settings.finger_drawing = FingerDrawing::On;
        s.object_tool(if vector { LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } }
            else { LayerCanvasTool::Selection { kind: SelectionTool::Polygon } }).unwrap();
        let first = s.state.camera.input_transform().map(Point { x: 200., y: 300. });
        for (i, at) in [[200., 300.], [400., 300.], [400., 500.]].into_iter().enumerate() {
            finger_sample(&mut s, 1, ContactPhase::Down, at, 10 + i as u64 * 100);
            finger_sample(&mut s, 1, ContactPhase::Up, at, 20 + i as u64 * 100);
        }
        let before = s.engine.document().artwork.clone();
        let checkpoint = s.engine.checkpoint();
        let old_path = s.layer_interaction.path.clone();
        let preview = s.engine.backend().vector_preview.clone();
        finger_sample(&mut s, 1, ContactPhase::Down, [200., 300.], 400);
        let closing_revision = s.engine.backend().vector_preview.as_ref().map(|preview| preview.revision);
        assert!(finger_sample(&mut s, 2, ContactPhase::Down, [600., 600.], 410).cancel_paint);
        if vector {
            let restored = s.engine.backend().vector_preview.as_ref().unwrap();
            assert_eq!(restored.path, preview.unwrap().path);
            assert!(restored.revision > closing_revision.unwrap());
        }
        finger_sample(&mut s, 1, ContactPhase::Up, [200., 300.], 430);
        finger_sample(&mut s, 2, ContactPhase::Up, [600., 600.], 450);
        finger_frames(&mut s, 452);
        assert_eq!(s.engine.document().artwork, before);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.layer_interaction.path, old_path);
        if vector { assert!(s.vector_paths.drafting()); }
        let close = surface_at(&s, [f64::from(first.x), f64::from(first.y)]);
        finger_sample(&mut s, 1, ContactPhase::Down, [close.x, close.y], 600);
        finger_sample(&mut s, 1, ContactPhase::Up, [close.x, close.y], 610);
        finger_frames(&mut s, 612);
        assert!(s.layer_interaction.path.is_empty());
        if vector { assert_ne!(s.engine.checkpoint(), checkpoint); }
        else { assert!(s.engine.document().working.selection.is_some()); }
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().artwork, before);
        assert!(s.engine.document().working.selection.is_none());
    }
}

#[test]
fn finger_anchor_switch_is_a_reversible_contact_and_one_history_step_on_release() {
    let (mut s, layer) = point_edit_session(Platform::Gtk);
    s.state.settings.finger_drawing = FingerDrawing::On;
    s.state.settings.gestures.insert("touch.tap.2".into(), String::new());
    invoke(&mut s, CommandId::EditPoints);
    let at = surface_at(&s, [100., 300.]);
    let at = [at.x, at.y];
    finger_sample(&mut s, 1, ContactPhase::Down, at, 10);
    finger_sample(&mut s, 1, ContactPhase::Up, at, 20);
    let before = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let checkpoint = s.engine.checkpoint();
    finger_sample(&mut s, 1, ContactPhase::Down, at, 50);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert!(finger_sample(&mut s, 2, ContactPhase::Down, [600., 600.], 60).cancel_paint);
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &before);
    for id in [1, 2] { finger_sample(&mut s, id, ContactPhase::Up, at, 80); }
    let at = surface_at(&s, [100., 300.]);
    let at = [at.x, at.y];
    finger_sample(&mut s, 1, ContactPhase::Down, at, 200);
    finger_sample(&mut s, 1, ContactPhase::Up, at, 210);
    assert_ne!(s.engine.checkpoint(), checkpoint);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &before);
}

#[test]
fn finger_clone_source_and_picker_taps_follow_the_mode_without_losing_source_on_cancel() {
    let mut s = clone_session();
    s.state.settings.finger_drawing = FingerDrawing::On;
    invoke(&mut s, CommandId::CloneSourceArm);
    let before = s.engine.clone_source();
    let checkpoint = s.engine.checkpoint();
    assert!(!finger_sample(&mut s, 1, ContactPhase::Down, [300., 300.], 10).paint);
    finger_sample(&mut s, 1, ContactPhase::Move, [350., 350.], 30);
    assert_ne!(s.engine.clone_source(), before);
    assert!(finger_sample(&mut s, 2, ContactPhase::Down, [600., 600.], 50).cancel_paint);
    assert_eq!(s.engine.clone_source(), before);
    assert!(s.retouch.armed);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    for mode in [FingerDrawing::On, FingerDrawing::Off] {
        let mut s = finger_session();
        s.state.settings.finger_drawing = mode;
        invoke(&mut s, CommandId::Eyedropper);
        assert!(!finger_sample(&mut s, 1, ContactPhase::Down, [300., 300.], 10).paint);
        assert_eq!(s.eyedropper.picking.touch, (mode == FingerDrawing::On).then_some(1));
        assert_eq!(s.eyedropper.picking.previous.is_some(), mode == FingerDrawing::On);
    }
}

#[test]
fn finger_held_contacts_keep_original_time_and_can_be_withdrawn_before_admission() {
    for cancel in [false, true] {
        let mut s = finger_session();
        s.renderer_mut().painting_unready = true;
        finger_sample(&mut s, 1, ContactPhase::Down, [300., 300.], 10);
        finger_sample(&mut s, 1, ContactPhase::Move, [400., 300.], 30);
        assert!(s.holding_input());
        if cancel { assert!(finger_sample(&mut s, 2, ContactPhase::Down, [600., 600.], 40).cancel_paint); }
        finger_sample(&mut s, 1, ContactPhase::Up, [400., 300.], 50);
        if cancel { finger_sample(&mut s, 2, ContactPhase::Up, [600., 600.], 60); }
        s.renderer_mut().painting_unready = false;
        finger_frames(&mut s, 70);
        assert!(!s.holding_input());
        assert_eq!(s.engine.metrics().committed_strokes, u64::from(!cancel));
        assert_eq!(s.engine.metrics().last_consumed_paint_ns, if cancel { 0 } else { 1_030_000_000 });
    }
}

#[test]
fn finger_withdrawal_keeps_original_tap_and_scrub_recognition() {
    for scrub in [false, true] {
        let mut s = painted_session();
        s.state.settings.finger_drawing = FingerDrawing::On;
        let before = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        for id in 0..if scrub { 3 } else { 2 } {
            finger_sample(&mut s, id, ContactPhase::Down, [300. + id as f32 * 80., 400.], id * 10);
        }
        if scrub {
            for (step, offset) in [60., 0., 60.].into_iter().enumerate() {
                for id in 0..3 {
                    finger_sample(&mut s, id, ContactPhase::Move, [300. + id as f32 * 80. + offset, 400.], 100 + step as u64 * 100);
                }
            }
        }
        for id in 0..if scrub { 3 } else { 2 } {
            finger_sample(&mut s, id, ContactPhase::Up, [300. + id as f32 * 80. + if scrub { 60. } else { 0. }, 400.], 350 + id * 10);
        }
        finger_frames(&mut s, 400);
        assert_ne!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.engine.metrics().committed_strokes, 1);
        if scrub {
            assert!(s.engine.document().target_operations(s.engine.document().working.target.unwrap()).unwrap().is_empty());
            invoke(&mut s, CommandId::Undo);
        } else { invoke(&mut s, CommandId::Redo); }
        assert_live_artwork_eq(s.engine.document(), &before);
    }
}

#[test]
fn finger_withdrawal_rolls_back_the_crop_contact_and_keeps_the_open_crop() {
    let mut s = crop_session();
    s.state.settings.finger_drawing = FingerDrawing::On;
    invoke(&mut s, CommandId::Crop);
    let before = crop_frame(&s);
    let corner = on_surface(&s, before.corners()[2]);
    let checkpoint = s.engine.checkpoint();
    finger_sample(&mut s, 1, ContactPhase::Down, [corner.x, corner.y], 10);
    finger_sample(&mut s, 1, ContactPhase::Move, [corner.x - 40., corner.y - 30.], 30);
    assert!(s.operation.dragging());
    assert!(finger_sample(&mut s, 2, ContactPhase::Down, [600., 500.], 50).cancel_paint);
    assert_eq!(crop_frame(&s), before);
    assert!(!s.operation.dragging());
    assert_eq!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn pen_detection_excludes_locked_vector_drawing_contacts() {
    for tool in [LayerCanvasTool::Paint, LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Outline },
        LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline }, LayerCanvasTool::Region { fill: true, source: RegionSource::Visible }] {
        let (mut s, id) = vector_session();
        s.object_tool(tool).unwrap();
        let doc = s.engine.document();
        let mut occurrence = doc.scene().occurrence(id).unwrap().clone();
        occurrence.locked = true;
        let edit = layer_core::Edit::Occurrence(layer_core::authored::RecordChange::replace(&doc.artwork.occurrences, id, Some(occurrence)).unwrap());
        s.engine.apply_edit(edit).unwrap();
        let checkpoint = s.engine.checkpoint();
        for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
            s.pen(event(&s, sequence, phase, 1.));
        }
        finger_frames(&mut s, 40);
        assert!(!s.state.settings.pen_seen, "{tool:?}");
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
}
