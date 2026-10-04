fn targeted_session()->UiSession<Recorder> {targeted_session_on(Platform::Gtk)}
fn targeted_session_on(platform:Platform)->UiSession<Recorder> {
    let mut s=session(platform);
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();s.frame(1,1).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer:s.engine.document().active_layer.0,epoch:s.state.layer_properties.epoch}}).unwrap();s
}
fn targeted_pointer(s:&mut UiSession<Recorder>,phase:ContactPhase,y:f32) {
    s.input(pointer_input(17,phase,PointerKind::Pen,PointerButton::Primary,[400.,y],0)).unwrap();
}
fn targeted_reply(s:&mut UiSession<Recorder>) {
    s.engine.backend_mut().snapshot_reply=Some(Ok(layer_render::SnapshotResult::ArtworkSample(layer_core::ArtworkSample::Color([0.25,0.25,0.25,1.]))));
    s.frame(30,30).unwrap();
}
fn targeted_points(s:&UiSession<Recorder>,page:usize)->Vec<[f32;2]> {
    match s.engine.document().layers[0].effect.as_ref().unwrap().value(&format!("curve_{page}")) {Some(layer_core::EffectValue::Curve(points))=>points.clone(),_=>panic!("missing curve")}
}

#[test]
fn targeted_curve_publishes_only_fixed_sample_size_and_restores_picker_preference() {
    let mut s=targeted_session();invoke(&mut s,CommandId::Move);
    s.dispatch(UiAction::SetColorSampleSize {width:15}).unwrap();
    s.eyedropper.picking.position=Some([320.,240.]);
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer:s.engine.document().active_layer.0,epoch:s.state.layer_properties.epoch}}).unwrap();
    assert!(s.color_picker_overlay().is_none());
    assert_eq!(s.state.color_picker.sample_width,5);assert_eq!(s.state.color_picker.sample_sizes,&[5]);
    let options=s.state.tool_options();
    assert!(!options.iter().any(|o|matches!(o,ToolOption::Choice {id:"sample-size",..})));
    s.dispatch(UiAction::SetColorSampleSize {width:101}).unwrap();
    s.dispatch(UiAction::ColorPicker {action:ColorPickerAction::Source {layer:true}}).unwrap();
    assert_eq!(s.state.color_picker.sample_width,5);
    targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
    let layer_render::SnapshotRequest::ArtworkSample(request)=s.engine.backend().snapshot_requests.last().unwrap() else {panic!("missing sample")};
    assert_eq!(request.width,5);
    invoke(&mut s,CommandId::Move);assert_eq!(s.state.color_picker.sample_width,15);
}

#[test]
fn targeted_curve_pending_release_commits_one_undo_with_fixed_x_and_logical_screen_delta() {
    for platform in [Platform::Gtk,Platform::Web] {for scale in [1.,2.] {for zoom in [0.5,3.] {
        let mut s=targeted_session_on(platform);s.state.camera.zoom=zoom;s.state.camera.rotation=0.7;
        s.logical_viewport=Some([s.state.camera.viewport[0] as f32/scale,s.state.camera.viewport[1] as f32/scale]);
        let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
        targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
        let query=match s.engine.backend().snapshot_requests.last().unwrap() {layer_render::SnapshotRequest::ArtworkSample(query)=>query,_=>panic!("wrong targeted request")};
        assert_eq!(query.source,layer_core::ArtworkSource::EffectInput(s.engine.document().active_layer));
        let mapped=s.state.camera.input_transform().map(layer_core::Point {x:400.,y:400.});assert_eq!(query.position,[mapped.x,mapped.y]);
        s.state.camera.zoom*=2.;s.state.camera.rotation-=0.4;
        s.logical_viewport=Some([s.state.camera.viewport[0] as f32/(scale*0.5),s.state.camera.viewport[1] as f32/(scale*0.5)]);
        targeted_pointer(&mut s,ContactPhase::Move,400.-20.*scale);targeted_pointer(&mut s,ContactPhase::Up,400.-51.*scale);
        assert_eq!(s.engine.document().layers,before);assert!(s.require_raster_snapshot().is_err());
        assert!(s.request_save(false).is_err());assert!(s.files.pending.is_none());targeted_reply(&mut s);
        let points=targeted_points(&s,0);assert_eq!(points.len(),3);
        let x=s.engine.document().color.space.encode(0.25) as f32;
        assert_eq!(points[1][0],x.min(1.));
        assert!((points[1][1]-(x+0.2).min(1.)).abs()<2e-6);
        assert_eq!(s.engine.backend().dabs,0);assert!(s.targeted_curve.is_some());assert!(!s.targeted_curve_busy());
        assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }}}
}

#[test]
fn targeted_curve_continuous_moves_reuse_one_sample_and_cancel_rolls_back() {
    for cancel in [0,1,2] {
        let mut s=targeted_session();let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
        targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();targeted_reply(&mut s);
        let requests=s.engine.backend().snapshot_requests.len();
        for y in [390.,380.,370.] {targeted_pointer(&mut s,ContactPhase::Move,y);s.frame(40,40).unwrap();}
        assert_eq!(s.engine.backend().snapshot_requests.len(),requests);
        match cancel {0=>targeted_pointer(&mut s,ContactPhase::Cancel,370.),1=>{key(&mut s,"Escape",true,false,false);},_=>{s.input(UiInput::Blur).unwrap();}}
        assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);assert!(!s.targeted_curve_busy());
    }
}

#[test]
fn targeted_curve_page_change_cancels_contact_stays_armed_and_requested_tool_is_preserved() {
    let mut s=targeted_session();let before=s.engine.document().layers.clone();
    targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
    let cancels=s.engine.backend().snapshot_cancels;
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:s.engine.document().active_layer.0,page:"red".into()}}).unwrap();
    assert!(s.targeted_curve.is_some());assert!(!s.targeted_curve_busy());assert!(s.engine.backend().snapshot_cancels>cancels);
    assert_eq!(s.engine.document().layers,before);
    targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(3,3).unwrap();targeted_pointer(&mut s,ContactPhase::Up,380.);targeted_reply(&mut s);
    assert_eq!(targeted_points(&s,0),vec![[0.,0.],[1.,1.]]);assert_ne!(targeted_points(&s,1),vec![[0.,0.],[1.,1.]]);
    invoke(&mut s,CommandId::Move);assert!(s.targeted_curve.is_none());assert_eq!(s.layer_interaction.tool,LayerCanvasTool::Move);
}

#[test]
fn targeted_curve_source_mutation_discards_pending_sample_without_editing_curve() {
    let mut s=targeted_session();targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
    let mut lower=s.engine.document().layers.last().unwrap().clone();lower.opacity=0.5;
    s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(lower))).unwrap();
    let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();let cancels=s.engine.backend().snapshot_cancels;
    targeted_pointer(&mut s,ContactPhase::Up,370.);targeted_reply(&mut s);
    assert!(s.targeted_curve.is_none());assert!(s.engine.backend().snapshot_cancels>cancels);
    assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
}

#[test]
fn targeted_curve_animated_time_advance_preserves_contact_sample_and_commits_once() {
    let mut s=session(Platform::Gtk);
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"film_grain".into()}}).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:s.engine.document().active_layer.0,key:"animate".into(),value:layer_core::EffectValue::Toggle(true)}}).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();s.frame(1,1).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::TargetCurve {layer:s.engine.document().active_layer.0,epoch:s.state.layer_properties.epoch}}).unwrap();
    let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
    targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
    let captured=match s.engine.backend().snapshot_requests.last().unwrap() {layer_render::SnapshotRequest::ArtworkSample(query)=>query.time,_=>panic!("wrong targeted query")};
    let requests=s.engine.backend().snapshot_requests.len();
    s.frame(5_000_000_000,5_000_000_000).unwrap();assert!(s.engine.animation_time()>captured);
    targeted_pointer(&mut s,ContactPhase::Up,370.);targeted_reply(&mut s);
    assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert!(!s.targeted_curve_busy());assert_ne!(s.engine.document().layers,before);
    assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
}

#[test]
fn targeted_curve_stale_toolbar_action_preserves_current_pending_contact() {
    let mut s=targeted_session();targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
    let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;
    let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
    for action in stale_property_actions(s.engine.document().active_layer.0,s.state.layer_properties.epoch.wrapping_sub(1)) {
        s.dispatch(UiAction::Effect {action}).unwrap();
        assert!(s.targeted_curve.is_some() && s.targeted_curve_busy());
        assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert_eq!(s.engine.backend().snapshot_cancels,cancels);
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn targeted_curve_retired_failure_reprojects_typed_notice_without_resampling_or_editing() {
    for (sample,message) in [
        (Err(layer_render::BackendError("literal İı { $tone } 🖌")),None),
        (Ok(layer_core::ArtworkSample::Empty),Some(MessageId::RESOURCES_PICKER_EMPTY)),
        (Ok(layer_core::ArtworkSample::Outside),Some(MessageId::RESOURCES_PICKER_EMPTY)),
        (Ok(layer_core::ArtworkSample::Color([f32::NAN,0.25,0.5,1.])),Some(MessageId::RESOURCES_CALIBRATION_FAILED)),
    ] {
        let mut s=targeted_session();targeted_pointer(&mut s,ContactPhase::Down,400.);s.frame(2,2).unwrap();
        targeted_pointer(&mut s,ContactPhase::Up,380.);
        let document=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
        let colors=s.state.colors.clone();let tool=s.layer_interaction.tool;
        s.engine.backend_mut().snapshot_reply=Some(sample.map(layer_render::SnapshotResult::ArtworkSample));s.frame(30,30).unwrap();
        assert!(s.targeted_curve.is_some());assert!(!s.targeted_curve_busy());assert!(s.effect_gesture.is_none());
        let notice=s.state.notice.clone().unwrap();
        let rendering=(s.engine.backend().snapshot_requests.len(),s.engine.backend().snapshot_cancels,s.engine.backend().dabs);
        for language in UiLanguage::ALL.iter().copied().chain([UiLanguage::English]) {
            s.set_localization(Localizer::shared(language));
            let current=s.state.notice.as_ref().unwrap();assert_eq!(current.id,notice.id);assert_eq!(current.action,notice.action);
            assert_eq!(current.text,message.map_or_else(||notice.text.clone(),|id|s.localization().text(id).to_string()),"{}",language.tag());
            assert!(s.targeted_curve.is_some());assert!(!s.targeted_curve_busy());assert!(s.effect_gesture.is_none());
            assert_eq!(s.engine.document(),&document);assert_eq!(s.engine.checkpoint(),checkpoint);assert_eq!(s.state.colors,colors);
            assert_eq!(s.layer_interaction.tool,tool);
            assert_eq!((s.engine.backend().snapshot_requests.len(),s.engine.backend().snapshot_cancels,s.engine.backend().dabs),rendering);
        }
    }
}
