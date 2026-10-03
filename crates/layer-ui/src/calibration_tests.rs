fn calibration_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "white_balance".into() } }).unwrap();
    s.frame(1, 1).unwrap();
    s
}
fn arm_calibration(s: &mut UiSession<Recorder>) {
    s.dispatch(UiAction::Effect { action: EffectAction::WhiteBalancePicker {
        layer: s.engine.document().active_layer.0, epoch: s.state.layer_properties.epoch,
    } }).unwrap();
}
fn release_calibration(s: &mut UiSession<Recorder>) {
    pen_at(s, 1, PenPhase::Down, [400., 400.]);
    pen_at(s, 2, PenPhase::Up, [401., 402.]);
    s.frame(2, 2).unwrap();
}
fn calibration_reply(s: &mut UiSession<Recorder>, result: Result<layer_core::ArtworkSample, layer_render::BackendError>) {
    s.engine.backend_mut().snapshot_reply = Some(result.map(layer_render::SnapshotResult::ArtworkSample));
    s.frame(30, 30).unwrap();
    s.frame(31, 31).unwrap();
}

#[test]
fn calibration_release_uses_exact_input_and_commits_one_atomic_undo_without_paint() {
    let mut s = calibration_session();
    let before = s.engine.document().layers.clone();
    let checkpoint = s.engine.checkpoint();
    let colors = s.state.display_colors().definition();
    let tool = s.layer_interaction.tool;
    arm_calibration(&mut s);
    assert!(s.eyedropper.calibration.is_some());
    assert_eq!(s.engine.document().layers, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    pen_at(&mut s, 1, PenPhase::Down, [400., 400.]);
    s.frame(2, 2).unwrap();
    assert!(s.engine.backend().snapshot_requests.is_empty());
    pen_at(&mut s, 2, PenPhase::Up, [401., 402.]);
    s.frame(3, 3).unwrap();
    let layer_render::SnapshotRequest::ArtworkSample(request) = s.engine.backend().snapshot_requests.last().unwrap() else { panic!("expected sample"); };
    assert_eq!(request.source, layer_core::ArtworkSource::EffectInput(s.engine.document().active_layer));
    assert_eq!(request.position, [401., 402.]);
    assert_eq!(request.width, 5);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert!(s.eyedropper.calibration.is_none());
    assert_eq!(s.layer_interaction.tool, tool);
    assert_eq!(s.state.display_colors().definition(), colors);
    assert_eq!(s.engine.backend().dabs, 0);
    assert_ne!(s.engine.document().layers, before);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().layers, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn calibration_pending_admission_keeps_released_contact_until_ready() {
    let mut s = calibration_session();
    s.engine.backend_mut().snapshot_wait = true;
    arm_calibration(&mut s);
    release_calibration(&mut s);
    assert!(s.engine.backend().snapshot_requests.is_empty());
    assert!(s.eyedropper.picking.finishing);
    s.engine.backend_mut().snapshot_wait = false;
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.25; 4])));
    assert!(s.eyedropper.calibration.is_none());
}

#[test]
fn calibration_failed_empty_outside_and_nonpositive_samples_preserve_history_and_retry() {
    for result in [
        Err(layer_render::BackendError("sample failed".into())),
        Ok(layer_core::ArtworkSample::Empty), Ok(layer_core::ArtworkSample::Outside),
        Ok(layer_core::ArtworkSample::Color([0., 0.25, 0.5, 1.])),
    ] {
        let mut s = calibration_session();
        let before = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        arm_calibration(&mut s);
        release_calibration(&mut s);
        calibration_reply(&mut s, result);
        assert_eq!(s.engine.document(), &before);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert!(s.eyedropper.calibration.is_some());
        assert!(!s.eyedropper.picking.finishing);
        assert!(notice_text(&s).is_some());
    }
}

#[test]
fn calibration_cancellation_discards_late_reply_and_restores_or_selects_requested_tool() {
    for cancellation in 0..4 {
        let mut s = calibration_session();
        let before = s.engine.document().layers.clone();
        let checkpoint = s.engine.checkpoint();
        arm_calibration(&mut s);
        release_calibration(&mut s);
        match cancellation {
            0 => { key(&mut s, "Escape", true, false, false); }
            1 => { s.input(UiInput::Blur).unwrap(); }
            2 => { invoke(&mut s, CommandId::Hand); }
            _ => { let (retired, _) = s.replace_renderer(Recorder::default()).unwrap(); assert!(retired.snapshot_cancels > 0); }
        }
        assert!(s.eyedropper.calibration.is_none());
        if cancellation == 2 { assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Hand); }
        calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
        assert_eq!(s.engine.document().layers, before);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
}

#[test]
fn calibration_stale_epoch_and_changed_artwork_cannot_publish() {
    let mut s = calibration_session();
    s.dispatch(UiAction::Effect { action: EffectAction::WhiteBalancePicker {
        layer: s.engine.document().active_layer.0, epoch: s.state.layer_properties.epoch.wrapping_add(1),
    } }).unwrap();
    assert!(s.eyedropper.calibration.is_none());
    for invalidation in 0..3 {
        let mut s = calibration_session();
        arm_calibration(&mut s);
        release_calibration(&mut s);
        let mut changed = if invalidation == 2 { s.engine.document().layers.iter().find(|layer| layer.kind == layer_core::LayerKind::Paint).unwrap().clone() } else { s.engine.document().layers[0].clone() };
        match invalidation {
            0 => { changed.opacity = 0.5; }
            1 => { std::sync::Arc::make_mut(changed.effect.as_mut().unwrap()).set("temperature", layer_core::EffectValue::Number(12.)).unwrap(); }
            _ => { changed.source = Some(layer_core::color::source::rgba8_source([1, 1], |_, _| [32; 4])); }
        }
        s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(changed))).unwrap();
        let expected = s.engine.document().layers.clone();
        calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
        assert_eq!(s.engine.document().layers, expected);
        assert!(s.eyedropper.calibration.is_none());
    }
}

#[test]
fn calibration_target_switch_or_document_replacement_discards_result() {
    for change in 0..2 {
        let mut s = calibration_session();
        arm_calibration(&mut s);
        release_calibration(&mut s);
        if change == 0 {
            let id = s.engine.document().layers.iter().find(|layer| layer.kind == layer_core::LayerKind::Paint).unwrap().id;
            s.engine.apply_edit(layer_core::Edit::SetActiveLayer { id }).unwrap();
        } else {
            s.state.document_file.epoch += 1;
        }
        let expected = s.engine.document().layers.clone();
        calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
        assert_eq!(s.engine.document().layers, expected);
        assert!(s.eyedropper.calibration.is_none());
    }
}

#[test]
fn calibration_time_only_advancement_preserves_released_frozen_sample() {
    let mut s = calibration_session();
    arm_calibration(&mut s);
    release_calibration(&mut s);
    let layer_render::SnapshotRequest::ArtworkSample(request) = s.engine.backend().snapshot_requests.last().unwrap() else { panic!("expected sample"); };
    let captured = request.time;
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert!(s.engine.animation_time() > captured);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert!(s.eyedropper.calibration.is_none());
    let effect = s.engine.document().layers[0].effect.as_ref().unwrap();
    assert_eq!(effect.value("temperature"), Some(&layer_core::EffectValue::Number(125.)));
}

#[test]
fn calibration_save_waits_for_atomic_correction_then_captures_corrected_values() {
    let mut s = calibration_session();
    arm_calibration(&mut s);
    release_calibration(&mut s);
    let before = s.engine.document().layers.clone();
    assert!(!s.command(CommandId::SaveDocument).enabled);
    assert!(s.request_save(false).is_err());
    assert!(s.files.pending.is_none());
    assert_eq!(s.engine.document().layers, before);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert!(s.command(CommandId::SaveDocument).enabled);
    invoke(&mut s, CommandId::SaveDocument);
    let id = s.files.pending.as_ref().unwrap().0;
    let project = s.capture_project_save(id, DocumentLocation { uri: "file:///private-query.capy".into(), name: "private-query.capy".into() }).unwrap();
    assert_eq!(project.document.layers, s.engine.document().layers);
    assert_ne!(project.document.layers, before);
    assert_eq!(project.document.layers[0].effect.as_ref().unwrap().value("temperature"), Some(&layer_core::EffectValue::Number(125.)));
}

#[test]
fn calibration_touch_loupe_samples_released_offset_contact_without_changing_paint_color() {
    let mut s = calibration_session();
    let colors = s.state.colors.clone();
    arm_calibration(&mut s);
    picker_pointer(&mut s, 8, ContactPhase::Down, PointerKind::Touch, [400., 400.]);
    assert!(s.eyedropper.calibration.is_some());
    s.input(UiInput::ColorPickerHold { id: 8, position: [400., 400.], offset: 44. }).unwrap();
    assert_eq!(s.color_picker_overlay().unwrap().sample, [400., 356.]);
    picker_pointer(&mut s, 8, ContactPhase::Move, PointerKind::Touch, [420., 430.]);
    let point = s.state.camera.input_transform().map(Point { x: 420., y: 386. });
    picker_pointer(&mut s, 8, ContactPhase::Up, PointerKind::Touch, [420., 430.]);
    s.frame(2, 2).unwrap();
    let layer_render::SnapshotRequest::ArtworkSample(request) = s.engine.backend().snapshot_requests.last().unwrap() else { panic!("expected sample"); };
    assert_eq!(request.position, [point.x, point.y]);
    assert!(s.engine.backend().sample_requests.is_empty());
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert_eq!(s.state.colors, colors);
    assert_eq!(s.engine.backend().dabs, 0);
}

fn calibration_touch_hold(s: &mut UiSession<Recorder>) {
    arm_calibration(s);
    picker_pointer(s, 8, ContactPhase::Down, PointerKind::Touch, [400., 400.]);
    s.input(UiInput::ColorPickerHold { id: 8, position: [400., 400.], offset: 44. }).unwrap();
    s.frame(2, 2).unwrap();
}
fn last_calibration_request(s: &UiSession<Recorder>) -> &layer_core::ArtworkSampleRequest {
    let layer_render::SnapshotRequest::ArtworkSample(request) = s.engine.backend().snapshot_requests.last().unwrap() else { panic!("expected sample"); };
    request
}

#[test]
fn calibration_touch_preview_coalesces_without_editing_and_release_waits_for_new_point() {
    let mut s = calibration_session();
    let before = s.engine.document().layers.clone();
    let colors = s.state.preview_colors().into_owned();
    let checkpoint = s.engine.checkpoint();
    calibration_touch_hold(&mut s);
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    picker_pointer(&mut s, 8, ContactPhase::Move, PointerKind::Touch, [410., 410.]);
    picker_pointer(&mut s, 8, ContactPhase::Move, PointerKind::Touch, [420., 430.]);
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 0.5])));
    assert_eq!(s.engine.document().layers, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert_eq!(*s.state.preview_colors(), colors);
    let preview = s.state.color_picker.preview.unwrap().linear_in(s.engine.document().color.space).unwrap();
    for (actual, expected) in preview.into_iter().zip([0.125, 0.25, 0.5, 1.]) { assert!((actual - expected).abs() < 1e-5); }
    assert_eq!(s.engine.backend().snapshot_requests.len(), 2);
    let point = s.state.camera.input_transform().map(Point { x: 420., y: 386. });
    assert_eq!(last_calibration_request(&s).position, [point.x, point.y]);
    let cancelled = s.engine.backend().snapshot_cancels;
    picker_pointer(&mut s, 8, ContactPhase::Up, PointerKind::Touch, [440., 450.]);
    s.frame(32, 32).unwrap();
    assert!(s.engine.backend().snapshot_cancels > cancelled);
    assert_eq!(s.engine.backend().snapshot_requests.len(), 3);
    let point = s.state.camera.input_transform().map(Point { x: 440., y: 406. });
    assert_eq!(last_calibration_request(&s).position, [point.x, point.y]);
    assert_eq!(s.engine.document().layers, before);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.25; 4])));
    assert_eq!(s.engine.document().layers, before);
    assert!(s.eyedropper.calibration.is_none());
}

#[test]
fn calibration_touch_width_change_cancels_old_preview_and_resamples_latest_contact() {
    let mut s = calibration_session();
    calibration_touch_hold(&mut s);
    picker_pointer(&mut s, 8, ContactPhase::Move, PointerKind::Touch, [420., 430.]);
    let cancelled = s.engine.backend().snapshot_cancels;
    s.dispatch(UiAction::SetColorSampleSize { width: 51 }).unwrap();
    assert!(s.engine.backend().snapshot_cancels > cancelled);
    assert!(s.state.color_picker.preview.is_none());
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), 2);
    assert_eq!(last_calibration_request(&s).width, 51);
    let point = s.state.camera.input_transform().map(Point { x: 420., y: 386. });
    assert_eq!(last_calibration_request(&s).position, [point.x, point.y]);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.25; 4])));
    assert!(s.state.color_picker.preview.is_some());
    assert!(s.eyedropper.calibration.is_some());
    let count = s.engine.backend().snapshot_requests.len();
    s.dispatch(UiAction::SetColorSampleSize { width: 51 }).unwrap();
    s.frame(32, 32).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), count);
}

#[test]
fn calibration_unported_hosts_hide_picker_and_refuse_direct_action_without_editing() {
    for platform in [Platform::Web, Platform::Android] {
        let mut s = session(platform);
        s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "white_balance".into() } }).unwrap();
        s.frame(1, 1).unwrap();
        assert!(s.state.layer_properties.actions.is_empty());
        let before = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        let tool = s.state.layer_tools.tool;
        let colors = s.state.preview_colors().into_owned();
        let unavailable = s.localization().text(MessageId::RESOURCES_PICKER_UNAVAILABLE).to_string();
        let result = s.dispatch(UiAction::Effect { action: EffectAction::WhiteBalancePicker {
            layer: s.engine.document().active_layer.0, epoch: s.state.layer_properties.epoch,
        } });
        assert_eq!(result.unwrap_err(), unavailable);
        assert_eq!(s.engine.document(), &before);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.state.layer_tools.tool, tool);
        assert_eq!(*s.state.preview_colors(), colors);
        assert!(s.eyedropper.calibration.is_none());
        assert!(s.engine.backend().snapshot_requests.is_empty());
    }
}
