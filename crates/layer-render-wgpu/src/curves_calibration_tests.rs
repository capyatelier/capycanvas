use crate::{artwork_sample_tests::{doubled_effect, gpu, insert_effect, effect_draft, set_effect}, artwork_statistics_tests::generated, snapshot::CaptureControl};
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, Document, EffectInstance, EffectValue};
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::{curves::calibrate_curves, levels::CalibrationRole};

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual-expected).abs()<=tolerance,"{actual} != {expected}, tolerance {tolerance}");
}
fn sample(doc: &Document, source: ArtworkSource) -> [f32;4] {
    let result = pollster::block_on(gpu().artwork_sample(ArtworkSampleRequest::new(doc,source,[0.,0.],1),CaptureControl::default())).unwrap();
    let ArtworkSample::Color(rgba) = result else {panic!("missing sampled color: {result:?}")}; rgba
}
fn curve(effect: &EffectInstance, page: u8) -> &[[f32;2]] {
    match effect.value(&format!("curve_{page}")) {Some(EffectValue::Curve(points))=>points,_=>panic!("missing curve")}
}
fn set_curve(effect: &mut EffectInstance, page: u8, points: &[[f32;2]]) {
    effect.set(&format!("curve_{page}"),EffectValue::Curve(points.to_vec())).unwrap();
}
fn interpolate(points: &[[f32;2]], x: f64) -> f64 {
    let p: Vec<_> = points.iter().map(|v|v.map(f64::from)).collect();
    let h: Vec<_> = p.windows(2).map(|v|v[1][0]-v[0][0]).collect();
    let d: Vec<_> = p.windows(2).zip(&h).map(|(v,h)|(v[1][1]-v[0][1])/h).collect();
    let tangent: Vec<_> = (0..p.len()).map(|i| {
        if i==0 {return d[0];} if i==p.len()-1 {return d[i-1];}
        if d[i-1]==0. || d[i]==0. || d[i-1].signum()!=d[i].signum() {return 0.;}
        let weight=(h[i-1]+2.*h[i])/(3.*(h[i-1]+h[i]));
        1./(weight/d[i-1]+(1.-weight)/d[i])
    }).collect();
    let i=p.windows(2).position(|v|x<=v[1][0]).unwrap_or(p.len()-2);
    let t=(x-p[i][0])/h[i]; let t2=t*t; let t3=t2*t;
    (2.*t3-3.*t2+1.)*p[i][1]+(t3-2.*t2+t)*h[i]*tangent[i]
        +(-2.*t3+3.*t2)*p[i+1][1]+(t3-t2)*h[i]*tangent[i+1]
}
fn encode(effect: &EffectInstance, space: RgbSpace, value: f64) -> f64 {
    if effect.choice("domain")!=Some("Log HDR") {return space.encode(value);}
    let Some(EffectValue::Number(stops))=effect.value("hdr_stops") else {panic!("missing stops")};
    let span=f64::from(*stops)+8.; let toe=std::f64::consts::E/256.;
    if value<=toe {value/(toe*span*std::f64::consts::LN_2)} else {(value.log2()+8.)/span}
}
fn decode(effect: &EffectInstance, space: RgbSpace, value: f64) -> f64 {
    if effect.choice("domain")!=Some("Log HDR") {return space.decode(value);}
    let Some(EffectValue::Number(stops))=effect.value("hdr_stops") else {panic!("missing stops")};
    let span=f64::from(*stops)+8.;
    if value*span<=std::f64::consts::LOG2_E {value*span*std::f64::consts::LN_2*std::f64::consts::E/256.}
    else {2f64.powf(value*span)/256.}
}
fn processed(effect: &EffectInstance, space: RgbSpace, sample: [f32;3]) -> [f64;3] {
    std::array::from_fn(|i|decode(effect,space,interpolate(curve(effect,0),interpolate(curve(effect,i as u8+1),encode(effect,space,f64::from(sample[i]))))))
}
fn fixture(space: RgbSpace, depth: SampleDepth, rgb: [f32;3], alpha: f32, master: &[[f32;2]], log: bool) -> Document {
    let pixels=[[rgb[0]*alpha*0.5,rgb[1]*alpha*0.5,rgb[2]*alpha*0.5,alpha]];
    let mut doc=generated([1,1],DocumentColor {space,depth},&pixels);
    insert_effect(&mut doc,doubled_effect(),0);
    let mut effect=EffectInstance::new(crate::tests::fixture("curves").program().for_depth(depth));
    if log {effect.set("domain",EffectValue::Choice(1)).unwrap();effect.set("hdr_stops",EffectValue::Number(4.)).unwrap();}
    set_curve(&mut effect,0,master);
    for (page,points) in [(1,[[0.,0.],[1.,0.7]]),(2,[[0.,0.1],[1.,0.9]]),(3,[[0.,0.25],[1.,1.]])] {set_curve(&mut effect,page,&points);}
    insert_effect(&mut doc,effect,0);insert_effect(&mut doc,doubled_effect(),0);doc
}
fn calibrate_and_render(mut doc: Document, expected_input: [f32;3], alpha: f32) {
    let sampled=sample(&doc,ArtworkSource::EffectInput(doc.scene().children(None)[1]));
    for c in 0..3 {close(f64::from(sampled[c]),f64::from(expected_input[c]),3e-6*f64::from(expected_input[c]).abs().max(1.));}
    close(f64::from(sampled[3]),f64::from(alpha),2e-5*f64::from(alpha));
    let rgb=[sampled[0],sampled[1],sampled[2]];
    let upper=doc.scene().children(None)[0];let curves=doc.scene().children(None)[1];
    doc.artwork.occurrences.get_mut(upper).unwrap().visible=false;
    let original=effect_draft(&doc,curves);
    let oracle=processed(&original,doc.composition().color.space,rgb);
    let before=sample(&doc,ArtworkSource::Visible);
    for c in 0..3 {close(f64::from(before[c]),oracle[c],2e-5*oracle[c].abs().max(1.));}
    let w=doc.composition().color.space.to_xyz()[1]; let gray=oracle[1]+w[0]*(oracle[0]-oracle[1])+w[2]*(oracle[2]-oracle[1]);
    for role in [CalibrationRole::Black,CalibrationRole::White,CalibrationRole::Gray] {
        let target=match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>gray};
        let candidate=calibrate_curves(&original,rgb,doc.composition().color.space,0,role).unwrap();
        assert_eq!(curve(&candidate,0),curve(&original,0));
        set_effect(&mut doc,curves,candidate);
        let actual=sample(&doc,ArtworkSource::Visible);
        for c in 0..3 {close(f64::from(actual[c]),target,2e-6f64.max(2e-4*target.abs()));}
        close(f64::from(actual[3]),f64::from(alpha),2e-5*f64::from(alpha));
    }
}

#[test]
fn curves_gpu_calibration_preserves_master_and_reaches_targets_across_profiles_and_depths() {
    let masters=[vec![[0.,0.],[0.5,0.25],[1.,1.]],vec![[0.,1.],[1.,0.]],vec![[0.,1.],[0.25,0.],[0.5,1.],[0.75,0.],[1.,1.]]];
    for space in RgbSpace::ALL {for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {for master in &masters {
        let rgb=[0.15,0.35,0.65]; let doc=fixture(space,depth,rgb,0.5,master,false);
        calibrate_and_render(doc,rgb,0.5);
    }}}
}

#[test]
fn curves_gpu_log_calibration_preserves_linear_brightness_and_tiny_covered_alpha() {
    for space in RgbSpace::ALL {for alpha in [0.5,1e-30] {for master in [vec![[0.,0.],[0.5,0.25],[1.,1.]],vec![[0.,1.],[1.,0.]]] {
        let rgb=[0.002,0.2,4.];let doc=fixture(space,SampleDepth::F32,rgb,alpha,&master,true);
        calibrate_and_render(doc,rgb,alpha);
    }}}
}

#[test]
fn curves_gpu_individual_calibration_changes_only_selected_channel() {
    for space in RgbSpace::ALL {
        let mut doc=fixture(space,SampleDepth::F32,[0.15,0.35,0.65],0.5,&[[0.,1.],[1.,0.]],false);
        let source=sample(&doc,ArtworkSource::EffectInput(doc.scene().children(None)[1]));let rgb=[source[0],source[1],source[2]];
        let upper=doc.scene().children(None)[0];let curves=doc.scene().children(None)[1];
        doc.artwork.occurrences.get_mut(upper).unwrap().visible=false;
        let original=effect_draft(&doc,curves);let before=processed(&original,space,rgb);
        let candidate=calibrate_curves(&original,rgb,space,2,CalibrationRole::White).unwrap();
        for page in [0,1,3] {assert_eq!(curve(&candidate,page),curve(&original,page));}
        set_effect(&mut doc,curves,candidate);let actual=sample(&doc,ArtworkSource::Visible);
        for c in 0..3 {let expected=if c==1 {1.} else {before[c]};close(f64::from(actual[c]),expected,2e-6f64.max(2e-4*expected.abs()));}
        close(f64::from(actual[3]),0.5,1e-6);
    }
}
