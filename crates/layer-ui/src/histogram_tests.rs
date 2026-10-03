fn histogram_open(s: &mut UiSession<Recorder>) {
    invoke(s, CommandId::Histogram);
    s.frame(100_000_000, 100_000_000).unwrap();
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview));
}
fn histogram_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    histogram_open(&mut s);
    s
}
fn histogram_value(s: &UiSession<Recorder>, pixels: u64) -> layer_core::color::histogram::Histogram {
    let mut value = layer_core::color::histogram::Histogram::new(s.engine.document().color);
    if let Some(layer_render::SnapshotRequest::ArtworkStatistics(request))=s.engine.backend().snapshot_requests.last()
        && let layer_core::ArtworkSource::EffectInput(id)|layer_core::ArtworkSource::EffectChannels(id)=request.query.source
        && let Some(effect)=request.query.document.layer(id).and_then(|layer|layer.effect.as_ref())
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
fn histogram_control(s: &mut UiSession<Recorder>, action: crate::HistogramAction) {
    s.dispatch(UiAction::Histogram { action }).unwrap();
}

#[test]
fn histogram_source_and_selected_coverage_changes_cancel_obsolete_queries() {
    for mutation in 0..3 {
        let mut s = histogram_session();
        histogram_control(&mut s, crate::HistogramAction::Source { index: 3 });
        s.engine.apply_edit(layer_core::Edit::SetSelection(Some(layer_core::Selection::full()))).unwrap();
        s.frame(200_000_000, 200_000_000).unwrap();
        let before = s.engine.backend().snapshot_cancels;
        s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s, 91))));
        match mutation {
            0 => histogram_control(&mut s, crate::HistogramAction::Source { index: 0 }),
            1 => { s.engine.apply_edit(layer_core::Edit::SetSelection(Some(layer_core::Selection::polygon(vec![Point {x:10.,y:20.}, Point {x:40.,y:20.},Point{x:40.,y:80.},Point{x:10.,y:80.}]).unwrap()))).unwrap(); }
            _ => { s.engine.apply_edit(layer_core::Edit::SetSelection(None)).unwrap(); }
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
            let id = s.engine.document().layers[1].id;
            s.engine.apply_edit(layer_core::Edit::SetActiveLayer { id }).unwrap();
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
        let count = s.engine.backend().snapshot_requests.len();
        s.frame(1_000_000_000, 1_000_000_000).unwrap();
        assert_eq!(s.engine.backend().snapshot_requests.len(), count);
    }
}

#[test]
fn histogram_old_preview_during_effect_draft_stays_updating_until_new_exact_result() {
    let mut s = calibration_session();
    histogram_open(&mut s);
    let layer = s.engine.document().active_layer.0;
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
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: s.engine.document().active_layer.0, key: "animate".into(), value: layer_core::EffectValue::Toggle(true) } }).unwrap();
    histogram_open(&mut s);
    let checkpoint = s.engine.checkpoint();
    let captured = match s.engine.backend().snapshot_requests.last().unwrap() { layer_render::SnapshotRequest::ArtworkStatistics(request) => request.query.time, _ => panic!() };
    histogram_reply(&mut s, 7, 1_000_000_000);
    assert!(s.engine.animation_time() > captured);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert_eq!(s.state.histogram.status, s.localization().text(MessageId::RESOURCES_HISTOGRAM_UPDATING));
    assert_eq!(s.state.histogram.data.as_ref().unwrap().pixels, 7);
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview && request.query.time > captured));
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
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.query.source==layer_core::ArtworkSource::EffectChannels(s.engine.document().active_layer)));
    s
}

#[test]
fn histogram_embedded_curve_page_selects_input_or_channel_source_and_cancels_old_result() {
    let mut s = tonal_histogram_session();
    let layer = s.engine.document().active_layer;
    histogram_reply(&mut s,7,150_000_000);
    assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,7);
    assert!(s.state.histogram.data.is_none());
    s.frame(350_000_000,350_000_000).unwrap();
    let cancels = s.engine.backend().snapshot_cancels;
    s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:"red".into()}}).unwrap();
    s.frame(400_000_000,400_000_000).unwrap();
    assert!(s.engine.backend().snapshot_cancels>cancels);
    assert!(s.state.tonal_histogram.data.is_none());
    assert_eq!(s.state.tonal_histogram.channel,1);
    assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.query.source==layer_core::ArtworkSource::EffectInput(layer)));
    histogram_reply(&mut s,11,450_000_000);
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:"blue".into()}}).unwrap();
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
    let layer = s.engine.document().active_layer.0;
    s.dispatch(UiAction::Effect {action:EffectAction::CurveSelectPoint {layer,key:"curve_0".into(),epoch:s.state.layer_properties.epoch,index:Some(0)}}).unwrap();
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
        s.frame(200_000_000,200_000_000).unwrap();
        assert!(s.state.histogram.data.is_none());
        assert!(s.state.tonal_histogram.data.is_none());
        assert!(!s.histogram.demand && !s.tonal_histogram.demand);
    }
}

#[test]
fn histogram_embedded_domain_change_discards_old_axis_and_pending_result() {
    for preview in [false,true] {
        let mut s = tonal_histogram_session();
        histogram_reply(&mut s,7,150_000_000);
        if preview {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:s.engine.document().active_layer.0,key:"curve_1".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.5]])}}).unwrap();
        }
        s.frame(350_000_000,350_000_000).unwrap();
        assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview==preview));
        let cancels=s.engine.backend().snapshot_cancels;
        s.engine.backend_mut().snapshot_reply=Some(Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram_value(&s,91))));
        s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:s.engine.document().active_layer.0,key:"domain".into(),value:layer_core::EffectValue::Choice(1)}}).unwrap();
        s.frame(400_000_000,400_000_000).unwrap();
        assert!(s.engine.backend().snapshot_cancels>cancels);
        assert!(s.state.tonal_histogram.data.is_none());
        assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview && request.query.document.layers[0].effect.as_ref().unwrap().value("domain")==Some(&layer_core::EffectValue::Choice(1))));
    }
}

#[test]
fn histogram_embedded_ignores_edits_excluded_from_its_source() {
    for input in [false,true] {
        let mut s=tonal_histogram_session();
        let layer=s.engine.document().active_layer.0;
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
        for (index,key) in if input {vec!["curve_0","curve_1"]} else {vec!["curve_0"]}.into_iter().enumerate() {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:key.into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.5]])}}).unwrap();
            let now=500_000_000+index as u64*100_000_000;
            s.frame(now,now).unwrap();
            assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
            assert_eq!(s.engine.backend().snapshot_cancels,cancels);
            assert!(s.tonal_histogram.settled);
            assert_eq!(s.state.tonal_histogram.data.as_ref().unwrap().pixels,11);
        }
        if !input {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:"curve_1".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[1.,0.75]])}}).unwrap();
            s.frame(700_000_000,700_000_000).unwrap();
            assert_eq!(s.engine.backend().snapshot_requests.len(),requests+1);
            assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(request)) if request.preview));
        }
    }
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
        let layer=s.engine.document().active_layer;
        s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:page.into()}}).unwrap();
        s.reveal_panel(Panel::Properties).unwrap();s.frame(100_000_000,100_000_000).unwrap();
        histogram_reply(&mut s,7,150_000_000);s.frame(350_000_000,350_000_000).unwrap();histogram_reply(&mut s,11,400_000_000);
        assert!(s.tonal_histogram.settled);
        let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;
        for index in [0,1] {
            let mut changed=s.engine.document().layers[index].clone();
            match mutation {
                0=>changed.opacity=0.5,
                1=>{let mut mask=layer_core::LayerMask::reveal_all(s.engine.allocate_layer_id(),layer_core::Point::default());mask.default_coverage=0.5;changed.mask=Some(mask);},
                _=>changed.properties.blend=layer_core::LayerBlend::Multiply,
            }
            s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(changed))).unwrap();s.refresh_document();
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
