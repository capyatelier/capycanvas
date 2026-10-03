fn lookup_resource(title:&str,value:f32)->std::sync::Arc<layer_core::Lut3d> {
    std::sync::Arc::new(layer_core::Lut3d::from_samples(2,[[0.;3],[1.;3]],title.into(),vec![[value;3];8].into()).unwrap())
}
fn lookup_session(platform:Platform)->UiSession<Recorder> {color_adjustment_session_on("color_lookup",platform)}
fn lookup_request(s:&mut UiSession<Recorder>)->u32 {
    let view=&s.state.layer_properties;let change=s.dispatch(UiAction::Effect {action:EffectAction::ImportLookup {layer:view.layer.unwrap(),epoch:view.epoch}}).unwrap();assert_ne!(change.regions&crate::regions::HOST,0,"new import request must reach the host");
    s.state.requests.iter().find_map(|r|matches!(&r.kind,HostRequestKind::Document {request:DocumentRequest::ImportLookup {..}}).then_some(r.id)).unwrap()
}
fn apply_lookup_complete(s:&mut UiSession<Recorder>,resource:std::sync::Arc<layer_core::Lut3d>) {
    let request=lookup_request(s);assert!(s.apply_lookup(request,resource).unwrap());s.complete_document_request(request,Ok(true)).unwrap();
}

#[test]
fn lookup_import_cancel_and_replacement_are_atomic_and_undoable() {
    let mut s=lookup_session(Platform::Gtk);let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();let request=lookup_request(&mut s);assert_eq!(s.engine.document(),&before);s.complete_document_request(request,Ok(false)).unwrap();assert_eq!(s.engine.checkpoint(),checkpoint);assert!(s.state.requests.is_empty());
    let first=lookup_resource("First",0.25);apply_lookup_complete(&mut s,first.clone());let layer=s.engine.document().active_layer;assert!(std::sync::Arc::ptr_eq(s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap().lut3d().unwrap(),&first));
    let second=lookup_resource("Second",0.75);apply_lookup_complete(&mut s,second.clone());invoke(&mut s,CommandId::Undo);assert!(std::sync::Arc::ptr_eq(s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap().lut3d().unwrap(),&first));invoke(&mut s,CommandId::Redo);assert!(std::sync::Arc::ptr_eq(s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap().lut3d().unwrap(),&second));
    let mut bytes=Vec::new();layer_core::Project {document:s.engine.document().clone()}.write(&mut bytes).unwrap();let reopened=layer_core::Project::read(bytes.as_slice(),Default::default()).unwrap();assert_eq!(reopened.document.layer(layer).unwrap().effect.as_ref().unwrap().lut3d().unwrap().digest(),second.digest());
}

#[test]
fn lookup_bad_space_refuses_import_and_later_space_changes_without_partial_history() {
    use layer_core::EffectValue;
    let resource=lookup_resource("HDR chosen-space",1e17);assert!(!resource.accepts(layer_core::color::RgbSpace::Srgb));assert!(resource.accepts(layer_core::color::RgbSpace::ProPhoto));let mut s=lookup_session(Platform::Gtk);let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();let request=lookup_request(&mut s);assert!(s.apply_lookup(request,resource.clone()).is_err());assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);s.complete_document_request(request,Ok(false)).unwrap();
    color_adjustment_set(&mut s,"color_space",EffectValue::Choice(3));apply_lookup_complete(&mut s,resource);let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();let layer=s.engine.document().active_layer.0;assert!(s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:"color_space".into(),value:EffectValue::Choice(0)}}).is_err());assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
}

#[test]
fn lookup_completion_rejects_stale_layer_revision_and_document_identity() {
    for reason in ["layer","revision","document","activation"] {
        let mut s=lookup_session(Platform::Gtk);let request=lookup_request(&mut s);
        match reason {
            "layer"=>{let id=s.engine.document().layers.iter().find(|l|l.id!=s.engine.document().active_layer).unwrap().id.0;s.dispatch(UiAction::SelectLayer {id}).unwrap();},
            "revision"=>{color_adjustment_set(&mut s,"intensity",layer_core::EffectValue::Number(60.));},
            "activation"=>{let mut parked=UiSession::new(Recorder::default(),s.engine.document().clone(),[800,800],Platform::Gtk).unwrap();parked.inherit_window_state(&s).unwrap();s.inherit_window_state(&parked).unwrap();},
            _=>{let mut document=s.engine.document().clone();document.id="different-tab-reused-layer-ids".into();let other=UiSession::new(Recorder::default(),document,[800,800],Platform::Gtk).unwrap();s.engine=other.engine;},
        }
        let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();assert!(!s.apply_lookup(request,lookup_resource("Late",0.5)).unwrap(),"{reason}");assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);s.complete_document_request(request,Ok(false)).unwrap();
    }
}

#[test]
fn loaded_lookup_retains_values_across_hosts_and_localizes_shared_actions_in_both_themes() {
    use layer_core::EffectValue;
    let mut original=lookup_session(Platform::Gtk);apply_lookup_complete(&mut original,lookup_resource("Literal LUT title",0.5));color_adjustment_set(&mut original,"intensity",EffectValue::Number(42.));let project=layer_core::Project {document:original.engine.document().clone()};
    for platform in [Platform::Gtk,Platform::Web,Platform::Android,Platform::Mac,Platform::Ios,Platform::Windows] {for theme in [Theme::Light,Theme::Dark] {
        let mut s=UiSession::from_project(Recorder::default(),project.clone(),None,[800,600],platform).unwrap();s.dispatch(UiAction::SetTheme {theme:Some(theme)}).unwrap();assert_eq!(s.state.layer_properties.description,"Literal LUT title");assert_eq!(s.state.layer_properties.resource_name.as_deref(),Some("Literal LUT title"));assert_eq!(s.state.layer_properties.controls.iter().map(|c|c.key.as_str()).collect::<Vec<_>>(),["color_space","intensity"]);let effect=s.engine.document().layer(s.engine.document().active_layer).unwrap().effect.as_ref().unwrap();assert_eq!(effect.value("intensity"),Some(&EffectValue::Number(42.)));assert!(effect.lut3d().is_some());
        if platform==Platform::Gtk {let action=s.state.layer_properties.actions.iter().find(|a|matches!(a.action,EffectAction::ImportLookup {..})).unwrap();assert_eq!(action.label.as_str(),s.localization().text(MessageId::RESOURCES_LOOKUP_REPLACE).as_ref());let request=lookup_request(&mut s);let request=s.document_request(request).unwrap();assert_eq!(request.title(s.localization()),s.localization().text(MessageId::RESOURCES_LOOKUP_IMPORT));}
        else {assert!(s.state.layer_properties.actions.is_empty());let before=s.engine.document().clone();let requests=serde_json::to_value(&s.state.requests).unwrap();let action=EffectAction::ImportLookup {layer:s.state.layer_properties.layer.unwrap(),epoch:s.state.layer_properties.epoch};assert!(s.dispatch(UiAction::Effect {action}).is_err());assert_eq!(s.engine.document(),&before);assert_eq!(serde_json::to_value(&s.state.requests).unwrap(),requests);}
    }}
}

#[test]
fn queued_host_request_notifies_once_through_any_changed_region() {
    let mut s=lookup_session(Platform::Gtk);s.changed(0,false);
    s.request(HostRequestKind::NewWindow).unwrap();
    let change=s.changed(crate::regions::LAYOUT,false);
    assert_ne!(change.regions&crate::regions::HOST,0);
    assert_eq!(s.changed(crate::regions::LAYOUT,false).regions&crate::regions::HOST,0);
}

#[test]
fn lookup_completion_wakes_pending_edits_but_settled_cancellation_does_not() {
    let mut s=lookup_session(Platform::Gtk);s.frame(1,1).unwrap();
    assert!(!s.engine.has_pending_document_edits());
    let cancelled=lookup_request(&mut s);
    assert!(!s.complete_document_request(cancelled,Ok(false)).unwrap().canvas_wake);
    let accepted=lookup_request(&mut s);
    assert!(s.apply_lookup(accepted,lookup_resource("Wake",0.5)).unwrap());
    assert!(s.engine.has_pending_document_edits());
    assert!(s.complete_document_request(accepted,Ok(true)).unwrap().canvas_wake);
}

#[test]
fn lookup_parse_failures_use_active_localization_and_leave_no_partial_edit() {
    for language in UiLanguage::ALL {
        let mut s=lookup_session(Platform::Gtk);s.set_localization(Localizer::shared(language));s.frame(1,1).unwrap();
        let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();let request=lookup_request(&mut s);
        s.complete_document_request(request,Err("literal parser diagnostic line 17".into())).unwrap();
        assert_eq!(s.state.host_error.as_deref(),Some(s.localization().text(MessageId::RESOURCES_LOOKUP_FAILED).as_ref()));
        assert!(!s.state.host_error.as_ref().unwrap().contains("line 17"));
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);assert!(s.state.requests.is_empty());
        for next in [UiLanguage::Japanese, UiLanguage::Russian, UiLanguage::English] {
            s.set_localization(Localizer::shared(next));
            assert_eq!(s.state.host_error.as_deref(),Some(s.localization().text(MessageId::RESOURCES_LOOKUP_FAILED).as_ref()));
            assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);assert!(s.state.requests.is_empty());
        }
        let cancelled=lookup_request(&mut s);s.complete_document_request(cancelled,Ok(false)).unwrap();
        assert!(s.state.host_error.is_none());assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
}
