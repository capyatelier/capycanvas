use super::*;
use crate::test_support::flush_engine;
use layer_core::authored::{PaintBase, PaintSource};

#[derive(Clone, Copy)]
enum Border { Below, Same, Above, AboveWhite, Solid, Flat, Junction, Soft, WideFlat, InsideSoft, WideAboveWhite }
#[derive(Clone, Copy)]
struct Case { name: &'static str, border: Border, ink: [f32; 4] }
const CASES: [Case; 11] = [
    Case {name:"alpha below",border:Border::Below,ink:[0.03,0.11,0.19,1.]},
    Case {name:"alpha same",border:Border::Same,ink:[0.03,0.11,0.19,1.]},
    Case {name:"alpha above",border:Border::Above,ink:[0.03,0.11,0.19,1.]},
    Case {name:"translucent below",border:Border::Below,ink:[0.3,0.04,0.12,0.55]},
    Case {name:"translucent same",border:Border::Same,ink:[0.3,0.04,0.12,0.55]},
    Case {name:"translucent above",border:Border::Above,ink:[0.3,0.04,0.12,0.55]},
    Case {name:"translucent above white reference",border:Border::AboveWhite,ink:[0.3,0.04,0.12,0.55]},
    Case {name:"opaque line",border:Border::Solid,ink:[0.02,0.05,0.12,1.]},
    Case {name:"flat to flat",border:Border::Flat,ink:[0.;4]},
    Case {name:"coloured junction",border:Border::Junction,ink:[0.4,0.06,0.02,0.8]},
    Case {name:"flattened soft",border:Border::Soft,ink:[0.03,0.08,0.24,0.65]},
];
const A:[f32;4]=[0.7,0.64,0.48,1.];
const F:[f32;4]=[0.08,0.55,0.22,1.];
const N:[f32;4]=[0.04,0.13,0.45,1.];
const OTHER:[f32;4]=[0.46,0.08,0.24,1.];

#[derive(Clone,Copy)]
struct Geometry {case:Case,normal:[f32;2],offset:f32,center:[f32;2],extent:[u32;2]}
impl Geometry {
    fn distances(self,x:f32,y:f32)->[f32;2] {
        let [nx,ny]=self.normal;let p=[x-self.center[0]-self.offset,y-self.center[1]];
        [p[0]*nx+p[1]*ny,-p[0]*ny+p[1]*nx]
    }
    fn selected(self,d:[f32;2])->bool {d[0]<0. && (!matches!(self.case.border,Border::Junction) || d[1]>0.)}
    fn alpha(self,d:[f32;2])->f32 {
        if matches!(self.case.border,Border::Flat|Border::WideFlat){return 0.;}
        if matches!(self.case.border,Border::WideAboveWhite) {return f32::from(d[0].abs()<24.)*self.case.ink[3];}
        if matches!(self.case.border,Border::InsideSoft) {
            let distance=(d[0]+8.).hypot((d[1].abs()-16.).max(0.));
            return ((6.-distance)/4.).clamp(0.,1.)*self.case.ink[3];
        }
        let distance=if matches!(self.case.border,Border::Junction){d[0].abs().min(d[1].abs())}else{d[0].abs()};
        let coverage=if matches!(self.case.border,Border::Soft){((8.-distance)/6.).clamp(0.,1.)}else{f32::from(distance<4.)};
        coverage*self.case.ink[3]
    }
    fn sample(self,x:f32,y:f32,replacement:bool,plane:u8)->[f32;4] {
        let d=self.distances(x,y);let selected=self.selected(d);let alpha=self.alpha(d);
        let stroke=[self.case.ink[0]*alpha,self.case.ink[1]*alpha,self.case.ink[2]*alpha,alpha];
        let flat=if selected {if replacement {F}else{A}}else if matches!(self.case.border,Border::Junction) && d[0]<0. {OTHER}else{N};
        let flat=if matches!(self.case.border,Border::WideFlat) && d[0]>=64. {A}else{flat};
        let opaque=matches!(self.case.border,Border::Solid|Border::Flat|Border::Junction|Border::Soft|Border::WideFlat|Border::InsideSoft);
        if opaque {return over(stroke,flat);}
        if plane==1 {return stroke;}
        let fill=if replacement && selected {F}else{[0.;4]};
        match self.case.border {
            Border::Same=>over(stroke,fill),
            Border::Above|Border::AboveWhite|Border::WideAboveWhite=>fill.map(|v|v*(1.-alpha)),
            _=>fill,
        }
    }
    fn pixel(self,x:u32,y:u32,replacement:bool,plane:u8)->[f32;4] {
        let mut pixel=[0_f64;4];
        for sy in 0..16 {for sx in 0..16 {
            let sample=self.sample(x as f32+(sx as f32+0.5)/16.,y as f32+(sy as f32+0.5)/16.,replacement,plane);
            for channel in 0..4 {pixel[channel]+=f64::from(sample[channel])/256.;}
        }}
        pixel.map(|value|value as f32)
    }
    fn projection(self,tolerance:f32,anchors:&layer_core::FillAnchors,primary:&SelectionPixels,half:bool)->Vec<u8> {
        let [width,height]=self.extent;
        let original=(0..height).flat_map(|y|(0..width).map(move|x|{let pixel=self.pixel(x,y,false,0);if half {pixel.map(|v|layer_core::color::f16::from_f32(v).to_f32())}else{pixel}})).collect::<Vec<_>>();
        let seed=original[((self.center[1] as u32+30)*width+self.center[0] as u32-32) as usize];
        let distance=|pixel:[f32;4]|pixel.iter().zip(seed).map(|(p,s)|(p-s)*(p-s)).sum::<f32>();
        let mut result=Vec::with_capacity((width*height*4) as usize);
        for y in 0..height {for x in 0..width {
            let pixel=original[(y*width+x) as usize];let mut output=pixel;
            let d=self.distances(x as f32+0.5,y as f32+0.5);
            if self.selected(d) {
                let mut neighbour=pixel;let mut owned=false;let mut legacy_neighbour=false;
                if let Some(q)=anchors.get([x,y])
                    && q[0]<width && q[1]<height && value(primary,q[0],q[1])>0 {
                    owned=true;
                    let delta=[i64::from(x)-i64::from(q[0]),i64::from(y)-i64::from(q[1])];
                    let steps=delta[0].abs().max(delta[1].abs());
                    let length=delta[0]*delta[0]+delta[1]*delta[1];
                    legacy_neighbour=steps==0;
                    if length*4>9 {owned=false;}
                    if steps!=0 && owned {
                        let mut current=original[(q[1]*width+q[0]) as usize];let mut plateau=false;
                        for i in 1_i64..=24 {
                            if i*i*length>24*24*steps*steps {break;}
                            let at=std::array::from_fn::<_,2,_>(|axis|i64::from(q[axis])+delta[axis].signum()*((delta[axis].abs()*i+steps/2)/steps));
                            if at[0]<0 || at[1]<0 || at[0]>=i64::from(width) || at[1]>=i64::from(height) {legacy_neighbour=true;break;}
                            let next=original[(at[1] as u32*width+at[0] as u32) as usize];
                            if fill_equal_cpu(current,next,half) && distance(current)>=tolerance*tolerance {
                                if i-1<steps {owned=false;}
                                neighbour=current;plateau=true;break;
                            }
                            current=next;
                        }
                        if !plateau {neighbour=current;}
                    }
                    if legacy_neighbour {
                        let mut maximum=-1.;
                        for ny in y.saturating_sub(24)..=(y+24).min(height-1) {for nx in x.saturating_sub(24)..=(x+24).min(width-1) {
                            if nx.abs_diff(x).pow(2)+ny.abs_diff(y).pow(2)>24*24 {continue;}
                            let candidate=original[(ny*width+nx) as usize];let score=distance(candidate);
                            if score>maximum {maximum=score;neighbour=candidate;}
                        }}
                    }
                }
                output=if distance(pixel)<tolerance*tolerance {F}else if !owned {pixel}else if legacy_neighbour {
                    fill_fraction_cpu(pixel,seed,neighbour,half).map_or(pixel,|t|std::array::from_fn(|c|neighbour[c]+t*(F[c]-neighbour[c])))
                }else{
                    let delta=std::array::from_fn::<_,4,_>(|channel|seed[channel]-neighbour[channel]);
                    let square=delta.iter().map(|v|v*v).sum::<f32>();
                    let fraction=if square==0. {0.}else{(pixel.iter().zip(neighbour).zip(delta).map(|((p,n),d)|(p-n)*d).sum::<f32>()/square).clamp(0.,1.)};
                    let fraction=fraction.min(seed_limit(pixel,seed));
                    std::array::from_fn(|channel|pixel[channel]+fraction*(F[channel]-seed[channel]))
                };
            }
            for channel in 0..4 {
                let code=if channel==3 {output[3]}else if output[3]>0. {layer_core::color::srgb_encode(output[channel]/output[3])}else{0.};
                result.push((code.clamp(0.,1.)*255.).round() as u8);
            }
        }}
        result
    }
    fn image(self,replacement:bool,plane:u8)->Arc<layer_core::color::source::SourceImage> {self.image_depth(replacement,plane,SampleDepth::F32)}
    fn image_depth(self,replacement:bool,plane:u8,depth:SampleDepth)->Arc<layer_core::color::source::SourceImage> {
        depth_source(self.extent,depth,RgbSpace::Srgb,self.extent[0] as usize*self.extent[1] as usize*16+4096,|x,y| {
            let mut pixel=self.pixel(x,y,replacement,plane);
            if pixel[3]>0. {for channel in 0..3 {pixel[channel]/=pixel[3];if !depth.is_float(){pixel[channel]=layer_core::color::srgb_encode(pixel[channel]);}}}
            pixel
        })
    }
    fn document(self,replacement:bool)->(Document,SourceTarget,Vec<SourceTarget>) {
        let mut document=paint_document(self.extent,self.case.name);
        let root=document.artwork.root;document.artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Linear;
        placement::set_source(&mut document,self.image(replacement,0));
        let target=target(&document);
        let separate=matches!(self.case.border,Border::Below|Border::Above|Border::AboveWhite|Border::WideAboveWhite);
        let reference=if separate {
            let (_,reference)=placement::append_paint(&mut document,"Reference",PaintSource {domain:self.extent,color_mode:Default::default(),
                raster:Default::default(),base:Some(PaintBase::new(self.image(false,1).into())),operations:Default::default()});
            if matches!(self.case.border,Border::Below) {
                let stack=document.composition().result;document.artwork.stacks.get_mut(stack).unwrap().entries.reverse();
                document=Document::from_artwork(document.artwork).unwrap();
            }
            reference
        }else{target};
        let mut references=vec![reference];
        if matches!(self.case.border,Border::AboveWhite|Border::WideAboveWhite) {
            let white=depth_source(self.extent,SampleDepth::F32,RgbSpace::Srgb,self.extent[0] as usize*self.extent[1] as usize*16+4096,|_,_|[1.;4]);
            let (_,background)=placement::append_paint(&mut document,"Opaque white reference",PaintSource {domain:self.extent,color_mode:Default::default(),
                raster:Default::default(),base:Some(PaintBase::new(white.into())),operations:Default::default()});
            references.push(background);
        }
        (document,target,references)
    }
}
fn fill_fraction_cpu(pixel:[f32;4],seed:[f32;4],neighbour:[f32;4],half:bool)->Option<f32> {
    let interval=|value:f32| {
        let magnitude=value.abs();
        let (previous,next)=if half {
            let bits=layer_core::color::f16::from_f32(magnitude).to_bits();
            (layer_core::color::f16::from_bits(bits.saturating_sub(1)).to_f32(),layer_core::color::f16::from_bits(bits+1).to_f32())
        }else{let bits=magnitude.to_bits();(f32::from_bits(bits.saturating_sub(1)),f32::from_bits(bits+1))};
        let high=(next-magnitude)*0.5;let low=if magnitude==0. {high}else{(magnitude-previous)*0.5};
        if value<0. {(high,low)}else{(low,high)}
    };
    let greater=|a:(i64,i64),b:(i64,i64)|i128::from(a.0)*i128::from(b.1)>i128::from(b.0)*i128::from(a.1);
    let mut lower=(0,1);let mut upper=(1,1);
    for channel in 0..4 {
        let (pl,ph)=interval(pixel[channel]);let (sl,sh)=interval(seed[channel]);let (nl,nh)=interval(neighbour[channel]);
        let largest=(pixel[channel].abs()+pl.max(ph)).max(seed[channel].abs()+sl.max(sh)).max(neighbour[channel].abs()+nl.max(nh));
        let exponent=((largest.to_bits()>>23)&255) as i32-127;let scale=2_f32.powi((27-exponent).min(127));
        let quantize=|v:f32,lo:f32,hi:f32|((v*scale).round_ties_even() as i64,(lo*scale+0.5).ceil() as i64,(hi*scale+0.5).ceil() as i64);
        let (qp,pl,ph)=quantize(pixel[channel],pl,ph);let (qs,sl,sh)=quantize(seed[channel],sl,sh);let (qn,nl,nh)=quantize(neighbour[channel],nl,nh);
        let a=qs+sh-qn-nh;let b=qp-pl-qn-nh;
        if a==0 && b>0 {return None;}
        if a>0 && greater((b,a),lower) {lower=(b,a);}
        if a<0 && greater(upper,(-b,-a)) {upper=(-b,-a);}
        let c=qs-sl-qn+nl;let d=qp+ph-qn+nl;
        if c==0 && d<0 {return None;}
        if c>0 && greater(upper,(d,c)) {upper=(d,c);}
        if c<0 && greater((-d,-c),lower) {lower=(-d,-c);}
    }
    (!greater(lower,upper)).then(||0.5*(lower.0 as f32/lower.1 as f32+upper.0 as f32/upper.1 as f32))
}
fn fill_equal_cpu(a:[f32;4],b:[f32;4],half:bool)->bool {fill_fraction_cpu(a,b,b,half).is_some()}
fn seed_limit(pixel:[f32;4],seed:[f32;4])->f32 {
    let mut limit=1_f32;
    for channel in 0..4 {if seed[channel]>0. {limit=limit.min(pixel[channel]/seed[channel]);}}
    for channel in 0..3 {
        let alpha=seed[3]-seed[channel];
        if alpha>0. {limit=limit.min((pixel[3]-pixel[channel])/alpha);}
    }
    limit.clamp(0.,1.)
}
fn over(top:[f32;4],bottom:[f32;4])->[f32;4] {std::array::from_fn(|channel|top[channel]+bottom[channel]*(1.-top[3]))}

#[test]
fn fill_recolours_antialiased_edges_like_direct_geometry() {
    let mut failures=Vec::new();
    let poses=[
        (0_f32,0.25_f32,[48_f32,40_f32],[96,80]),(17.,0.6,[48.,40.],[96,80]),(-31.,0.35,[48.,40.],[96,80]),
        (17.,0.6,[256.,40.],[384,80]),(17.,0.6,[256.,256.],[384,384]),(17.,0.6,[48.,0.],[96,80]),
    ];
    let rows=CASES.into_iter().flat_map(|case|poses.map(move |(angle,offset,center,extent)|(case,angle,offset,center,extent,false))).chain([
        (Case {name:"64px neighbouring flat",border:Border::WideFlat,ink:[0.;4]},0.,0.25,[64.,48.],[160,96],false),
        (Case {name:"12px soft stroke inside flat",border:Border::InsideSoft,ink:[0.03,0.08,0.24,0.65]},0.,0.25,[64.,48.],[160,96],false),
        (Case {name:"all visible opaque target",border:Border::WideFlat,ink:[0.;4]},0.,0.25,[64.,48.],[160,96],true),
        (Case {name:"all visible transparent wide target",border:Border::WideAboveWhite,ink:[0.03,0.11,0.19,1.]},0.,0.25,[80.,48.],[192,96],true),
    ]);
    for (case,degrees,offset,center,extent,all_visible) in rows {
        let angle=degrees.to_radians();let geometry=Geometry {case,normal:[angle.cos(),angle.sin()],offset,center,extent};
        let (document,target,references)=geometry.document(false);
        let (direct,_,_)=geometry.document(true);
        let mut direct_renderer=WgpuRasterizer::new_native_headless(direct.composition().color).unwrap();
        frame(&mut direct_renderer,&direct,extent);let expected=direct_renderer.readback_srgb_rgba8().unwrap();
        let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
            crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
        flush_engine(&mut engine);let before=engine.backend_mut().readback_srgb_rgba8().unwrap();
        let scope=if all_visible {layer_core::SceneScope::All}else if references.len()==1 {layer_core::SceneScope::Raw(references[0])}else {
            layer_core::SceneScope::Members(references.iter().map(|reference|engine.document().scene().source_owner(*reference).unwrap()).collect::<Vec<_>>().into())
        };
        let mut query=request(RegionSource::Scene {snapshot:engine.document().snapshot(),scope},
            rectangle([0,0,1,1]),RegionRefinement {close_gaps:false,edges:RegionEdges::LineMiddle,smooth_edges:true},1./32.);
        query.colour_mode=if all_visible {layer_render::RegionColourMode::Target(target)}
            else if matches!(case.border,Border::Solid|Border::Flat|Border::Junction|Border::Soft|Border::WideFlat|Border::InsideSoft) {layer_render::RegionColourMode::ColourOnSolid}
            else {layer_render::RegionColourMode::AlphaBorder};
        query.enclosure=None;query.position=[center[0] as u32-32,center[1] as u32+30];query.fill=Some(Default::default());
        let snapshot=engine.document().snapshot();let position=Point {x:query.position[0] as f32,y:query.position[1] as f32};let tolerance=query.tolerance;
        let result=receive_request(engine.backend_mut(),query);
        let anchors=result.anchors.clone();
        let primary=result.pixels.clone();
        if matches!(case.border,Border::WideAboveWhite) {
            let band=result.underfill.as_ref().unwrap();
            if value(band,56,48)!=255 {failures.push(format!("wide reference soft rim 56: {} != 255",value(band,56,48)));}
            for x in 57..80 {
                let support=if x<59 {64}else{0};
                if value(band,x,48)>support {failures.push(format!("wide reference plateau {x}: {} > {support}",value(band,x,48)));}
            }
        }
        let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(),extent,[0;2]);
        coverage.source.default_coverage=0.;coverage.selection=Some(Selection::pixels(result.pixels));
        let edges=result.underfill.as_ref().map(|_|Box::new(layer_core::FillEdges {snapshot:snapshot.clone(),position,tolerance,anchors:anchors.clone(),
            lines:if matches!(case.border,Border::Below|Border::Above|Border::AboveWhite|Border::WideAboveWhite){references.iter().map(|reference|layer_core::FillLine {snapshot:snapshot.clone(),scope:layer_core::SceneScope::Raw(*reference),fill_above:matches!(case.border,Border::Above|Border::AboveWhite|Border::WideAboveWhite)}).collect()}else{Vec::new()}}));
        let operation=layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,kind:layer_core::RasterOperationKind::Fill {
            color:F,texture:layer_core::FillTexture {strength:1.,paper:None},alpha_locked:false,edges,underfill:result.underfill.map(Selection::pixels)}};
        engine.append_raster_operation(target,operation).unwrap();flush_engine(&mut engine);
        let actual=engine.backend_mut().readback_srgb_rgba8().unwrap();
        let projected=matches!(case.border,Border::Junction|Border::Soft|Border::InsideSoft).then(||geometry.projection(tolerance,anchors.as_deref().expect("colour fill returns assignment anchors"),&primary,engine.backend().device.working_format()==wgpu::TextureFormat::Rgba16Float));
        let mut direct_errors=Vec::new();
        let mut maximum=[0u8;4];let mut first_bad=None;let mut preserved_seed=0;let mut formula_preserved=0;
        for y in 0..extent[1] {for x in 0..extent[0] {
            let d=geometry.distances(x as f32+0.5,y as f32+0.5);let selected=geometry.selected(d);
            let edge=d[0].abs()<12. || matches!(case.border,Border::Junction) && d[1].abs()<12.
                || matches!(case.border,Border::InsideSoft) && (d[0]+8.).hypot((d[1].abs()-16.).max(0.))<6.+std::f32::consts::FRAC_1_SQRT_2;
            let complex=matches!(case.border,Border::Junction|Border::Soft)
                || matches!(case.border,Border::InsideSoft) && (d[0]+8.).hypot((d[1].abs()-16.).max(0.))<6.+std::f32::consts::FRAC_1_SQRT_2;
            let outside=d[0]>std::f32::consts::FRAC_1_SQRT_2 || matches!(case.border,Border::Junction) && d[1]< -std::f32::consts::FRAC_1_SQRT_2;
            let unchanged=outside || complex && !selected;
            let limit=if unchanged {1}else if !edge {0}else if complex {2}else{3};
            let at=((y*extent[0]+x)*4) as usize;
            let reference=if unchanged {&before[at..at+4]}else if complex {&projected.as_ref().unwrap()[at..at+4]}else{&expected[at..at+4]};
            if complex && selected && edge && actual[at..at+4]==before[at..at+4] {
                if expected[at..at+4]!=before[at..at+4] {preserved_seed+=1;}
                if reference!=&before[at..at+4] {formula_preserved+=1;}
            }
            if selected && edge {direct_errors.push(actual[at..at+4].iter().zip(&expected[at..at+4]).map(|(a,b)|a.abs_diff(*b)).max().unwrap());}
            let category=if unchanged {3}else if !edge {0}else if complex {2}else{1};
            for channel in 0..4 {
                let difference=actual[at+channel].abs_diff(reference[channel]);maximum[category]=maximum[category].max(difference);
                if difference>limit && first_bad.is_none() {first_bad=Some(format!("{} angle {degrees} offset {offset} center {center:?} at {x},{y} channel {channel}: {} != {} limit {limit}",case.name,actual[at+channel],reference[channel]));}
            }
        }}
        direct_errors.sort_unstable();let direct_max=direct_errors.last().copied().unwrap_or(0);
        let direct_p95=direct_errors.get((direct_errors.len()*95).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0);
        eprintln!("FILL_EDGE_PRESERVED case={} angle={degrees} offset={offset} center={center:?} seed_side={preserved_seed} formula_side={formula_preserved}",case.name);
        eprintln!("FILL_EDGE_DIRECT case={} angle={degrees} offset={offset} center={center:?} region_edge_max={direct_max} p95={direct_p95}",case.name);
        eprintln!("FILL_EDGE_ROW case={} angle={degrees} offset={offset} center={center:?} maximum_interior_edge_complex_far={maximum:?} first_bad={first_bad:?}",case.name);
        if let Some(failure)=first_bad {failures.push(failure);}
    }
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}

fn unmix_gpu(r:&WgpuRasterizer,cases:&[[[f32;4];4]],codes:[u32;4])->Vec<([f32;4],bool,f32)> {
    use wgpu::util::DeviceExt;
    let source=[include_str!("sdr_color.wgsl"),include_str!("fill_unmix.wgsl"),r#"
struct TestCase {pixel:vec4<f32>,seed:vec4<f32>,neighbour:vec4<f32>,paint:vec4<f32>}
struct TestResult {projected:vec4<f32>,data:vec4<u32>}
@group(0) @binding(0) var<storage,read> cases:array<TestCase>;
@group(0) @binding(1) var<storage,read_write> results:array<TestResult>;
fn stored(value:vec4<f32>)->vec4<f32> {
    var result=value;
    if fill_precision.codes.x!=0u {
        let maximum=f32(fill_precision.codes.x);
        result=vec4(sdr_decode(round(sdr_encode(value.rgb,0u)*maximum)/maximum,0u),round(value.a*maximum)/maximum);
    }
    if fill_precision.codes.z==10u {result=quantizeToF16(result);}
    return result;
}
@compute @workgroup_size(64)
fn check(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=arrayLength(&cases) {return;}
    let item=cases[id.x];let pixel=stored(item.pixel);let seed=stored(item.seed);let neighbour=stored(item.neighbour);
    let delta=seed-neighbour;
    let fraction=clamp(dot(pixel-neighbour,delta)/max(dot(delta,delta),1e-12),0.,1.);
    results[id.x]=TestResult(fill_reconstruct(pixel,seed,neighbour,item.paint,1.),vec4(u32(fill_fraction(pixel,seed,neighbour)<0.),bitcast<u32>(fraction),0u,0u));
}
"#].concat();
    let shader=r.device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("fill unmix normal math test"),source:wgpu::ShaderSource::Wgsl(source.into())});
    let io=crate::bindings::layout(&r.device,"fill unmix test io",&[
        crate::bindings::buffer(0,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Storage {read_only:true},false,None),
        crate::bindings::buffer(1,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Storage {read_only:false},false,None),
    ]);
    let empty=crate::bindings::layout(&r.device,"fill unmix empty",&[]);
    let precision_layout=crate::bindings::layout(&r.device,"fill unmix test precision",&[
        crate::bindings::buffer(4,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Uniform,false,None),
    ]);
    let layout=r.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:Some("fill unmix normal math test"),bind_group_layouts:&[Some(&io),Some(&empty),Some(&precision_layout)],immediate_size:0});
    let pipeline=r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {label:Some("fill unmix normal math test"),layout:Some(&layout),module:&shader,entry_point:Some("check"),compilation_options:Default::default(),cache:None});
    let bytes=cases.iter().flatten().flatten().flat_map(|value|value.to_ne_bytes()).collect::<Vec<_>>();
    let input=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("fill unmix cases"),contents:&bytes,usage:wgpu::BufferUsages::STORAGE});
    let output=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill unmix results"),size:cases.len() as u64*32,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false});
    let mut bytes=[1f32,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.].into_iter().flat_map(f32::to_ne_bytes).collect::<Vec<_>>();
    bytes.extend(codes.into_iter().flat_map(u32::to_ne_bytes));bytes.extend([0_u8;16]);
    let precision=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("fill unmix source precision"),contents:&bytes,usage:wgpu::BufferUsages::UNIFORM});
    let binding=crate::bindings::group(&r.device,"fill unmix test io",&io,[input.as_entire_binding(),output.as_entire_binding()]);
    let precision_binding=r.device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("fill unmix test precision"),layout:&precision_layout,entries:&[wgpu::BindGroupEntry {binding:4,resource:precision.as_entire_binding()}]});
    let mut encoder=r.device.create_command_encoder(&Default::default());
    {let mut pass=encoder.begin_compute_pass(&Default::default());pass.set_pipeline(&pipeline);pass.set_bind_group(0,&binding,&[]);pass.set_bind_group(2,&precision_binding,&[]);pass.dispatch_workgroups((cases.len() as u32).div_ceil(64),1,1);}
    r.queue.submit([encoder.finish()]);
    let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,&output)).unwrap();
    bytes.as_chunks::<32>().0.iter().map(|bytes| {
        let word=|index|u32::from_le_bytes(bytes[index*4..index*4+4].try_into().unwrap());
        (std::array::from_fn(|i|f32::from_bits(word(i))),word(4)!=0,f32::from_bits(word(5)))
    }).collect()
}

#[test]
fn fill_unmix_precision_preserves_every_two_colour_fraction() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cases=Vec::new();
    for (seed,neighbour) in [(A,N),([0.91,0.07,0.12,1.],[0.02,0.83,0.53,1.]),([0.,0.,0.,1.],[1.,1.,1.,1.])] {
        for step in 0..=256 {
            let t=step as f64/256.;let pixel=std::array::from_fn(|channel|(t*f64::from(seed[channel])+(1.-t)*f64::from(neighbour[channel])) as f32);
            cases.push([pixel,seed,neighbour,F]);
        }
    }
    let counterexample=cases.len();cases.push([[0.2645,0.276,0.324,1.],A,N,F]);
    cases.push([[0.412,0.064,0.064,1.],A,N,F]);
    cases.push([[0.035,0.11,0.3675,1.],A,[0.02,0.05,0.12,1.],F]);
    cases.push([[0.,0.276,0.324,1.],A,N,F]);
    cases.push([[0.25,0.9,0.4,0.9],A,N,F]);
    for maximum in [0,255,65535] {for (mantissa,minimum) in [(10,24),(23,149)] {
        let results=unmix_gpu(&r,&cases,[maximum,0,mantissa,minimum]);
        for (index,(_,ambiguous,_)) in results[..counterexample].iter().enumerate() {assert!(!ambiguous,"two-colour fraction {index}, codec {maximum}, mantissa {mantissa}");}
        assert!(results[counterexample].1,"non-collinear soft mixture must remain ambiguous");
        assert!(results[counterexample+1].1,"non-collinear junction mixture must remain ambiguous");
        if maximum==0 && mantissa==23 {
            let golden=[0.06971533,0.24772481,0.24231611,1.];
            assert!(results[counterexample].0.iter().zip(golden).all(|(a,b)|(a-b).abs()<1e-6));
            for (index,golden) in [(2,[0.004,0.1055,0.3545,1.]),(3,[0.,0.276,0.324,1.]),(4,[0.25,0.9,0.4,0.9])] {
                assert!(results[counterexample+index].0.iter().zip(golden).all(|(a,b)|(a-b).abs()<1e-6),"gamut case {index}: {:?}",results[counterexample+index].0);
            }
        }
    }}
}

#[test]
fn fill_imported_u8_edge_keeps_precision_after_an_unrelated_page_edit() {
    let extent=[384,80];let angle=17_f32.to_radians();
    let geometry=Geometry {case:Case {name:"Imported U8",border:Border::Flat,ink:[0.;4]},normal:[angle.cos(),angle.sin()],offset:0.6,center:[96.,40.],extent};
    for edited in [false,true] {
        let create=|replacement| {
            let (mut document,target,_)=geometry.document(replacement);
            placement::set_source(&mut document,geometry.image_depth(replacement,0,SampleDepth::U8));
            let root=document.artwork.root;document.artwork.compositions.get_mut(root).unwrap().color.depth=SampleDepth::F32;
            let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
            let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
                crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
            flush_engine(&mut engine);
            if edited {
                let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(),extent,[0;2]);
                coverage.source.default_coverage=0.;coverage.selection=Some(rectangle([320,20,336,36]));
                engine.append_raster_operation(target,layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,
                    kind:layer_core::RasterOperationKind::Fill {color:[0.2,0.3,0.4,1.],texture:Default::default(),alpha_locked:false,edges:None,underfill:None}}).unwrap();
                flush_engine(&mut engine);
                let data=engine.document().scene().raster(target).unwrap().try_data().unwrap().unwrap();
                assert!(!data.tiles.is_empty());assert!(data.tiles.keys().all(|key|key.coordinate[0]>0));
            }
            (engine,target)
        };
        let (mut direct,_)=create(true);let expected=direct.backend_mut().readback_srgb_rgba8().unwrap();
        let (mut engine,target)=create(false);let before=engine.backend_mut().readback_srgb_rgba8().unwrap();
        let snapshot=engine.document().snapshot();
        let mut query=request(RegionSource::Scene {snapshot:snapshot.clone(),scope:layer_core::SceneScope::Raw(target)},rectangle([0,0,1,1]),
            RegionRefinement {close_gaps:false,edges:RegionEdges::LineMiddle,smooth_edges:true},1./32.);
        query.colour_mode=layer_render::RegionColourMode::ColourOnSolid;
        query.enclosure=None;query.position=[64,70];query.fill=Some(Default::default());
        let result=receive_request(engine.backend_mut(),query);
        let anchors=result.anchors.clone();
        let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(),extent,[0;2]);
        coverage.source.default_coverage=0.;coverage.selection=Some(Selection::pixels(result.pixels));
        engine.append_raster_operation(target,layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,
            kind:layer_core::RasterOperationKind::Fill {color:F,texture:layer_core::FillTexture {strength:1.,paper:None},alpha_locked:false,
                edges:Some(Box::new(layer_core::FillEdges {snapshot,position:Point {x:64.,y:70.},tolerance:1./32.,anchors,lines:Vec::new()})),
                underfill:result.underfill.map(Selection::pixels)}}).unwrap();flush_engine(&mut engine);
        let actual=engine.backend_mut().readback_srgb_rgba8().unwrap();
        for y in 0..extent[1] {for x in 0..extent[0] {
            let d=geometry.distances(x as f32+0.5,y as f32+0.5)[0];let at=((y*extent[0]+x)*4) as usize;
            let outside=d>std::f32::consts::FRAC_1_SQRT_2;
            let reference=if outside {&before[at..at+4]}else{&expected[at..at+4]};
            let limit=if outside {1}else if d.abs()>12. {0}else{3};
            for channel in 0..4 {assert!(actual[at+channel].abs_diff(reference[channel])<=limit,
                "U8 edited={edited} at {x},{y} channel {channel}: {} != {} limit {limit}",actual[at+channel],reference[channel]);}
        }}
    }
}

#[test]
fn fill_support_without_a_valid_line_profile_preserves_the_source() {
    let extent=[64,64];
    let mut document=paint_document(extent,"Unsupported fringe");
    let source=depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,64*64*16+4096,|_,_|[0.2,0.3,0.4,0.25]);
    placement::set_source(&mut document,source);
    let target=target(&document);
    let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
        crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    flush_engine(&mut engine);let before=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let mut main=vec![0;8*64];main[32*8+4]=1|(1<<8);
    let mut band=vec![0;8*64];band[32*8+4]=1;
    let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(),extent,[0;2]);
    coverage.source.default_coverage=0.;coverage.selection=Some(Selection::pixels(Arc::new(SelectionPixels::new(extent,[32,32,35,33],main).unwrap())));
    let snapshot=engine.document().snapshot();
    engine.append_raster_operation(target,layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,
        kind:layer_core::RasterOperationKind::Fill {color:F,texture:layer_core::FillTexture {strength:1.,paper:None},alpha_locked:false,
            edges:Some(Box::new(layer_core::FillEdges {snapshot,position:Point {x:8.,y:8.},tolerance:1./32.,anchors:None,lines:Vec::new()})),
            underfill:Some(Selection::pixels(Arc::new(SelectionPixels::new(extent,[32,32,33,33],band).unwrap())))}}).unwrap();
    flush_engine(&mut engine);let actual=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let support=(32*64+32)*4;let selected=(32*64+34)*4;
    assert_eq!(&actual[support..support+4],&before[support..support+4]);
    assert_ne!(&actual[selected..selected+4],&before[selected..selected+4]);
}

#[test]
fn fill_unmix_reconstructs_quantised_red_blue_edges_without_old_red() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let seed=[1.,0.,0.,1.];let neighbour=[0.,0.,1.,1.];let paint=[0.,1.,0.,1.];
    for (maximum,mantissa,minimum) in [(255,10,24),(255,23,149),(0,10,24)] {
        let pixel=if maximum==0 {[0.75,0.,0.25,1.]}else{[layer_core::color::srgb_decode(225./255.),0.,layer_core::color::srgb_decode(137./255.),1.]};
        let result=unmix_gpu(&r,&[[pixel,seed,neighbour,paint]],[maximum,0,mantissa,minimum]);
        assert!(!result[0].1);
        let actual=result[0].0.map(|value|(layer_core::color::srgb_encode(value).clamp(0.,1.)*255.).round() as u8);
        assert_eq!(actual,[0,225,137,255],"codec {maximum}, mantissa {mantissa}");
    }
}

#[test]
fn recolouring_a_shape_never_paints_outside_it() {
    let extent=[128u32,96];
    let coverage=|x:u32,y:u32|((32.-(x as f32+0.5-64.).abs()).min(24.-(y as f32+0.5-48.).abs())+0.25).clamp(0.,1.);
    for opaque in [false,true] {
        for mode in [layer_render::RegionColourMode::AlphaBorder,layer_render::RegionColourMode::ColourOnSolid] {
            let mut document=paint_document(extent,"Recoloured shape");
            placement::set_source(&mut document,crate::test_support::depth_source(extent,SampleDepth::F32,layer_core::color::RgbSpace::Srgb,128*96*16+4096,|x,y| {
                let alpha=coverage(x,y);
                if opaque {[1.,1.-alpha,1.-alpha,1.]}else{[1.,0.,0.,alpha]}
            }));
            let target=target(&document);
            let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
            let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
                crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
            flush_engine(&mut engine);let before=engine.backend_mut().readback_srgb_rgba8().unwrap();
            let snapshot=engine.document().snapshot();
            let mut query=request(RegionSource::Scene {snapshot:snapshot.clone(),scope:layer_core::SceneScope::Raw(target)},
                rectangle([0,0,1,1]),RegionRefinement {close_gaps:false,edges:RegionEdges::LineMiddle,smooth_edges:true},1./32.);
            query.colour_mode=mode;query.enclosure=None;query.position=[64,48];query.fill=Some(Default::default());
            let tolerance=query.tolerance;
            let result=receive_request(engine.backend_mut(),query);
            let mut selection=layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(),extent,[0;2]);
            selection.source.default_coverage=0.;selection.selection=Some(Selection::pixels(result.pixels));
            let edges=result.underfill.as_ref().map(|_|Box::new(layer_core::FillEdges {snapshot:snapshot.clone(),position:Point {x:64.,y:48.},tolerance,anchors:result.anchors.clone(),lines:Vec::new()}));
            let operation=layer_core::RasterOperation {placement:Affine::IDENTITY,coverage:selection,kind:layer_core::RasterOperationKind::Fill {
                color:F,texture:layer_core::FillTexture {strength:1.,paper:None},alpha_locked:false,edges,underfill:result.underfill.map(Selection::pixels)}};
            engine.append_raster_operation(target,operation).unwrap();flush_engine(&mut engine);
            let actual=engine.backend_mut().readback_srgb_rgba8().unwrap();
            for y in 0..96u32 {for x in 0..128u32 {
                let at=((y*128+x)*4) as usize;
                if coverage(x,y)==0. {assert_eq!(actual[at..at+4],before[at..at+4],"{mode:?} opaque={opaque} {x},{y}");}
            }}
        }
    }
}

#[test]
fn alpha_below_zero_tolerance_keeps_the_transparent_band_pixel() {
    let extent=[96,80];
    let geometry=Geometry {case:CASES[0],normal:[1.,0.],offset:0.25,center:[48.,40.],extent};
    let (document,target,references)=geometry.document(false);
    let (direct,_,_)=geometry.document(true);
    let mut direct_renderer=WgpuRasterizer::new_native_headless(direct.composition().color).unwrap();
    frame(&mut direct_renderer,&direct,extent);
    let expected=direct_renderer.readback_srgb_rgba8().unwrap();
    let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
        crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    flush_engine(&mut engine);
    let snapshot=engine.document().snapshot();
    let mut query=request(RegionSource::Scene {snapshot:snapshot.clone(),scope:layer_core::SceneScope::Raw(references[0])},
        rectangle([0,0,1,1]),RegionRefinement {close_gaps:false,edges:RegionEdges::LineMiddle,smooth_edges:true},0.);
    query.colour_mode=layer_render::RegionColourMode::AlphaBorder;
    query.enclosure=None;query.position=[16,70];query.fill=Some(Default::default());
    let result=receive_request(engine.backend_mut(),query);
    assert_eq!(value(&result.pixels,43,40),255,"transparent pixel belongs to the primary mask");
    assert_eq!(value(result.underfill.as_ref().unwrap(),43,40),255,"transparent pixel belongs to the under-fill band");
    let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(),extent,[0;2]);
    coverage.source.default_coverage=0.;coverage.selection=Some(Selection::pixels(result.pixels));
    let edges=Box::new(layer_core::FillEdges {snapshot:snapshot.clone(),position:Point {x:16.,y:70.},tolerance:0.,anchors:result.anchors,
        lines:references.iter().map(|reference|layer_core::FillLine {snapshot:snapshot.clone(),scope:layer_core::SceneScope::Raw(*reference),fill_above:false}).collect()});
    let operation=layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,kind:layer_core::RasterOperationKind::Fill {
        color:F,texture:layer_core::FillTexture {strength:1.,paper:None},alpha_locked:false,edges:Some(edges),underfill:result.underfill.map(Selection::pixels)}};
    engine.append_raster_operation(target,operation).unwrap();flush_engine(&mut engine);
    let actual=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let at=((40*96+43)*4) as usize;
    assert_eq!(actual[at..at+4],expected[at..at+4],"zero-tolerance AlphaBelow fill must paint its transparent band pixel");
}
