fn analysis_session()->UiSession<Recorder> {
    let mut s=color_adjustment_session("shadows_highlights");s.frame(1,1).unwrap();s
}
fn analysis_poll(s:&mut UiSession<Recorder>,now:u64) {s.poll_effect_analyses(now).unwrap();}
fn analysis_ready(s:&mut UiSession<Recorder>,now:u64) {s.engine.backend_mut().analysis_reply=Some(Ok(()));analysis_poll(s,now);}

#[test]
fn effect_analysis_reuses_own_consumption_and_keeps_snapshot_slot_independent() {
    let mut s=analysis_session();analysis_poll(&mut s,100_000_000);assert_eq!(s.engine.backend().analysis_requests.len(),1);
    let snapshot=layer_render::SnapshotResult::ArtworkSample(layer_core::ArtworkSample::Empty);
    s.engine.backend_mut().snapshot_reply=Some(Ok(snapshot));
    analysis_ready(&mut s,110_000_000);assert_eq!(s.engine.backend().analysis_accepts,1);assert!(s.engine.backend().snapshot_reply.is_some());
    let layer=s.engine.document().active_layer;
    color_adjustment_set(&mut s,"shadows",layer_core::EffectValue::Number(50.));s.frame(200_000_000,200_000_000).unwrap();
    s.engine.apply_edit(layer_core::Edit::SetLayerOpacity{id:layer,opacity:0.5}).unwrap();s.frame(300_000_000,300_000_000).unwrap();
    let mut own=s.engine.document().layer(layer).unwrap().clone();let mut mask=layer_core::LayerMask::reveal_all(s.engine.document().layers.iter().map(|l|l.id).max().map(|id|layer_core::LayerId(id.0+100)).unwrap(),layer_core::Point::default());mask.default_coverage=0.25;own.mask=Some(mask);own.properties.blend=layer_core::LayerBlend::Multiply;
    s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(own))).unwrap();s.frame(400_000_000,400_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_requests.len(),1);assert_eq!(s.engine.backend().analysis_accepts,1);
    assert!(s.engine.backend().snapshot_reply.is_some());assert_eq!(s.engine.backend().snapshot_cancels,0);
}

#[test]
fn effect_analysis_stacked_guides_invalidate_only_contributing_lower_amounts() {
    let mut s=analysis_session();let lower=s.engine.document().active_layer;
    s.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"clarity".into()}}).unwrap();s.frame(1,1).unwrap();let upper=s.engine.document().active_layer;
    analysis_poll(&mut s,100_000_000);analysis_ready(&mut s,200_000_000);analysis_ready(&mut s,300_000_000);
    assert_eq!(s.engine.backend().analysis_requests.len(),2);assert_eq!(s.engine.backend().analysis_accepts,2);
    let before=s.engine.checkpoint();
    s.dispatch(UiAction::Effect{action:EffectAction::Set{layer:lower.0,key:"shadows".into(),value:layer_core::EffectValue::Number(40.)}}).unwrap();s.frame(400_000_000,400_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_requests.len(),3);
    assert_eq!(s.engine.backend().analysis_requests.last().unwrap().source,layer_core::ArtworkSource::EffectInput(upper));
    analysis_ready(&mut s,500_000_000);assert_eq!(s.engine.backend().analysis_accepts,3);assert_ne!(s.engine.checkpoint(),before);
    let checkpoint=s.engine.checkpoint();analysis_poll(&mut s,600_000_000);assert_eq!(s.engine.checkpoint(),checkpoint);
}

#[test]
fn effect_analysis_pending_and_failed_sources_do_not_retry_until_relevant_change() {
    let mut s=analysis_session();analysis_poll(&mut s,100_000_000);
    for time in [200_000_000,300_000_000] {analysis_poll(&mut s,time);}assert_eq!(s.engine.backend().analysis_requests.len(),1);
    s.engine.backend_mut().analysis_reply=Some(Err(layer_render::BackendError("guide failure")));analysis_poll(&mut s,400_000_000);
    for time in [500_000_000,600_000_000] {analysis_poll(&mut s,time);}assert_eq!(s.engine.backend().analysis_requests.len(),1);assert_eq!(s.engine.backend().analysis_accepts,0);
    assert_eq!(s.effect_analyses.status(s.engine.document().active_layer),Some(MessageId::RESOURCES_ANALYSIS_ERROR));
    color_adjustment_set(&mut s,"shadows",layer_core::EffectValue::Number(40.));s.frame(700_000_000,700_000_000).unwrap();assert_eq!(s.engine.backend().analysis_requests.len(),1);
    let lower=s.engine.document().layers.iter().find(|l|l.kind==layer_core::LayerKind::Paint).unwrap().id;
    s.engine.apply_edit(layer_core::Edit::SetLayerOpacity{id:lower,opacity:0.5}).unwrap();s.frame(800_000_000,800_000_000).unwrap();assert_eq!(s.engine.backend().analysis_requests.len(),2);
}

#[test]
fn effect_analysis_stale_completions_are_rejected_after_activation_source_and_target_changes() {
    for stale_error in [false,true] {for reason in ["activation","source","target"] {
        let mut s=analysis_session();analysis_poll(&mut s,100_000_000);let accepts=s.engine.backend().analysis_accepts;
        s.engine.backend_mut().analysis_reply=Some(if stale_error {Err(layer_render::BackendError("old failure"))} else {Ok(())});
        match reason {
            "activation"=>{let mut parked=UiSession::new(Recorder::default(),s.engine.document().clone(),[800,800],Platform::Gtk).unwrap();parked.inherit_window_state(&s).unwrap();s.inherit_window_state(&parked).unwrap();},
            "source"=>{let lower=s.engine.document().layers.iter().find(|l|l.kind==layer_core::LayerKind::Paint).unwrap().id;s.engine.apply_edit(layer_core::Edit::SetLayerOpacity{id:lower,opacity:0.5}).unwrap();},
            _=>{let target=s.engine.document().active_layer;s.engine.apply_edit(layer_core::Edit::SetLayerVisibility{id:target,visible:false}).unwrap();},
        }
        analysis_poll(&mut s,200_000_000);assert_eq!(s.engine.backend().analysis_accepts,accepts,"{reason}");assert!(s.engine.backend().analysis_reply.is_none());
        assert_ne!(s.effect_analyses.status(s.engine.document().active_layer),Some(MessageId::RESOURCES_ANALYSIS_ERROR));
    }}
}

#[test]
fn effect_analysis_metadata_and_pending_copy_are_localized_without_history() {
    for language in UiLanguage::ALL {for id in ["shadows_highlights","clarity"] {
        let mut s=color_adjustment_session(id);s.set_localization(Localizer::shared(language));s.frame(1,1).unwrap();let checkpoint=s.engine.checkpoint();
        let effect=s.engine.document().layers[0].effect.as_ref().unwrap();assert!(effect.program.analysis().is_some());assert!(!effect.program.image_boundary());assert!(effect.program.fusion_boundary());
        analysis_poll(&mut s,100_000_000);assert_eq!(s.effect_analyses.status(s.engine.document().active_layer),Some(MessageId::RESOURCES_ANALYSIS_UPDATING));assert_eq!(s.engine.checkpoint(),checkpoint);
        assert!(!s.localization().text(MessageId::RESOURCES_ANALYSIS_UPDATING).is_empty());assert!(!s.localization().text(MessageId::RESOURCES_ANALYSIS_ERROR).is_empty());
    }}
}

#[test]
fn effect_analysis_lower_provisional_gesture_invalidates_upper_without_rescanning_own_guide() {
    let mut s=analysis_session();let lower=s.engine.document().active_layer;
    s.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"clarity".into()}}).unwrap();s.frame(1,1).unwrap();let upper=s.engine.document().active_layer;
    analysis_poll(&mut s,100_000_000);analysis_ready(&mut s,200_000_000);analysis_ready(&mut s,300_000_000);
    let gesture=|phase|UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Set{layer:lower.0,key:"shadows".into(),value:layer_core::EffectValue::Number(60.)})}};
    s.dispatch(gesture(ContactPhase::Down)).unwrap();s.frame(400_000_000,400_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_requests.len(),2,"provisional edit retires upper guide but waits for idle");
    assert_eq!(s.effect_analyses.status(lower),None);assert_eq!(s.effect_analyses.status(upper),Some(MessageId::RESOURCES_ANALYSIS_UPDATING));
    s.dispatch(gesture(ContactPhase::Up)).unwrap();s.frame(500_000_000,500_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_requests.len(),3);assert_eq!(s.engine.backend().analysis_requests.last().unwrap().source,layer_core::ArtworkSource::EffectInput(upper));
}

#[test]
fn effect_analysis_freezes_animation_in_flight_and_refreshes_ready_guides_at_half_second() {
    let mut s=color_adjustment_session("film_grain");color_adjustment_set(&mut s,"animate",layer_core::EffectValue::Toggle(true));
    s.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"clarity".into()}}).unwrap();s.frame(1,1).unwrap();
    s.frame(100_000_000,100_000_000).unwrap();assert_eq!(s.engine.backend().analysis_requests.len(),1);
    let captured=s.engine.backend().analysis_requests[0].time;let cancels=s.engine.backend().analysis_cancels;
    s.frame(1_100_000_000,1_100_000_000).unwrap();assert!(s.engine.animation_time()>captured);assert_eq!(s.engine.backend().analysis_requests.len(),1);assert_eq!(s.engine.backend().analysis_cancels,cancels);
    analysis_ready(&mut s,1_100_000_001);assert_eq!(s.engine.backend().analysis_accepts,1);
    analysis_poll(&mut s,1_200_000_000);assert_eq!(s.engine.backend().analysis_requests.len(),2,"ready frozen time is now old enough to refresh");
    let current=s.engine.backend().analysis_requests[1].time;analysis_ready(&mut s,1_200_000_001);
    s.frame(1_300_000_000,1_300_000_000).unwrap();assert!(s.engine.animation_time()-current<0.5);assert_eq!(s.engine.backend().analysis_requests.len(),2);
}

#[test]
fn effect_analysis_completion_waits_until_an_unrelated_active_gesture_settles() {
    let mut s=analysis_session();analysis_poll(&mut s,100_000_000);
    let layer=s.engine.document().active_layer;
    let gesture=|phase|UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Set{layer:layer.0,key:"shadows".into(),value:layer_core::EffectValue::Number(60.)})}};
    s.dispatch(gesture(ContactPhase::Down)).unwrap();s.engine.backend_mut().analysis_reply=Some(Ok(()));analysis_poll(&mut s,200_000_000);
    assert_eq!(s.engine.backend().analysis_accepts,0);assert!(s.engine.backend().analysis_reply.is_some());
    s.dispatch(gesture(ContactPhase::Up)).unwrap();s.frame(300_000_000,300_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_accepts,1);assert!(s.engine.backend().analysis_reply.is_none());
}

#[test]
fn effect_analysis_backend_retention_failure_preserves_pending_owner_state() {
    let mut s=analysis_session();analysis_poll(&mut s,100_000_000);analysis_ready(&mut s,200_000_000);
    let lower=s.engine.document().layers.iter().find(|l|l.kind==layer_core::LayerKind::Paint).unwrap().id;
    s.engine.apply_edit(layer_core::Edit::SetLayerOpacity{id:lower,opacity:0.5}).unwrap();s.engine.backend_mut().analysis_retain_fails=true;
    assert!(s.poll_effect_analyses(300_000_000).is_err());
    assert!(s.effect_analyses.busy(),"retention failure cannot discard the pending replacement task");
    assert_eq!(s.effect_analyses.status(s.engine.document().active_layer),Some(MessageId::RESOURCES_ANALYSIS_UPDATING));
    s.engine.backend_mut().analysis_retain_fails=false;s.frame(400_000_000,400_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_requests.len(),2);
}

#[test]
fn effect_analysis_100_own_amount_edits_keep_one_request_and_one_publication() {
    for id in ["shadows_highlights", "clarity"] {
        let mut session = color_adjustment_session(id);
        session.frame(1, 1).unwrap();
        analysis_poll(&mut session, 100_000_000);
        analysis_ready(&mut session, 110_000_000);
        let key = if id == "clarity" { "amount" } else { "shadows" };
        for amount in 0..100 {
            color_adjustment_set(&mut session, key, layer_core::EffectValue::Number(amount as f32));
            let time = 200_000_000 + amount as u64 * 100_000_000;
            session.frame(time, time).unwrap();
            analysis_poll(&mut session, time);
            assert_eq!(session.engine.backend().analysis_requests.len(), 1, "{id}: {amount}");
            assert_eq!(session.engine.backend().analysis_accepts, 1, "{id}: {amount}");
        }
    }
}

#[test]
fn effect_analysis_headings_publish_each_localized_status_once_on_every_host() {
    for platform in [Platform::Gtk,Platform::Web,Platform::Android,Platform::Mac,Platform::Ios,Platform::Windows] {for language in UiLanguage::ALL {
        let mut s=color_adjustment_session_on("clarity",platform);s.set_localization(Localizer::shared(language));let title=s.state.layer_properties.title.clone();s.frame(1,1).unwrap();
        analysis_poll(&mut s,100_000_000);let pending=s.localization().text(MessageId::RESOURCES_ANALYSIS_UPDATING).to_string();
        for _ in 0..3 {s.refresh_document();assert_eq!(s.state.layer_properties.title,format!("{title} · {pending}"));assert_eq!(s.state.layer_properties.title.matches(&pending).count(),1);assert_eq!(s.state.layer_properties.description,pending);}
        analysis_ready(&mut s,200_000_000);assert_eq!(s.state.layer_properties.title,title);
        let lower=s.engine.document().layers.iter().find(|l|l.kind==layer_core::LayerKind::Paint).unwrap().id;s.engine.apply_edit(layer_core::Edit::SetLayerOpacity{id:lower,opacity:0.5}).unwrap();s.frame(300_000_000,300_000_000).unwrap();
        s.engine.backend_mut().analysis_reply=Some(Err(layer_render::BackendError("private diagnostic")));analysis_poll(&mut s,400_000_000);let failure=s.localization().text(MessageId::RESOURCES_ANALYSIS_ERROR).to_string();
        for _ in 0..3 {s.refresh_document();assert_eq!(s.state.layer_properties.title,format!("{title} · {failure}"));assert_eq!(s.state.layer_properties.title.matches(&failure).count(),1);assert_eq!(s.state.layer_properties.description,failure);}
    }}
}
