fn effect_test_paint(document:&layer_core::Document)->layer_core::OccurrenceHandle {
    let scene=document.scene();scene.order().iter().copied().find(|h| matches!(scene.occurrence(*h).unwrap().content,layer_core::authored::OccurrenceContent::Paint(_))).unwrap()
}
fn effect_test_occurrence_edit(document:&layer_core::Document,handle:layer_core::OccurrenceHandle,mutate:impl FnOnce(&mut layer_core::authored::Occurrence))->layer_core::Edit {
    let mut occurrence=document.scene().occurrence(handle).unwrap().clone();mutate(&mut occurrence);
    layer_core::Edit::Occurrence(layer_core::authored::RecordChange::replace(&document.artwork.occurrences,handle,Some(occurrence)).unwrap())
}
fn effect_test_selection_edit(document:&layer_core::Document,selection:Option<layer_core::Selection>)->layer_core::Edit {
    let mut working=document.working.clone();working.selection=selection;layer_core::Edit::Working(working)
}
fn effect_test_mask_edit(document:&layer_core::Document,handle:layer_core::OccurrenceHandle,default_coverage:f32)->layer_core::Edit {
    use layer_core::authored::{CoverageSource,MaskUse,RecordChange};
    let coverage=RecordChange::insert(&document.artwork.coverage,CoverageSource {domain:document.scene().local_extent(handle),initial:None,default_coverage,raster:Default::default(),operations:Default::default()});
    let occurrence=effect_test_occurrence_edit(document,handle,|o|o.mask=Some(MaskUse {source:coverage.handle,enabled:true,linked:true,inverted:false,translation:Point::default(),placement:layer_core::Projective::IDENTITY}));
    layer_core::Edit::Batch(vec![layer_core::Edit::Coverage(coverage),occurrence])
}
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
    let layer=s.engine.document().working.occurrence.unwrap();
    color_adjustment_set(&mut s,"shadows",layer_core::EffectValue::Number(50.));s.frame(200_000_000,200_000_000).unwrap();
    s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),layer,|o|o.opacity=0.5)).unwrap();s.frame(300_000_000,300_000_000).unwrap();
    s.engine.apply_edit(effect_test_mask_edit(s.engine.document(),layer,0.25)).unwrap();
    s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),layer,|o|o.blend=layer_core::LayerBlend::Multiply)).unwrap();s.frame(400_000_000,400_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_requests.len(),1);assert_eq!(s.engine.backend().analysis_accepts,1);
    assert!(s.engine.backend().snapshot_reply.is_some());assert_eq!(s.engine.backend().snapshot_cancels,0);
}

#[test]
fn effect_analysis_stacked_guides_invalidate_only_contributing_lower_amounts() {
    let mut s=analysis_session();let lower=s.engine.document().working.occurrence.unwrap();
    s.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"clarity".into()}}).unwrap();s.frame(1,1).unwrap();let upper=s.engine.document().working.occurrence.unwrap();
    analysis_poll(&mut s,100_000_000);analysis_ready(&mut s,200_000_000);analysis_ready(&mut s,300_000_000);
    assert_eq!(s.engine.backend().analysis_requests.len(),2);assert_eq!(s.engine.backend().analysis_accepts,2);
    let before=s.engine.checkpoint();
    s.dispatch(UiAction::Effect{action:EffectAction::Set{layer:occurrence_token(lower),key:"shadows".into(),value:layer_core::EffectValue::Number(40.)}}).unwrap();s.frame(400_000_000,400_000_000).unwrap();
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
    assert_eq!(s.effect_analyses.status(s.engine.document().working.occurrence.unwrap()),Some(MessageId::RESOURCES_ANALYSIS_ERROR));
    color_adjustment_set(&mut s,"shadows",layer_core::EffectValue::Number(40.));s.frame(700_000_000,700_000_000).unwrap();assert_eq!(s.engine.backend().analysis_requests.len(),1);
    let lower=effect_test_paint(s.engine.document());
    s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),lower,|o|o.opacity=0.5)).unwrap();s.frame(800_000_000,800_000_000).unwrap();assert_eq!(s.engine.backend().analysis_requests.len(),2);
}

#[test]
fn effect_analysis_stale_completions_are_rejected_after_activation_source_and_target_changes() {
    for stale_error in [false,true] {for reason in ["activation","source","target"] {
        let mut s=analysis_session();analysis_poll(&mut s,100_000_000);let accepts=s.engine.backend().analysis_accepts;
        s.engine.backend_mut().analysis_reply=Some(if stale_error {Err(layer_render::BackendError("old failure"))} else {Ok(())});
        match reason {
            "activation"=>{let mut parked=UiSession::new(Recorder::default(),s.engine.document().clone(),[800,800],Platform::Gtk).unwrap();parked.inherit_window_state(&s).unwrap();s.inherit_window_state(&parked).unwrap();},
            "source"=>{let lower=effect_test_paint(s.engine.document());s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),lower,|o|o.opacity=0.5)).unwrap();},
            _=>{let target=s.engine.document().working.occurrence.unwrap();s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),target,|o|o.visible=false)).unwrap();},
        }
        analysis_poll(&mut s,200_000_000);assert_eq!(s.engine.backend().analysis_accepts,accepts,"{reason}");assert!(s.engine.backend().analysis_reply.is_none());
        assert_ne!(s.effect_analyses.status(s.engine.document().working.occurrence.unwrap()),Some(MessageId::RESOURCES_ANALYSIS_ERROR));
    }}
}

#[test]
fn effect_analysis_metadata_and_pending_copy_are_localized_without_history() {
    for language in UiLanguage::ALL {for id in ["shadows_highlights","clarity"] {
        let mut s=color_adjustment_session(id);s.set_localization(Localizer::shared(language));s.frame(1,1).unwrap();let checkpoint=s.engine.checkpoint();
        let effect=s.engine.document().scene().effect(s.engine.document().scene().order()[0]).unwrap();assert!(effect.program.analysis().is_some());assert!(!effect.program.image_boundary());assert!(effect.program.fusion_boundary());
        analysis_poll(&mut s,100_000_000);assert_eq!(s.effect_analyses.status(s.engine.document().working.occurrence.unwrap()),Some(MessageId::RESOURCES_ANALYSIS_UPDATING));assert_eq!(s.engine.checkpoint(),checkpoint);
        assert!(!s.localization().text(MessageId::RESOURCES_ANALYSIS_UPDATING).is_empty());assert!(!s.localization().text(MessageId::RESOURCES_ANALYSIS_ERROR).is_empty());
    }}
}

#[test]
fn effect_analysis_lower_provisional_gesture_invalidates_upper_without_rescanning_own_guide() {
    let mut s=analysis_session();let lower=s.engine.document().working.occurrence.unwrap();
    s.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"clarity".into()}}).unwrap();s.frame(1,1).unwrap();let upper=s.engine.document().working.occurrence.unwrap();
    analysis_poll(&mut s,100_000_000);analysis_ready(&mut s,200_000_000);analysis_ready(&mut s,300_000_000);
    let gesture=|phase|UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Set{layer:occurrence_token(lower),key:"shadows".into(),value:layer_core::EffectValue::Number(60.)})}};
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
    let captured=s.engine.backend().analysis_requests[0].snapshot.context.elapsed;let cancels=s.engine.backend().analysis_cancels;
    s.frame(1_100_000_000,1_100_000_000).unwrap();assert!(s.engine.animation_time()>captured);assert_eq!(s.engine.backend().analysis_requests.len(),1);assert_eq!(s.engine.backend().analysis_cancels,cancels);
    analysis_ready(&mut s,1_100_000_001);assert_eq!(s.engine.backend().analysis_accepts,1);
    analysis_poll(&mut s,1_200_000_000);assert_eq!(s.engine.backend().analysis_requests.len(),2,"ready frozen time is now old enough to refresh");
    let current=s.engine.backend().analysis_requests[1].snapshot.context.elapsed;analysis_ready(&mut s,1_200_000_001);
    s.frame(1_300_000_000,1_300_000_000).unwrap();assert!(s.engine.animation_time()-current<0.5);assert_eq!(s.engine.backend().analysis_requests.len(),2);
}

#[test]
fn effect_analysis_completion_waits_until_an_unrelated_active_gesture_settles() {
    let mut s=analysis_session();analysis_poll(&mut s,100_000_000);
    let layer=s.engine.document().working.occurrence.unwrap();
    let gesture=|phase|UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Set{layer:occurrence_token(layer),key:"shadows".into(),value:layer_core::EffectValue::Number(60.)})}};
    s.dispatch(gesture(ContactPhase::Down)).unwrap();s.engine.backend_mut().analysis_reply=Some(Ok(()));analysis_poll(&mut s,200_000_000);
    assert_eq!(s.engine.backend().analysis_accepts,0);assert!(s.engine.backend().analysis_reply.is_some());
    s.dispatch(gesture(ContactPhase::Up)).unwrap();s.frame(300_000_000,300_000_000).unwrap();
    assert_eq!(s.engine.backend().analysis_accepts,1);assert!(s.engine.backend().analysis_reply.is_none());
}

#[test]
fn effect_analysis_backend_retention_failure_preserves_pending_owner_state() {
    let mut s=analysis_session();analysis_poll(&mut s,100_000_000);analysis_ready(&mut s,200_000_000);
    let lower=effect_test_paint(s.engine.document());
    s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),lower,|o|o.opacity=0.5)).unwrap();s.engine.backend_mut().analysis_retain_fails=true;
    assert!(s.poll_effect_analyses(300_000_000).is_err());
    assert!(s.effect_analyses.busy(),"retention failure cannot discard the pending replacement task");
    assert_eq!(s.effect_analyses.status(s.engine.document().working.occurrence.unwrap()),Some(MessageId::RESOURCES_ANALYSIS_UPDATING));
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
        let lower=effect_test_paint(s.engine.document());s.engine.apply_edit(effect_test_occurrence_edit(s.engine.document(),lower,|o|o.opacity=0.5)).unwrap();s.frame(300_000_000,300_000_000).unwrap();
        s.engine.backend_mut().analysis_reply=Some(Err(layer_render::BackendError("private diagnostic")));analysis_poll(&mut s,400_000_000);let failure=s.localization().text(MessageId::RESOURCES_ANALYSIS_ERROR).to_string();
        for _ in 0..3 {s.refresh_document();assert_eq!(s.state.layer_properties.title,format!("{title} · {failure}"));assert_eq!(s.state.layer_properties.title.matches(&failure).count(),1);assert_eq!(s.state.layer_properties.description,failure);}
    }}
}

#[test]
fn effect_analysis_dead_renderer_can_suspend_and_recover_without_retirement_calls() {
    let mut s=analysis_session();let document=s.engine.document().clone();let checkpoint=s.engine.checkpoint();s.engine.backend_mut().analysis_retain_fails=true;
    s.suspend_renderer().unwrap();assert!(s.rendering_suspended());assert_eq!(s.engine.document(),&document);assert_eq!(s.engine.checkpoint(),checkpoint);
    let retired=s.engine.backend().analysis_retained.len();s.frame(100_000_000,100_000_000).unwrap();assert!(s.rendering_suspended());assert_eq!(s.engine.backend().analysis_retained.len(),retired);
    let (old,_)=s.replace_renderer(Recorder::default()).unwrap();assert!(old.analysis_retain_fails);assert!(!s.rendering_suspended());assert_eq!(s.engine.checkpoint(),checkpoint);
}

#[test]
fn effect_analysis_dead_renderer_direct_replacement_does_not_retire_failed_backend() {
    let mut s=analysis_session();s.engine.backend_mut().analysis_retain_fails=true;let checkpoint=s.engine.checkpoint();
    s.replace_renderer(Recorder::default()).unwrap();assert_eq!(s.engine.checkpoint(),checkpoint);assert!(!s.rendering_suspended());
}

#[test]
fn effect_analysis_parked_inheritance_and_replacement_preserve_history() {
    let mut previous=analysis_session();previous.suspend_renderer().unwrap();let checkpoint=previous.engine.checkpoint();
    let mut parked=UiSession::new(Recorder::default(),previous.engine.document().clone(),[800,800],Platform::Gtk).unwrap();parked.suspend_renderer().unwrap();parked.engine.backend_mut().analysis_retain_fails=true;
    parked.inherit_window_state(&previous).unwrap();assert!(parked.rendering_suspended());parked.frame(100_000_000,100_000_000).unwrap();parked.replace_renderer(Recorder::default()).unwrap();assert_eq!(previous.engine.checkpoint(),checkpoint);assert!(!parked.rendering_suspended());
}
