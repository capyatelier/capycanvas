use crate::{EffectInstance,EffectValue,bundled_effect_catalog,color::RgbSpace};
use crate::levels::{CalibrationRole,LevelsStage,LevelsStatistics,auto_levels,calibrate_levels};

fn effect() -> EffectInstance {EffectInstance::new(bundled_effect_catalog().get("levels").unwrap().program())}
fn stage(effect:&EffectInstance,page:usize)->[f64;5] {
    let prefix=["","red_","green_","blue_"][page];
    ["black","white","gamma","output_black","output_white"].map(|key| match effect.value(&format!("{prefix}{key}")) {Some(EffectValue::Number(v))=>f64::from(*v),_=>panic!("missing {prefix}{key}")})
}
fn configure(effect:&mut EffectInstance,page:usize,values:[f32;5]) {
    let prefix=["","red_","green_","blue_"][page];
    for (key,value) in ["black","white","gamma","output_black","output_white"].into_iter().zip(values) {
        effect.set(&format!("{prefix}{key}"),EffectValue::Number(value)).unwrap();
    }
}
fn scalar([low,high,gamma,a,b]:[f64;5],x:f64,clamps:[bool;2])->f64 {
    let normalized=(x-low)/(high-low);
    let normalized=if clamps[0] {normalized.clamp(0.,1.)}else{normalized};
    let power=if gamma==1. {normalized}else{normalized.abs().powf(gamma.recip()).copysign(normalized)};
    let result=a+(b-a)*power;
    if clamps[1] {result.clamp(0.,1.)}else{result}
}
fn processed(effect:&EffectInstance,sample:[f32;3],space:RgbSpace)->[f64;3] {
    let clamps=["clamp_input","clamp_output"].map(|key|effect.value(key)==Some(&EffectValue::Toggle(true)));
    std::array::from_fn(|c|scalar(stage(effect,0),scalar(stage(effect,c+1),space.encode(f64::from(sample[c])),clamps),clamps))
}
fn close(a:f64,b:f64) {assert!((a-b).abs()<3e-5,"{a} != {b}");}

#[test]
fn levels_schema_has_four_neutral_pages_and_twenty_two_parameters() {
    let effect=effect();
    assert_eq!(effect.program.parameters.len(),22);
    assert_eq!(effect.program.pages.iter().map(|page|page.id.as_ref()).collect::<Vec<_>>(),["rgb","red","green","blue"]);
    for page in 0..4 {assert_eq!(stage(&effect,page),[0.,1.,1.,0.,1.]);}
    assert_eq!(effect.value("clamp_input"),Some(&EffectValue::Toggle(false)));
    assert_eq!(effect.value("clamp_output"),Some(&EffectValue::Toggle(false)));
}

#[test]
fn levels_scalar_signed_gamma_reversed_outputs_and_clamps_match_independent_reference() {
    let values=[0.13,0.87,1.7,0.91,0.04];
    for input in [-100.,-0.01,0.,0.37,1.,4.,100.] {for clamps in [[false,false],[true,false],[false,true],[true,true]] {
        close(LevelsStage(values).apply(input,clamps),scalar(values,input,clamps));
    }}
    let channel=[0.07,0.82,0.7,0.14,0.93];
    let master=[0.19,0.94,0.4,0.08,0.79];
    let x=0.4;
    let correct=scalar(master,scalar(channel,x,[false;2]),[false;2]);
    let reversed=scalar(channel,scalar(master,x,[false;2]),[false;2]);
    assert!((correct-reversed).abs()>0.02);
    close(LevelsStage(master).apply(LevelsStage(channel).apply(x,[false;2]),[false;2]),correct);
}

#[test]
fn levels_calibration_preserves_nonidentity_master_and_output_anchors_on_every_page() {
    for space in RgbSpace::ALL {for page in 0..4 {for role in [CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White] {
        let mut original=effect();
        configure(&mut original,0,[0.1,0.9,1.3,0.05,0.95]);
        for channel in 1..4 {configure(&mut original,channel,[0.02,0.98,1.1,0.,1.]);}
        let sample=[0.45,0.58,0.71].map(|x|space.decode(x) as f32);
        let before=processed(&original,sample,space);
        let weights=space.to_xyz()[1];
        let target=match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>before[1]+weights[0]*(before[0]-before[1])+weights[2]*(before[2]-before[1])};
        let frozen=original.clone();
        let candidate=calibrate_levels(&original,sample,space,page,role).unwrap();
        assert_eq!(original,frozen);
        assert_eq!(stage(&candidate,0),stage(&original,0));
        let actual=processed(&candidate,sample,space);
        for channel in 1..4 {
            let selected=page==0 || usize::from(page)==channel;
            if selected {close(actual[channel-1],target);assert_eq!(stage(&candidate,channel)[3..],stage(&original,channel)[3..]);}
            else {assert_eq!(stage(&candidate,channel),stage(&original,channel));}
        }
        candidate.validate().unwrap();
    }}}
}

#[test]
fn levels_calibration_handles_reversed_master_and_channel_outputs_without_resetting_them() {
    for role in [CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White] {
        let mut original=effect();
        for page in 0..4 {configure(&mut original,page,[0.,1.,1.,1.,0.]);}
        let sample=[0.35,0.5,0.7].map(|x|RgbSpace::Srgb.decode(x) as f32);
        let before=processed(&original,sample,RgbSpace::Srgb);
        let weights=RgbSpace::Srgb.to_xyz()[1];
        let target=match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>before[1]+weights[0]*(before[0]-before[1])+weights[2]*(before[2]-before[1])};
        let candidate=calibrate_levels(&original,sample,RgbSpace::Srgb,0,role).unwrap();
        for actual in processed(&candidate,sample,RgbSpace::Srgb) {close(actual,target);}
        for page in 0..4 {assert_eq!(stage(&candidate,page)[3..],[1.,0.]);}
        assert_eq!(stage(&candidate,0),stage(&original,0));
    }
}

#[test]
fn levels_unreachable_or_invalid_calibration_is_atomic() {
    for failure in 0..5 {
        let mut original=effect();
        let (sample,role)=match failure {
            0=>([f32::NAN,0.5,0.5],CalibrationRole::Gray),
            1=>{configure(&mut original,0,[0.,1.,1.,0.2,0.8]);original.set("clamp_input",EffectValue::Toggle(true)).unwrap();([0.5;3],CalibrationRole::Black)},
            2=>{configure(&mut original,3,[0.,1.,1.,0.5,0.5]);([0.5;3],CalibrationRole::Black)},
            3=>([-0.5,0.3,0.8],CalibrationRole::Gray),
            _=>([0.5;3],CalibrationRole::Gray),
        };
        let frozen=original.clone();
        assert!(calibrate_levels(&original,sample,RgbSpace::Srgb,if failure==4 {4}else{0},role).is_err());
        assert_eq!(original,frozen);
    }
}

fn statistics(minimum:[f64;3],maximum:[f64;3],positions:[[usize;3];2],counts:[u64;2])->LevelsStatistics {
    let mut bins:[Vec<u64>;3]=std::array::from_fn(|_|vec![0;4096]);
    for channel in 0..3 {for index in 0..2 {bins[channel][positions[index][channel]]+=counts[index];}}
    LevelsStatistics {minimum,maximum,bins,pixels:counts.into_iter().sum()}
}
#[test]
fn levels_auto_uses_nearest_rank_quantiles_and_only_changes_selected_input_stage() {
    let stats=statistics([0.,0.1,0.2],[0.8,0.9,1.],[[10,20,30],[3900,3910,3920]],[1,999]);
    let mut original=effect();
    for page in 0..4 {configure(&mut original,page,[0.,1.,1.7,0.1,0.9]);}
    original.set("clamp_input",EffectValue::Toggle(true)).unwrap();
    for page in 0..4 {
        let result=auto_levels(&original,&stats,page).unwrap();
        let selected:Vec<_>=if page==0 {(0..3).collect()}else{vec![page as usize-1]};
        let low=selected.iter().map(|&c|stats.minimum[c]+(stats.maximum[c]-stats.minimum[c])*(10.+10.*c as f64+0.5)/4096.).fold(f64::INFINITY,f64::min);
        let high=selected.iter().map(|&c|stats.minimum[c]+(stats.maximum[c]-stats.minimum[c])*(3900.+10.*c as f64+0.5)/4096.).fold(f64::NEG_INFINITY,f64::max);
        close(stage(&result,page as usize)[0],low);close(stage(&result,page as usize)[1],high);
        assert_eq!(stage(&result,page as usize)[2],1.);
        for other in 0..4 {if other!=usize::from(page) {assert_eq!(stage(&result,other),stage(&original,other));}}
        assert_eq!(stage(&result,page as usize)[3..],stage(&original,page as usize)[3..]);
        assert_eq!(result.value("clamp_input"),original.value("clamp_input"));
        assert_eq!(result.value("clamp_output"),original.value("clamp_output"));
    }
}

#[test]
fn levels_quantile_ranks_reject_bad_counts_and_keep_constant_channel_in_rgb_interval() {
    let mut stats=statistics([0.2,0.3,0.4],[0.2,0.8,0.9],[[0,0,0],[0,4095,4095]],[1,999]);
    assert!(stats.stretch(1).is_err());
    assert!(stats.stretch(0).unwrap()[1]>0.8);
    stats.maximum=stats.minimum;
    assert!(stats.stretch(0).is_ok());
    stats.minimum=[0.2;3];stats.maximum=[0.2;3];assert!(stats.stretch(0).is_err());
    for bad in 0..5 {
        let mut invalid=stats.clone();
        match bad {0=>invalid.pixels=0,1=>invalid.minimum[0]=f64::NAN,2=>invalid.bins[0].pop().map(|_|()).unwrap(),3=>invalid.bins[0][0]=u64::MAX,_=>invalid.pixels=(1<<30)+1}
        assert!(invalid.stretch(0).is_err());
    }
}

#[test]
fn levels_quantile_ceil_ranks_do_not_round_the_upper_tail_down() {
    let mut stats=statistics([0.;3],[1.;3],[[10;3],[4000;3]],[2,2]);
    for bins in &mut stats.bins {bins[2000]=997;}
    stats.pixels=1001;
    let [low,high]=stats.stretch(0).unwrap();
    close(low,10.5/4096.);close(high,4000.5/4096.);
    for bins in &mut stats.bins {bins[10]=1;bins[4000]=1;bins[2000]=999;}
    assert!(stats.stretch(0).is_err());
}

#[test]
fn levels_ordered_anchor_gap_remains_distinct_at_large_f32_magnitudes() {
    let mut effect=EffectInstance::new(bundled_effect_catalog().get("levels").unwrap().program().for_depth(crate::color::SampleDepth::F32));
    for page in 0..4 {
        let prefix=["","red_","green_","blue_"][page];
        let white=format!("{prefix}white");let black=format!("{prefix}black");
        for endpoint in [65504.,-65504.] {
            if endpoint>0. {effect.set(&white,EffectValue::Number(endpoint)).unwrap();effect.set(&black,EffectValue::Number(endpoint)).unwrap();}
            else {effect.set(&black,EffectValue::Number(endpoint)).unwrap();effect.set(&white,EffectValue::Number(endpoint)).unwrap();}
            effect.validate().unwrap();let values=stage(&effect,page);
            assert!(values[1]-values[0]>=0.001,"{prefix}: {}..{}",values[0],values[1]);
            let mut equal=effect.clone();
            let index=equal.program.parameters.iter().position(|parameter|parameter.key.as_ref()==black).unwrap();
            equal.values[index]=EffectValue::Number(values[1] as f32);
            assert!(equal.validate().is_err());
        }
    }
}

#[test]
fn levels_anchor_bounds_follow_document_depth_without_widening_gamma() {
    use crate::color::SampleDepth;
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        let program=bundled_effect_catalog().get("levels").unwrap().program().for_depth(depth);
        let expected=if depth.is_float() {(-65504.,65504.)}else{(0.,1.)};
        for parameter in program.parameters.iter() {
            if let crate::EffectParameterKind::Number {min,max,..}=&parameter.kind {
                if parameter.key.ends_with("gamma") {assert_eq!((*min,*max),(0.1,10.));}
                else {assert_eq!((*min,*max),expected,"{depth:?} {}",parameter.key);}
            }
        }
        let original=effect();let mut resolved=EffectInstance::new(program);
        assert_eq!(resolved.values,original.values);
        if depth.is_float() {
            resolved.set("red_output_black",EffectValue::Number(-65504.)).unwrap();
            resolved.set("blue_output_white",EffectValue::Number(32768.)).unwrap();
            for integer in [SampleDepth::U8,SampleDepth::U16] {
                let mut retained=resolved.clone();retained.program=resolved.program.for_depth(integer);
                assert_eq!(retained,resolved);assert!(retained.validate().is_ok());
            }
        }
        else {assert!(resolved.set("red_output_black",EffectValue::Number(-0.01)).is_err());}
    }
}
