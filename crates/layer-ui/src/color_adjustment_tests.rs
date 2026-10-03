fn color_adjustment_session(id:&str)->UiSession<Recorder> {
    color_adjustment_session_on(id,Platform::Gtk)
}
fn color_adjustment_session_on(id:&str,platform:Platform)->UiSession<Recorder> {
    let mut s=session(platform);s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:id.into()}}).unwrap();s
}
fn color_adjustment_set(s:&mut UiSession<Recorder>,key:&str,value:layer_core::EffectValue) {
    s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:s.engine.document().active_layer.0,key:key.into(),value}}).unwrap();
}

#[test]
fn colorize_filters_empty_pages_preserves_range_values_and_undo_restores_controls() {
    use layer_core::EffectValue;
    let mut s=color_adjustment_session("hue_saturation");let layer=s.engine.document().active_layer;
    assert_eq!(s.state.layer_properties.pages.len(),7);
    assert_eq!(s.state.layer_properties.controls.iter().map(|c|c.key.as_str()).collect::<Vec<_>>(),["hue","saturation","lightness","colorize"]);
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:"reds".into()}}).unwrap();
    color_adjustment_set(&mut s,"reds_hue",EffectValue::Number(47.));
    let before=s.engine.document().layers.clone();color_adjustment_set(&mut s,"colorize",EffectValue::Toggle(true));
    assert_eq!(s.state.layer_properties.pages.iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),["rgb"]);
    let keys:Vec<_>=s.state.layer_properties.controls.iter().map(|c|c.key.as_str()).collect();
    assert_eq!(keys,["colorize_hue","colorize_saturation","lightness","colorize"]);
    assert!(keys.contains(&"lightness"));assert!(!keys.contains(&"hue"));assert!(!keys.contains(&"saturation"));assert!(!keys.iter().any(|k|k.starts_with("reds_")));
    assert_eq!(s.engine.document().layers[0].effect.as_ref().unwrap().value("reds_hue"),Some(&EffectValue::Number(47.)));
    invoke(&mut s,CommandId::Undo);assert_eq!(s.engine.document().layers,before);assert_eq!(s.state.layer_properties.pages.len(),7);
    invoke(&mut s,CommandId::Redo);assert_eq!(s.state.layer_properties.pages.len(),1);
    color_adjustment_set(&mut s,"colorize",EffectValue::Toggle(false));assert_eq!(s.state.layer_properties.pages.len(),7);
    s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:"reds".into()}}).unwrap();
    assert!(s.state.layer_properties.controls.iter().any(|c|c.key.as_str()=="reds_hue"&&c.value==EffectValue::Number(47.)));
}

#[test]
fn simple_adjustments_publish_existing_typed_controls_and_roundtrip_complete_effect_values() {
    use layer_core::{EffectValue,Project,ProjectLimits};
    for id in ["invert","threshold","desaturate","photo_filter","hue_saturation"] {
        let mut s=color_adjustment_session(id);
        match id {
            "invert"|"desaturate"=>assert!(s.state.layer_properties.controls.is_empty()),
            "threshold"=>{assert_eq!(s.state.layer_properties.controls.len(),1);assert!(matches!(s.state.layer_properties.controls[0].kind,PropertyKind::Number {..}));color_adjustment_set(&mut s,"threshold",EffectValue::Number(0.73));},
            "photo_filter"=>{assert_eq!(s.state.layer_properties.controls.len(),3);assert!(matches!(s.state.layer_properties.controls[0].value,EffectValue::Color(_)));assert!(matches!(s.state.layer_properties.controls[1].kind,PropertyKind::Number {..}));assert!(matches!(s.state.layer_properties.controls[2].value,EffectValue::Toggle(true)));color_adjustment_set(&mut s,"density",EffectValue::Number(67.));color_adjustment_set(&mut s,"preserve_luminance",EffectValue::Toggle(false));},
            _=>{color_adjustment_set(&mut s,"blues_saturation",EffectValue::Number(-34.));color_adjustment_set(&mut s,"colorize",EffectValue::Toggle(true));},
        }
        let document=s.engine.document().clone();let mut bytes=Vec::new();Project {document:document.clone()}.write(&mut bytes).unwrap();
        let reopened=Project::read(bytes.as_slice(),ProjectLimits::default()).unwrap();assert_eq!(reopened.document.layers,document.layers);assert_eq!(reopened.document.color,document.color);
    }
}

#[test]
fn threshold_properties_use_depth_bounds_and_soft_slider_limits_on_each_host() {
    use layer_core::color::SampleDepth;
    for platform in [Platform::Gtk,Platform::Web,Platform::Android] {for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        let mut document=session(platform).engine.document().clone();document.color.depth=depth;document.blend_space=layer_core::BlendSpace::Linear;
        let renderer=Recorder {color:document.color,..Recorder::default()};
        let mut s=UiSession::new(renderer,document,[800,800],platform).unwrap();
        s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"threshold".into()}}).unwrap();
        let PropertyKind::Number {numeric}=&s.state.layer_properties.controls[0].kind else {panic!("missing threshold number")};
        assert_eq!((numeric.min,numeric.max),if depth.is_float(){(-65504.,65504.)}else{(0.,1.)});
        assert_eq!((numeric.soft_min,numeric.soft_max),(0.,1.));
    }}
}

#[test]
fn selective_color_page_choice_is_transient_and_continuous_ink_edit_has_one_undo() {
    use layer_core::EffectValue;
    for platform in [Platform::Gtk,Platform::Web,Platform::Android] {
        let mut s=color_adjustment_session_on("selective_color",platform);let layer=s.engine.document().active_layer;
        assert_eq!(s.state.layer_properties.pages.len(),9);let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
        s.dispatch(UiAction::Effect {action:EffectAction::SelectPage {layer:layer.0,page:"blacks".into()}}).unwrap();
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
        let keys:Vec<_>=s.state.layer_properties.controls.iter().map(|control|control.key.as_str()).collect();
        assert!(keys.contains(&"mode"));for ink in ["cyan","magenta","yellow","black"] {assert!(keys.contains(&format!("blacks_{ink}").as_str()));}
        for (phase,value) in [(ContactPhase::Down,10.),(ContactPhase::Move,20.),(ContactPhase::Up,30.)] {
            s.dispatch(UiAction::Effect {action:EffectAction::Gesture {phase,action:Box::new(EffectAction::Set {layer:layer.0,key:"blacks_black".into(),value:EffectValue::Number(value)})}}).unwrap();
        }
        assert_eq!(s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap().value("blacks_black"),Some(&EffectValue::Number(30.)));
        invoke(&mut s,CommandId::Undo);assert_eq!(s.engine.document().layers,before.layers);assert_eq!(s.engine.checkpoint(),checkpoint);
        invoke(&mut s,CommandId::Redo);assert_eq!(s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap().value("blacks_black"),Some(&EffectValue::Number(30.)));
    }
}

#[test]
fn channel_mixer_monochrome_projects_gray_page_and_retains_all_hidden_rows() {
    use layer_core::EffectValue;
    for platform in [Platform::Gtk,Platform::Web,Platform::Android] {
        let mut s=color_adjustment_session_on("channel_mixer",platform);let layer=s.engine.document().active_layer;
        assert_eq!(s.state.layer_properties.pages.iter().map(|page|page.id.as_str()).collect::<Vec<_>>(),["red","green","blue"]);
        color_adjustment_set(&mut s,"red_green",EffectValue::Number(-123.));color_adjustment_set(&mut s,"blue_constant",EffectValue::Number(31.));
        let before=s.engine.document().layers.clone();color_adjustment_set(&mut s,"monochrome",EffectValue::Toggle(true));
        assert_eq!(s.state.layer_properties.pages.iter().map(|page|page.id.as_str()).collect::<Vec<_>>(),["gray"]);
        let effect=s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap();
        assert_eq!(effect.values.len(),17);assert_eq!(effect.value("red_green"),Some(&EffectValue::Number(-123.)));assert_eq!(effect.value("blue_constant"),Some(&EffectValue::Number(31.)));
        let prior=before.iter().find(|item|item.id==layer).unwrap().effect.as_ref().unwrap();
        for parameter in effect.program.parameters.iter().filter(|parameter|parameter.key.as_ref()!="monochrome") {assert_eq!(effect.value(&parameter.key),prior.value(&parameter.key));}
        invoke(&mut s,CommandId::Undo);assert_eq!(s.engine.document().layers,before);assert_eq!(s.state.layer_properties.pages.len(),3);
        invoke(&mut s,CommandId::Redo);color_adjustment_set(&mut s,"gray_red",EffectValue::Number(47.));color_adjustment_set(&mut s,"monochrome",EffectValue::Toggle(false));
        assert_eq!(s.engine.document().layer(layer).unwrap().effect.as_ref().unwrap().value("gray_red"),Some(&EffectValue::Number(47.)));
    }
}

#[test]
fn color_adjustment_edits_and_archive_keep_source_identity_and_hidden_values() {
    use layer_core::{EffectValue,Project,ProjectLimits};
    for id in ["selective_color","channel_mixer"] {
        let mut document=session(Platform::Gtk).engine.document().clone();
        let source=layer_core::color::source::rgba8_source([2,1],|x,_|if x==0 {[255,20,80,128]}else{[30,200,90,255]});
        document.layers[0].source=Some(source.clone());let source_layer=document.layers[0].id;
        let mut s=UiSession::new(Recorder {tiled_sources:true,..Recorder::default()},document,[800,800],Platform::Gtk).unwrap();
        s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:id.into()}}).unwrap();
        if id=="selective_color" {color_adjustment_set(&mut s,"neutrals_magenta",EffectValue::Number(-44.));color_adjustment_set(&mut s,"mode",EffectValue::Choice(1));}
        else {color_adjustment_set(&mut s,"green_blue",EffectValue::Number(173.));color_adjustment_set(&mut s,"monochrome",EffectValue::Toggle(true));}
        assert!(std::sync::Arc::ptr_eq(s.engine.document().layer(source_layer).unwrap().source.as_ref().unwrap(),&source));
        let document=s.engine.document().clone();let mut bytes=Vec::new();Project {document:document.clone()}.write(&mut bytes).unwrap();
        let reopened=Project::read(bytes.as_slice(),ProjectLimits::default()).unwrap();assert_eq!(reopened.document.layers,document.layers);
        assert_eq!(reopened.document.layer(source_layer).unwrap().source.as_deref(),Some(source.as_ref()));
    }
}
