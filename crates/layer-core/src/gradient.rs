use crate::{ColorMixSpace, color::{RgbColor, RgbSpace, SampleDepth, oklab, rgb}};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f32,
    pub color: RgbColor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientDefinition {
    pub stops: Vec<GradientStop>,
    pub interpolation: ColorMixSpace,
}
impl Default for GradientDefinition {
    fn default() -> Self {
        Self::new(vec![
            GradientStop { position: 0., color: RgbColor::new(RgbSpace::Srgb, [0., 0., 0., 1.]).unwrap() },
            GradientStop { position: 1., color: RgbColor::new(RgbSpace::Srgb, [1.; 4]).unwrap() },
        ])
    }
}
impl GradientDefinition {
    pub fn new(stops: Vec<GradientStop>) -> Self {
        Self { stops, interpolation: ColorMixSpace::Oklab }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if valid_positions(self.stops.iter().map(|stop| stop.position))
            && self.stops.iter().all(|stop| stop.color.validate_working_spaces().is_ok()) {
            Ok(())
        } else { Err("Invalid gradient") }
    }
    pub fn reverse(&mut self) {
        self.stops.reverse();
        let last=self.stops.len()-1;
        let mut previous=0.;
        for (i,stop) in self.stops.iter_mut().enumerate() {
            stop.position=if i==0 {0.} else if i==last {1.} else {
                (1.-stop.position).clamp(f32::next_up(previous),f32::from_bits(1_f32.to_bits()-(last-i) as u32))
            };
            previous=stop.position;
        }
    }
    fn coordinates(&self, color: RgbColor, space: RgbSpace) -> Result<([f64; 3], f64), String> {
        let [r,g,b,a] = color.linear_in(space)?.map(f64::from);
        let rgb = match self.interpolation {
            ColorMixSpace::LinearRgb => [r,g,b],
            ColorMixSpace::Classic => [r,g,b].map(|v| space.encode(v)),
            ColorMixSpace::Oklab => oklab::to_lab(rgb::apply(space.linear_transform(RgbSpace::Srgb), [r,g,b])),
        };
        Ok((rgb,a))
    }
    pub fn sample(&self, x: f32, space: RgbSpace) -> Result<RgbColor, String> {
        self.validate()?;
        self.sample_validated(x,space)
    }
    pub fn samples(&self,positions:impl IntoIterator<Item=f32>,space:RgbSpace)->Result<Vec<RgbColor>,String> {
        self.validate()?;
        positions.into_iter().map(|x|self.sample_validated(x,space)).collect()
    }
    pub fn preview(&self,size:[u32;2],space:RgbSpace,depth:SampleDepth)->Result<Vec<RgbColor>,String> {
        let [width,height]=size;
        if width==0 || height==0 || width>4096 || height>256 {return Err("Invalid gradient preview size".into());}
        let columns=self.samples((0..width).map(|x|x as f32/(width-1).max(1) as f32),space)?;
        let levels=if depth.is_float() {0.} else {((1u32<<depth.bits())-1) as f64};
        let columns=columns.into_iter().enumerate().map(|(x,color)| {
            let position=x as f32/(width-1).max(1) as f32;
            let index=self.stops.partition_point(|stop|stop.position<position).saturating_sub(1).min(self.stops.len()-2);
            let (a,b)=(&self.stops[index],&self.stops[index+1]);
            let dither=position>a.position && position<b.position && color.rgba[3]>0.
                && a.color.linear_in(space)?[..3]!=b.color.linear_in(space)?[..3];
            let mut encoded=color.linear_in(space)?.map(f64::from);
            for c in &mut encoded[..3] {*c=space.encode(*c);}
            Ok((color,encoded,dither))
        }).collect::<Result<Vec<_>,String>>()?;
        let mut pixels=Vec::with_capacity(width as usize*height as usize);
        for y in 0..height {for (x,(color,encoded,dither)) in columns.iter().enumerate() {
            if levels==0. {pixels.push(*color);continue;}
            let noise=if *dither {f64::from(gradient_noise([x as u32,y]))} else {0.};
            let rgba=std::array::from_fn(|c|((encoded[c]*levels+if c<3 {noise} else {0.}).round()/levels).clamp(0.,1.) as f32);
            pixels.push(RgbColor::new(space,rgba)?);
        }}
        Ok(pixels)
    }
    fn sample_validated(&self,x:f32,space:RgbSpace)->Result<RgbColor,String> {
        if !x.is_finite() { return Err("Invalid gradient sample".into()); }
        let i = self.stops.partition_point(|stop| stop.position < x).saturating_sub(1).min(self.stops.len()-2);
        let (a,b) = (&self.stops[i], &self.stops[i+1]);
        if x <= a.position { return Ok(a.color); }
        if x >= b.position { return Ok(b.color); }
        let t = (f64::from(x)-f64::from(a.position))/(f64::from(b.position)-f64::from(a.position));
        let la=a.color.linear_in(space)?;let lb=b.color.linear_in(space)?;
        if la[3]==0. && lb[3]==0. {return RgbColor::from_linear(space,[0.;4]);}
        if la[..3]==lb[..3] {
            return RgbColor::from_linear(space,[la[0],la[1],la[2],(f64::from(la[3])*(1.-t)+f64::from(lb[3])*t) as f32]);
        }
        let (ca,aa) = self.coordinates(a.color,space)?;
        let (cb,ab) = self.coordinates(b.color,space)?;
        let alpha = aa*(1.-t)+ab*t;
        let mixed = std::array::from_fn(|c| if alpha>0. { (ca[c]*aa*(1.-t)+cb[c]*ab*t)/alpha } else { 0. });
        let linear = match self.interpolation {
            ColorMixSpace::LinearRgb => mixed,
            ColorMixSpace::Classic => mixed.map(|v| space.decode(v)),
            ColorMixSpace::Oklab => rgb::apply(RgbSpace::Srgb.linear_transform(space), oklab::from_lab(mixed)),
        };
        RgbColor::from_linear(space, [linear[0] as f32, linear[1] as f32, linear[2] as f32, alpha as f32])
    }
    pub fn parameters(&self, space: RgbSpace) -> Result<[[f32;4];crate::EFFECT_TABLE_VECTORS],String> {
        self.validate()?;
        let mut data = [[0.;4];crate::EFFECT_TABLE_VECTORS];
        data[0] = [self.stops.len() as f32,self.interpolation as u8 as f32,2.,0.];
        for (i,stop) in self.stops.iter().enumerate() {
            let [r,g,b,a] = stop.color.linear_in(space)?;
            let (coordinates,_) = self.coordinates(stop.color,space)?;
            data[1+i*2] = [stop.position,r,g,b];
            data[2+i*2] = [a,coordinates[0] as f32,coordinates[1] as f32,coordinates[2] as f32];
        }
        Ok(data)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum GradientShape {
    #[default]
    Linear,
    Radial,
    Reflected,
}
impl GradientShape {
    pub const ALL: [Self;3] = [Self::Linear, Self::Radial, Self::Reflected];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalarGradientStop {
    pub position: f32,
    pub value: f32,
    pub opacity: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalarGradient {
    pub stops: Vec<ScalarGradientStop>,
}
impl ScalarGradient {
    pub fn validate(&self) -> bool {
        valid_positions(self.stops.iter().map(|stop|stop.position)) && self.stops.iter().all(|stop|
            [stop.value,stop.opacity].into_iter().all(|v|v.is_finite() && (0. ..=1.).contains(&v)))
    }
    pub fn parameters(&self) -> Result<[[f32;4];crate::EFFECT_TABLE_VECTORS],String> {
        if !self.validate() {return Err("Invalid scalar gradient".into());}
        let mut data = [[0.;4];crate::EFFECT_TABLE_VECTORS];
        data[0] = [self.stops.len() as f32,0.,2.,0.];
        for (i,stop) in self.stops.iter().enumerate() {
            data[1+i*2] = [stop.position,stop.value,0.,0.];
            data[2+i*2] = [stop.opacity,stop.value,0.,0.];
        }
        Ok(data)
    }
}
fn gradient_noise(point:[u32;2])->f32 {
    let seed=point[0].wrapping_mul(0x9e3779b9).wrapping_add(point[1].wrapping_mul(0x85ebca6b)).wrapping_add(0x632be59b);
    let hash=|mut value:u32| {
        value=(value^(value>>16)).wrapping_mul(0x7feb352d);
        value=(value^(value>>15)).wrapping_mul(0x846ca68b);
        (value^(value>>16))&65535
    };
    ((hash(seed)+hash(seed^0xa511e9b3)) as f32+1.)/65536.-1.
}
fn valid_positions(positions: impl ExactSizeIterator<Item=f32>) -> bool {
    if !(2..=32).contains(&positions.len()) { return false; }
    let mut previous = -1.;
    for (i,position) in positions.enumerate() {
        if !position.is_finite() || !(0. ..=1.).contains(&position) || position<=previous || (i==0 && position!=0.) { return false; }
        previous=position;
    }
    previous==1.
}

#[cfg(test)]
mod tests {
    use super::*;
    const GOLDENS:[[[f64;3];3];4]=[[[0.4585305461215648, 0.23301646750773217, 0.18256518339019923], [0.5500000081956387, 0.2357142918876239, 0.2821428533643484], [0.5037354990799368, 0.2590620995093382, 0.22518965715963804]], [[0.4585305461215648, 0.23301646750773217, 0.18256518339019923], [0.5500000081956387, 0.2357142918876239, 0.2821428533643484], [0.49916031660640503, 0.26445576933391646, 0.22091131326677238]], [[0.46530286993656567, 0.2331889249907596, 0.18855229681895444], [0.5500000081956387, 0.2357142918876239, 0.2821428533643484], [0.4881364711724619, 0.27317008741635707, 0.23582435560230106]], [[0.4827884935373779, 0.23364863030340333, 0.20503696418330908], [0.5500000081956387, 0.2357142918876239, 0.2821428533643484], [0.4367719011989252, 0.28342801437357096, 0.23543844858162793]]];
    fn pair(space:RgbSpace,alpha:[f32;2])->GradientDefinition {
        GradientDefinition::new(vec![
            GradientStop{position:0.,color:RgbColor::from_linear(space,[0.1,0.3,0.7,alpha[0]]).unwrap()},
            GradientStop{position:1.,color:RgbColor::from_linear(space,[0.8,0.2,0.05,alpha[1]]).unwrap()}])
    }
    #[test]
    fn alpha_weighted_four_profile_three_space_goldens() {
        for (profile,space) in RgbSpace::ALL.into_iter().enumerate(){
            for (mode,interpolation) in [ColorMixSpace::Classic,ColorMixSpace::LinearRgb,ColorMixSpace::Oklab].into_iter().enumerate(){
                let mut gradient=pair(space,[0.25,0.75]);gradient.interpolation=interpolation;
                let actual=gradient.sample(0.375,space).unwrap().linear_in(space).unwrap();
                assert_eq!(actual[3],0.4375);
                for channel in 0..3{assert!((f64::from(actual[channel])-GOLDENS[profile][mode][channel]).abs()<2e-6,"{space:?} {interpolation:?} {actual:?}");}
                let packed=gradient.parameters(space).unwrap();assert_eq!(packed[0],[2.,interpolation as u8 as f32,2.,0.]);
            }
        }
    }
    #[test]
    fn hidden_color_has_no_influence_and_subnormal_alpha_remains_visible() {
        for space in RgbSpace::ALL{for interpolation in [ColorMixSpace::Classic,ColorMixSpace::LinearRgb,ColorMixSpace::Oklab]{
            for alpha in [[1.,0.],[f32::from_bits(1),f32::from_bits(1)],[0.,0.]]{
                let mut gradient=pair(space,alpha);gradient.interpolation=interpolation;
                let actual=gradient.sample(0.5,space).unwrap().linear_in(space).unwrap();
                assert_eq!(actual[3],((f64::from(alpha[0])+f64::from(alpha[1]))*0.5) as f32);
                if alpha==[1.,0.]{for c in 0..3{assert!((actual[c]-gradient.stops[0].color.linear_in(space).unwrap()[c]).abs()<2e-6);}}
                if alpha==[0.,0.]{assert_eq!(actual,[0.;4]);}else{assert!(actual[..3].iter().any(|v|*v>0.));}
            }
        }}
    }
    #[test]
    fn exact_tagged_knots_adjacent_positions_and_constant_rgb_identity() {
        for space in RgbSpace::ALL{for interpolation in [ColorMixSpace::Classic,ColorMixSpace::LinearRgb,ColorMixSpace::Oklab]{
            let mut gradient=pair(space,[0.25,0.75]);gradient.interpolation=interpolation;
            gradient.stops.insert(1,GradientStop{position:0.5,color:RgbColor::new(RgbSpace::DisplayP3,[0.2,0.4,0.6,0.5]).unwrap()});
            gradient.stops.insert(2,GradientStop{position:f32::from_bits(0.5_f32.to_bits()+1),color:gradient.stops[2].color});
            for stop in &gradient.stops{assert_eq!(gradient.sample(stop.position,space).unwrap(),stop.color);}
            assert_eq!(gradient.sample(-1.,space).unwrap(),gradient.stops[0].color);assert_eq!(gradient.sample(2.,space).unwrap(),gradient.stops.last().unwrap().color);
            let color=RgbColor::from_linear(space,[0.2,0.4,4.,0.25]).unwrap();
            let mut end=color;end.rgba[3]=0.75;
            gradient.stops=vec![GradientStop{position:0.,color},GradientStop{position:1.,color:end}];
            assert_eq!(gradient.sample(0.5,space).unwrap().linear_in(space).unwrap(),[0.2,0.4,4.,0.5]);
        }}
    }
    #[test]
    fn invalid_color_and_scalar_definitions_fail_before_packing() {
        for positions in [vec![],vec![0.],vec![0.,0.],vec![0.,0.5],vec![0.1,1.],vec![0.,f32::NAN,1.],vec![0.,f32::INFINITY,1.],vec![0.,-0.1,1.],vec![0.,1.1,1.],(0..33).map(|i|i as f32/32.).collect()]{
            let color=GradientDefinition::new(positions.iter().map(|position|GradientStop{position:*position,color:RgbColor::new(RgbSpace::Srgb,[0.;4]).unwrap()}).collect());
            let scalar=ScalarGradient{stops:positions.iter().map(|position|ScalarGradientStop{position:*position,value:0.5,opacity:0.5}).collect()};
            assert!(color.validate().is_err());assert!(color.parameters(RgbSpace::Srgb).is_err());assert!(!scalar.validate());assert!(scalar.parameters().is_err());
        }
        let gradient=GradientDefinition::default();for x in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY]{assert!(gradient.sample(x,RgbSpace::Srgb).is_err());}
        for invalid in [-0.1,1.1,f32::NAN,f32::INFINITY]{for opacity in [false,true]{let scalar=ScalarGradient{stops:vec![ScalarGradientStop{position:0.,value:if opacity{0.5}else{invalid},opacity:if opacity{invalid}else{1.}},ScalarGradientStop{position:1.,value:1.,opacity:1.}]};assert!(scalar.parameters().is_err());}}
        let scalar=ScalarGradient{stops:(0..32).map(|i|ScalarGradientStop{position:i as f32/31.,value:0.25,opacity:0.5}).collect()};let packed=scalar.parameters().unwrap();assert_eq!(packed[0],[32.,0.,2.,0.]);assert_eq!(packed[64],[0.5,0.25,0.,0.]);
    }
    #[test]
    fn definition_serialization_preserves_mode_and_exact_tagged_knots() {
        let mut gradient=pair(RgbSpace::ProPhoto,[0.25,0.75]);gradient.interpolation=ColorMixSpace::LinearRgb;
        let json=serde_json::to_string(&gradient).unwrap();let restored:GradientDefinition=serde_json::from_str(&json).unwrap();assert_eq!(restored,gradient);assert_eq!(restored.parameters(RgbSpace::DisplayP3).unwrap(),gradient.parameters(RgbSpace::DisplayP3).unwrap());
    }
    #[test]
    fn transparent_constant_interior_hides_rgb_but_exact_knots_keep_tags(){
        for space in RgbSpace::ALL{for interpolation in [ColorMixSpace::Classic,ColorMixSpace::LinearRgb,ColorMixSpace::Oklab]{
            let color=RgbColor::from_linear(space,[0.2,0.4,4.,0.]).unwrap();
            let gradient=GradientDefinition{stops:vec![GradientStop{position:0.,color},GradientStop{position:1.,color}],interpolation};
            assert_eq!(gradient.sample(0.,space).unwrap(),color);assert_eq!(gradient.sample(1.,space).unwrap(),color);
            assert_eq!(gradient.sample(0.5,space).unwrap().linear_in(space).unwrap(),[0.;4]);
            let strip=gradient.samples([0.,0.5,1.],space).unwrap();assert_eq!(strip[0],color);assert_eq!(strip[2],color);assert_eq!(strip[1].linear_in(space).unwrap(),[0.;4]);
        }}
    }

    #[test]
    fn preview_dither_changes_integer_rows_without_endpoint_constant_or_alpha_noise(){
        let mut gradient=pair(RgbSpace::Srgb,[0.25,0.75]);
        let noisy=gradient.preview([67,8],RgbSpace::Srgb,SampleDepth::U8).unwrap();

        let plain=gradient.preview([67,1],RgbSpace::Srgb,SampleDepth::F32).unwrap();
        assert!(noisy.iter().zip(&plain).any(|(a,b)|a!=b));
        assert!(noisy.chunks(67).skip(1).any(|row|row!=&noisy[..67]));
        for row in 0..8{assert_eq!(noisy[row*67],noisy[0]);assert_eq!(noisy[row*67+66],noisy[66]);}
        for (i,a) in noisy.iter().enumerate(){assert_eq!(a.rgba[3],(plain[i%67].rgba[3]*255.).round()/255.);assert!(a.rgba.iter().all(|v|(0.0..=1.0).contains(v)));}

        let float=gradient.preview([67,8],RgbSpace::Srgb,SampleDepth::F32).unwrap();
        for row in float.chunks(67){for (x,color) in row.iter().enumerate(){assert_eq!(*color,gradient.sample(x as f32/66.,RgbSpace::Srgb).unwrap());}}
        gradient.stops[1].color=gradient.stops[0].color;
        let constant=gradient.preview([67,8],RgbSpace::Srgb,SampleDepth::U8).unwrap();
        assert!(constant.iter().all(|v|*v==constant[0]));
        for size in [[0,1],[1,0],[4097,1],[1,257]]{assert!(gradient.preview(size,RgbSpace::Srgb,SampleDepth::U8).is_err());}
    }
    #[test]
    fn reverse_keeps_adjacent_knots_strictly_ordered_and_reverses_colors(){
        let mut gradient=pair(RgbSpace::Srgb,[0.25,0.75]);
        for position in [0.2,0.5,f32::next_up(0.5)]{gradient.stops.insert(gradient.stops.len()-1,GradientStop{position,color:gradient.stops[0].color});}
        let colors=gradient.stops.iter().rev().map(|s|s.color).collect::<Vec<_>>();
        gradient.reverse();gradient.validate().unwrap();
        assert_eq!(gradient.stops.iter().map(|s|s.color).collect::<Vec<_>>(),colors);
        assert_eq!(gradient.stops[3].position,0.8);
        assert!(gradient.stops.windows(2).all(|p|p[0].position<p[1].position));
    }

}
