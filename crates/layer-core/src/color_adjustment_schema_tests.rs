use crate::{bundled_effect_catalog,EffectInstance,EffectValue,EffectParameterKind};
use crate::color::{RgbSpace,SampleDepth};

fn color_effect(id:&str)->EffectInstance {EffectInstance::new(bundled_effect_catalog().get(id).unwrap().program())}

#[test]
fn color_adjustment_schemas_have_independent_ranges_and_tagged_defaults() {
    let hue=color_effect("hue_saturation");assert_eq!(hue.program.parameters.len(),42);assert_eq!(hue.program.pages.len(),7);
    for (prefix,center) in ["reds","yellows","greens","cyans","blues","magentas"].into_iter().zip([30.,110.,145.,195.,265.,330.]) {
        for (suffix,value) in [("hue",0.),("saturation",0.),("lightness",0.),("center",center),("width",30.),("feather",30.)] {
            assert_eq!(hue.value(&format!("{prefix}_{suffix}")),Some(&EffectValue::Number(value)));
        }
    }
    assert_eq!(hue.value("colorize"),Some(&EffectValue::Toggle(false)));
    assert_eq!(hue.value("colorize_saturation"),Some(&EffectValue::Number(25.)));
    for id in ["invert","desaturate"] {assert!(color_effect(id).program.parameters.is_empty());}
    let photo=color_effect("photo_filter");assert_eq!(photo.program.parameters.len(),3);
    assert_eq!(photo.value("density"),Some(&EffectValue::Number(25.)));
    assert_eq!(photo.value("preserve_luminance"),Some(&EffectValue::Toggle(true)));
    let Some(EffectValue::Color(color))=photo.value("color") else {panic!("missing tagged color")};
    assert_eq!(color.space,RgbSpace::Srgb);assert_eq!(color.rgba,[1.,0.72,0.45,1.]);
}

#[test]
fn hue_angular_bounds_and_independent_range_limits_reject_invalid_values_atomically() {
    let mut hue=color_effect("hue_saturation");
    for key in ["reds_center","colorize_hue"] {
        assert!(hue.set(key,EffectValue::Number(360f32.next_down())).is_ok());
        let before=hue.clone();assert!(hue.set(key,EffectValue::Number(360.)).is_err());assert_eq!(hue,before);
    }
    for (key,value) in [("reds_width",180.),("reds_feather",90.)] {hue.set(key,EffectValue::Number(value)).unwrap();}
    let before=hue.clone();assert!(hue.set("reds_feather",EffectValue::Number(90f32.next_up())).is_err());assert_eq!(hue,before);
}

#[test]
fn threshold_depth_bounds_preserve_existing_float_schema_without_clamping() {
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        let mut threshold=EffectInstance::new(color_effect("threshold").program.for_depth(depth));
        assert_eq!(threshold.value("threshold"),Some(&EffectValue::Number(0.5)));
        let parameter=&threshold.program.parameters[0];assert_eq!(parameter.soft_bounds,Some([0.,1.]));
        let EffectParameterKind::Number {min,max,..}=parameter.kind else {panic!("threshold must be numeric")};
        assert_eq!((min,max),if depth.is_float(){(-65504.,65504.)}else{(0.,1.)});
        if depth.is_float() {
            threshold.set("threshold",EffectValue::Number(-32768.)).unwrap();
            for integer in [SampleDepth::U8,SampleDepth::U16] {
                let mut converted=threshold.clone();converted.program=converted.program.for_depth(integer);
                assert_eq!(converted,threshold);assert!(converted.validate().is_ok());
            }
        } else {assert!(threshold.set("threshold",EffectValue::Number(-0.01)).is_err());}
    }
}

fn color_bounds(effect:&EffectInstance,key:&str)->(f64,f64) {
    let parameter=effect.program.parameters.iter().find(|parameter|parameter.key.as_ref()==key).unwrap();
    let EffectParameterKind::Number {min,max,..}=parameter.kind else {panic!("expected number {key}")};(f64::from(min),f64::from(max))
}

#[test]
fn selective_color_schema_is_neutral_with_independent_bounded_ink_pages() {
    let mut effect=color_effect("selective_color");let pages=["reds","yellows","greens","cyans","blues","magentas","whites","neutrals","blacks"];
    assert_eq!(effect.program.parameters.len(),37);
    assert_eq!(effect.program.pages.iter().map(|page|page.id.as_ref()).collect::<Vec<_>>(),pages);
    assert_eq!(effect.value("mode"),Some(&EffectValue::Choice(0)));
    for page in pages {for ink in ["cyan","magenta","yellow","black"] {
        let key=format!("{page}_{ink}");assert_eq!(effect.value(&key),Some(&EffectValue::Number(0.)));assert_eq!(color_bounds(&effect,&key),(-100.,100.));
        for value in [-100.,100.] {effect.set(&key,EffectValue::Number(value)).unwrap();}
        let before=effect.clone();assert!(effect.set(&key,EffectValue::Number(100f32.next_up())).is_err());assert_eq!(effect,before);
    }}
    effect.set("mode",EffectValue::Choice(1)).unwrap();let before=effect.clone();assert!(effect.set("mode",EffectValue::Choice(2)).is_err());assert_eq!(effect,before);
}

#[test]
fn channel_mixer_schema_has_identity_rgb_and_explicit_gray_defaults() {
    let mut effect=color_effect("channel_mixer");assert_eq!(effect.program.parameters.len(),17);
    assert_eq!(effect.program.pages.iter().map(|page|page.id.as_ref()).collect::<Vec<_>>(),["red","green","blue","gray"]);
    assert_eq!(effect.value("monochrome"),Some(&EffectValue::Toggle(false)));
    for (row,page) in ["red","green","blue","gray"].into_iter().enumerate() {for (column,input) in ["red","green","blue","constant"].into_iter().enumerate() {
        let key=format!("{page}_{input}");let expected=if row==3 {[21.26,71.52,7.22,0.][column]}else if row==column {100.}else{0.};
        assert_eq!(effect.value(&key),Some(&EffectValue::Number(expected)));
        let limit=if input=="constant" {100.}else{200.};assert_eq!(color_bounds(&effect,&key),(-f64::from(limit),f64::from(limit)));
        for value in [-limit,limit] {effect.set(&key,EffectValue::Number(value)).unwrap();}
        let before=effect.clone();assert!(effect.set(&key,EffectValue::Number(limit.next_up())).is_err());assert_eq!(effect,before);
    }}
}
