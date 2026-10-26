use super::*;
use layer_render::panel_geometry::{PanelContour, PanelEvaluator, panel_radii};

const ZERO_LEVEL_EPSILON: f32 = 1e-5;

fn contours(surface: &PanelSurface) -> Vec<PanelContour> {
    let top=surface.groups.first().unwrap();
    let bottom=surface.groups.last().unwrap();
    let facing=if surface.direction>0. {[0,3]} else {[1,2]};
    let first=panel_radii(top.bounds,top.radii);
    let last=panel_radii(bottom.bounds,bottom.radii);
    surface.groups.iter().enumerate().map(|(i,group)| {
        let b=group.bounds;
        let r=panel_radii(b,group.radii);
        let radii=if surface.direction>0. {r} else {[r[1],r[0],r[3],r[2]]};
        let mut gaps: Vec<_>=surface.groups.windows(2).map(|w|
            [w[0].bounds[1]+w[0].bounds[3]-b[1],w[1].bounds[1]-b[1]]).collect();
        if surface.anchors.iter().any(|a|a[1]<top.bounds[1]) {
            gaps.push([top.bounds[1]-6.-b[1],top.bounds[1]-b[1]]);
        }
        let end=bottom.bounds[1]+bottom.bounds[3];
        if surface.anchors.iter().any(|a|a[1]+a[3]>end) {gaps.push([end-b[1],end+6.-b[1]]);}
        PanelContour {neighbors:surface.groups.iter().enumerate().filter(|(j,_)|*j!=i).map(|(_,g)| {
                let r=panel_radii(g.bounds,g.radii);
                ([0.,g.bounds[1]-b[1],g.bounds[2],g.bounds[3]],
                    if surface.direction>0. {r} else {[r[1],r[0],r[3],r[2]]})
            }).collect(),bounds:[0.,0.,b[2],b[3]],radii,
            anchors:surface.anchors.iter().map(|a|[a[1]-b[1],a[1]+a[3]-b[1]]).collect(),
            width:surface.bridge_width,gaps,
            envelope:[top.bounds[1]-b[1],end-b[1],first[facing[0]],last[facing[1]]],
            clip:[if i==0 {-1e6} else {(surface.groups[i-1].bounds[1]+surface.groups[i-1].bounds[3]+b[1])/2.-b[1]},
                if i+1==surface.groups.len() {1e6} else {(b[1]+b[3]+surface.groups[i+1].bounds[1])/2.-b[1]}]}
    }).collect()
}

fn flattened(commands: &[[f32;7]]) -> Vec<([f32;2],[f32;2])> {
    let mut result=Vec::new();let mut point=[0.;2];let mut start=point;
    for c in commands {match c[0] as u32 {
        0=>{point=[c[1],c[2]];start=point;},
        1=>{let next=[c[1],c[2]];result.push((point,next));point=next;},
        2=>{let a=point;for i in 1..=64 {let t=i as f32/64.;let s=1.-t;
            let next=std::array::from_fn(|j|s*s*s*a[j]+3.*s*s*t*c[1+j]+3.*s*t*t*c[3+j]+t*t*t*c[5+j]);
            result.push((point,next));point=next;
        }},
        3=>{result.push((point,start));point=start;},
        _=>panic!("invalid contour command"),
    }}
    result
}

#[test]
fn gpu_panel_vector_differential_keyframes_mirrors_small_panes_and_multiple_anchors() {
    let renderer=renderer();
    let mut presenter=ViewportPresenter::for_renderer(&renderer,wgpu::TextureFormat::Rgba8Unorm);
    let mut cases=Vec::new();
    for gap in [73.5,74.,74.1,77.,79.,80.,80.1,104.,127.9,128.,128.1,129.,133.9,134.,134.1,160.] {
        cases.push((format!("gap={gap}"),surface(gap)));
    }
    for gap in [26.,28.,32.,104.] {
        let mut s=surface(gap);s.groups[0].radii=[18.,4.,12.,0.];s.groups[1].radii=[3.,14.,0.,9.];
        cases.push((format!("asymmetric gap={gap}"),s));
    }
    let mut multiple=surface(104.);
    multiple.anchors=vec![[54.,24.,36.,36.],[54.,164.,36.,36.]];
    cases.push(("multiple anchors".into(),multiple));
    let mut flat=surface(104.);
    flat.groups=vec![group([PLANE,88.,144.,24.])];flat.groups[0].radii=[0.;4];
    flat.anchors=vec![[54.,54.,36.,36.],[54.,110.,36.,36.]];
    cases.push(("flat pane with two anchors".into(),flat));
    let mut absent=surface(104.);absent.anchors.clear();cases.push(("absent anchors".into(),absent));
    let mut failures=Vec::new();let mut compared=0usize;
    for (name,original) in cases {for flip in 0..4 {
        let mut surface=original.clone();
        if flip&1!=0 {surface=mirrored(surface,false);}
        if flip&2!=0 {surface=mirrored(surface,true);}
        let specs=contours(&surface);
        let mut unbounded=specs.clone();
        for spec in &mut unbounded {spec.clip=[-1e6,1e6];}
        let evaluators: Vec<_>=unbounded.iter().map(PanelContour::prepare).collect();
        let paths: Vec<_>=specs.iter().map(|c|flattened(&c.commands(0.025))).collect();
        let bytes=pixels(&renderer,&mut presenter,&surface,4,wgpu::TextureFormat::Rgba8Unorm);
        let mut mismatches=0;let mut first=None;
        for row in 16..(SIZE[1]*4-16) {
            let y=(row as f32+0.5)/4.;
            let crossings: Vec<Vec<_>>=paths.iter().zip(&surface.groups).map(|(path,g)| {
                let y=y-g.bounds[1];
                path.iter().filter_map(|(a,b)|
                    ((a[1]<=y && b[1]>y)||(b[1]<=y && a[1]>y)).then(||
                        (a[0]+(y-a[1])*(b[0]-a[0])/(b[1]-a[1]),if b[1]>a[1] {1i32} else {-1i32}))).collect()
            }).collect();
            for column in -23..96 {
                let x=(column as f32+0.5)/4.;
                let world_x=surface.plane+surface.direction*x;
                let value=red(&bytes,4,world_x,y);
                if value>2 && value<253 {continue;}
                let field=evaluators.iter().zip(&surface.groups).map(|(c,g)|c.field([x,y-g.bounds[1]])).fold(f32::INFINITY,f32::min);
                if field.abs()<0.15 {continue;}
                let inside=crossings.iter().any(|crossings|crossings.iter().filter(|(at,_)|*at>x).map(|(_,w)|w).sum::<i32>()!=0);
                compared+=1;
                if inside!=(value>=253) {
                    mismatches+=1;
                    first.get_or_insert((x,y,value,field,inside));
                }
            }
        }
        if mismatches>0 {failures.push(format!("{name} flip={flip}: {mismatches} mismatched pixels, first={first:?}"));}
    }}
    eprintln!("vector/GPU compared={compared}, failing cases={}\n{}",failures.len(),failures.join("\n"));
    assert!(compared>1_000_000);
    assert!(failures.is_empty(),"vector contours differ from approved GPU field; see per-case pixel diagnostics");
}

fn mesh_regions(surface: &PanelSurface,scale: u32) -> Vec<crate::BackdropRegion> {
    let scale=scale as f32;
    let mut regions=Vec::new();
    for (spec,group) in contours(surface).iter().zip(&surface.groups) {
        let packed: Vec<_>=spec.commands(0.025).into_iter().map(|mut c| {
            let points=match c[0] as u32 {0|1=>1,2=>3,_=>0};
            if surface.direction<0. {for i in (1..=points*2).step_by(2) {c[i]=group.bounds[2]-c[i];}}
            std::array::from_fn(|i|if i==0 {c[i] as i32} else {(c[i]*1024.).round() as i32})
        }).collect();
        let mut global=spec.clone();global.clip=[-1e6,1e6];
        let evaluator=global.prepare();
        let source=[if surface.direction>0. {surface.sidebar[0]-surface.plane}
            else {surface.plane-surface.sidebar[0]-surface.sidebar[2]},surface.sidebar[1]-group.bounds[1],surface.sidebar[2],surface.sidebar[3]];
        let r=surface.sidebar_radii;
        let source_radii=if surface.direction>0. {r} else {[r[1],r[0],r[3],r[2]]};
        for t in layer_render::panel_mesh_with_coverage(&packed,|p| {
            let p=[if surface.direction>0. {p[0]} else {group.bounds[2]-p[0]},p[1]];
            evaluator.field(p).min(layer_render::panel_geometry::panel_rect(p,source,source_radii))<=0.
        }) {
            let p=std::array::from_fn::<_,6,_>(|i| {
                let value=t[i] as f32/1024.;
                if i%2==0 {(group.bounds[0]+value)*scale} else {(group.bounds[1]+value)*scale}
            });
            regions.push(crate::BackdropRegion::triangle(p,t[6] as u32));
        }
        regions.push(crate::BackdropRegion::rounded(group.bounds.map(|v|v*scale),group.radii.map(|v|v*scale),crate::BackdropRegion::SQUIRCLE));
    }
    regions
}

fn white_pipeline(r: &WgpuRasterizer) -> wgpu::RenderPipeline {
    let module=r.device().create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("white blur coverage source"),
        source:wgpu::ShaderSource::Wgsl("@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));return vec4(p[i],0.,1.);}@fragment fn fragment()->@location(0) vec4<f32>{return vec4(1.);}".into())});
    r.device().create_render_pipeline(&wgpu::RenderPipelineDescriptor {label:Some("white blur coverage source"),layout:None,
        vertex:wgpu::VertexState {module:&module,entry_point:Some("vertex"),compilation_options:Default::default(),buffers:&[]},
        fragment:Some(wgpu::FragmentState {module:&module,entry_point:Some("fragment"),compilation_options:Default::default(),
            targets:&[Some(wgpu::ColorTargetState {format:wgpu::TextureFormat::Rgba8Unorm,blend:None,write_mask:wgpu::ColorWrites::ALL})]}),
        primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview_mask:None,cache:None})
}

fn mesh_pixels(r: &WgpuRasterizer,white: &wgpu::RenderPipeline,surface: &PanelSurface,scale: u32) -> Vec<u8> {
    let size=SIZE.map(|v|v*scale);
    let mut blur=crate::backdrop_blur::BackdropBlur::new(&r.device,wgpu::TextureFormat::Rgba8Unorm);
    blur.set_regions(&mesh_regions(surface,scale));
    let mut encoder=r.device().create_command_encoder(&Default::default());
    let mut uploads=crate::Uploads::new(r.device(),4096);
    blur.encode(r,&mut encoder,&mut uploads,size,0,layer_core::Affine::IDENTITY,false,None,
        |pass| {pass.set_pipeline(white);pass.draw(0..3,0..1);},None,&mut Vec::new()).unwrap();
    let target=crate::create_target(r.device(),size,wgpu::TextureFormat::Rgba8Unorm,"mesh blur coverage").0;
    let view=target.create_view(&Default::default());
    {
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {label:Some("mesh blur coverage"),
            color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:&view,depth_slice:None,resolve_target:None,
                ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::BLACK),store:wgpu::StoreOp::Store}})],
            depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None});
        blur.draw(&mut pass,crate::pixel_rect::PixelRect::new(0,0,size[0],size[1]));
    }
    uploads.finish(&encoder);r.queue().submit([encoder.finish()]);
    crate::layer_tests::page_bytes(r,&target)
}

fn silhouette_field(specs: &[PanelEvaluator<'_>],surface: &PanelSurface,x: f32,y: f32) -> f32 {
    specs.iter().zip(&surface.groups).map(|(c,g)|c.field([x,y-g.bounds[1]])).fold(f32::INFINITY,f32::min)
}

fn pixel_footprint(specs: &[PanelEvaluator<'_>],surface: &PanelSurface,x: f32,y: f32,scale: u32) -> ([f32;4],u32,[f32;2],f32) {
    let s=scale as f32;let half=0.5/s;
    let corners=[[-half,-half],[half,-half],[half,half],[-half,half]].map(|p|
        silhouette_field(specs,surface,x+p[0],y+p[1]));
    let mut inside=0;
    for row in 0..8 {for column in 0..8 {
        let p=[x+((column as f32+0.5)/8.-0.5)/s,y+((row as f32+0.5)/8.-0.5)/s];
        inside+=u32::from(silhouette_field(specs,surface,p[0],p[1])<0.);
    }}
    let e=0.01/s;
    let gradient=[(silhouette_field(specs,surface,x+e,y)-silhouette_field(specs,surface,x-e,y))/(2.*e),
        (silhouette_field(specs,surface,x,y+e)-silhouette_field(specs,surface,x,y-e))/(2.*e)];
    let distance=silhouette_field(specs,surface,x,y)*s/gradient[0].hypot(gradient[1]).max(1e-6);
    (corners,inside,gradient,distance)
}

#[test]
#[ignore = "standalone field-gradient and pixel-footprint diagnostic; no GPU"]
fn panel_vector_aa_coverage_diagnostic() {
    let mut multiple=surface(104.1);multiple.anchors=vec![[54.,24.,36.,36.],[54.,164.,36.,36.]];
    for (name,surface,p) in [("tile bottom gap",surface(128.1),[-0.375,79.125]),
        ("disconnected cap",surface(134.1),[-0.375,79.125]),
        ("multiple mirrored",mirrored(multiple,true),[0.125,105.375]),
        ("partial upper",surface(79.1),[-3.875,80.875]),
        ("partial mirrored",mirrored(surface(79.1),false),[-0.125,78.125]),
        ("partial vertical",mirrored(surface(79.1),true),[-3.875,140.375])] {
        let mut specs=contours(&surface);for c in &mut specs {c.clip=[-1e6,1e6];}
        let evaluators: Vec<_>=specs.iter().map(PanelContour::prepare).collect();
        let footprint=pixel_footprint(&evaluators,&surface,p[0],p[1],4);
        let field=silhouette_field(&evaluators,&surface,p[0],p[1]);
        eprintln!("AA probe {name}: p={p:?} field={field} field_physical={} corners/inside64/gradient/signed_normal_px={footprint:?}",field*4.);
        assert!(footprint.0.into_iter().chain(footprint.2).all(f32::is_finite));
    }
}

#[test]
fn gpu_panel_vector_mesh_backdrop_keeps_fractional_slab_seams_and_gap_holes() {
    let renderer=renderer();let white=white_pipeline(&renderer);
    let mut presenter=ViewportPresenter::for_renderer(&renderer,wgpu::TextureFormat::Rgba8Unorm);
    let mut cases=vec![("partial attachment",surface(79.1)),("tile bottom gap",surface(128.1)),("disconnected cap",surface(134.1))];
    let mut multiple=surface(104.1);multiple.anchors=vec![[54.,24.,36.,36.],[54.,164.,36.,36.]];
    cases.push(("multiple anchors",multiple));
    let mut flat=surface(104.);flat.groups=vec![group([PLANE,88.1,144.,24.])];flat.groups[0].radii=[0.;4];
    flat.anchors=vec![[54.,54.1,36.,36.],[54.,110.1,36.,36.]];cases.push(("flat outer gaps",flat));
    let mut failures=Vec::new();let mut compared=0usize;let mut fringe=0usize;let mut zero_touch=0usize;
    let mut fringe_diagnostics=Vec::new();let mut touch_diagnostics=Vec::new();
    for (name,original) in cases {for flip in 0..4 {for scale in [1,2,4] {
        let mut surface=original.clone();
        if flip&1!=0 {surface=mirrored(surface,false);}
        if flip&2!=0 {surface=mirrored(surface,true);}
        let expected=pixels(&renderer,&mut presenter,&surface,scale,wgpu::TextureFormat::Rgba8Unorm);
        let actual=mesh_pixels(&renderer,&white,&surface,scale);
        let mut specs=contours(&surface);for c in &mut specs {c.clip=[-1e6,1e6];}
        let evaluators: Vec<_>=specs.iter().map(PanelContour::prepare).collect();
        let mut mismatches=0;let mut first=None;
        for row in 4*scale..(SIZE[1]-4)*scale {for column in -5i32*scale as i32..24*scale as i32 {
            let x=(column as f32+0.5)/scale as f32;let y=(row as f32+0.5)/scale as f32;
            let world_x=surface.plane+surface.direction*x;
            let reference=red(&expected,scale,world_x,y);
            let field=silhouette_field(&evaluators,&surface,x,y);
            let want=if field<0. {255u8} else {0};
            let got=red(&actual,scale,world_x,y);
            if got.abs_diff(want)>3 {
                let footprint=pixel_footprint(&evaluators,&surface,x,y,scale);
                let entirely_inside=footprint.1==64 && footprint.0.iter().all(|v|*v< -ZERO_LEVEL_EPSILON);
                let entirely_outside=footprint.1==0 && footprint.0.iter().all(|v|*v>ZERO_LEVEL_EPSILON);
                if !entirely_inside && !entirely_outside {
                    fringe+=1;
                    if footprint.0.iter().any(|v|v.abs()<=ZERO_LEVEL_EPSILON) {
                        zero_touch+=1;
                        if touch_diagnostics.len()<6 {touch_diagnostics.push(format!("{name} flip={flip} scale={scale} p=({x},{y}) reference={reference} mesh={got} corners={:?}",footprint.0));}
                    }
                    if fringe_diagnostics.len()<12 && reference.abs_diff(got)>20 {
                        fringe_diagnostics.push(format!("{name} flip={flip} scale={scale} p=({x},{y}) reference={reference} mesh={got} field={field} corners/inside64/gradient/signed_normal_px={footprint:?}"));
                    }
                    continue;
                }
                mismatches+=1;first.get_or_insert((x,y,reference,want,got,field,footprint));
            }
            compared+=1;
        }}
        if mismatches>0 {failures.push(format!("{name} flip={flip} scale={scale}: {mismatches} pixels, first={first:?}"));}
    }}}
    eprintln!("mesh backdrop/GPU binary-mask comparisons={compared}, boundary-footprint mismatches={fringe}, zero-touch={zero_touch}, zero_epsilon={ZERO_LEVEL_EPSILON}, failing cases={}\n{}\nfringe diagnostics:\n{}\nzero-touch diagnostics:\n{}",failures.len(),failures.join("\n"),fringe_diagnostics.join("\n"),touch_diagnostics.join("\n"));
    assert!(compared>500_000);
    assert!(failures.is_empty(),"mesh backdrop differs from approved panel coverage");
}
