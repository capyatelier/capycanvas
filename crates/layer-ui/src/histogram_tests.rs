fn histogram_open(s: &mut UiSession<Recorder>) {
    invoke(s, CommandId::Histogram);
    s.frame(100_000_000, 100_000_000).unwrap();
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview));
}

#[test]
fn histogram_controls_have_captions_before_the_first_frame() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios] {
        let s = session(platform);
        for view in [&s.state.histogram, &s.state.waveform, &s.state.tonal_histogram] {
            assert_eq!(view.sources.len(), 4);
            assert_eq!(view.channels.len(), 5);
            assert_eq!(view.labels.len(), 3);
            assert!(view.sources.iter().chain(&view.channels).chain(&view.labels).all(|label| !label.is_empty()));
            assert_eq!(view.axis, ["0", "1"]);
            assert!(view.data.is_none());
        }
        assert!(s.engine.backend().snapshot_requests.is_empty());
    }
}
fn histogram_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    histogram_open(&mut s);
    s
}
fn histogram_value(s: &UiSession<Recorder>, pixels: u64) -> layer_core::color::histogram::Histogram {
    let mut value = layer_core::color::histogram::Histogram::new(s.engine.document().composition().color);
    if let Some(layer_render::SnapshotRequest::ArtworkStatistics(request))=s.engine.backend().snapshot_requests.last()
        && let layer_core::ArtworkSource::EffectInput(id)|layer_core::ArtworkSource::EffectChannels(id)=request.query.source
        && let Some(effect)=request.query.snapshot.view().effect(id)
        && matches!(effect.program.id.as_ref(),"curves"|"levels") {
        value.domain=if effect.value("domain")==Some(&layer_core::EffectValue::Choice(1)) {
            let stops=match effect.value("hdr_stops") {Some(layer_core::EffectValue::Number(value))=>*value,_=>4.};
            layer_core::color::histogram::HistogramDomain::CurveLog {stops}
        } else {layer_core::color::histogram::HistogramDomain::Encoded};
    }
    value.pixels = pixels;
    value
}
fn histogram_reply(s: &mut UiSession<Recorder>, pixels: u64, now: u64) {
    s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(s, pixels))));
    s.frame(now, now).unwrap();
}

#[test]
fn histogram_language_refresh_reprojects_retained_independent_and_embedded_captions() {
    for embedded in [false, true] {
        let mut s = if embedded { tonal_histogram_session() } else { histogram_session() };
        histogram_reply(&mut s, 7, 150_000_000);
        s.frame(350_000_000, 350_000_000).unwrap();
        histogram_reply(&mut s, 11, 400_000_000);
        let before = if embedded { &s.state.tonal_histogram } else { &s.state.histogram };
        let data = before.data.clone().unwrap();
        let source = before.captured_source.clone();
        let time = before.captured_time;
        let status = before.status.clone();
        let description = before.description.clone();
        let requests = s.engine.backend().snapshot_requests.len();
        let cancels = s.engine.backend().snapshot_cancels;
        let checkpoint = s.engine.checkpoint();
        assert!(s.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let after = if embedded { &s.state.tonal_histogram } else { &s.state.histogram };
        assert_eq!(after.status, s.localization().text(MessageId::RESOURCES_HISTOGRAM_EXACT));
        assert_ne!(after.status, status);
        assert_ne!(after.description, description);
        assert!(std::sync::Arc::ptr_eq(after.data.as_ref().unwrap(), &data));
        assert_eq!(after.captured_source, source);
        assert_eq!(after.captured_time, time);
        assert_eq!(s.engine.backend().snapshot_requests.len(), requests);
        assert_eq!(s.engine.backend().snapshot_cancels, cancels);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
}
fn histogram_control(s: &mut UiSession<Recorder>, action: crate::HistogramAction) {
    s.dispatch(UiAction::Histogram { action }).unwrap();
}

#[test]
fn histogram_retained_captions_reuse_buffers_during_status_only_publication() {
    let buffers = |view: &crate::HistogramView| [view.description.as_ptr() as usize, view.range.as_ptr() as usize,
        view.axis[0].as_ptr() as usize, view.axis[1].as_ptr() as usize,
        view.sources.as_ptr() as usize, view.channels.as_ptr() as usize, view.labels.as_ptr() as usize];
    for embedded in [false, true] {
        let mut s = session(Platform::Gtk);
        s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "film_grain".into() } }).unwrap();
        s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: occurrence_token(s.engine.document().working.occurrence.unwrap()), key: "animate".into(), value: layer_core::EffectValue::Toggle(true) } }).unwrap();
        if embedded {
            s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
            s.reveal_panel(Panel::Properties).unwrap();
            s.frame(100_000_000, 100_000_000).unwrap();
        } else { histogram_open(&mut s); }
        histogram_reply(&mut s, 7, 150_000_000);
        s.frame(350_000_000, 350_000_000).unwrap();
        histogram_reply(&mut s, 11, 400_000_000);
        let view = if embedded { &s.state.tonal_histogram } else { &s.state.histogram };
        let retained = buffers(view);
        let sample = std::sync::Arc::as_ptr(view.data.as_ref().unwrap());
        s.histogram_copy();
        let view = if embedded { &s.state.tonal_histogram } else { &s.state.histogram };
        assert_eq!(buffers(view), retained, "unchanged captured sample, embedded={embedded}");
        s.frame(500_000_000, 500_000_000).unwrap();
        let view = if embedded { &s.state.tonal_histogram } else { &s.state.histogram };
        assert_eq!(view.status, s.localization().text(MessageId::RESOURCES_HISTOGRAM_UPDATING));
        assert_eq!(std::sync::Arc::as_ptr(view.data.as_ref().unwrap()), sample);
        assert_eq!(buffers(view), retained, "status-only publication, embedded={embedded}");
        let checkpoint = s.engine.checkpoint();
        let requests = s.engine.backend().snapshot_requests.len();
        for _ in 0..4 { s.histogram_copy(); }
        let view = if embedded { &s.state.tonal_histogram } else { &s.state.histogram };
        assert_eq!(buffers(view), retained);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.engine.backend().snapshot_requests.len(), requests);
    }
}

#[test]
fn histogram_retained_panel_configurations_publish_only_supported_host_controls() {
    for platform in Platform::ALL {
        let s=session(platform);let workspace=s.state.workspace.clone();
        let serialized=serde_json::to_vec(&workspace).unwrap();
        let restored:WorkspaceState=serde_json::from_slice(&serialized).unwrap();assert_eq!(restored,workspace);
        assert!(workspace.layout.panels.iter().any(|config|config.id==Panel::Histogram));
        for config in &workspace.layout.panels {
            let copy=crate::customization::PanelCopy::new(&s.state,config);
            let view=crate::customization::panel_view(&s.state,config.id,&copy).unwrap();
            let actual:Vec<_>=view.controls.iter().map(|control|control.control).collect();
            let expected=if config.id.available_on(platform) {PanelControl::available(config.id)} else {&[]};
            assert_eq!(actual,expected,"{platform:?} {:?}",config.id);
            if config.id==Panel::Histogram {assert_eq!(actual.is_empty(),!Panel::Histogram.available_on(platform));}
        }
        assert_eq!(serde_json::to_vec(&s.state.workspace).unwrap(),serialized);
    }
}

#[test]
fn histogram_unsupported_host_actions_preserve_state_backend_history_and_workspace() {
    use crate::HistogramAction::*;
    for platform in Platform::ALL.into_iter().filter(|platform|!Panel::Histogram.available_on(*platform)) {
        for contact in [false,true] {
        let mut s=session(platform);if contact {s.pen(event(&s,1,PenPhase::Down,0.5)).unwrap();}
        let state=serde_json::to_vec(&s.state).unwrap();let document=s.engine.document().clone();
        let checkpoint=s.engine.checkpoint();let clipping=s.engine.backend().clipping_previews.clone();
        let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;
        let composites=s.engine.backend().composites;let dabs=s.engine.backend().dabs;
        for action in [Source {index:3},Channel {index:4},Logarithmic {enabled:true},Shadows {enabled:true},Highlights {enabled:true}] {
            s.dispatch(UiAction::Histogram {action}).unwrap();
            assert!(serde_json::to_vec(&s.state).unwrap()==state,"unsupported Histogram action changed UI state on {platform:?}");assert_eq!(s.engine.document(),&document);assert_eq!(s.engine.checkpoint(),checkpoint);
            assert_eq!(s.engine.backend().clipping_previews,clipping);assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert_eq!(s.engine.backend().snapshot_cancels,cancels);
            assert_eq!(s.engine.backend().composites,composites);assert_eq!(s.engine.backend().dabs,dabs);
            assert_eq!(s.pen_contact,contact);
        }
        }
    }
}

#[test]
fn histogram_source_and_selected_coverage_changes_cancel_obsolete_queries() {
    for mutation in 0..3 {
        let mut s = histogram_session();
        histogram_control(&mut s, crate::HistogramAction::Source { index: 3 });
        s.engine.apply_edit(effect_test_selection_edit(s.engine.document(),Some(layer_core::Selection::full()))).unwrap();
        s.frame(200_000_000, 200_000_000).unwrap();
        let before = s.engine.backend().snapshot_cancels;
        s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s, 91))));
        match mutation {
            0 => histogram_control(&mut s, crate::HistogramAction::Source { index: 0 }),
            1 => { s.engine.apply_edit(effect_test_selection_edit(s.engine.document(),Some(layer_core::Selection::polygon(vec![Point {x:10.,y:20.}, Point {x:40.,y:20.},Point{x:40.,y:80.},Point{x:10.,y:80.}]).unwrap()))).unwrap(); }
            _ => { s.engine.apply_edit(effect_test_selection_edit(s.engine.document(),None)).unwrap(); }
        }
        s.frame(300_000_000, 300_000_000).unwrap();
        assert!(s.engine.backend().snapshot_cancels > before);
        assert!(s.state.histogram.data.is_none());
        if mutation == 2 { assert!(s.histogram.settled); }
    }
}

#[test]
fn histogram_document_epoch_and_selected_layer_target_reject_stale_completion() {
    for mutation in 0..2 {
        let mut s = histogram_session();
        histogram_control(&mut s, crate::HistogramAction::Source { index: 1 });
        s.frame(200_000_000, 200_000_000).unwrap();
        let before = s.engine.backend().snapshot_cancels;
        s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s, 91))));
        if mutation == 0 { s.state.document_file.epoch += 1; }
        else {
            let id = s.engine.document().scene().order()[1];
            s.engine.apply_edit(s.engine.document().select_occurrence_edit(id).unwrap()).unwrap();
        }
        s.frame(300_000_000, 300_000_000).unwrap();
        assert!(s.engine.backend().snapshot_cancels > before);
        assert!(s.state.histogram.data.is_none());
    }
}

#[test]
fn histogram_hidden_or_suspended_releases_query_and_retained_data() {
    for suspend in [false, true] {
        let mut s = histogram_session();
        histogram_reply(&mut s, 7, 150_000_000);
        assert_eq!(s.state.histogram.data.as_ref().unwrap().pixels, 7);
        let sample = std::sync::Arc::downgrade(s.state.histogram.data.as_ref().unwrap());
        s.frame(350_000_000, 350_000_000).unwrap();
        let before = s.engine.backend().snapshot_cancels;
        if suspend { s.suspend_renderer().unwrap(); }
        else {
            customize(&mut s, CustomizationAction::CloseExpanded);
            customize(&mut s, CustomizationAction::SetPanelVisible { panel: Panel::Histogram, visible: false });
        }
        s.frame(400_000_000, 400_000_000).unwrap();
        assert!(s.engine.backend().snapshot_cancels > before);
        assert!(s.state.histogram.data.is_none());
        assert!(sample.upgrade().is_none());
        let count = s.engine.backend().snapshot_requests.len();
        s.frame(1_000_000_000, 1_000_000_000).unwrap();
        assert_eq!(s.engine.backend().snapshot_requests.len(), count);
    }
}

#[test]
fn histogram_old_preview_during_effect_draft_stays_updating_until_new_exact_result() {
    let mut s = calibration_session();
    histogram_open(&mut s);
    let layer = occurrence_token(s.engine.document().working.occurrence.unwrap());
    let gesture = |phase| UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Set {
        layer, key: "temperature".into(), value: layer_core::EffectValue::Number(12.),
    }) } };
    s.dispatch(gesture(ContactPhase::Down)).unwrap();
    histogram_reply(&mut s, 7, 200_000_000);
    assert_eq!(s.state.histogram.data.as_ref().unwrap().pixels, 7);
    assert_eq!(s.state.histogram.status, s.localization().text(MessageId::RESOURCES_HISTOGRAM_UPDATING));
    s.dispatch(gesture(ContactPhase::Up)).unwrap();
    s.frame(400_000_000, 400_000_000).unwrap();
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview));
    histogram_reply(&mut s, 11, 450_000_000);
    s.frame(700_000_000, 700_000_000).unwrap();
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if !request.preview));
    histogram_reply(&mut s, 99, 750_000_000);
    assert_eq!(s.state.histogram.data.as_ref().unwrap().pixels, 99);
    assert_eq!(s.state.histogram.status, s.localization().text(MessageId::RESOURCES_HISTOGRAM_EXACT));
}

#[test]
fn histogram_time_only_animation_marks_frozen_results_updating() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "film_grain".into() } }).unwrap();
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: occurrence_token(s.engine.document().working.occurrence.unwrap()), key: "animate".into(), value: layer_core::EffectValue::Toggle(true) } }).unwrap();
    histogram_open(&mut s);
    let checkpoint = s.engine.checkpoint();
    let captured = match s.engine.backend().snapshot_requests.last().unwrap() { layer_render::SnapshotRequest::ArtworkStatistics(request) => request.query.snapshot.context.elapsed, _ => panic!() };
    histogram_reply(&mut s, 7, 1_000_000_000);
    assert!(s.engine.animation_time() > captured);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert_eq!(s.state.histogram.status, s.localization().text(MessageId::RESOURCES_HISTOGRAM_UPDATING));
    assert_eq!(s.state.histogram.data.as_ref().unwrap().pixels, 7);
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview && request.query.snapshot.context.elapsed > captured));
}

#[test]
fn histogram_lower_priority_reply_is_discarded_before_white_balance_or_bounds() {
    for picker in [false, true] {
        let mut s = calibration_session();
        histogram_open(&mut s);
        s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s, 91))));
        let cancels = s.engine.backend().snapshot_cancels;
        if picker {
            arm_calibration(&mut s);
            assert!(s.engine.backend().snapshot_reply.is_none());
            release_calibration(&mut s);
            assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkSample(_))));
            calibration_reply(&mut s, Ok(layer_core::ArtworkSample::Color([0.125, 0.25, 0.5, 1.])));
            assert!(s.eyedropper.calibration.is_none());
        } else {
            invoke(&mut s, CommandId::Crop);
            s.engine.backend_mut().bounds_wait = true;
            s.request_content_bounds(super::image_geometry::ContentUse::FitContent).unwrap();
            assert!(s.engine.backend().snapshot_reply.is_none());
            assert!(s.content_bounds.busy());
        }
        assert!(s.engine.backend().snapshot_cancels > cancels);
        assert!(s.state.histogram.data.is_none());
    }
}

#[test]
fn histogram_display_controls_and_sources_never_change_document_or_history() {
    let mut s = histogram_session();
    let before = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    use crate::HistogramAction::*;
    for action in [Channel {index:4}, Logarithmic {enabled:true}, Shadows {enabled:true}, Highlights {enabled:true}, Source {index:2}, Source {index:0}] {
        histogram_control(&mut s, action);
    }
    assert_eq!(s.engine.document(), &before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert!(!s.engine.can_undo());
    assert!(s.state.histogram.shadows && s.state.histogram.highlights);
}

fn tonal_histogram_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();
    s.frame(100_000_000,100_000_000).unwrap();
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.query.source==layer_core::ArtworkSource::EffectChannels(s.engine.document().working.occurrence.unwrap())));
    s
}

#[test]
fn histogram_embedded_curve_page_selects_input_or_channel_source_and_cancels_old_result() {
    let mut s = tonal_histogram_session();
    let layer = s.engine.document().working.occurrence.unwrap();
    histogram_reply(&mut s,7,150_000_000);
    assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,7);
    assert!(s.state.histogram.data.is_none());
    s.frame(350_000_000,350_000_000).unwrap();
    let cancels = s.engine.backend().snapshot_cancels;
    s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:occurrence_token(layer),page:"red".into()}}).unwrap();
    s.frame(400_000_000,400_000_000).unwrap();
    assert!(s.engine.backend().snapshot_cancels>cancels);
    assert!(s.state.tonal_histogram.data.is_none());
    assert_eq!(s.state.tonal_histogram.channel,1);
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.query.source==layer_core::ArtworkSource::EffectInput(layer)));
    histogram_reply(&mut s,11,450_000_000);
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:occurrence_token(layer),page:"blue".into()}}).unwrap();
    s.frame(500_000_000,500_000_000).unwrap();
    assert!(s.state.tonal_histogram.data.is_none());
    assert_eq!(s.state.tonal_histogram.channel,3);
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.query.source==layer_core::ArtworkSource::EffectInput(layer)));
}

#[test]
fn histogram_embedded_priority_and_hidden_properties_route_reply_to_one_owner() {
    let mut s = histogram_session();
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();
    s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
    let cancels = s.engine.backend().snapshot_cancels;
    s.frame(200_000_000,200_000_000).unwrap();
    assert!(s.engine.backend().snapshot_cancels>cancels);
    assert!(s.state.histogram.data.is_none());
    assert!(s.state.tonal_histogram.data.is_none());
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if matches!(request.query.source,layer_core::ArtworkSource::EffectChannels(_))));
    histogram_reply(&mut s,7,250_000_000);
    assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,7);
    assert!(s.state.histogram.data.is_none());
    s.frame(450_000_000,450_000_000).unwrap();
    let cancels = s.engine.backend().snapshot_cancels;
    customize(&mut s,CustomizationAction::CloseExpanded);
    customize(&mut s,CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:false});
    s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
    s.frame(500_000_000,500_000_000).unwrap();
    assert!(s.engine.backend().snapshot_cancels>cancels);
    assert!(s.state.tonal_histogram.data.is_none());
    assert!(s.state.histogram.data.is_none());
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.query.source==layer_core::ArtworkSource::Visible));
    histogram_reply(&mut s,11,550_000_000);
    assert_eq!(s.state.histogram.data.as_ref().unwrap().pixels,11);
    assert!(s.state.tonal_histogram.data.is_none());
}

#[test]
fn histogram_embedded_focus_changes_no_history_or_query_identity() {
    let mut s = tonal_histogram_session();
    let before = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    let requests = s.engine.backend().snapshot_requests.len();
    let cancels = s.engine.backend().snapshot_cancels;
    let layer = occurrence_token(s.engine.document().working.occurrence.unwrap());
    s.dispatch(UiAction::Effect {action:EffectAction::CurveSelectPoint {layer,key:"rgb".into(),epoch:s.state.layer_properties.epoch,index:Some(0)}}).unwrap();
    s.frame(150_000_000,150_000_000).unwrap();
    assert_eq!(s.engine.document(),&before);
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
    assert_eq!(s.engine.backend().snapshot_cancels,cancels);
    histogram_reply(&mut s,7,200_000_000);
    assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,7);
}

#[test]
fn histogram_embedded_and_independent_owners_yield_to_explicit_picker_or_bounds() {
    for picker in [false,true] {
        let mut s = tonal_histogram_session();
        invoke(&mut s,CommandId::Histogram);
        let cancels = s.engine.backend().snapshot_cancels;
        s.engine.backend_mut().snapshot_reply=Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
        if picker {
            s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"white_balance".into()}}).unwrap();
            arm_calibration(&mut s);
        } else {
            invoke(&mut s,CommandId::Crop);
            s.engine.backend_mut().bounds_wait=true;
            s.request_content_bounds(super::image_geometry::ContentUse::FitContent).unwrap();
        }
        assert!(s.engine.backend().snapshot_cancels>cancels);
        assert!(s.engine.backend().snapshot_reply.is_none());
        let requests=s.engine.backend().snapshot_requests.len();
        s.frame(200_000_000,200_000_000).unwrap();
        assert!(s.state.histogram.data.is_none());
        assert!(s.state.tonal_histogram.data.is_none());
        assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
    }
}

#[test]
fn histogram_embedded_domain_change_discards_old_axis_and_pending_result() {
    for preview in [false,true] {
        let mut s = tonal_histogram_session();
        histogram_reply(&mut s,7,150_000_000);
        if preview {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:occurrence_token(s.engine.document().working.occurrence.unwrap()),key:"red".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.5]])}}).unwrap();
        }
        s.frame(350_000_000,350_000_000).unwrap();
        assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview==preview));
        let cancels=s.engine.backend().snapshot_cancels;
        s.engine.backend_mut().snapshot_reply=Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
        s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:occurrence_token(s.engine.document().working.occurrence.unwrap()),key:"domain".into(),value:layer_core::EffectValue::Choice(1)}}).unwrap();
        s.frame(400_000_000,400_000_000).unwrap();
        assert!(s.engine.backend().snapshot_cancels>cancels);
        assert!(s.state.tonal_histogram.data.is_none());
        assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview && request.query.snapshot.view().effect(request.query.snapshot.view().order()[0]).unwrap().value("domain")==Some(&layer_core::EffectValue::Choice(1))));
    }
}

#[test]
fn histogram_embedded_ignores_edits_excluded_from_its_source() {
    for input in [false,true] {
        let mut s=tonal_histogram_session();
        let layer=occurrence_token(s.engine.document().working.occurrence.unwrap());
        if input {
            s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer,page:"red".into()}}).unwrap();
            s.frame(120_000_000,120_000_000).unwrap();
        }
        histogram_reply(&mut s,7,150_000_000);
        s.frame(350_000_000,350_000_000).unwrap();
        histogram_reply(&mut s,11,400_000_000);
        assert!(s.tonal_histogram.settled);
        let requests=s.engine.backend().snapshot_requests.len();
        let cancels=s.engine.backend().snapshot_cancels;
        for (index,key) in if input {vec!["rgb","red"]} else {vec!["rgb"]}.into_iter().enumerate() {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:key.into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.5]])}}).unwrap();
            let now=500_000_000+index as u64*100_000_000;
            s.frame(now,now).unwrap();
            assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
            assert_eq!(s.engine.backend().snapshot_cancels,cancels);
            assert!(s.tonal_histogram.settled);
            assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,11);
        }
        if !input {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:"red".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.75]])}}).unwrap();
            s.frame(700_000_000,700_000_000).unwrap();
            assert_eq!(s.engine.backend().snapshot_requests.len(),requests+1);
            assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview));
        }
    }
}

#[test]
fn targeted_sampling_retains_settled_tonal_statistics_for_unchanged_input() {
    let mut s=tonal_histogram_session();
    histogram_reply(&mut s,7,150_000_000);
    s.frame(350_000_000,350_000_000).unwrap();
    histogram_reply(&mut s,11,400_000_000);
    let data=s.state.tonal_histogram.data.clone().unwrap();
    let requests=s.engine.backend().snapshot_requests.len();
    let layer=occurrence_token(s.engine.document().working.occurrence.unwrap());
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer,epoch:s.state.layer_properties.epoch}}).unwrap();
    s.frame(500_000_000,500_000_000).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:"rgb".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.5]])}}).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer,epoch:s.state.layer_properties.epoch}}).unwrap();
    s.frame(800_000_000,800_000_000).unwrap();
    assert!(s.tonal_histogram.settled);
    assert!(std::sync::Arc::ptr_eq(&data,s.state.tonal_histogram.data.as_ref().unwrap()));
    assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer,epoch:s.state.layer_properties.epoch}}).unwrap();
    let lower_handle=s.engine.document().scene().order()[1];
    let mut lower=s.engine.document().scene().occurrence(lower_handle).unwrap().clone();lower.opacity=0.5;
    let edit=layer_core::authored::RecordChange::replace(&s.engine.document().artwork.occurrences,lower_handle,Some(lower)).unwrap();
    s.engine.apply_edit(layer_core::Edit::Occurrence(edit)).unwrap();s.refresh_document();
    assert!(s.targeted_curve.is_none());
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer,epoch:s.state.layer_properties.epoch}}).unwrap();
    s.frame(900_000_000,900_000_000).unwrap();
    assert!(!s.tonal_histogram.settled);
    assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer,epoch:s.state.layer_properties.epoch}}).unwrap();
    s.frame(1_100_000_000,1_100_000_000).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(),requests+1);
}

#[test]
fn histogram_settled_tab_hide_and_show_wake_retirement_without_unrelated_layout_wakes() {
    for effect in [None,Some("curves"),Some("levels")] {
    let mut s=session(Platform::Gtk);
    if let Some(effect)=effect {s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:effect.into()}}).unwrap();}
    let embedded=effect.is_some();
    let panel=if embedded {Panel::Properties}else{Panel::Histogram};
    let other=if embedded {Panel::Histogram}else{Panel::Properties};
    s.reveal_panel(Panel::Histogram).unwrap();
    let group=s.state.workspace.layout.panel_group(Panel::Properties).unwrap();
    s.dispatch(UiAction::MovePanel {panel:Panel::Histogram,target:DockTarget::Tab {group,index:None},viewport:[1000.,800.]}).unwrap();
    s.reveal_panel(panel).unwrap();s.frame(100_000_000,100_000_000).unwrap();
    histogram_reply(&mut s,7,150_000_000);s.frame(350_000_000,350_000_000).unwrap();histogram_reply(&mut s,11,400_000_000);
    assert!(if embedded {s.tonal_histogram.settled && s.tonal_histogram.demand}else{s.histogram.settled && s.histogram.demand});
    assert!(!s.engine.has_pending_document_edits());
    let hidden=s.dispatch(UiAction::SelectPanelTab {group,panel:other}).unwrap();
    assert!(hidden.canvas_wake);
    s.frame(450_000_000,450_000_000).unwrap();
    assert!(if embedded {!s.tonal_histogram.demand && s.state.tonal_histogram.data.is_none()}else{!s.histogram.demand && s.state.histogram.data.is_none()});
    let shown=s.dispatch(UiAction::SelectPanelTab {group,panel}).unwrap();assert!(shown.canvas_wake);
    s.frame(500_000_000,500_000_000).unwrap();assert!(if embedded {s.tonal_histogram.demand}else{s.histogram.demand});
    histogram_reply(&mut s,7,550_000_000);s.frame(750_000_000,750_000_000).unwrap();histogram_reply(&mut s,11,800_000_000);
    assert!(if embedded {s.tonal_histogram.settled}else{s.histogram.settled});
    let unchanged=s.dispatch(UiAction::Customize {action:CustomizationAction::SetTabStyle {group,style:TabStyle::ALL[1]}}).unwrap();
    assert!(!unchanged.canvas_wake);
    }
}

#[test]
fn histogram_hidden_after_explicit_query_preemption_releases_retained_data() {
    let mut s=histogram_session();histogram_reply(&mut s,7,150_000_000);
    assert!(s.state.histogram.data.is_some());
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"white_balance".into()}}).unwrap();arm_calibration(&mut s);
    assert!(s.eyedropper.calibration.is_some());
    customize(&mut s,CustomizationAction::CloseExpanded);
    customize(&mut s,CustomizationAction::SetPanelVisible {panel:Panel::Histogram,visible:false});
    s.frame(200_000_000,200_000_000).unwrap();
    assert!(s.state.histogram.data.is_none());assert!(!s.histogram.demand);
}

#[test]
fn histogram_embedded_own_composition_changes_preserve_completed_data_while_lower_changes_resample() {
    for effect in ["curves","levels"] {for page in ["rgb","red"] {for mutation in 0..3 {
        let mut s=session(Platform::Gtk);
        s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:effect.into()}}).unwrap();
        let layer=s.engine.document().working.occurrence.unwrap();
        s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:occurrence_token(layer),page:page.into()}}).unwrap();
        s.reveal_panel(Panel::Properties).unwrap();s.frame(100_000_000,100_000_000).unwrap();
        histogram_reply(&mut s,7,150_000_000);s.frame(350_000_000,350_000_000).unwrap();histogram_reply(&mut s,11,400_000_000);
        assert!(s.tonal_histogram.settled);
        let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;
        for index in [0,1] {
            let handle=s.engine.document().scene().order()[index];
            let edit=match mutation {
                0=>effect_test_occurrence_edit(s.engine.document(),handle,|o|o.opacity=0.5),
                1=>effect_test_mask_edit(s.engine.document(),handle,0.5),
                _=>effect_test_occurrence_edit(s.engine.document(),handle,|o|o.blend=layer_core::LayerBlend::Multiply),
            };
            s.engine.apply_edit(edit).unwrap();s.refresh_document();
            let now=500_000_000+index as u64*100_000_000;s.frame(now,now).unwrap();
            if index==0 {
                assert_eq!(s.engine.backend().snapshot_requests.len(),requests,"{effect} {page} {mutation}");
                assert_eq!(s.engine.backend().snapshot_cancels,cancels);
                assert!(s.tonal_histogram.settled);assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,11);
            } else {
                assert_eq!(s.engine.backend().snapshot_requests.len(),requests+1,"lower source {effect} {page} {mutation}");
                assert!(!s.tonal_histogram.settled);
            }
        }
    }}}
}

fn waveform_reply(s:&mut UiSession<Recorder>,now:u64) {
    let mut value=histogram_value(s,3);let mut waveform=layer_core::color::histogram::Waveform{counts:vec![0;layer_core::color::histogram::Waveform::WORDS]};waveform.counts[12*256+34]=3;value.waveform=Some(waveform);
    s.engine.backend_mut().snapshot_reply=Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(value)));s.frame(now,now).unwrap();
}
#[test]
fn waveform_retained_captions_keep_preferences_and_sample_across_status_and_language_updates() {
    let buffers=|v:&crate::HistogramView|[v.description.as_ptr(),v.range.as_ptr(),v.axis[0].as_ptr(),v.axis[1].as_ptr()];
    let mut s=session(Platform::Gtk);
    s.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"film_grain".into()}}).unwrap();
    s.dispatch(UiAction::Effect{action:EffectAction::Set{layer:occurrence_token(s.engine.document().working.occurrence.unwrap()),key:"animate".into(),value:layer_core::EffectValue::Toggle(true)}}).unwrap();
    s.reveal_panel(Panel::Waveform).unwrap();s.frame(100_000_000,100_000_000).unwrap();waveform_reply(&mut s,150_000_000);
    s.frame(350_000_000,350_000_000).unwrap();waveform_reply(&mut s,400_000_000);
    histogram_control(&mut s,HistogramAction::WaveformChannel{index:4});histogram_control(&mut s,HistogramAction::WaveformLogarithmic{enabled:true});
    let sample=s.state.waveform.data.clone().unwrap();let source=s.state.waveform.captured_source.clone();let time=s.state.waveform.captured_time;
    assert_ne!(s.state.waveform.range,s.state.histogram.range);let retained=buffers(&s.state.waveform);
    s.histogram_copy();assert_eq!(buffers(&s.state.waveform),retained);
    s.frame(500_000_000,500_000_000).unwrap();assert_eq!(s.state.waveform.status,s.localization().text(MessageId::RESOURCES_HISTOGRAM_UPDATING));
    assert_eq!(buffers(&s.state.waveform),retained);assert!(std::sync::Arc::ptr_eq(s.state.waveform.data.as_ref().unwrap(),&sample));
    let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;let checkpoint=s.engine.checkpoint();
    s.set_localization(Localizer::shared(UiLanguage::Japanese));
    assert_eq!((s.state.waveform.channel,s.state.waveform.logarithmic),(4,true));assert_ne!(s.state.waveform.range,s.state.histogram.range);
    assert_eq!(s.state.waveform.captured_source,source);assert_eq!(s.state.waveform.captured_time,time);assert!(std::sync::Arc::ptr_eq(s.state.waveform.data.as_ref().unwrap(),&sample));
    assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert_eq!(s.engine.backend().snapshot_cancels,cancels);assert_eq!(s.engine.checkpoint(),checkpoint);
    let retained=buffers(&s.state.waveform);s.histogram_copy();assert_eq!(buffers(&s.state.waveform),retained);
}

#[test]
fn waveform_and_histogram_share_one_query_but_keep_independent_channel_preferences() {
    let mut s=histogram_session();let count=s.engine.backend().snapshot_requests.len();let cancellations=s.engine.backend().snapshot_cancels;
    s.reveal_panel(Panel::Waveform).unwrap();s.frame(200_000_000,200_000_000).unwrap();
    assert_eq!(s.engine.backend().snapshot_cancels,cancellations+1);assert_eq!(s.engine.backend().snapshot_requests.len(),count+1);
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(r)) if r.waveform));
    waveform_reply(&mut s,210_000_000);assert!(std::sync::Arc::ptr_eq(s.state.histogram.data.as_ref().unwrap(),s.state.waveform.data.as_ref().unwrap()));
    let count=s.engine.backend().snapshot_requests.len();let checkpoint=s.engine.checkpoint();
    histogram_control(&mut s,HistogramAction::Channel{index:1});histogram_control(&mut s,HistogramAction::WaveformChannel{index:4});histogram_control(&mut s,HistogramAction::WaveformLogarithmic{enabled:true});
    assert_eq!((s.state.histogram.channel,s.state.waveform.channel),(1,4));assert!(!s.state.histogram.logarithmic);assert!(s.state.waveform.logarithmic);assert_eq!(s.engine.backend().snapshot_requests.len(),count);assert_eq!(s.engine.checkpoint(),checkpoint);
    customize(&mut s,CustomizationAction::SetPanelVisible{panel:Panel::Waveform,visible:false});s.frame(300_000_000,300_000_000).unwrap();assert!(s.state.waveform.data.is_none());
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(r)) if !r.waveform));
    histogram_reply(&mut s,7,310_000_000);assert!(s.state.histogram.data.is_some());assert!(s.state.waveform.data.is_none());
}

#[test]
fn waveform_source_mutation_and_suspension_retire_shared_data_and_query() {
    let mut s=session(Platform::Gtk);s.reveal_panel(Panel::Waveform).unwrap();s.frame(100_000_000,100_000_000).unwrap();waveform_reply(&mut s,110_000_000);
    assert!(s.state.waveform.data.is_some());histogram_control(&mut s,HistogramAction::Source{index:1});assert!(s.state.waveform.data.is_none());
    s.frame(200_000_000,200_000_000).unwrap();assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(r)) if r.waveform && r.query.source==layer_core::ArtworkSource::Source(s.engine.document().working.target.unwrap())));
    waveform_reply(&mut s,210_000_000);s.suspend_renderer().unwrap();assert!(s.state.waveform.data.is_none());assert!(s.state.histogram.data.is_none());
}

#[test]
fn waveform_dedicated_command_is_localized_and_queries_only_supported_hosts() {
    for platform in Platform::ALL {for language in UiLanguage::ALL {
        let mut s=session(platform);s.set_localization(Localizer::shared(language));
        assert_eq!(CommandId::Waveform.localized_label(s.localization()),s.localization().text(MessageId::COMMAND_WAVEFORM));
        let config=s.state.workspace.layout.panel(Panel::Waveform).unwrap();assert_eq!(crate::customization::PanelCopy::new(&s.state,config).title,s.localization().text(MessageId::RESOURCES_WAVEFORM));
        if Panel::Histogram.available_on(platform) {invoke(&mut s,CommandId::Waveform);s.frame(100_000_000,100_000_000).unwrap();assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(r)) if r.waveform));}
        else {let requests=s.engine.backend().snapshot_requests.len();s.reveal_panel(Panel::Waveform).unwrap();s.frame(100_000_000,100_000_000).unwrap();assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert!(s.state.waveform.data.is_none());assert!(!Panel::Waveform.available_on(platform));}
    }}
}

#[test]
fn waveform_rgba_maps_hdr_endpoints_and_tiny_counts_to_premultiplied_pixels() {
    use layer_core::color::{DocumentColor,SampleDepth,histogram::{Histogram,Waveform}};
    let color=DocumentColor{depth:SampleDepth::F32,..Default::default()};let mut data=Histogram::new(color);let bins=data.plot_bins();
    let mut counts=vec![0;Waveform::WORDS];counts[0*256+7]=1;counts[255*256+9]=1;counts[((bins.start+bins.end)/2)*256+11]=1000;data.waveform=Some(Waveform{counts});
    let mut view=crate::HistogramView::default();view.data=Some(std::sync::Arc::new(data));view.channel=1;view.logarithmic=true;
    let (size,rgba)=view.waveform_premultiplied_rgba([[255,0,0],[0,255,0],[0,0,255],[255,255,255]]).unwrap();assert_eq!(size,[256,bins.len() as u32]);
    let pixel=|x:usize,y:usize|&rgba[(y*256+x)*4..(y*256+x+1)*4];assert!(pixel(7,bins.len()-1)[3]>0);assert!(pixel(9,0)[3]>0);assert_eq!(pixel(11,bins.len()-1-((bins.start+bins.end)/2-bins.start)),[255,0,0,255]);
    for pixel in rgba.chunks_exact(4) {assert!(pixel[..3].iter().all(|c|*c<=pixel[3]));}
    view.data=None;assert!(view.waveform_premultiplied_rgba([[0;3];4]).is_none());
}

#[test]
fn tonal_histogram_axis_uses_captured_domain_instead_of_document_hdr_depth() {
    use layer_core::color::{SampleDepth,histogram::HistogramDomain};
    for id in ["levels","curves"] {
        let original=color_adjustment_session(id);let mut document=original.engine.document().clone();let composition=document.artwork.compositions.get_mut(document.artwork.root).unwrap();composition.color.depth=SampleDepth::F32;composition.blend=layer_core::BlendSpace::Linear;let color=composition.color;let mut s=UiSession::new(Recorder{color,..Default::default()},document,[800,800],Platform::Gtk).unwrap();s.reveal_panel(Panel::Properties).unwrap();s.frame(100_000_000,100_000_000).unwrap();
        histogram_reply(&mut s,7,110_000_000);let data=s.state.tonal_histogram.data.as_ref().unwrap();assert_eq!(data.domain,HistogramDomain::Encoded);assert_eq!(data.axis().bins,[0,256]);assert!(data.axis().stops.is_none());assert!(data.axis().white.is_none());
        if id=="levels" {continue;}
        color_adjustment_set(&mut s,"domain",layer_core::EffectValue::Choice(1));s.frame(200_000_000,200_000_000).unwrap();histogram_reply(&mut s,7,210_000_000);
        let data=s.state.tonal_histogram.data.as_ref().unwrap();assert!(matches!(data.domain,HistogramDomain::CurveLog{..}));assert_eq!(data.axis().bins,[0,256]);
    }
}

#[test]
fn waveform_floating_tab_retires_demand_when_histogram_becomes_active() {
    let mut s=histogram_session();s.reveal_panel(Panel::Waveform).unwrap();s.frame(100_000_000,100_000_000).unwrap();waveform_reply(&mut s,110_000_000);
    let group=s.state.workspace.layout.panel_group(Panel::Histogram).unwrap();
    s.dispatch(UiAction::MovePanel{panel:Panel::Waveform,target:DockTarget::Float{position:[300.,200.]},viewport:[1000.,800.]}).unwrap();
    s.dispatch(UiAction::MovePanel{panel:Panel::Waveform,target:DockTarget::Tab{group,index:None},viewport:[1000.,800.]}).unwrap();
    s.dispatch(UiAction::SelectPanelTab{group,panel:Panel::Histogram}).unwrap();s.frame(200_000_000,200_000_000).unwrap();
    assert!(s.histogram.demand);assert!(s.state.waveform.data.is_none());
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if !request.waveform));
}

#[test]
fn photo_monitors_share_default_tabs_and_remain_accessible_through_window_items() {
    let mut s=session(Platform::Gtk);s.state.workspace.layout=crate::WorkspacePreset::Photographer.layout(Platform::Gtk);let initial=s.state.workspace.layout.clone();
    let group=initial.panel_group(Panel::Histogram).unwrap();assert_eq!(initial.panel_group(Panel::Waveform),Some(group));assert_eq!(initial.active_panel(Panel::Waveform),Some(Panel::Histogram));assert_eq!(s.state.waveform.channel,0);
    for panel in [Panel::Color,Panel::Palettes] {assert!(initial.panel(panel).is_ok());assert!(initial.panel_group(panel).is_none());}
    let items=s.workspace_panel_items(PanelKind::Content);for panel in [Panel::Histogram,Panel::Waveform,Panel::Color,Panel::Palettes] {assert!(items.iter().any(|item|matches!(item.action,Some(UiAction::Customize{action:CustomizationAction::SetPanelVisible{panel:p,..}}) if p==panel)));}
    assert!(!VIEW_MENU.sections.iter().flat_map(|section|section.iter()).any(|id|matches!(id,CommandId::Histogram|CommandId::Waveform)));
    s.dispatch(UiAction::MovePanel{panel:Panel::Waveform,target:DockTarget::Float{position:[280.,180.]},viewport:[1000.,800.]}).unwrap();let customized=s.state.workspace.layout.clone();assert_ne!(customized,initial);
    let mut parked=session(Platform::Gtk);parked.inherit_window_state(&s).unwrap();assert_eq!(parked.state.workspace.layout,customized);parked.reveal_panel(Panel::Waveform).unwrap();assert_eq!(parked.state.workspace.layout,customized);
}

#[test]
fn window_menu_routes_monitor_panels_and_preserves_legacy_histogram_on_other_hosts() {
    for platform in Platform::ALL {
        let s=session(platform);let menu=s.application_menu(ApplicationMenu::Window);let items=menu.sections.iter().flatten().collect::<Vec<_>>();
        if Panel::Histogram.available_on(platform) {
            for panel in [Panel::Histogram,Panel::Waveform] {assert!(items.iter().any(|item|matches!(item.action,Some(UiAction::Customize{action:CustomizationAction::SetPanelVisible{panel:p,..}}) if p==panel)));}
        } else {
            assert!(items.iter().any(|item|matches!(item.action,Some(UiAction::Invoke{command:CommandId::Histogram}))));
            assert!(!items.iter().any(|item|matches!(item.action,Some(UiAction::Customize{action:CustomizationAction::SetPanelVisible{panel:Panel::Histogram|Panel::Waveform,..}}))));
        }
    }
}

#[test]
fn statistics_publication_identity_includes_controls_and_every_visible_caption() {
    let mut view=crate::HistogramView::default();view.data=Some(std::sync::Arc::new(histogram_value(&histogram_session(),3)));view.captured_source=Some(layer_core::ArtworkSource::Visible);view.captured_time=Some(1.);
    assert!(view.same_publication(&view.clone()));
    let mut copied=view.clone();copied.data=Some(std::sync::Arc::new((**view.data.as_ref().unwrap()).clone()));assert!(!view.same_publication(&copied));
    for field in 0..15 {let mut changed=view.clone();match field {
        0=>changed.source=1,1=>changed.channel=4,2=>changed.logarithmic=true,3=>changed.shadows=true,4=>changed.highlights=true,
        5=>changed.captured_time=Some(2.),6=>changed.captured_source=Some(layer_core::ArtworkSource::Reference),7=>changed.status="Updating".into(),
        8=>changed.description="count".into(),9=>changed.range="range".into(),10=>changed.axis[0]="low".into(),11=>changed.axis[1]="high".into(),
        12=>changed.sources.push("source".into()),13=>changed.channels.push("channel".into()),_=>changed.labels.push("label".into())}
        assert!(!view.same_publication(&changed),"field {field}");}
}

#[test]
fn histogram_plot_uses_selected_channels_shared_peak_and_log_counts() {
    use layer_core::color::{DocumentColor,histogram::Histogram};
    let mut data=Histogram::new(DocumentColor::default());data.channels[0].bins[10]=1;data.channels[1].bins[20]=4;data.channels[2].bins[30]=2;data.channels[3].bins[40]=100;
    let mut view=crate::HistogramView::default();assert!(view.histogram_plot().is_empty());view.data=Some(std::sync::Arc::new(data));
    let plot=view.histogram_plot();assert_eq!(plot.iter().map(|p|p.0).collect::<Vec<_>>(),[0,1,2]);assert_eq!(plot[0].1[10],0.25);assert_eq!(plot[1].1[20],1.);assert_eq!(plot[2].1[30],0.5);assert_eq!(plot[0].1[11],0.);
    view.logarithmic=true;let plot=view.histogram_plot();assert!((f64::from(plot[0].1[10])-2_f64.ln()/5_f64.ln()).abs()<1e-7);
    view.channel=4;let plot=view.histogram_plot();assert_eq!(plot.len(),1);assert_eq!(plot[0].0,3);assert_eq!(plot[0].1[40],1.);
    let mut hdr=Histogram::new(DocumentColor{depth:layer_core::color::SampleDepth::F32,..Default::default()});let visible=hdr.plot_bins();assert!(visible.start>0&&visible.end<256);hdr.channels[0].bins[0]=10000;hdr.channels[0].bins[visible.start]=1;hdr.channels[0].bins[visible.end-1]=2;let admitted=hdr.plot_bins();
    view.data=Some(std::sync::Arc::new(hdr));view.channel=1;view.logarithmic=false;let plot=view.histogram_plot();assert_eq!(plot[0].1.len(),admitted.len());assert_eq!(plot[0].1[visible.start-admitted.start],0.5);assert_eq!(plot[0].1[visible.end-1-admitted.start],1.);

}

#[test]
fn waveform_straight_rgba_preserves_palette_chroma_and_transparent_background() {
    use layer_core::color::{DocumentColor,histogram::{Histogram,Waveform}};
    let mut data=Histogram::new(DocumentColor::default());let mut counts=vec![0;Waveform::WORDS];counts[20*256+7]=1;counts[20*256+9]=4;data.waveform=Some(Waveform{counts});
    let mut view=crate::HistogramView::default();view.data=Some(std::sync::Arc::new(data));view.channel=1;
    let colors=[[200,80,40],[0,255,0],[0,0,255],[255;3]];let (size,straight)=view.waveform_rgba(colors).unwrap();let (premul_size,premul)=view.waveform_premultiplied_rgba(colors).unwrap();assert_eq!(size,premul_size);
    let offset=((size[1] as usize-1-20)*256+7)*4;assert_eq!(&straight[offset..offset+4],&[200,80,40,64]);assert_eq!(&premul[offset..offset+4],&[50,20,10,64]);assert_eq!(&straight[0..4],&[0;4]);
    let (_,changed)=view.waveform_rgba([[40,160,240],colors[1],colors[2],colors[3]]).unwrap();assert_eq!(&changed[offset..offset+4],&[40,160,240,64]);
    view.logarithmic=true;let (_,log)=view.waveform_rgba(colors).unwrap();assert_eq!(&log[offset..offset+3],&colors[0]);assert!(log[offset+3]>straight[offset+3]);
}

#[test]
fn reopened_photo_content_panels_remain_usable_in_narrow_viewports() {
    for platform in [Platform::Gtk,Platform::Web,Platform::Android,Platform::Mac,Platform::Ios] {
        let mut s=session(platform);s.set_viewport([640.,800.],[640,800]).unwrap();
        s.state.workspace.layout=crate::WorkspacePreset::Photographer.layout(platform);
        let document=s.engine.document().clone();
        let group=s.state.workspace.layout.panel_group(Panel::Histogram).unwrap();
        s.dispatch(UiAction::MoveGroup{group,target:DockTarget::Float{position:[180.,160.]},viewport:[640.,800.]}).unwrap();
        for panel in [Panel::Histogram,Panel::Waveform] {customize(&mut s,CustomizationAction::SetPanelVisible{panel,visible:false});}
        for (panel,minimum) in [(Panel::Histogram,Panel::Histogram.default_width()),(Panel::Waveform,Panel::Waveform.default_width()),(Panel::Navigator,192.)] {
            customize(&mut s,CustomizationAction::SetPanelVisible{panel,visible:false});
            customize(&mut s,CustomizationAction::SetPanelVisible{panel,visible:true});
            let resolved=s.state.workspace.layout.resolved([640.,800.]);
            let bounds=resolved.groups.iter().find(|g|g.panels.contains(&panel)).unwrap().bounds;
            assert!(bounds.width>=minimum,"{platform:?} {panel:?}: {bounds:?}");
            assert!(bounds.x>=0.&&bounds.x+bounds.width<=640.);
            assert!(bounds.y>=0.&&bounds.y+bounds.height<=800.);
            assert_eq!(s.state.workspace.layout.active_panel(panel),Some(panel));
        }
        s.dispatch(UiAction::MovePanel{panel:Panel::Histogram,target:DockTarget::Float{position:[100.,120.]},viewport:[640.,800.]}).unwrap();
        let before=s.state.workspace.layout.clone();
        customize(&mut s,CustomizationAction::SetPanelVisible{panel:Panel::Histogram,visible:true});
        assert_eq!(s.state.workspace.layout,before);
        assert_eq!(s.engine.document(),&document);
    }
}

#[test]
fn histogram_captured_source_serializes_compact_public_identity_without_records(){
    use layer_core::{ArtworkSource as S,authored::{SourceTarget,PaintHandle,CoverageHandle,SelectionHandle}};
    let app=tonal_histogram_session();let occurrence=app.engine.document().working.occurrence.unwrap();
    let token=u64::from(occurrence.index())+1;
    let baseline=effects::effect_baseline(app.engine.document(),occurrence).unwrap();
    let mut cases=vec![(None,serde_json::Value::Null),(Some(S::Visible),serde_json::json!("Visible")),(Some(S::Reference),serde_json::json!("Reference")),
        (Some(S::EffectInput(occurrence)),serde_json::json!({"EffectInput":token})),
        (Some(S::EffectChannels(occurrence)),serde_json::json!({"EffectChannels":token})),
        (Some(S::EffectBaseline(baseline)),serde_json::json!({"EffectBaseline":token}))];
    for target in [SourceTarget::Paint(PaintHandle::from_index(3)),SourceTarget::Coverage(CoverageHandle::from_index(4)),SourceTarget::Selection(SelectionHandle::from_index(5))]{
        cases.push((Some(S::Source(target)),serde_json::json!({"Source":target})));
    }
    for (source,expected) in cases{
        let mut view=app.state.histogram.clone();view.captured_source=source;
        let published=serde_json::to_value(&view).unwrap();
        assert_eq!(published.get("captured_source"),Some(&expected));
        let compact=serde_json::to_string(&published["captured_source"]).unwrap();
        assert!(compact.len()<80);assert!(!compact.contains("application"));assert!(!compact.contains("values"));
    }
}
