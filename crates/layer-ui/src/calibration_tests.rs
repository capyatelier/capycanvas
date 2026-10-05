fn calibration_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "white_balance".into() } }).unwrap();
    s.frame(1, 1).unwrap();
    s
}
fn arm_calibration(s: &mut UiSession<Recorder>) {
    s.dispatch(UiAction::Effect { action: EffectAction::Calibrate { role:layer_core::levels::CalibrationRole::Gray,
        layer: occurrence_token(s.engine.document().working.occurrence.unwrap()), epoch: s.state.layer_properties.epoch,
    } }).unwrap();
}
#[test]
fn calibration_stale_toolbar_action_preserves_released_pending_sample() {
    let mut s=calibration_session();arm_calibration(&mut s);release_calibration(&mut s);
    let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;
    let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
    for action in stale_property_actions(occurrence_token(s.engine.document().working.occurrence.unwrap()),s.state.layer_properties.epoch.wrapping_sub(1)) {
        s.dispatch(UiAction::Effect {action}).unwrap();
        assert!(s.eyedropper.calibration.as_ref().is_some_and(|calibration|calibration.submitted));
        assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert_eq!(s.engine.backend().snapshot_cancels,cancels);
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
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
    let before = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    let colors = s.state.display_colors().definition();
    let tool = s.layer_interaction.tool;
    arm_calibration(&mut s);
    assert!(s.eyedropper.calibration.is_some());
    assert_eq!(s.engine.document().artwork, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    pen_at(&mut s, 1, PenPhase::Down, [400., 400.]);
    s.frame(2, 2).unwrap();
    assert!(s.engine.backend().snapshot_requests.is_empty());
    pen_at(&mut s, 2, PenPhase::Up, [401., 402.]);
    s.frame(3, 3).unwrap();
    let layer_render::SnapshotRequest::ArtworkSample(request) = s.engine.backend().snapshot_requests.last().unwrap() else { panic!("expected sample"); };
    assert_eq!(request.source, layer_core::ArtworkSource::EffectInput(s.engine.document().working.occurrence.unwrap()));
    assert_eq!(request.position, [401., 402.]);
    assert_eq!(request.width, 5);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert!(s.eyedropper.calibration.is_none());
    assert_eq!(s.layer_interaction.tool, tool);
    assert_eq!(s.state.display_colors().definition(), colors);
    assert_eq!(s.engine.backend().dabs, 0);
    assert_ne!(s.engine.document().artwork, before);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, before);
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
        Err(layer_render::BackendError("sample failed")),
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
        let before = s.engine.document().artwork.clone();
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
        assert_eq!(s.engine.document().artwork, before);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
}

#[test]
fn calibration_stale_epoch_and_changed_artwork_cannot_publish() {
    let mut s = calibration_session();
    s.dispatch(UiAction::Effect { action: EffectAction::Calibrate { role:layer_core::levels::CalibrationRole::Gray,
        layer: occurrence_token(s.engine.document().working.occurrence.unwrap()), epoch: s.state.layer_properties.epoch.wrapping_add(1),
    } }).unwrap();
    assert!(s.eyedropper.calibration.is_none());
    for invalidation in 0..3 {
        let mut s = calibration_session();
        arm_calibration(&mut s);
        release_calibration(&mut s);
        let document=s.engine.document();let target=document.working.occurrence.unwrap();
        let edit=match invalidation {
            0=>effect_test_occurrence_edit(document,target,|o|o.opacity=0.5),
            1=>{let mut draft=effects::effect_draft(document,target).unwrap();draft.set("temperature",layer_core::EffectValue::Number(12.)).unwrap();effects::effect_edit(document,target,draft).unwrap()},
            _=>{let paint=effect_test_paint(document);let layer_core::SourceTarget::Paint(handle)=document.scene().source_target(paint).unwrap() else {unreachable!()};let mut source=document.artwork.paint.get(handle).unwrap().clone();source.base=Some(layer_core::PaintBase::new(layer_core::Image::new(layer_core::color::source::rgba8_source([1,1],|_,_|[32;4]))));layer_core::Edit::Paint(layer_core::authored::RecordChange::replace(&document.artwork.paint,handle,Some(source)).unwrap())},
        };
        s.engine.apply_edit(edit).unwrap();
        let expected = s.engine.document().artwork.clone();
        calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
        assert_eq!(s.engine.document().artwork, expected);
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
            let id = effect_test_paint(s.engine.document());
            s.engine.apply_edit(s.engine.document().select_occurrence_edit(id).unwrap()).unwrap();
        } else {
            s.state.document_file.epoch += 1;
        }
        let expected = s.engine.document().artwork.clone();
        calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
        assert_eq!(s.engine.document().artwork, expected);
        assert!(s.eyedropper.calibration.is_none());
    }
}

#[test]
fn calibration_time_only_advancement_preserves_released_frozen_sample() {
    let mut s = calibration_session();
    arm_calibration(&mut s);
    release_calibration(&mut s);
    let layer_render::SnapshotRequest::ArtworkSample(request) = s.engine.backend().snapshot_requests.last().unwrap() else { panic!("expected sample"); };
    let captured = request.query.snapshot.context.elapsed;
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert!(s.engine.animation_time() > captured);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert!(s.eyedropper.calibration.is_none());
    let effect = s.engine.document().scene().effect(s.engine.document().scene().order()[0]).unwrap();
    assert_eq!(effect.value("temperature"), Some(&layer_core::EffectValue::Number(125.)));
}

#[test]
fn calibration_save_waits_for_atomic_correction_then_captures_corrected_values() {
    let mut s = calibration_session();
    arm_calibration(&mut s);
    release_calibration(&mut s);
    let before = s.engine.document().artwork.clone();
    assert!(!s.command(CommandId::SaveDocument).enabled);
    assert!(s.request_save(false).is_err());
    assert!(s.files.pending.is_none());
    assert_eq!(s.engine.document().artwork, before);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
    assert!(s.command(CommandId::SaveDocument).enabled);
    invoke(&mut s, CommandId::SaveDocument);
    let id = s.files.pending.as_ref().unwrap().0;
    let project = s.capture_project_save(id, DocumentLocation { uri: "file:///private-query.capy".into(), name: "private-query.capy".into() }).unwrap();
    let captured_document=layer_core::Document::from_artwork(project.artwork.as_ref().clone()).unwrap();
    let captured=captured_document.scene();
    let live=s.engine.document().scene();
    assert_eq!(captured.effect_application(captured.order()[0]),live.effect_application(live.order()[0]));
    assert_eq!(project.artwork.occurrences,s.engine.document().artwork.occurrences);
    assert_ne!(*project.artwork,before);
    assert_eq!(captured.effect(captured.order()[0]).unwrap().value("temperature"), Some(&layer_core::EffectValue::Number(125.)));
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
    let before = s.engine.document().artwork.clone();
    let colors = s.state.preview_colors().into_owned();
    let checkpoint = s.engine.checkpoint();
    calibration_touch_hold(&mut s);
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    picker_pointer(&mut s, 8, ContactPhase::Move, PointerKind::Touch, [410., 410.]);
    picker_pointer(&mut s, 8, ContactPhase::Move, PointerKind::Touch, [420., 430.]);
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 0.5])));
    assert_eq!(s.engine.document().artwork, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert_eq!(*s.state.preview_colors(), colors);
    let preview = s.state.color_picker.preview.unwrap().linear_in(s.engine.document().composition().color.space).unwrap();
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
    assert_eq!(s.engine.document().artwork, before);
    calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.25; 4])));
    assert_eq!(s.engine.document().artwork, before);
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
fn calibration_picker_groups_publish_stable_localized_identity_in_every_language() {
    for language in UiLanguage::ALL {for id in ["white_balance","levels","curves"] {
        let mut s=color_adjustment_session(id);s.set_localization(Localizer::shared(language));
        let actions:Vec<_>=s.state.layer_properties.actions.iter().filter(|a|matches!(a.action,EffectAction::Calibrate{..})).collect();
        assert_eq!(actions.len(),if id=="white_balance"{1}else{3});
        for action in actions {
            assert_eq!(action.icon.as_deref(),Some("layer-eyedropper-symbolic"));
            let EffectAction::Calibrate{role,..}=action.action else {unreachable!()};
            let label=match role {layer_core::levels::CalibrationRole::Black=>MessageId::RESOURCES_PICKER_BLACK,layer_core::levels::CalibrationRole::Gray=>MessageId::RESOURCES_PICKER_NEUTRAL,layer_core::levels::CalibrationRole::White=>MessageId::RESOURCES_PICKER_WHITE};
            assert_eq!(action.label,s.localization().text(label).as_ref());
            if id=="white_balance" {assert!(action.group.is_none());} else {let group=action.group.as_ref().unwrap();assert_eq!(group.id,"calibration");assert_eq!(group.label,s.localization().text(MessageId::RESOURCES_PICKER_POINTS).as_ref());}
        }
    }}
}

#[test]
fn calibration_failed_notice_language_refresh_preserves_picker_request_document_and_history() {
    for (sample, message) in [
        (Err(layer_render::BackendError("sample failed { $name } 🖌")), None),
        (Ok(layer_core::ArtworkSample::Empty), Some(MessageId::RESOURCES_PICKER_EMPTY)),
        (Ok(layer_core::ArtworkSample::Outside), Some(MessageId::RESOURCES_PICKER_EMPTY)),
        (Ok(layer_core::ArtworkSample::Color([0., 0.25, 0.5, 1.])), Some(MessageId::RESOURCES_PICKER_NEUTRAL_FAILED)),
    ] {
        let mut s = calibration_session();
        arm_calibration(&mut s);
        release_calibration(&mut s);
        let document = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        let colors = s.state.colors.clone();
        let tool = s.layer_interaction.tool;
        let calibration = |s: &UiSession<Recorder>| {
            let c = s.eyedropper.calibration.as_ref().unwrap();
            (c.original.clone(), c.epoch, c.document_epoch, c.width, c.request.as_ref().map(|r| (r.snapshot.clone(), std::sync::Arc::as_ptr(&r.snapshot), r.source.clone(), r.selection.clone(), r.position, r.width)), c.queued, c.submitted)
        };
        let pending = calibration(&s);
        let rendering = (s.engine.backend().snapshot_requests.len(), s.engine.backend().snapshot_cancels, s.engine.backend().dabs);
        for language in UiLanguage::ALL {
            s.set_localization(Localizer::shared(language));
            assert_eq!(calibration(&s), pending, "{}", language.tag());
            assert!(s.eyedropper.picking.finishing);
            assert_eq!(s.engine.document(), &document);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!((s.engine.backend().snapshot_requests.len(), s.engine.backend().snapshot_cancels, s.engine.backend().dabs), rendering);
            assert_eq!(s.state.canvas_bar.as_ref().unwrap().label.as_deref(), Some(s.localization().text(MessageId::RESOURCES_PICKER_PROMPT).as_ref()));
        }
        s.set_localization(Localizer::shared(UiLanguage::English));
        calibration_reply(&mut s, sample);
        let notice = s.state.notice.clone().unwrap();
        let failed = calibration(&s);
        assert!(failed.4.is_none());
        assert!(!failed.6);
        for language in UiLanguage::ALL.iter().copied().chain([UiLanguage::English]) {
            s.set_localization(Localizer::shared(language));
            let current = s.state.notice.as_ref().unwrap();
            assert_eq!(current.id, notice.id);
            assert_eq!(current.action, notice.action);
            let expected = message.map_or_else(|| notice.text.clone(), |message| s.localization().text(message).to_string());
            assert_eq!(current.text, expected, "{}", language.tag());
            assert_eq!(calibration(&s), failed);
            assert!(!s.eyedropper.picking.finishing);
            assert_eq!(s.state.canvas_bar.as_ref().unwrap().label.as_deref(), Some(s.localization().text(MessageId::RESOURCES_PICKER_PROMPT).as_ref()));
            assert_eq!(s.engine.document(), &document);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.state.colors, colors);
            assert_eq!(s.layer_interaction.tool, tool);
            assert_eq!((s.engine.backend().snapshot_requests.len(), s.engine.backend().snapshot_cancels, s.engine.backend().dabs), rendering);
        }
    }
}

#[test]
fn ported_calibration_exposes_shared_action_and_commits_one_undo_without_changing_paint() {
    for platform in [Platform::Web,Platform::Android,Platform::Windows] {
    let mut s=calibration_session();s.set_platform(platform);s.frame(1,1).unwrap();
    assert!(s.state.layer_properties.actions.iter().any(|a|matches!(a.action,EffectAction::Calibrate{..})));
    let before=s.engine.document().clone();let paint=s.state.preview_colors().into_owned();let checkpoint=s.engine.checkpoint();arm_calibration(&mut s);release_calibration(&mut s);
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkSample(_))));
    calibration_reply(&mut s,Ok(layer_core::ArtworkSample::Color([0.6,0.3,0.2,1.])));
    assert_ne!(s.engine.checkpoint(),checkpoint);assert_eq!(*s.state.preview_colors(),paint);invoke(&mut s,CommandId::Undo);assert_eq!(s.engine.document().artwork,before.artwork);
    }
}
