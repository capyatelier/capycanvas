fn levels_session()->UiSession<Recorder> {
    let mut s=session(Platform::Gtk);
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"levels".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();s.frame(1,1).unwrap();s
}
fn levels_auto(s:&mut UiSession<Recorder>) {
    s.dispatch(UiAction::Effect {action:EffectAction::AutoLevels {layer:s.engine.document().active_layer.0,epoch:s.state.layer_properties.epoch}}).unwrap();
    s.frame(10,10).unwrap();
}
fn levels_result()->layer_core::levels::LevelsStatistics {
    let mut bins=std::array::from_fn(|_|vec![0;4096]);
    for channel in &mut bins {channel[1024]=500;channel[3072]=500;}
    layer_core::levels::LevelsStatistics {minimum:[0.;3],maximum:[1.;3],bins,pixels:1000}
}
fn levels_reply(s:&mut UiSession<Recorder>,result:Result<layer_render::SnapshotResult,layer_render::BackendError>) {
    s.engine.backend_mut().snapshot_reply=Some(result);s.frame(20,20).unwrap();
}
fn stale_property_actions(layer:u64,epoch:u64)->Vec<EffectAction> {
    vec![
        EffectAction::AutoLevels {layer,epoch},EffectAction::TargetCurve {layer,epoch},
        EffectAction::Calibrate {layer,epoch,role:layer_core::levels::CalibrationRole::Gray},
        EffectAction::CurveSelectPoint {layer,epoch,key:"curve_0".into(),index:Some(0)},
        EffectAction::CurveRemoveAt {layer,epoch,key:"curve_0".into(),point:[0.;2],extent:[255.;2],point_count:Some(2)},
        EffectAction::CurveContact {layer,epoch,key:"curve_0".into(),phase:ContactPhase::Down,point:[100.;2],extent:[255.;2]},
        EffectAction::CurveKey {layer,epoch,key:"curve_0".into(),key_event:"ArrowUp".into(),pressed:true,repeat:false,modifiers:Modifiers::default()},
        EffectAction::CurveNumber {layer,epoch,key:"curve_0".into(),axis:CurveAxis::Output,operation:NumericOperation::Step {steps:1.}},
        EffectAction::Gesture {phase:ContactPhase::Down,action:Box::new(EffectAction::CurveContact {layer,epoch,key:"curve_0".into(),phase:ContactPhase::Down,point:[100.;2],extent:[255.;2]})},
    ]
}

#[test]
fn levels_auto_uses_current_page_source_and_commits_one_undo_preserving_other_values() {
    for page in ["rgb","red","green","blue"] {
        let mut s=levels_session();let layer=s.engine.document().active_layer;
        for (key,value) in [("output_black",0.1),("output_white",0.9),("red_output_white",0.8),("green_gamma",1.7)] {
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:layer.0,key:key.into(),value:layer_core::EffectValue::Number(value)}}).unwrap();
        }
        s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:page.into()}}).unwrap();
        let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
        let paints=s.state.display_colors().definition();levels_auto(&mut s);
        let layer_render::SnapshotRequest::LevelsStatistics(query)=s.engine.backend().snapshot_requests.last().unwrap() else {panic!("wrong Auto request")};
        assert_eq!(query.source,if page=="rgb" {layer_core::ArtworkSource::EffectChannels(layer)}else{layer_core::ArtworkSource::EffectInput(layer)});
        assert_eq!(s.engine.document().layers,before);
        let index=["rgb","red","green","blue"].iter().position(|p|*p==page).unwrap();
        let expected=layer_core::levels::auto_levels(before[0].effect.as_ref().unwrap(),&levels_result(),index as u8).unwrap();
        levels_reply(&mut s,Ok(layer_render::SnapshotResult::LevelsStatistics(levels_result())));
        assert!(s.auto_levels.is_none());assert_eq!(s.engine.document().layers[0].effect.as_deref(),Some(&expected));
        assert_eq!(s.state.display_colors().definition(),paints);assert_eq!(s.engine.backend().dabs,0);
        assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn levels_auto_invalid_or_wrong_results_preserve_complete_effect_and_history() {
    for result in [Err(layer_render::BackendError("failed".into())),Ok(layer_render::SnapshotResult::ArtworkSample(layer_core::ArtworkSample::Empty)),Ok(layer_render::SnapshotResult::LevelsStatistics(layer_core::levels::LevelsStatistics {minimum:[0.;3],maximum:[0.;3],bins:std::array::from_fn(|_|{let mut bins=vec![0;4096];bins[0]=1000;bins}),pixels:1000}))] {
        let mut s=levels_session();let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();levels_auto(&mut s);
        levels_reply(&mut s,result);assert!(s.auto_levels.is_none());assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn levels_auto_stale_page_document_effect_and_hidden_properties_cancel_pending_result() {
    for mutation in 0..4 {
        let mut s=levels_session();levels_auto(&mut s);let cancels=s.engine.backend().snapshot_cancels;
        match mutation {
            0=>{s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:s.engine.document().active_layer.0,page:"red".into()}}).unwrap();},
            1=>{s.state.document_file.epoch+=1;},
            2=>{s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:s.engine.document().active_layer.0,key:"gamma".into(),value:layer_core::EffectValue::Number(1.7)}}).unwrap();},
            _=>{customize(&mut s,CustomizationAction::CloseExpanded);customize(&mut s,CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:false});},
        };
        let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
        levels_reply(&mut s,Ok(layer_render::SnapshotResult::LevelsStatistics(levels_result())));
        assert!(s.auto_levels.is_none());assert!(s.engine.backend().snapshot_cancels>cancels);
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn levels_auto_cancels_histogram_and_picker_priority_cancels_auto() {
    let mut s=levels_session();s.frame(100_000_000,100_000_000).unwrap();
    let cancels=s.engine.backend().snapshot_cancels;levels_auto(&mut s);
    assert!(s.engine.backend().snapshot_cancels>cancels);
    let cancels=s.engine.backend().snapshot_cancels;
    arm_calibration(&mut s);
    assert!(s.auto_levels.is_none());assert!(s.engine.backend().snapshot_cancels>cancels);
    assert!(s.eyedropper.calibration.is_some());
}

#[test]
fn levels_auto_preserves_frozen_animated_time_and_refuses_save_until_complete() {
    let mut s=session(Platform::Gtk);
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"film_grain".into()}}).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:s.engine.document().active_layer.0,key:"animate".into(),value:layer_core::EffectValue::Toggle(true)}}).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"levels".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();s.frame(1,1).unwrap();levels_auto(&mut s);
    let layer_render::SnapshotRequest::LevelsStatistics(query)=s.engine.backend().snapshot_requests.last().unwrap() else {panic!("wrong Auto request")};
    let captured=query.time;let before=s.engine.document().layers.clone();
    assert!(s.require_raster_snapshot().is_err());
    assert!(!s.command(CommandId::SaveDocument).enabled);
    assert!(s.request_save(false).is_err());assert!(s.files.pending.is_none());
    s.frame(5_000_000_000,5_000_000_000).unwrap();assert!(s.engine.animation_time()>captured);
    levels_reply(&mut s,Ok(layer_render::SnapshotResult::LevelsStatistics(levels_result())));
    assert!(s.auto_levels.is_none());assert_ne!(s.engine.document().layers,before);
    assert!(s.require_raster_snapshot().is_ok());assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,before);
}

#[test]
fn levels_calibration_roles_apply_current_page_as_one_edit_without_paint() {
    use layer_core::levels::{CalibrationRole,calibrate_levels};
    for page in ["rgb","red","green","blue"] {for role in [CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White] {
        let mut s=levels_session();let layer=s.engine.document().active_layer;
        s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:page.into()}}).unwrap();
        let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
        let index=["rgb","red","green","blue"].iter().position(|p|*p==page).unwrap();
        let expected=calibrate_levels(before[0].effect.as_ref().unwrap(),[0.2,0.3,0.4],s.engine.document().color.space,index as u8,role).unwrap();
        s.dispatch(UiAction::Effect {action:EffectAction::Calibrate {layer:layer.0,epoch:s.state.layer_properties.epoch,role}}).unwrap();
        release_calibration(&mut s);
        let layer_render::SnapshotRequest::ArtworkSample(query)=s.engine.backend().snapshot_requests.last().unwrap() else {panic!("wrong calibration request")};
        assert_eq!(query.source,layer_core::ArtworkSource::EffectInput(layer));
        calibration_reply(&mut s,Ok(layer_core::ArtworkSample::Color([0.2,0.3,0.4,1.])));
        assert_eq!(s.engine.document().layers[0].effect.as_deref(),Some(&expected));assert_eq!(s.engine.backend().dabs,0);
        assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }}
}

#[test]
fn curves_calibration_roles_commit_one_atomic_current_page_edit_and_preserve_master() {
    use layer_core::levels::CalibrationRole;
    for page in ["rgb","red","green","blue"] {for role in [CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White] {
        let mut s=session(Platform::Gtk);
        s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
        s.reveal_panel(Panel::Properties).unwrap();s.frame(1,1).unwrap();let layer=s.engine.document().active_layer;
        s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:page.into()}}).unwrap();
        let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
        let index=["rgb","red","green","blue"].iter().position(|p|*p==page).unwrap();
        let expected=layer_core::curves::calibrate_curves(before[0].effect.as_ref().unwrap(),[0.2,0.3,0.4],s.engine.document().color.space,index as u8,role).unwrap();
        s.dispatch(UiAction::Effect {action:EffectAction::Calibrate {layer:layer.0,epoch:s.state.layer_properties.epoch,role}}).unwrap();release_calibration(&mut s);
        calibration_reply(&mut s,Ok(layer_core::ArtworkSample::Color([0.2,0.3,0.4,1.])));
        assert_eq!(s.engine.document().layers[0].effect.as_deref(),Some(&expected));assert_eq!(s.engine.backend().dabs,0);
        assert_eq!(expected.value("curve_0"),before[0].effect.as_ref().unwrap().value("curve_0"));
        assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }}
}

#[test]
fn curves_calibration_unreachable_master_refuses_all_channels_without_history() {
    let mut s=session(Platform::Gtk);
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
    s.reveal_panel(Panel::Properties).unwrap();s.frame(1,1).unwrap();let layer=s.engine.document().active_layer;
    s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:layer.0,key:"curve_0".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.2],[1.,0.8]])}}).unwrap();
    let before=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
    s.dispatch(UiAction::Effect {action:EffectAction::Calibrate {layer:layer.0,epoch:s.state.layer_properties.epoch,role:layer_core::levels::CalibrationRole::Black}}).unwrap();release_calibration(&mut s);
    calibration_reply(&mut s,Ok(layer_core::ArtworkSample::Color([0.2,0.3,0.4,1.])));
    assert_eq!(s.engine.document().layers,before);assert_eq!(s.engine.checkpoint(),checkpoint);
    assert!(s.eyedropper.calibration.is_some());assert_eq!(s.engine.backend().dabs,0);
}

#[test]
fn levels_auto_stale_toolbar_action_preserves_current_pending_owner() {
    let mut s=levels_session();levels_auto(&mut s);
    let requests=s.engine.backend().snapshot_requests.len();let cancels=s.engine.backend().snapshot_cancels;
    let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
    for action in stale_property_actions(s.engine.document().active_layer.0,s.state.layer_properties.epoch.wrapping_sub(1)) {
        s.dispatch(UiAction::Effect {action}).unwrap();
        assert!(s.auto_levels.is_some());assert_eq!(s.engine.backend().snapshot_requests.len(),requests);assert_eq!(s.engine.backend().snapshot_cancels,cancels);
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn levels_pending_auto_and_calibration_preserve_concurrent_label_rename_and_undo() {
    for automatic in [false,true] {
        let mut s=levels_session();
        if automatic {levels_auto(&mut s);}else{arm_calibration(&mut s);release_calibration(&mut s);}
        let mut layer=s.engine.document().layers[0].clone();layer.name="Renamed adjustment".into();
        s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(layer))).unwrap();
        let renamed=s.engine.document().layers.clone();let checkpoint=s.engine.checkpoint();
        if automatic {levels_reply(&mut s,Ok(layer_render::SnapshotResult::LevelsStatistics(levels_result())));}
        else {calibration_reply(&mut s,Ok(layer_core::ArtworkSample::Color([0.2,0.3,0.4,1.])));}
        assert_eq!(s.engine.document().layers[0].name.as_ref(),"Renamed adjustment");
        assert!(s.engine.document().layers[0].effect!=renamed[0].effect,"accepted correction should change effect");
        assert!(s.engine.undo().unwrap());assert_eq!(s.engine.document().layers,renamed);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn levels_auto_success_noop_and_refusal_restart_visible_histogram_without_another_input() {
    for outcome in 0..3 {
        let mut s=levels_session();
        if outcome==1 {
            let candidate=layer_core::levels::auto_levels(s.engine.document().layers[0].effect.as_ref().unwrap(),&levels_result(),0).unwrap();
            let mut layer=s.engine.document().layers[0].clone();layer.effect=Some(std::sync::Arc::new(candidate));
            s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(layer))).unwrap();s.refresh_document();
        }
        levels_auto(&mut s);let before=s.engine.document().layers.clone();
        let result=if outcome==2 {Err(layer_render::BackendError("Refused Auto".into()))}else{Ok(layer_render::SnapshotResult::LevelsStatistics(levels_result()))};
        levels_reply(&mut s,result);assert!(s.auto_levels.is_none());
        if outcome!=0 {assert_eq!(s.engine.document().layers,before);}
        assert!(s.tonal_histogram.demand,"completion must restore visible consumer {outcome}");
        assert!(s.wants_continuous_frames());s.frame(100_000_000,100_000_000).unwrap();
        assert!(matches!(s.engine.backend().snapshot_requests.last(),Some(layer_render::SnapshotRequest::ArtworkStatistics(_))));
    }
}
