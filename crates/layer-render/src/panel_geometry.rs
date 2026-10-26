use std::sync::OnceLock;

pub const PANEL_ATTACHMENT: f32 = 6.;

pub fn panel_phase(value: f32) -> f32 {
    let t = value.clamp(0., 1.);
    t*t*t*(10.+t*(-15.+6.*t))
}

pub fn panel_radii(bounds: [f32; 4], radii: [f32; 4]) -> [f32; 4] {
    let [_, _, w, h] = bounds;
    let factor = [(w,radii[0]+radii[1]),(w,radii[2]+radii[3]),
        (h,radii[0]+radii[3]),(h,radii[1]+radii[2])].into_iter()
        .fold(1f32, |a,(size,sum)| a.min(size/sum.max(0.0001)));
    radii.map(|r| r.max(0.)*factor)
}

pub fn panel_rect(p: [f32; 2], b: [f32; 4], radii: [f32; 4]) -> f32 {
    rect(p,b,panel_radii(b,radii))
}

fn rect(p: [f32;2],b: [f32;4],r: [f32;4]) -> f32 {
    let [x,y,w,h] = b;
    let mut field = ((p[0]-x-w/2.).abs()-w/2.).max((p[1]-y-h/2.).abs()-h/2.);
    for (radius,cx,cy,inside) in [
        (r[0],x+r[0],y+r[0],p[0]<x+r[0] && p[1]<y+r[0]),
        (r[1],x+w-r[1],y+r[1],p[0]>x+w-r[1] && p[1]<y+r[1]),
        (r[2],x+w-r[2],y+h-r[2],p[0]>x+w-r[2] && p[1]>y+h-r[2]),
        (r[3],x+r[3],y+h-r[3],p[0]<x+r[3] && p[1]>y+h-r[3]),
    ] {
        if inside { let a=(p[0]-cx).powi(2);let b=(p[1]-cy).powi(2);field=field.max((a*a+b*b).sqrt().sqrt()-radius); }
    }
    field
}

pub const PANEL_PROFILE_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/panel-profiles.bin"));

pub fn panel_profiles() -> &'static [[f32; 2]] {
    static PROFILES: OnceLock<Vec<[f32; 2]>> = OnceLock::new();
    PROFILES.get_or_init(|| PANEL_PROFILE_BYTES.chunks_exact(8).map(|c| [
        f32::from_le_bytes(c[..4].try_into().unwrap()),f32::from_le_bytes(c[4..].try_into().unwrap()),
    ]).collect())
}

fn profile(u: f32,delta: f32) -> [f32; 2] {
    if delta==0. || u>=3. { return [u,1.]; }
    if u<= -5. { return [u+std::f32::consts::SQRT_2*delta,1.]; }
    let x=(u+5.)*511./8.;let y=delta.clamp(0.,4.)*64.;
    let ix=x.floor() as usize;let iy=y.floor() as usize;
    let jx=(ix+1).min(511);let jy=(iy+1).min(256);
    let table=panel_profiles();
    std::array::from_fn(|i| {
        let a=table[iy*512+ix][i]*(1.-x.fract())+table[iy*512+jx][i]*x.fract();
        let b=table[jy*512+ix][i]*(1.-x.fract())+table[jy*512+jx][i]*x.fract();
        a*(1.-y.fract())+b*y.fract()
    })
}

fn upper(p: [f32; 2],panel_top: f32,tile_top: f32,radius: f32) -> f32 {
    if tile_top<=panel_top || radius<=0. { return tile_top.min(panel_top)-p[1]; }
    let mut delta=(tile_top-panel_top)/radius;
    let mut y=(p[1]-panel_top)/radius;
    if delta>4. {
        if y>=2. && y<=delta-2. { return -p[0]; }
        if y>delta-2. { y-=delta-4.; }
        delta=4.;
    }
    let x=p[0]/radius;
    let [value,slope]=profile((x-y)*std::f32::consts::FRAC_1_SQRT_2,delta);
    (value-(x+y)*std::f32::consts::FRAC_1_SQRT_2)*radius/(1.+slope*slope).sqrt()
}

fn smooth_max(a: f32,b: f32,width: f32) -> f32 {
    if width<=0.00001 { return a.max(b); }
    a.max(b)+(width-(a-b).abs()).max(0.).powi(3)/(6.*width*width)
}

struct PreparedGap {
    bounds: [f32;2], anchor: [f32;2], half: f32, depth: f32, offset: f32, smoothing: [f32;2], floor: f32,
}
impl PreparedGap {
    fn new(g: [f32;2],anchor: [f32;2],width: f32) -> Self {
        let h=(g[1]-g[0]).max(0.001);
        let top=panel_phase((g[1]-anchor[0])/h);let bottom=panel_phase((anchor[1]-g[0])/h);
        let depth=(2.*width/3.).min(2.*h/3.);let smoothing=(width/4.).min(h/4.);
        Self {bounds:g,anchor,half:h/2.,depth:depth.max(0.001),offset:depth*(1.-top*bottom),
            smoothing:[smoothing*(1.-top),smoothing*(1.-bottom)],floor:1./depth.max(h)}
    }
    fn field(&self,p: [f32;2]) -> f32 {
        let g=self.bounds;
        let q=(p[1]-(g[0]+g[1])/2.)/self.half;
        let x=(self.offset-p[0]).max(0.)/self.depth;
        let value=1.-x*x*x-q*q;
        let gradient=(3.*x*x/self.depth).hypot(2.*q/self.half);
        let distance=smooth_max(value/gradient.max(self.floor),self.anchor[0]-p[1],self.smoothing[0]);
        smooth_max(distance,p[1]-self.anchor[1],self.smoothing[1]).min((p[1]-g[0]).min(g[1]-p[1]))
    }
}

struct PreparedReceiver {bounds: [f32;4],radii: [f32;4],attachment: f32}
struct PreparedAnchor {anchor: [f32;2],receivers: Vec<PreparedReceiver>,gaps: Vec<PreparedGap>}
pub struct PanelEvaluator<'a> {contour: &'a PanelContour, attachments: Vec<PreparedAnchor>,radii: [f32;4], search: SeedSearch}
impl std::ops::Deref for PanelEvaluator<'_> {
    type Target=PanelContour;
    fn deref(&self) -> &PanelContour {self.contour}
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelContour {
    pub bounds: [f32; 4],
    pub radii: [f32; 4],
    pub anchors: Vec<[f32; 2]>,
    pub width: f32,
    pub gaps: Vec<[f32; 2]>,
    pub envelope: [f32; 4],
    pub clip: [f32; 2],
    pub neighbors: Vec<([f32;4],[f32;4])>,
}

struct SeedSearch {range: [f32;2],knots: Vec<f32>}
impl SeedSearch {
    fn new(c: &PanelContour) -> Self {
        let reach=2.*c.radii.into_iter().fold(0f32,f32::max)+PANEL_ATTACHMENT;
        let mut lo=c.anchors.iter().map(|a|a[0]-reach).fold(c.bounds[1]+c.bounds[3],f32::min);
        let mut hi=c.anchors.iter().map(|a|a[1]+reach).fold(c.bounds[1],f32::max);
        for (bounds,radii) in std::iter::once(&(c.bounds,c.radii)).chain(c.neighbors.iter()) {
            if radii[0]<=0. {lo=lo.min(bounds[1]);}
            if radii[3]<=0. {hi=hi.max(bounds[1]+bounds[3]);}
        }
        let lo=lo.max(c.clip[0]);let hi=hi.min(c.clip[1]);
        if hi<=lo {return Self {range:[lo,hi],knots:Vec::new()};}
        let mut knots=[0.;32];let mut count=0;
        let mut add=|y: f32| {if count<knots.len() {knots[count]=y.clamp(lo,hi);count+=1;}};
        for y in [lo,hi,c.bounds[1],c.bounds[1]+c.bounds[3],c.bounds[1]+c.bounds[3]/2.] {add(y);}
        for a in &c.anchors {for y in [a[0],a[1],(a[0]+a[1])/2.] {add(y);}}
        for g in &c.gaps {for y in [g[0],g[1],(g[0]+g[1])/2.] {add(y);}}
        let mut values=knots[..count].to_vec();
        knots[..count].sort_by(f32::total_cmp);
        values.extend(knots[..count].windows(2).map(|w|(w[0]+w[1])/2.));
        let mut unique=Vec::with_capacity(values.len());
        for y in values {if !unique.contains(&y) {unique.push(y);}}
        Self {range:[lo,hi],knots:unique}
    }
}

impl PanelContour {
    pub fn commands(&self,tolerance: f32) -> Vec<[f32; 7]> {
        let mut result=Vec::new();
        self.patches(tolerance,&mut result);
        panel_outline(self.bounds,self.radii,&mut result);
        result
    }

    fn patches(&self,tolerance: f32,result: &mut Vec<[f32;7]>) {
        if self.anchors.is_empty() {return;}
        if self.anchors.len()>1 {
            for &anchor in &self.anchors {
                let mut single=self.clone();single.anchors=vec![anchor];
                single.patches(tolerance,result);
            }
            return;
        }
        if let Some(cut)=self.gaps.iter().map(|g|(g[0]+g[1])/2.)
            .find(|y|*y>self.clip[0]+0.0001 && *y<self.clip[1]-0.0001) {
            let mut a=self.clone();a.clip[1]=cut;a.patches(tolerance,result);
            let mut b=self.clone();b.clip[0]=cut;b.patches(tolerance,result);
            return;
        }
        self.prepare().append(tolerance,result);
    }

    pub fn prepare(&self) -> PanelEvaluator<'_> {
        let attachments=self.anchors.iter().map(|&anchor| {
            let receivers=std::iter::once(&(self.bounds,self.radii)).chain(self.neighbors.iter()).filter_map(|(b,r)| {
                let attachment=panel_phase((b[1]+b[3]+PANEL_ATTACHMENT-anchor[0])/PANEL_ATTACHMENT)
                    *panel_phase((anchor[1]+PANEL_ATTACHMENT-b[1])/PANEL_ATTACHMENT);
                (attachment>0.).then(|| PreparedReceiver {bounds:*b,radii:panel_radii(*b,*r),attachment})
            }).collect();
            PreparedAnchor {anchor,receivers,gaps:self.gaps.iter().map(|&g|PreparedGap::new(g,anchor,self.width)).collect()}
        }).collect();
        PanelEvaluator {contour:self,attachments,radii:panel_radii(self.bounds,self.radii),search:SeedSearch::new(self)}
    }

    pub fn field(&self,p: [f32;2]) -> f32 {self.prepare().field(p)}
}

impl PanelEvaluator<'_> {
    fn seed(&self,x: f32) -> Option<[f32;3]> {
        let [lo,hi]=self.search.range;
        self.search.knots.iter().copied().find(|y|self.field([x,*y])<=0.).map(|y|[lo,hi,y])
    }

    fn interval(&self,x: f32) -> Option<[f32; 2]> {
        let [lo,hi,middle]=self.seed(x)?;
        let root=|outside: f32,inside: f32| {
            let distance=|y| {let value=self.field([x,y]);if value==0. {-f32::EPSILON} else {value}};
            let mut a=outside;let mut b=inside;let mut c=b;
            let mut fa=distance(a);let mut fb=distance(b);let mut fc=fb;
            if fa<=0. {return outside;}
            let mut step=b-a;let mut previous=step;
            for _ in 0..64 {
                if (fb>0.)==(fc>0.) {c=a;fc=fa;step=b-a;previous=step;}
                if fc.abs()<fb.abs() {a=b;b=c;c=a;fa=fb;fb=fc;fc=fa;}
                let tolerance=0.000025+f32::EPSILON*b.abs();
                let midpoint=(c-b)/2.;
                if midpoint.abs()<=tolerance {break;}
                if previous.abs()>=tolerance && fa.abs()>fb.abs() {
                    let ratio=fb/fa;
                    let (mut p,mut q)=if a==c {(2.*midpoint*ratio,1.-ratio)} else {
                        let q=fa/fc;let r=fb/fc;
                        (ratio*(2.*midpoint*q*(q-r)-(b-a)*(r-1.)),(q-1.)*(r-1.)*(ratio-1.))
                    };
                    if p>0. {q= -q;} else {p= -p;}
                    if 2.*p<(3.*midpoint*q-(tolerance*q).abs()).min((previous*q).abs()) {
                        previous=step;step=p/q;
                    } else {step=midpoint;previous=step;}
                } else {step=midpoint;previous=step;}
                a=b;fa=fb;
                b+=if step.abs()>tolerance {step} else {tolerance.copysign(midpoint)};
                fb=distance(b);
            }
            let y=b;
            self.anchors.iter().flatten().copied().chain([self.bounds[1],self.bounds[1]+self.bounds[3]])
                .find(|boundary|(y-boundary).abs()<0.001).unwrap_or(y)
        };
        Some([root(lo,middle),root(hi,middle)])
    }

    fn append(&self,tolerance: f32,result: &mut Vec<[f32;7]>) {
        let r=panel_radii(self.bounds,self.radii);
        let extent=r[0].max(r[3]).max(0.001);
        let mut ranges=Vec::new();
        let root=|mut a: f32,mut b: f32,inside: bool| {
            for _ in 0..18 {let x=(a+b)/2.;if self.seed(x).is_some()==inside {a=x;} else {b=x;}}
            (a+b)/2.
        };
        let mut previous=-self.width;
        let mut present=self.seed(previous).is_some();
        let mut start=previous;
        let continuous=present && self.anchors.iter().any(|a|
            a[0].max(self.bounds[1]).max(self.clip[0])<a[1].min(self.bounds[1]+self.bounds[3]).min(self.clip[1]));
        if continuous {ranges.push([-self.width,extent]);}
        for i in 1..=if continuous {0} else {128} {
            let x=-self.width+(extent+self.width)*i as f32/128.;
            let next=self.seed(x).is_some();
            if next!=present {
                let boundary=root(previous,x,present);
                if next {start=boundary;} else {ranges.push([start,boundary]);}
            }
            previous=x;present=next;
        }
        if present && !continuous {ranges.push([start,extent]);}
        for [left,extent] in ranges {
            if left<0. && extent>0. {
                self.trace(left,0.,tolerance,result);
                self.trace(0.,extent,tolerance,result);
            } else {self.trace(left,extent,tolerance,result);}
        }
    }

    fn trace(&self,left: f32,extent: f32,tolerance: f32,result: &mut Vec<[f32;7]>) {
        let mut cache=std::collections::HashMap::new();
        let mut sample=|t: f32| -> [f32; 3] {
            *cache.entry(t.to_bits()).or_insert_with(|| {
                let x=left+(extent-left)*(t*std::f32::consts::FRAC_PI_2).sin().powi(2);
                let query=if extent==0. && x>= -0.0001 {-0.0001} else {x};
                let ys=self.interval(query).unwrap_or_else(|| {
                    let a=self.bounds[1].max(self.anchors.first().map_or(self.bounds[1],|a|a[0]));
                    [a,a]
                });
                [x,ys[0],ys[1]]
            })
        };
        let first=sample(0.);result.push([0.,first[0],first[1],0.,0.,0.,0.]);
        for side in [1,2] {
            let mut point=|t: f32| { let p=sample(if side==1 { t } else { 1.-t });[p[0],p[side]] };
            let mut derivative=|t: f32| {
                let lo=(t-0.001).max(0.);let hi=(t+0.001).min(1.);
                let a=point(lo);let b=point(hi);[(b[0]-a[0])/(hi-lo),(b[1]-a[1])/(hi-lo)]
            };
            let d0=derivative(0.);let d1=derivative(1.);
            let a=point(0.);let b=point(1.);
            if side==2 {result.push([1.,a[0],a[1],0.,0.,0.,0.]);}
            fit(&mut point,0.,1.,a,b,d0,d1,tolerance.clamp(0.01,0.1),0,result);
        }
        result.push([3.,0.,0.,0.,0.,0.,0.]);
    }

    pub fn field(&self,p: [f32;2]) -> f32 {
        let rounded=rect(p,self.bounds,self.radii);
        let mut joined=rounded;
        for a in &self.attachments {
            let anchor=a.anchor;let mut bridge=f32::INFINITY;
            for receiver in &a.receivers {
                let b=receiver.bounds;let r=receiver.radii;
                let rounded=rect(p,b,r);
                let top=upper(p,b[1],anchor[0],r[0]);
                let bottom=upper([p[0],-p[1]],-b[1]-b[3],-anchor[1],r[3]);
                let candidate=top.max(bottom).max((-self.width-p[0]).max(p[0]-r[0].max(r[3])));
                bridge=bridge.min(rounded*(1.-receiver.attachment)+rounded.min(candidate)*receiver.attachment);
            }
            for g in &a.gaps {bridge=bridge.max(g.field(p));}
            bridge=bridge.max(upper(p,self.envelope[0],anchor[0],self.envelope[2]));
            bridge=bridge.max(upper([p[0],-p[1]],-self.envelope[1],-anchor[1],self.envelope[3]));
            joined=joined.min(bridge);
        }
        joined.max(self.clip[0]-p[1]).max(p[1]-self.clip[1])
    }

}

fn tangent(point: &mut impl FnMut(f32)->[f32;2],t: f32) -> [f32;2] {
    let lo=(t-0.001).max(0.);let hi=(t+0.001).min(1.);
    let a=point(lo);let b=point(hi);
    [(b[0]-a[0])/(hi-lo),(b[1]-a[1])/(hi-lo)]
}

#[allow(clippy::too_many_arguments)]
fn fit(point: &mut impl FnMut(f32)->[f32;2],t0: f32,t1: f32,a: [f32;2],b: [f32;2],
    d0: [f32;2],d1: [f32;2],tolerance: f32,depth: u32,out: &mut Vec<[f32;7]>) {
    let step=(t1-t0)/3.;
    let c=[a[0]+step*d0[0],a[1]+step*d0[1]];
    let d=[b[0]-step*d1[0],b[1]-step*d1[1]];
    let error=[0.25,0.5,0.75].into_iter().map(|t| {
        let p=point(t0+(t1-t0)*t);let s=1.-t;
        let q=std::array::from_fn::<_,2,_>(|i|s*s*s*a[i]+3.*s*s*t*c[i]+3.*s*t*t*d[i]+t*t*t*b[i]);
        (p[0]-q[0]).hypot(p[1]-q[1])
    }).fold(0f32,f32::max);
    if error<=tolerance || depth>=9 {
        out.push([2.,c[0],c[1],d[0],d[1],b[0],b[1]]);
    } else {
        let t=(t0+t1)/2.;let p=point(t);let derivative=tangent(point,t);
        fit(point,t0,t,a,p,d0,derivative,tolerance,depth+1,out);
        fit(point,t,t1,p,b,derivative,d1,tolerance,depth+1,out);
    }
}

pub fn panel_outline(b: [f32;4],radii: [f32;4],out: &mut Vec<[f32;7]>) {
    let r=panel_radii(b,radii);
    let [x,y,w,h]=b;
    out.push([0.,x+r[0],y,0.,0.,0.,0.]);
    for (radius,center,start,end) in [
        (r[1],[x+w-r[1],y+r[1]],[0.,-1.],[1.,0.]),
        (r[2],[x+w-r[2],y+h-r[2]],[1.,0.],[0.,1.]),
        (r[3],[x+r[3],y+h-r[3]],[0.,1.],[-1.,0.]),
        (r[0],[x+r[0],y+r[0]],[-1.,0.],[0.,-1.]),
    ] {
        let mut point=|t: f32| {
            let angle=(1.-(t*std::f32::consts::PI).cos())*std::f32::consts::FRAC_PI_4;
            let a=angle.cos().max(0.).sqrt();let b=angle.sin().max(0.).sqrt();
            [center[0]+radius*(start[0]*a+end[0]*b),center[1]+radius*(start[1]*a+end[1]*b)]
        };
        let a=point(0.);let b=point(1.);
        out.push([1.,a[0],a[1],0.,0.,0.,0.]);
        let d0=tangent(&mut point,0.);let d1=tangent(&mut point,1.);
        fit(&mut point,0.,1.,a,b,d0,d1,0.025,0,out);
    }
    out.push([3.,0.,0.,0.,0.,0.,0.]);
}

#[cfg(test)]
mod tests {
    use super::*;

    mod mesh_tests { include!("panel_mesh_tests.rs"); }

    fn contour(bottom: f32) -> PanelContour {
        PanelContour {neighbors:vec![],bounds:[0.,0.,120.,bottom],radii:[12.;4],anchors:vec![[40.,76.]],width:6.,
            gaps:vec![[bottom,bottom+6.]],envelope:[0.,180.,12.,12.],clip:[-1000.,bottom+3.]}
    }

    fn edges(commands: &[[f32;7]]) -> Vec<([f32;2],[f32;2])> {
        let mut result=Vec::new();let mut point=[0.;2];let mut start=point;
        for c in commands {
            match c[0] as u32 {
                0=>{point=[c[1],c[2]];start=point;},
                1=>{let p=[c[1],c[2]];result.push((point,p));point=p;},
                2=>{let a=point;for i in 1..=64 {let t=i as f32/64.;let s=1.-t;
                    let p=[s*s*s*a[0]+3.*s*s*t*c[1]+3.*s*t*t*c[3]+t*t*t*c[5],
                        s*s*s*a[1]+3.*s*s*t*c[2]+3.*s*t*t*c[4]+t*t*t*c[6]];
                    result.push((point,p));point=p;
                }},
                3=>{result.push((point,start));point=start;},
                _=>panic!("invalid command"),
            }
        }
        result
    }

    #[test]
    fn vector_boundaries_match_fields_through_corner_and_gap_keyframes() {
        for bottom in [33.5,34.,34.1,35.,37.,39.,40.,40.1,70.,75.9,76.,76.1,77.,81.,82.,82.5,100.] {
            let c=contour(bottom);let commands=c.commands(0.025);
            let evaluator=c.prepare();
            assert!(commands.len()<250,"unbounded contour {}: {}",bottom,commands.len());
            assert!(commands.iter().flatten().all(|v|v.is_finite()));
            let edges=edges(&commands);
            for x in -28..=60 {for y in 60..=430 {
                let p=[x as f32/4.+0.031,y as f32/4.+0.017];
                let field=evaluator.field(p);
                if field.abs()<0.12 {continue;}
                let winding=edges.iter().fold(0,|w,(a,b)| {
                    let cross=(b[0]-a[0])*(p[1]-a[1])-(p[0]-a[0])*(b[1]-a[1]);
                    w+if a[1]<=p[1] && b[1]>p[1] && cross>0. {1}
                        else if a[1]>p[1] && b[1]<=p[1] && cross<0. {-1} else {0}
                });
                assert_eq!(winding!=0,field<0.,"bottom={bottom} p={p:?} field={field} interval={:?} commands={commands:?}",evaluator.interval(p[0]));
            }}
        }
    }

    #[test]
    fn absent_anchors_preserve_original_corners_and_entry_is_gradual() {
        let mut c=contour(100.);c.anchors.clear();
        assert!(c.field([0.5,0.5])>0.);
        let mut values=Vec::new();
        for bottom in [34.,35.,36.,37.,38.,39.,40.] {
            let c=contour(bottom);
            values.push(c.field([-1.,37.]));
        }
        assert!(values.windows(2).all(|v|v[1]<=v[0]+0.0001),"{values:?}");
        assert_eq!(panel_phase(0.),0.);assert_eq!(panel_phase(1.),1.);
    }

    fn assert_command_field_agreement(c: &PanelContour, name: &str) {
        let commands=c.commands(0.025);
        let evaluator=c.prepare();
        assert!(commands.iter().flatten().all(|v|v.is_finite()),"{name}: nonfinite commands");
        assert!(commands.len()<1000,"{name}: {} commands",commands.len());
        let edges=edges(&commands);
        let lo=c.anchors.iter().fold(c.bounds[1],|y,a|y.min(a[0])).max(c.clip[0])-1.;
        let hi=c.anchors.iter().fold(c.bounds[1]+c.bounds[3],|y,a|y.max(a[1])).min(c.clip[1])+1.;
        for row in (lo*4.).floor() as i32..=(hi*4.).ceil() as i32 {
            let y=row as f32/4.+0.017;
            let crossings: Vec<_>=edges.iter().filter_map(|(a,b)| {
                ((a[1]<=y && b[1]>y)||(b[1]<=y && a[1]>y)).then(||
                    (a[0]+(y-a[1])*(b[0]-a[0])/(b[1]-a[1]),if b[1]>a[1] {1} else {-1}))
            }).collect();
            for column in -28..=80 {
                let p=[column as f32/4.+0.031,y];
                let field=evaluator.field(p);
                if field.abs()<0.12 {continue;}
                let winding: i32=crossings.iter().filter(|(x,_)|*x>p[0]).map(|(_,w)|w).sum();
                assert_eq!(winding!=0,field<0.,"{name}: p={p:?}, field={field}, interval={:?}, commands={}",evaluator.interval(p[0]),commands.len());
            }
        }
    }

    #[test]
    fn lower_pane_vector_boundaries_match_field_at_both_tile_edges() {
        for bottom in [33.5,34.,34.1,35.,37.,39.,40.,40.1,69.5,70.,70.1,75.9,76.,76.1,77.,81.9,82.,82.1,100.] {
            let top=bottom+6.;
            let c=PanelContour {neighbors:vec![],bounds:[0.,0.,120.,180.-top],radii:[12.;4],anchors:vec![[40.-top,76.-top]],width:6.,
                gaps:vec![[-6.,0.]],envelope:[-top,180.-top,12.,12.],clip:[-3.,1000.]};
            assert_command_field_agreement(&c,&format!("lower top={top}"));
        }
    }

    #[test]
    fn multiple_anchors_and_small_asymmetric_panes_match_vector_fill() {
        for height in [2.,4.,8.,12.,24.,72.] {
            for radii in [[0.;4],[18.;4],[18.,3.,6.,12.]] {
                let radii=panel_radii([0.,0.,120.,height],radii);
                let c=PanelContour {neighbors:vec![],bounds:[0.,0.,120.,height],radii,
                    anchors:vec![[-34.,2.],[height-2.,height+34.]],width:6.,
                    gaps:vec![[-6.,0.],[height,height+6.]],envelope:[0.,height,radii[0],radii[3]],clip:[-1e6,1e6]};
                assert_command_field_agreement(&c,&format!("height={height} radii={radii:?}"));
            }
        }
    }

    #[test]
    fn lower_remote_corner_isolation() {
        let c=PanelContour {neighbors:vec![],bounds:[0.,0.,120.,140.5],radii:[12.;4],anchors:vec![[0.5,36.5]],width:6.,
            gaps:vec![[-6.,0.]],envelope:[-39.5,140.5,12.,12.],clip:[-3.,1000.]};
        let p=[0.031,134.267];
        let commands=c.commands(0.025);
        let base=commands.iter().rposition(|v|v[0]==0.).unwrap();
        let winding=|commands:&[[f32;7]]|edges(commands).iter().fold(0,|w,(a,b)| {
            let cross=(b[0]-a[0])*(p[1]-a[1])-(p[0]-a[0])*(b[1]-a[1]);
            w+if a[1]<=p[1] && b[1]>p[1] && cross>0. {1}
                else if a[1]>p[1] && b[1]<=p[1] && cross<0. {-1} else {0}
        });
        let mut absent=c.clone();absent.anchors.clear();
        eprintln!("remote-corner probe={p:?} field={} rect={} interval={:?} patch_winding={} base_winding={} no_anchor_winding={} patch_commands={} base_commands={}",
            c.field(p),panel_rect(p,c.bounds,c.radii),c.prepare().interval(p[0]),winding(&commands[..base]),winding(&commands[base..]),winding(&absent.commands(0.025)),base,commands.len()-base);
        assert_eq!(winding(&commands[base..])!=0,panel_rect(p,c.bounds,c.radii)<0.,"base rounded rectangle overfills remote corner");
    }

    #[test]
    fn midpoint_partition_keeps_fully_attached_neighbor_coverage() {
        let upper=PanelContour {neighbors:vec![([0.,61.,144.,115.],[18.;4])],
            bounds:[0.,0.,144.,55.],radii:[18.;4],anchors:vec![[56.,104.]],width:6.,
            gaps:vec![[55.,61.]],envelope:[0.,176.,18.,18.],clip:[-1e6,58.]};
        let p=[-5.625,56.125];
        assert!(upper.field(p)<0.,"neighbor coverage was lost: {}",upper.field(p));
        assert_command_field_agreement(&upper,"midpoint neighbor coverage");
        let lower=PanelContour {neighbors:vec![([0.,-61.,144.,55.],[18.;4])],
            bounds:[0.,0.,144.,115.],radii:[18.;4],anchors:vec![[-5.,43.]],width:6.,
            gaps:vec![[-6.,0.]],envelope:[-61.,115.,18.,18.],clip:[-3.,1e6]};
        assert_command_field_agreement(&lower,"opposite midpoint clip");
    }

    #[test]
    fn translated_panel_origins_preserve_vector_fill() {
        for offset in [-317.25,257.25] {
            let mut c=contour(39.);
            c.bounds[1]+=offset;
            for anchor in &mut c.anchors {for y in anchor {*y+=offset;}}
            for gap in &mut c.gaps {for y in gap {*y+=offset;}}
            c.envelope[0]+=offset;c.envelope[1]+=offset;
            c.clip[0]+=offset;c.clip[1]+=offset;
            assert_command_field_agreement(&c,&format!("translated origin {offset}"));
        }
    }

    #[test]
    fn zero_level_plateaus_keep_both_outer_interval_boundaries() {
        for offset in [0.,257.25] {
            let c=PanelContour {neighbors:vec![],bounds:[0.,offset,120.,24.],radii:[0.;4],
                anchors:vec![[offset+2.,offset+22.]],width:6.,gaps:Vec::new(),
                envelope:[offset,offset+24.,0.,0.],clip:[-1e6,1e6]};
            let evaluator=c.prepare();
            for x in [-6.,-5.999,0.,0.001] {
                let interval=evaluator.interval(x).unwrap();
                assert_eq!(interval,[offset,offset+24.],"zero-valued inside plateau lost its outer boundary at x={x}");
            }
            assert!(evaluator.interval(-6.001).is_none());
        }
    }

    #[test]
    #[ignore = "standalone desktop warm contour diagnostic; no reference-tier qualification"]
    fn warm_panel_command_generation_diagnostic() {
        let cold=std::time::Instant::now();
        panel_profiles();
        eprintln!("panel-profile-table initialization_ms={} scope=desktop-diagnostic-no-tier-qualification",cold.elapsed().as_secs_f64()*1000.);
        for (name,c) in [("upper separated",contour(37.)),("upper touching",contour(76.)),
            ("upper full",contour(100.)),("lower entry",PanelContour {neighbors:vec![],bounds:[0.,0.,120.,98.1],radii:[12.;4],
                anchors:vec![[-41.9,-5.9]],width:6.,gaps:vec![[-6.,0.]],envelope:[-81.9,98.1,12.,12.],clip:[-3.,1000.]}),
            ("flat two anchors outer gaps",PanelContour {neighbors:vec![],bounds:[0.,0.,120.,24.],radii:[0.;4],
                anchors:vec![[-34.,2.],[22.,58.]],width:6.,gaps:vec![[-6.,0.],[24.,30.]],envelope:[0.,24.,0.,0.],clip:[-1e6,1e6]}),
            ("neighbor midpoint",PanelContour {neighbors:vec![([0.,61.,144.,115.],[18.;4])],bounds:[0.,0.,144.,55.],radii:[18.;4],
                anchors:vec![[56.,104.]],width:6.,gaps:vec![[55.,61.]],envelope:[0.,176.,18.,18.],clip:[-1e6,58.]})] {
            let mut elapsed=Vec::new();let mut mesh_elapsed=Vec::new();let mut count=0;let mut triangles=0;
            let mut global=c.clone();global.clip=[-1e6,1e6];
            let evaluator=global.prepare();
            for _ in 0..101 {
                let start=std::time::Instant::now();
                let commands=std::hint::black_box(&c).commands(0.025);
                elapsed.push(start.elapsed().as_secs_f64()*1000.);count=commands.len();
                let packed: Vec<_>=commands.into_iter().map(|v|std::array::from_fn(|i|
                    if i==0 {v[i] as i32} else {(v[i]*1024.).round() as i32})).collect();
                let start=std::time::Instant::now();
                let mesh=crate::panel_mesh_with_coverage(std::hint::black_box(&packed),|p|evaluator.field(p)<=0.);
                mesh_elapsed.push(start.elapsed().as_secs_f64()*1000.);triangles=std::hint::black_box(mesh).len();
            }
            elapsed.sort_by(f64::total_cmp);mesh_elapsed.sort_by(f64::total_cmp);
            eprintln!("warm-panel-commands name={name:?} commands={count} p50_ms={} p95_ms={} p99_ms={} max_ms={} samples=101 scope=desktop-diagnostic-no-tier-qualification",elapsed[50],elapsed[95],elapsed[99],elapsed[100]);
            eprintln!("warm-panel-mesh name={name:?} triangles={triangles} p50_ms={} p95_ms={} p99_ms={} max_ms={} samples=101 scope=desktop-diagnostic-no-tier-qualification",mesh_elapsed[50],mesh_elapsed[95],mesh_elapsed[99],mesh_elapsed[100]);
        }
    }
}
