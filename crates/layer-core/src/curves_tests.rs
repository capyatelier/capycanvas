use crate::{EffectInstance, EffectValue, bundled_effect_catalog, color::{RgbSpace, SampleDepth}, curve_inverse, curve_value};
use crate::curves::{calibrate_curves, curve_place_knot, curve_point_between, curve_reusable_knot, targeted_curve_point};
use crate::levels::CalibrationRole;

fn effect() -> EffectInstance {EffectInstance::new(bundled_effect_catalog().get("curves").unwrap().program())}
fn points(effect: &EffectInstance, page: u8) -> &[[f32; 2]] {
    match effect.value(&format!("curve_{page}")) {Some(EffectValue::Curve(value)) => value, _ => panic!("missing curve")}
}
fn set_curve(effect: &mut EffectInstance, page: u8, points: &[[f32; 2]]) {
    effect.set(&format!("curve_{page}"), EffectValue::Curve(points.to_vec())).unwrap();
}
fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() <= tolerance, "{actual} != {expected}, tolerance {tolerance}");
}
fn hermite(points: &[[f32; 2]], x: f64) -> f64 {
    let knots: Vec<[f64; 2]> = points.iter().map(|p| p.map(f64::from)).collect();
    let widths: Vec<f64> = knots.windows(2).map(|p| p[1][0]-p[0][0]).collect();
    let secants: Vec<f64> = knots.windows(2).zip(&widths).map(|(p, h)| (p[1][1]-p[0][1])/h).collect();
    let derivatives: Vec<f64> = (0..knots.len()).map(|i| {
        if i == 0 {return secants[0];}
        if i == knots.len()-1 {return secants[i-1];}
        if secants[i-1].signum() != secants[i].signum() || secants[i-1] == 0. || secants[i] == 0. {return 0.;}
        let fraction = (2.*widths[i]+widths[i-1])/(3.*(widths[i]+widths[i-1]));
        1./(fraction/secants[i-1]+(1.-fraction)/secants[i])
    }).collect();
    let i = knots.windows(2).position(|p| x <= p[1][0]).unwrap_or(knots.len()-2);
    let t = (x-knots[i][0])/widths[i];
    if t <= 0. {return knots[i][1]+(x-knots[i][0])*derivatives[i];}
    if t >= 1. {return knots[i+1][1]+(x-knots[i+1][0])*derivatives[i+1];}
    let t2 = t*t; let t3 = t2*t;
    (2.*t3-3.*t2+1.)*knots[i][1] + (t3-2.*t2+t)*widths[i]*derivatives[i]
        + (-2.*t3+3.*t2)*knots[i+1][1] + (t3-t2)*widths[i]*derivatives[i+1]
}
fn encode(effect: &EffectInstance, space: RgbSpace, value: f64) -> f64 {
    if effect.choice("domain") != Some("Log HDR") {return space.encode(value);}
    let Some(EffectValue::Number(stops)) = effect.value("hdr_stops") else {panic!("missing stops")};
    let span = f64::from(*stops)+8.;
    let toe = std::f64::consts::E/256.;
    if value <= toe {value/(toe*std::f64::consts::LN_2*span)} else {(value.log2()+8.)/span}
}
fn decode(effect: &EffectInstance, space: RgbSpace, value: f64) -> f64 {
    if effect.choice("domain") != Some("Log HDR") {return space.decode(value);}
    let Some(EffectValue::Number(stops)) = effect.value("hdr_stops") else {panic!("missing stops")};
    let span = f64::from(*stops)+8.;
    if value*span <= std::f64::consts::LOG2_E {value*span*std::f64::consts::LN_2*std::f64::consts::E/256.}
    else {2f64.powf(value*span)/256.}
}
fn processed(effect: &EffectInstance, sample: [f32; 3], space: RgbSpace) -> [f64; 3] {
    std::array::from_fn(|i| {
        let x = encode(effect, space, f64::from(sample[i])) as f32;
        let channel = hermite(points(effect, i as u8+1), f64::from(x)) as f32;
        let master = hermite(points(effect, 0), f64::from(channel)) as f32;
        decode(effect, space, f64::from(master))
    })
}
fn luminance(rgb: [f64; 3], space: RgbSpace) -> f64 {
    let w = space.to_xyz()[1]; rgb[1]+w[0]*(rgb[0]-rgb[1])+w[2]*(rgb[2]-rgb[1])
}
fn log_effect(stops: f32) -> EffectInstance {
    let mut effect = EffectInstance::new(bundled_effect_catalog().get("curves").unwrap().program().for_depth(SampleDepth::F32));
    let crate::EffectParameterKind::Choice {options} = &effect.program.parameters.iter().find(|p| p.key.as_ref() == "domain").unwrap().kind else {panic!("missing domain")};
    let index = options.iter().position(|o| o.value() == "Log HDR").unwrap();
    effect.set("domain", EffectValue::Choice(index as u32)).unwrap();
    effect.set("hdr_stops", EffectValue::Number(stops)).unwrap(); effect
}

#[test]
fn inverse_enumerates_w_and_m_segments_and_chooses_nearest_root() {
    for ys in [[1.,0.,1.,0.,1.], [0.,1.,0.,1.,0.]] {
        let curve: Vec<_> = ys.into_iter().enumerate().map(|(i,y)| [i as f32/4.,y]).collect();
        for (current, expected) in [(0.4,0.375),(0.6,0.625),(0.5,0.375)] {
            let root = curve_inverse(&curve,0.5,current).unwrap();
            assert_eq!(root,expected); near(hermite(&curve,f64::from(root)),0.5,1e-12);
        }
    }
}

#[test]
fn inverse_increasing_decreasing_and_closed_endpoint_roots() {
    for curve in [[[0.,0.1],[1.,0.9]],[[0.,0.9],[1.,0.1]]] {
        for target in [0.1,0.2,0.5,0.8,0.9] {
            let root = curve_inverse(&curve,target,0.87).unwrap();
            let expected = (f64::from(target)-f64::from(curve[0][1]))/(f64::from(curve[1][1])-f64::from(curve[0][1]));
            near(f64::from(root),expected,2e-7);
            near(hermite(&curve,f64::from(root)),f64::from(target),2e-7);
        }
        assert_eq!(curve_inverse(&curve,curve[0][1],0.8),Some(0.));
        assert_eq!(curve_inverse(&curve,curve[1][1],0.2),Some(1.));
    }
}

#[test]
fn inverse_plateau_projects_current_output_and_refuses_unreachable_targets() {
    let plateau = [[0.,0.],[0.25,0.4],[0.75,0.4],[1.,1.]];
    for (current, expected) in [(0.1,0.25),(0.37,0.37),(0.9,0.75)] {
        assert_eq!(curve_inverse(&plateau,0.4,current),Some(expected));
    }
    let bounded = [[0.,0.2],[0.25,0.8],[0.75,0.3],[1.,0.6]];
    for target in [0.,1.,f32::NAN,f32::INFINITY] {assert_eq!(curve_inverse(&bounded,target,0.4),None);}
    assert_eq!(curve_inverse(&bounded,0.5,f32::NAN),None);
}

#[test]
fn inverse_checks_authoritative_float32_root_instead_of_returning_rounded_approximation() {
    let x = 0.5f32;
    let curve = [[0.,0.],[x,0.],[x.next_up(),1.],[1.,1.]];
    assert_eq!(curve_inverse(&curve,0.5,x),None);
    assert_eq!(curve_inverse(&curve,0.,x),Some(x));
    assert_eq!(curve_inverse(&curve,1.,x),Some(x.next_up()));
    let linear = [[0.,0.],[1.,1.]];
    for target in [0.12345679,0.99999994,f32::from_bits(1)] {
        let root = curve_inverse(&linear,target,0.2).unwrap();
        near(f64::from(root),f64::from(target),f64::from(8.*f32::EPSILON));
    }
}

#[test]
fn curve_scalar_matches_independent_hermite_basis_with_nonuniform_and_reversed_knots() {
    for curve in [vec![[0.,0.02],[0.07,0.3],[0.4,0.8],[0.83,0.2],[1.,0.7]], vec![[0.,1.],[0.2,0.7],[0.6,0.3],[1.,0.]]] {
        for i in 0..257 {let x = i as f32/256.; near(f64::from(curve_value(&curve,x)),hermite(&curve,f64::from(x)),8e-7);}
    }
}

#[test]
fn knot_placement_moves_exact_endpoints_and_refuses_near_endpoint_insertion() {
    for (x,index) in [(0.,0),(1.,1)] {
        let mut curve = vec![[0.,0.],[1.,1.]];
        assert_eq!(curve_place_knot(&mut curve,[x,0.37]),Ok(index)); assert_eq!(curve[index],[x,0.37]);
    }
    for x in [0.001,0.002,0.999,0.9981] {
        let mut curve = vec![[0.,0.],[1.,1.]]; let original = curve.clone();
        assert!(curve_place_knot(&mut curve,[x,0.37]).is_err()); assert_eq!(curve,original);
    }
}

#[test]
fn reusable_knot_is_nearest_interior_with_lower_x_tie_and_exact_sample_coordinate() {
    let curve = [[0.,0.],[0.4990234375,0.3],[0.5009765625,0.7],[1.,1.]];
    assert_eq!(curve_reusable_knot(&curve,0.5),Some(1));
    assert_eq!(curve_reusable_knot(&curve,0.5008),Some(2));
    let mut crowded = curve.to_vec(); assert!(curve_place_knot(&mut crowded,[0.5,0.4]).is_err()); assert_eq!(crowded,curve);
    let mut moved = vec![curve[0],curve[1],curve[3]]; assert_eq!(curve_place_knot(&mut moved,[0.5,0.4]),Ok(1));
    assert_eq!(moved[1],[0.5,0.4]);
    let mut near_endpoint = vec![[0.,0.],[0.0015,0.2],[0.5,0.5],[1.,1.]];
    assert_eq!(curve_place_knot(&mut near_endpoint,[0.001,0.3]),Ok(1));
    let original = near_endpoint.clone();
    assert!(curve_place_knot(&mut near_endpoint,[0.0005,0.3]).is_err()); assert_eq!(near_endpoint,original);
}

#[test]
fn adjacent_float32_neighbors_refuse_horizontal_motion_without_collapsing_knots() {
    let lower = 0.5f32; let adjacent = lower.next_up(); let upper = adjacent.next_up();
    assert_eq!(curve_point_between(0.8,lower,adjacent),None);
    assert_eq!(curve_point_between(0.8,lower,upper),Some(adjacent));
    let mut curve = vec![[0.,0.],[lower,0.2],[adjacent,0.5],[upper,0.8],[1.,1.]];
    let original = curve.clone();
    assert!(curve_place_knot(&mut curve,[upper.next_up(),0.4]).is_err()); assert_eq!(curve,original);
    assert_eq!(curve_place_knot(&mut curve,[adjacent,0.4]),Ok(2));
    assert!(curve.windows(2).all(|p|p[0][0]<p[1][0]));
}

#[test]
fn full_table_reuses_a_knot_but_never_deletes_to_admit_a_new_one() {
    let mut curve: Vec<_> = (0..32).map(|i|[i as f32/31.,i as f32/31.]).collect();
    let x = curve[15][0]; assert_eq!(curve_place_knot(&mut curve,[x,0.7]),Ok(15));
    let original = curve.clone();
    assert!(curve_place_knot(&mut curve,[0.5,0.4]).is_err()); assert_eq!(curve,original); assert_eq!(curve.len(),32);
}

#[test]
fn calibration_reaches_black_white_and_gray_without_resetting_master_or_unselected_channels() {
    for space in RgbSpace::ALL {for page in 0..4 {for role in [CalibrationRole::Black,CalibrationRole::White,CalibrationRole::Gray] {
        let mut original = effect();
        set_curve(&mut original,0,&[[0.,0.],[0.3,0.12],[0.7,0.88],[1.,1.]]);
        for channel in 1..4 {set_curve(&mut original,channel,&[[0.,0.],[0.25,0.15+channel as f32*0.04],[0.8,0.91],[1.,1.]]);}
        let sample = [0.37,0.53,0.68].map(|x|space.decode(x) as f32);
        let before = processed(&original,sample,space);
        let target = match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>luminance(before,space)};
        let frozen = original.clone(); let candidate = calibrate_curves(&original,sample,space,page,role).unwrap();
        assert_eq!(original,frozen); assert_eq!(points(&candidate,0),points(&original,0));
        let actual = processed(&candidate,sample,space);
        for channel in 1..4 {if page == 0 || page == channel {
            let value = actual[usize::from(channel)-1];
            near(value,target,2e-6f64.max(2e-4*target.abs()));
            near(encode(&candidate,space,value),encode(&candidate,space,target),f64::from(8.*f32::EPSILON));
            let x = space.encode(f64::from(sample[usize::from(channel)-1])) as f32;
            assert!(points(&candidate,channel).iter().any(|p|p[0]==x));
        } else {assert_eq!(points(&candidate,channel),points(&original,channel));}}
        candidate.validate().unwrap();
    }}}
}

#[test]
fn calibration_handles_decreasing_and_multiple_root_masters() {
    for master in [vec![[0.,1.],[1.,0.]],vec![[0.,1.],[0.25,0.],[0.5,1.],[0.75,0.],[1.,1.]]] {
        let mut original = effect(); set_curve(&mut original,0,&master);
        let sample = [0.35,0.6,0.8].map(|x|RgbSpace::Srgb.decode(x) as f32);
        for role in [CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White] {
            let target = match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>luminance(processed(&original,sample,RgbSpace::Srgb),RgbSpace::Srgb)};
            let candidate = calibrate_curves(&original,sample,RgbSpace::Srgb,0,role).unwrap();
            assert_eq!(points(&candidate,0),master);
            for value in processed(&candidate,sample,RgbSpace::Srgb) {near(value,target,2e-6f64.max(2e-4*target));}
        }
    }
}

#[test]
fn calibration_at_endpoints_only_changes_endpoint_outputs() {
    for (sample,role,target) in [(0.,CalibrationRole::White,1.),(1.,CalibrationRole::Black,0.)] {
        let original = effect(); let candidate = calibrate_curves(&original,[sample;3],RgbSpace::Srgb,0,role).unwrap();
        for page in 1..4 {assert_eq!(points(&candidate,page).len(),2); assert_eq!(points(&candidate,page)[sample as usize],[sample,target]);}
    }
}

#[test]
fn refused_rgb_calibration_keeps_all_channels_and_master_unchanged() {
    for failure in 0..7 {
        let mut original = effect();
        let (sample,role,page) = match failure {
            0 => ([0.3,f32::NAN,0.7],CalibrationRole::Gray,0),
            1 => ([-0.1,0.4,0.7],CalibrationRole::Black,0),
            2 => ([0.3,0.4,f32::INFINITY],CalibrationRole::White,0),
            3 => {set_curve(&mut original,0,&[[0.,0.2],[1.,0.8]]); ([0.3,0.4,0.7],CalibrationRole::Black,0)},
            4 => ([0.;3],CalibrationRole::Gray,0),
            5 => ([0.3;3],CalibrationRole::Gray,4),
            _ => {let full: Vec<_> = (0..32).map(|i|[i as f32/31.,i as f32/31.]).collect(); set_curve(&mut original,3,&full); ([0.3,0.4,0.5].map(|x|RgbSpace::Srgb.decode(x) as f32),CalibrationRole::White,0)},
        };
        let frozen = original.clone(); assert!(calibrate_curves(&original,sample,RgbSpace::Srgb,page,role).is_err()); assert_eq!(original,frozen);
    }
}

#[test]
fn calibration_refuses_master_roots_between_adjacent_float32_values_atomically() {
    let mut original = effect(); let x = 0.5f32;
    set_curve(&mut original,0,&[[0.,0.],[x,0.],[x.next_up(),1.],[1.,1.]]);
    let sample = [0.4,0.6,0.8].map(|x|RgbSpace::Srgb.decode(x) as f32);
    let target = luminance(processed(&original,sample,RgbSpace::Srgb),RgbSpace::Srgb);
    assert!(target>0. && target<1.);
    let frozen = original.clone();
    assert!(calibrate_curves(&original,sample,RgbSpace::Srgb,0,CalibrationRole::Gray).is_err());
    assert_eq!(original,frozen);
}

#[test]
fn log_axis_toe_knee_white_and_range127_match_physical_reference() {
    for stops in [0.,4.,16.,127.] {
        let original = log_effect(stops); let toe = std::f64::consts::E/256.; let knee = std::f64::consts::LOG2_E/(f64::from(stops)+8.);
        for value in [0.,f64::from(f32::from_bits(1)),toe/2.,toe,toe*2.,1.,2f64.powf(f64::from(stops))] {
            let coordinate = crate::log_curve_encode(value,f64::from(stops));
            near(coordinate,encode(&original,RgbSpace::Srgb,value),2e-15);
            near(crate::log_curve_decode(coordinate,f64::from(stops)),value,1e-12*value.max(1.));
        }
        near(crate::log_curve_encode(toe,f64::from(stops)),knee,2e-15);
        near(crate::log_curve_encode(1.,f64::from(stops)),8./(f64::from(stops)+8.),2e-15);
        assert_eq!(crate::log_curve_encode(2f64.powf(f64::from(stops)),f64::from(stops)),1.);
    }
}

#[test]
fn log_calibration_preserves_processed_linear_brightness_and_physical_error_bound() {
    for stops in [4.,127.] {for space in RgbSpace::ALL {
        let mut original = log_effect(stops);
        set_curve(&mut original,0,&[[0.,0.],[0.4,0.33],[0.8,0.88],[1.,1.]]);
        for channel in 1..4 {set_curve(&mut original,channel,&[[0.,0.],[0.25,0.2+channel as f32*0.03],[1.,1.]]);}
        let sample = if stops == 127. {[2f32.powi(109),2f32.powi(113),2f32.powi(117)]} else {[0.002,0.02,3.]};
        let target = luminance(processed(&original,sample,space),space);
        let candidate = calibrate_curves(&original,sample,space,0,CalibrationRole::Gray).unwrap();
        assert_eq!(points(&candidate,0),points(&original,0));
        for actual in processed(&candidate,sample,space) {
            near(actual,target,2e-6f64.max(2e-4*target));
            near(encode(&candidate,space,actual),encode(&candidate,space,target),f64::from(8.*f32::EPSILON));
        }
        for role in [CalibrationRole::Black,CalibrationRole::White] {
            let target = if role == CalibrationRole::Black {0.} else {1.};
            let candidate = calibrate_curves(&original,sample,space,0,role).unwrap();
            for actual in processed(&candidate,sample,space) {near(actual,target,2e-6f64.max(2e-4*target));}
        }
    }}
}

#[test]
fn targeted_rgb_uses_linear_luminance_after_channels_and_individual_pages_use_source() {
    for space in RgbSpace::ALL {for log in [false,true] {
        let mut original = if log {log_effect(4.)} else {effect()};
        set_curve(&mut original,0,&[[0.,0.],[0.4,0.15],[0.8,0.93],[1.,1.]]);
        for channel in 1..4 {set_curve(&mut original,channel,&[[0.,0.],[0.35,0.2+channel as f32*0.13],[1.,1.]]);}
        let sample = [0.3,0.5,0.7].map(|x|decode(&original,space,x) as f32);
        let input = sample.map(|v|encode(&original,space,f64::from(v)) as f32);
        let channels: [f64;3] = std::array::from_fn(|i|decode(&original,space,f64::from(hermite(points(&original,i as u8+1),f64::from(input[i])) as f32)));
        let expected = encode(&original,space,luminance(channels,space)) as f32;
        let encoded_average = luminance(std::array::from_fn(|i|hermite(points(&original,i as u8+1),f64::from(input[i]))),space);
        assert!((f64::from(expected)-encoded_average).abs()>0.005);
        for page in 0..4 {
            let (curve,index) = targeted_curve_point(&original,sample,space,page).unwrap();
            let x = if page == 0 {expected} else {input[usize::from(page)-1]};
            assert_eq!(curve[index][0],x); near(f64::from(curve[index][1]),hermite(points(&original,page),f64::from(x)),8e-7);
        }
    }}
}

#[test]
fn targeted_reused_knot_starts_at_original_curve_value_at_fixed_sample_x() {
    let mut original = effect(); set_curve(&mut original,1,&[[0.,0.],[0.5,0.1],[1.,1.]]);
    let sample = [0.501,0.3,0.4].map(|x|RgbSpace::Srgb.decode(x) as f32);
    let x = RgbSpace::Srgb.encode(f64::from(sample[0])) as f32;
    let (curve,index) = targeted_curve_point(&original,sample,RgbSpace::Srgb,1).unwrap();
    assert_eq!(curve.len(),3); assert_eq!(index,1); assert_eq!(curve[index][0],x);
    assert_ne!(curve[index][1],points(&original,1)[1][1]);
    near(f64::from(curve[index][1]),hermite(points(&original,1),f64::from(x)),8e-8);
    assert_eq!(points(&original,1)[1],[0.5,0.1]);
}

#[test]
fn targeted_rejects_nonfinite_outside_and_endpoint_near_samples_atomically() {
    for encoded in [[0.001;3],[-0.1,0.5,0.5],[1.1,0.5,0.5],[f64::NAN,0.5,0.5]] {
        let original = effect(); let frozen = original.clone(); let sample = encoded.map(|x|RgbSpace::Srgb.decode(x) as f32);
        assert!(targeted_curve_point(&original,sample,RgbSpace::Srgb,1).is_err()); assert_eq!(original,frozen);
    }
}

#[test]
fn targeted_individual_channels_admit_finite_extended_other_channels_but_reject_invalid_input() {
    for space in RgbSpace::ALL {for log in [false,true] {for page in 1..4 {
        let original=if log {log_effect(4.)} else {effect()};let frozen=original.clone();
        let mut sample=if log {[64.,-1.,32.]} else {[2.,-1.,3.]};
        let selected=usize::from(page)-1;sample[selected]=0.25;
        let (curve,index)=targeted_curve_point(&original,sample,space,page).unwrap();
        let expected=encode(&original,space,0.25) as f32;
        assert_eq!(curve[index],[expected,expected]);assert_eq!(original,frozen);
        assert!(targeted_curve_point(&original,sample,space,0).is_err());
        for value in [-1.,if log {64.} else {2.}] {
            let mut outside=sample;outside[selected]=value;
            assert!(targeted_curve_point(&original,outside,space,page).is_err());
        }
        for value in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY] {
            let mut invalid=sample;invalid[usize::from(page)%3]=value;
            assert!(targeted_curve_point(&original,invalid,space,page).is_err());
        }
        assert_eq!(original,frozen);
    }}}
}
