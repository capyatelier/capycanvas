use crate::{EffectInstance,EffectValue,color::RgbSpace,curve_value,curve_inverse,levels::CalibrationRole};

pub fn curve_point_between(value:f32,lower:f32,upper:f32)->Option<f32> {
    if !value.is_finite() || !lower.is_finite() || !upper.is_finite() {return None;}
    let first=lower.next_up();let last=upper.next_down();
    if first>last {return None;}
    let gap=((upper-lower)*0.25).min(0.001);
    let low=(lower+gap).max(first);let high=(upper-gap).min(last);
    Some(value.clamp(if low<=high {low}else{first},if low<=high {high}else{last}))
}
pub fn curve_reusable_knot(points:&[[f32;2]],x:f32)->Option<usize> {
    if x==0. {return Some(0);}
    if x==1. {return points.len().checked_sub(1);}
    points.iter().enumerate().skip(1).take(points.len().saturating_sub(2))
        .filter(|(_,p)|(p[0]-x).abs()<=0.002)
        .min_by(|a,b|(a.1[0]-x).abs().total_cmp(&(b.1[0]-x).abs()).then(a.0.cmp(&b.0))).map(|(i,_)|i)
}
pub fn curve_place_knot(points:&mut Vec<[f32;2]>,point:[f32;2])->Result<usize,&'static str> {
    if points.len()<2 || point.iter().any(|v|!v.is_finite() || !(0. ..=1.).contains(v)) {return Err("The sampled tone is outside the curve");}
    let x=point[0];
    if let Some(index)=curve_reusable_knot(points,x) {
        if index>0 && index+1<points.len() && curve_point_between(x,points[index-1][0],points[index+1][0])!=Some(x) {return Err("There is no room for this curve point");}
        points[index]=point;return Ok(index);
    }
    if points.len()>=32 || points.iter().any(|p|(p[0]-x).abs()<=0.002) {return Err("There is no room for this curve point");}
    let index=points.partition_point(|p|p[0]<x);points.insert(index,point);Ok(index)
}
fn curve(effect:&EffectInstance,page:u8)->Result<&[[f32;2]],&'static str> {
    match effect.value(&format!("curve_{page}")) {Some(EffectValue::Curve(points))=>Ok(points),_=>Err("Missing curve")}
}
fn transform(effect:&EffectInstance,space:RgbSpace,value:f64,encode:bool)->f64 {
    if effect.choice("domain")==Some("Log HDR") {
        let Some(EffectValue::Number(stops))=effect.value("hdr_stops") else {return f64::NAN;};
        if encode {crate::log_curve_encode(value,f64::from(*stops))} else {crate::log_curve_decode(value,f64::from(*stops))}
    } else if encode {space.encode(value)} else {space.decode(value)}
}
fn coordinates(effect:&EffectInstance,rgb:[f32;3],space:RgbSpace)->Result<[f32;3],&'static str> {
    let coordinates=rgb.map(|v|transform(effect,space,f64::from(v),true) as f32);
    if coordinates.iter().any(|v|!v.is_finite() || !(0. ..=1.).contains(v)) {return Err("The sampled tone is outside the curve");}
    Ok(coordinates)
}
pub fn calibrate_curves(effect:&EffectInstance,rgb:[f32;3],space:RgbSpace,page:u8,role:CalibrationRole)->Result<EffectInstance,&'static str> {
    effect.validate()?;if page>3 {return Err("Invalid curve channel");}
    let input=coordinates(effect,rgb,space)?;let master=curve(effect,0)?;
    let mut channel=[0.;3];let mut processed=[0.;3];
    for i in 0..3 {
        channel[i]=curve_value(curve(effect,i as u8+1)?,input[i]);
        processed[i]=transform(effect,space,f64::from(curve_value(master,channel[i])),false);
    }
    if processed.iter().any(|v|!v.is_finite()) {return Err("The adjustment cannot represent this tone");}
    let weights=space.to_xyz()[1];
    let target=match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>processed[1]+weights[0]*(processed[0]-processed[1])+weights[2]*(processed[2]-processed[1])};
    if !target.is_finite() || (role==CalibrationRole::Gray && target<=0.) {return Err("Choose a brighter neutral point");}
    let normalized=transform(effect,space,target,true);
    if !(0. ..=1.).contains(&normalized) {return Err("The target tone is outside the curve");}
    let mut result=effect.clone();
    for i in 0..3 {
        if page!=0 && usize::from(page)!=i+1 {continue;}
        let y=curve_inverse(master,normalized as f32,channel[i]).ok_or("The master curve cannot reach this tone")?;
        let mut points=curve(effect,i as u8+1)?.to_vec();curve_place_knot(&mut points,[input[i],y])?;
        result.set(&format!("curve_{}",i+1),EffectValue::Curve(points))?;
    }
    result.validate()?;
    for (i, &value) in input.iter().enumerate() {
        if page!=0 && usize::from(page)!=i+1 {continue;}
        let actual=curve_value(master,curve_value(curve(&result,i as u8+1)?,value));
        let linear=transform(effect,space,f64::from(actual),false);
        if !linear.is_finite() || (f64::from(actual)-normalized).abs()>f64::from(8.*f32::EPSILON)
            || (linear-target).abs()>2e-6f64.max(2e-4*target.abs()) {return Err("The curve cannot represent this correction");}
    }
    Ok(result)
}
pub fn targeted_curve_point(effect:&EffectInstance,rgb:[f32;3],space:RgbSpace,page:u8)->Result<(Vec<[f32;2]>,usize),&'static str> {
    effect.validate()?;if page>3 {return Err("Invalid curve channel");}
    if rgb.iter().any(|v|!v.is_finite()) {return Err("The sampled tone is outside the curve");}
    let x=if page==0 {
        let input=coordinates(effect,rgb,space)?;
        let mut channels=[0.;3];for i in 0..3 {channels[i]=transform(effect,space,f64::from(curve_value(curve(effect,i as u8+1)?,input[i])),false);}
        let w=space.to_xyz()[1];let y=channels[1]+w[0]*(channels[0]-channels[1])+w[2]*(channels[2]-channels[1]);
        transform(effect,space,y,true) as f32
    } else {transform(effect,space,f64::from(rgb[usize::from(page)-1]),true) as f32};
    if !x.is_finite() || !(0. ..=1.).contains(&x) {return Err("The sampled tone is outside the curve");}
    let original=curve(effect,page)?;let y=curve_value(original,x);let mut points=original.to_vec();
    let index=curve_place_knot(&mut points,[x,y])?;Ok((points,index))
}
