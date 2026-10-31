use layer_core::Point;
use layer_render::Dab;
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Copy, Debug)]
struct Vertex { dab: Dab, anchor: Point, distance: f64 }

#[derive(Clone, Debug, Default)]
pub(super) struct Path {
    vertices: Arc<VecDeque<Vertex>>,
    next: usize,
    started: bool,
}

fn vector(a: Point, b: Point) -> [f32; 2] { [b.x-a.x, b.y-a.y] }
fn length(v: [f32; 2]) -> f32 { v[0].hypot(v[1]) }
fn unit(v: [f32; 2]) -> [f32; 2] { let n=length(v).max(1e-12); v.map(|x|x/n) }
fn smooth(x: f32) -> f32 { let t=x.clamp(0.,1.); t*t*(3.-2.*t) }

impl Path {
    pub fn append(&mut self, dab: Dab) -> Vec<Dab> {
        let mut output=Vec::new();
        if self.vertices.is_empty() {
            Arc::make_mut(&mut self.vertices).push_back(Vertex { dab, anchor: dab.center, distance: 0. });
            self.next=1;
            return output;
        }
        let last=*self.vertices.back().unwrap();
        let distance=last.distance+f64::from(length(vector(last.dab.center,dab.center)));
        if distance<=last.distance {return output;}
        if !self.started {
            output.push(last.dab.completion([0.;2],unit(vector(last.dab.center,dab.center)),1.));
            self.started=true;
        }
        let radius=dab.radii[0].min(dab.radii[1]).max(0.005);
        let merge=self.vertices.len()>self.next && self.vertices.len()>1
            && length(vector(last.anchor,dab.center))<(radius*0.05).max(0.25);
        let vertices=Arc::make_mut(&mut self.vertices);
        if merge {
            let last=vertices.back_mut().unwrap();
            last.dab=dab;
            last.distance=distance;
        } else {vertices.push_back(Vertex {dab,anchor:dab.center,distance});}
        self.resolve(false,&mut output);
        output
    }

    pub fn advance(&mut self, head: Point) -> Vec<Dab> {
        let mut output=Vec::new();
        let Some(&last)=self.vertices.back() else {return output;};
        if !self.started {return output;}
        let distance=last.distance+f64::from(length(vector(last.dab.center,head)));
        if distance<=last.distance {return output;}
        Arc::make_mut(&mut self.vertices).push_back(Vertex {dab:Dab {center:head,..last.dab},anchor:head,distance});
        self.resolve(false,&mut output);
        Arc::make_mut(&mut self.vertices).pop_back();
        self.next=self.next.min(self.vertices.len());
        output
    }

    fn point_at(&self, distance: f64) -> Point {
        let mut prior=self.vertices.front().unwrap();
        for next in self.vertices.iter().skip(1) {
            if next.distance>=distance {
                let t=((distance-prior.distance)/(next.distance-prior.distance).max(1e-12)).clamp(0.,1.) as f32;
                return Point {x:prior.dab.center.x+t*(next.dab.center.x-prior.dab.center.x),
                    y:prior.dab.center.y+t*(next.dab.center.y-prior.dab.center.y)};
            }
            prior=next;
        }
        prior.dab.center
    }

    fn corner(&self, first: usize, mut last: usize) -> usize {
        let start=self.vertices[first].dab;
        let tolerance=(start.radii[0].min(start.radii[1])*0.025).max(0.01);
        loop {
            let chord=vector(start.center,self.vertices[last].dab.center);
            let norm=(chord[0]*chord[0]+chord[1]*chord[1]).max(1e-12);
            let mut maximum=tolerance*tolerance;
            let mut split=last;
            for i in first+1..last {
                let point=vector(start.center,self.vertices[i].dab.center);
                let t=((point[0]*chord[0]+point[1]*chord[1])/norm).clamp(0.,1.);
                let error=(point[0]-t*chord[0]).powi(2)+(point[1]-t*chord[1]).powi(2);
                if error>maximum {maximum=error;split=i;}
            }
            if split==last {return last;}
            last=split;
        }
    }

    fn resolve(&mut self, finish: bool, output: &mut Vec<Dab>) {
        while self.next+1<self.vertices.len() {
            let first=self.next-1;
            let i=self.corner(first,self.vertices.len()-1);
            if i+1==self.vertices.len() {
                if !finish && self.vertices.len()>=256 {self.next=i;}
                break;
            }
            let vertex=self.vertices[i];
            let radius=f64::from(vertex.dab.radii[0].min(vertex.dab.radii[1]).max(0.005));
            let end=self.vertices.back().unwrap().distance;
            let travel=end-vertex.distance;
            let pending=!finish && self.vertices.len()<256;
            if pending && travel<0.25*radius {break;}
            let before=vector(self.point_at(vertex.distance-radius),vertex.dab.center);
            let after=vector(vertex.dab.center,self.point_at(vertex.distance+radius));
            let a=length(before);let b=length(after);
            let weight=if a>1e-6 && b>1e-6 {
                let cosine=((before[0]*after[0]+before[1]*after[1])/(a*b)).clamp(-1.,1.);
                let angle=cosine.acos().to_degrees();
                let reach=(f64::from(a)/(vertex.distance-self.vertices.front().unwrap().distance).min(radius).max(1e-12))
                    .min(f64::from(b)/travel.min(radius).max(1e-12)) as f32;
                smooth((angle-10.)/20.)*smooth((reach-0.3)/0.3)
            } else {0.};
            if pending && travel<radius && weight<1. {break;}
            if weight>0. {
                let incoming=unit(vector(vertex.dab.center,self.vertices[first].dab.center));
                let outgoing=unit(vector(vertex.dab.center,self.vertices[self.corner(i,self.vertices.len()-1)].dab.center));
                let dot=incoming[0]*outgoing[0]+incoming[1]*outgoing[1];
                if dot > -0.999999 {
                    output.push(vertex.dab.completion(incoming,outgoing,weight));
                }
            }
            self.next=i+1;
        }
        if !finish && self.next>1 {
            let next=self.vertices[self.next.min(self.vertices.len()-1)];
            let radius=f64::from(next.dab.radii[0].min(next.dab.radii[1]).max(0.005));
            let vertices=Arc::make_mut(&mut self.vertices);
            while self.next>1 && (vertices[1].distance<next.distance-radius || vertices.len()>=256) {
                vertices.pop_front();self.next-=1;
            }
        }
    }

    pub fn finish(&mut self) -> Vec<Dab> {
        let mut output=Vec::new();
        if self.started && self.vertices.len()>1 {
            self.resolve(true,&mut output);
            let end=self.vertices.back().unwrap().dab;
            let previous=self.vertices[self.next.saturating_sub(1).min(self.vertices.len()-2)].dab;
            output.push(end.completion(unit(vector(end.center,previous.center)),[0.;2],1.));
        }
        *self=Self::default();
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DabGenerator, test_support::point};
    use layer_core::{BrushSnapshot, DefaultBrushPreset, default_brush};

    fn charcoal() -> BrushSnapshot {
        let mut brush=default_brush(DefaultBrushPreset::Charcoal);
        brush.diameter=100.;brush.stabilization.strength=0.;
        brush
    }

    #[test]
    fn dry_corner_completion_resolves_at_modeled_input_rate() {
        let brush=charcoal();
        let mut generator=DabGenerator::default();let mut output=Vec::new();
        let sample=|x,y,t| layer_core::StrokePoint {tilt:[40f32.to_radians(),0.],..point(x,y,1.,t)};
        generator.append(sample(0.,0.,0),&brush,&mut output);
        let radius=generator.last_contact.unwrap().radii.into_iter().fold(f32::INFINITY,f32::min);
        let step=radius/20.;let corner=Point {x:120.*step,y:0.};
        for i in 1..=120 {generator.append(sample(i as f32*step,0.,i*5_000),&brush,&mut output);}
        generator.append(sample(corner.x,step,121*5_000),&brush,&mut output);
        let mut reference=generator.dry_completion.clone().unwrap();
        let last=*reference.vertices.back().unwrap();
        let end=Dab {center:Point {x:corner.x,y:4.*radius},..last.dab};
        Arc::make_mut(&mut reference.vertices).push_back(Vertex {dab:end,anchor:end.center,distance:last.distance+f64::from(4.*radius)});
        let expected=reference.finish().into_iter().find(|dab|dab.center==corner).unwrap();
        let mut first=None;
        for i in 2..=240 {
            let start=output.len();
            let damage=generator.append(sample(corner.x,i as f32*step,(120+i)*5_000),&brush,&mut output);
            if let Some(dab)=output[start..].iter().find(|dab|dab.is_completion() && dab.center==corner) {
                assert_eq!(damage.union(dab.bounds()),damage);
                assert!(first.replace((i as f32*step,*dab)).is_none());
            }
        }
        generator.finish(&brush,&mut output);
        let (travel,actual)=first.expect("the corner completes before pen-up");
        assert!(travel>=0.25*radius && travel<=0.35*radius,"outgoing travel: {} R",travel/radius);
        assert_eq!(actual,expected);
    }

    #[test]
    fn dry_short_zigzag_completes_only_its_first_reversal() {
        let brush=charcoal();
        let mut generator=DabGenerator::default();let mut output=Vec::new();
        generator.append(point(0.,0.,1.,0),&brush,&mut output);
        let radius=generator.last_contact.unwrap().radii.into_iter().fold(f32::INFINITY,f32::min);
        let mut head=Point::default();let mut first=None;
        for leg in 0..12 {
            let angle=80f32.to_radians()*if leg%2==0 {1.} else {-1.};
            let end=Point {x:head.x+0.5*radius*angle.cos(),y:head.y+0.5*radius*angle.sin()};
            if leg==0 {first=Some(end);}
            for i in 1..=10 {
                generator.append(point(head.x+(end.x-head.x)*i as f32/10.,head.y+(end.y-head.y)*i as f32/10.,
                    1.,(leg*10+i)*5_000),&brush,&mut output);
            }
            head=end;
        }
        generator.finish(&brush,&mut output);
        let turns:Vec<_>=output.iter().filter(|dab|dab.is_completion() && dab.previous_contact[..2]!=[0.;2]
            && dab.previous_contact[2..]!=[0.;2]).collect();
        assert!(turns.len()<=1,"{} interior completions",turns.len());
        assert!(turns.iter().all(|dab|length(vector(first.unwrap(),dab.center))<radius*0.01));
    }

    #[test]
    fn advancing_a_preview_keeps_only_real_vertices_and_preserves_the_original() {
        let mut generator=DabGenerator::default();
        let brush=charcoal();
        generator.append(point(0.,0.,1.,0),&brush,&mut Vec::new());
        let mut preview=generator.preview();let mut output=Vec::new();
        for i in 1..=6 {preview.append(point(i as f32*100.,0.,1.,i*5_000),&brush,&mut output);}
        assert!(output.len()>1 && output.iter().all(|dab|!dab.is_completion()));
        let dab=Dab {radii:[10.;2],..generator.last_contact.unwrap()};
        let mut path=Path::default();
        assert!(path.advance(Point::default()).is_empty());
        path.append(dab);
        assert!(path.advance(Point {x:1.,y:0.}).is_empty());
        for center in [Point {x:100.,y:0.},Point {x:100.,y:1.}] {path.append(Dab {center,..dab});}
        let vertices:Vec<_>=path.vertices.iter().map(|v|(v.dab,v.anchor,v.distance)).collect();
        let next=path.next;let mut preview=path.clone();
        for head in [Point {x:100.,y:1.},Point {x:100.,y:3.},Point {x:100.,y:30.}] {
            assert!(preview.advance(head).iter().all(|dab|dab.previous_contact.into_iter().all(f32::is_finite)));
            assert!(preview.vertices.iter().all(|v|vertices.contains(&(v.dab,v.anchor,v.distance))));
            assert!((1..=preview.vertices.len()).contains(&preview.next));
        }
        assert_eq!(path.vertices.iter().map(|v|(v.dab,v.anchor,v.distance)).collect::<Vec<_>>(),vertices);
        assert_eq!(path.next,next);
        preview.append(Dab {center:Point {x:100.,y:30.},..dab});
        assert!(!preview.finish().is_empty());
    }
}
