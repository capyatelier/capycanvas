use crate::{EffectInstance, EffectValue, color::RgbSpace};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum CalibrationRole { Black, Gray, White }

const LEVELS_PREFIXES: [&str;4] = ["","red_","green_","blue_"];
const LEVELS_KEYS: [&str;5] = ["black","white","gamma","output_black","output_white"];

#[derive(Clone, Copy, Debug)]
pub struct LevelsStage(pub [f64;5]);
impl LevelsStage {
    pub fn read(effect:&EffectInstance,page:usize)->Result<Self,&'static str> {
        let prefix=LEVELS_PREFIXES.get(page).ok_or("Invalid Levels channel")?;
        let mut values=[0.;5];
        for (index,key) in LEVELS_KEYS.iter().enumerate() {
            let Some(EffectValue::Number(value))=effect.value(&format!("{prefix}{key}")) else {return Err("Missing Levels value");};
            values[index]=f64::from(*value);
        }
        Ok(Self(values))
    }
    pub fn apply(self,x:f64,clamps:[bool;2])->f64 {
        let [low,high,gamma,a,b]=self.0;
        let mut u=(x-low)/(high-low);
        if clamps[0] {u=u.clamp(0.,1.);}
        let v=if gamma==1. {u} else {u.signum()*u.abs().powf(1./gamma)};
        let y=a+(b-a)*v;
        if clamps[1] {y.clamp(0.,1.)} else {y}
    }
    fn inverse(self,y:f64,clamps:[bool;2])->Option<f64> {
        let [low,high,gamma,a,b]=self.0;
        if a==b || high<=low {return None;}
        let v=(y-a)/(b-a);let x=low+(high-low)*v.signum()*v.abs().powf(gamma);
        (x.is_finite() && close(self.apply(x,clamps),y)).then_some(x)
    }
}
fn close(actual:f64,target:f64)->bool {actual.is_finite() && (actual-target).abs()<=2e-6f64.max(2e-4*target.abs())}
fn levels_clamps(effect:&EffectInstance)->[bool;2] {
    ["clamp_input","clamp_output"].map(|key|effect.value(key)==Some(&EffectValue::Toggle(true)))
}
fn replace(effect:&mut EffectInstance,key:&str,value:f64)->Result<(),&'static str> {
    let value=value as f32;
    if !value.is_finite() {return Err("The correction is outside the adjustment range");}
    let index=effect.program.parameters.iter().position(|parameter|parameter.key.as_ref()==key).ok_or("Missing adjustment value")?;
    effect.program.parameters[index].validate(&EffectValue::Number(value))?;
    effect.values[index]=EffectValue::Number(value);Ok(())
}

pub fn calibrate_levels(effect:&EffectInstance,rgb:[f32;3],space:RgbSpace,page:u8,role:CalibrationRole)->Result<EffectInstance,&'static str> {
    if page>3 || rgb.iter().any(|value|!value.is_finite()) {return Err("Invalid Levels sample");}
    let source=rgb.map(|value|space.encode(f64::from(value)));
    let master=LevelsStage::read(effect,0)?;
    let channels=[LevelsStage::read(effect,1)?,LevelsStage::read(effect,2)?,LevelsStage::read(effect,3)?];
    let clamps=levels_clamps(effect);
    let processed=std::array::from_fn::<_,3,_>(|i|master.apply(channels[i].apply(source[i],clamps),clamps));
    let weights=space.to_xyz()[1];
    let target=match role {CalibrationRole::Black=>0.,CalibrationRole::White=>1.,CalibrationRole::Gray=>
        processed[1]+weights[0]*(processed[0]-processed[1])+weights[2]*(processed[2]-processed[1])};
    if !target.is_finite() || processed.iter().any(|value|!value.is_finite()) {return Err("This tone cannot be calibrated");}
    let z=master.inverse(target,clamps).ok_or("The master adjustment cannot reach this tone")?;
    let mut result=effect.clone();
    for index in 0..3 {
        if page!=0 && usize::from(page)!=index+1 {continue;}
        if close(processed[index],target) {continue;}
        let [low,high,gamma,a,b]=channels[index].0;
        if a==b {return Err("The channel output is constant");}
        let x=source[index];let v=(z-a)/(b-a);let q=v.signum()*v.abs().powf(gamma);
        let (key,value)=match role {
            CalibrationRole::Black=>("black",(x-q*high)/(1.-q)),
            CalibrationRole::White=>("white",low+(x-low)/q),
            CalibrationRole::Gray=>{
                let u=(x-low)/(high-low);
                if u==0. || v==0. || u.signum()!=v.signum() || v.abs()==1. {return Err("This tone cannot be made neutral");}
                ("gamma",u.abs().ln()/v.abs().ln())
            },
        };
        replace(&mut result,&format!("{}{key}",LEVELS_PREFIXES[index+1]),value)?;
    }
    result.validate()?;
    for (index, &value) in source.iter().enumerate() {
        if page!=0 && usize::from(page)!=index+1 {continue;}
        let actual=master.apply(LevelsStage::read(&result,index+1)?.apply(value,clamps),clamps);
        if !close(actual,target) {return Err("This tone cannot be calibrated within the adjustment range");}
    }
    Ok(result)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LevelsStatistics {
    pub minimum:[f64;3],
    pub maximum:[f64;3],
    pub bins:[Vec<u64>;3],
    pub pixels:u64,
}
impl LevelsStatistics {
    pub fn stretch(&self,page:u8)->Result<[f64;2],&'static str> {
        if page>3 || self.pixels==0 || self.pixels>1<<30 {return Err("There are no usable pixels for Auto");}
        let ranks=[self.pixels.div_ceil(1000),(self.pixels*999).div_ceil(1000)];
        let mut interval=[f64::INFINITY,f64::NEG_INFINITY];
        for channel in 0..3 {
            if page!=0 && usize::from(page)!=channel+1 {continue;}
            let (low,high,bins)=(self.minimum[channel],self.maximum[channel],&self.bins[channel]);
            if !low.is_finite() || !high.is_finite() || high<low || bins.len()!=4096
                || bins.iter().try_fold(0u64,|sum,n|sum.checked_add(*n))!=Some(self.pixels) {return Err("Invalid Auto statistics");}
            let quantile=|rank| {
                let mut sum=0;
                let index=bins.iter().position(|count|{sum+=count;sum>=rank}).unwrap();
                (low+(high-low)*(index as f64+0.5)/4096.).clamp(low,high)
            };
            interval[0]=interval[0].min(quantile(ranks[0]));interval[1]=interval[1].max(quantile(ranks[1]));
        }
        if interval[1]-interval[0]<0.001 {return Err("The selected tones are constant");}
        Ok(interval)
    }
}
pub fn auto_levels(effect:&EffectInstance,statistics:&LevelsStatistics,page:u8)->Result<EffectInstance,&'static str> {
    let [low,high]=statistics.stretch(page)?;
    let prefix=LEVELS_PREFIXES[usize::from(page)];let mut result=effect.clone();
    for (key,value) in [("black",low),("white",high),("gamma",1.)] {replace(&mut result,&format!("{prefix}{key}"),value)?;}
    result.validate()?;Ok(result)
}
