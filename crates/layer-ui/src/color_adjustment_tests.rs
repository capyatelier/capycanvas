fn color_adjustment_session(id:&str)->UiSession<Recorder> {
    let mut s=session(Platform::Gtk);s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:id.into()}}).unwrap();s
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
