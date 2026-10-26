use super::*;

fn packed(c: &PanelContour) -> Vec<[i32;7]> {
    c.commands(0.025).into_iter().map(|command|std::array::from_fn(|i|
        if i==0 {command[i] as i32} else {(command[i]*1024.).round() as i32})).collect()
}

fn triangle_points(t: &[i32;7]) -> [[f32;2];3] {
    std::array::from_fn(|i|[t[i*2] as f32/1024.,t[i*2+1] as f32/1024.])
}

fn cross(a: [f32;2],b: [f32;2],p: [f32;2]) -> f32 {
    (b[0]-a[0])*(p[1]-a[1])-(b[1]-a[1])*(p[0]-a[0])
}

#[test]
fn packed_patch_mesh_preserves_fill_for_gap_slabs_and_small_panes() {
    let mut cases: Vec<_>=[33.5,34.1,37.,39.,40.,70.,75.9,76.,76.1,81.9,82.1,100.]
        .into_iter().map(|bottom|(format!("upper {bottom}"),contour(bottom))).collect();
    for height in [2.,4.,8.,12.,24.,72.] {for radii in [[0.;4],[18.;4],[18.,3.,6.,12.]] {
        let radii=panel_radii([0.,0.,120.,height],radii);
        cases.push((format!("small {height} {radii:?}"),PanelContour {neighbors:vec![],bounds:[0.,0.,120.,height],radii,
            anchors:vec![[-34.,2.],[height-2.,height+34.]],width:6.,gaps:vec![[-6.,0.],[height,height+6.]],
            envelope:[0.,height,radii[0],radii[3]],clip:[-1e6,1e6]}));
    }}
    let mut compared=0;let mut nonpositive=0;
    for (name,c) in cases {
        let evaluator=c.prepare();
        let triangles=crate::panel_mesh(&packed(&c));
        for t in &triangles {
            let p=triangle_points(t);
            if cross(p[0],p[1],p[2])<=0. {
                nonpositive+=1;
                if nonpositive<=8 {eprintln!("{name}: nonpositive packed triangle {t:?}");}
            }
            assert!(t[6]>=0 && t[6]<=7,"{name}: invalid boundary bits {t:?}");
        }
        let triangles: Vec<_>=triangles.iter().map(triangle_points).collect();
        let lo=c.anchors.iter().fold(c.bounds[1],|y,a|y.min(a[0])).max(c.clip[0])-1.;
        let hi=c.anchors.iter().fold(c.bounds[1]+c.bounds[3],|y,a|y.max(a[1])).min(c.clip[1])+1.;
        for row in (lo*2.).floor() as i32..=(hi*2.).ceil() as i32 {
            let y=row as f32/2.+0.037;
            let relevant: Vec<_>=triangles.iter().filter(|p|
                p.iter().any(|p|p[1]<=y) && p.iter().any(|p|p[1]>=y)).collect();
            for column in -28..=80 {
                let p=[column as f32/4.+0.019,y];let field=evaluator.field(p);
                if field.abs()<0.18 {continue;}
                let filled=panel_rect(p,c.bounds,c.radii)<0. || relevant.iter().any(|t|
                    cross(t[0],t[1],p)>=0. && cross(t[1],t[2],p)>=0. && cross(t[2],t[0],p)>=0.);
                compared+=1;
                assert_eq!(filled,field<0.,"{name}: p={p:?} field={field} triangles={}",triangles.len());
            }
        }
    }
    eprintln!("packed mesh compared={compared} points");
    assert_eq!(nonpositive,0,"packed mesh contains inverted or degenerate triangles");
}

#[test]
#[ignore = "pointwise backdrop half-plane diagnostic; no GPU or window"]
fn partial_gap_strip_triangle_coverage_diagnostic() {
    let target=[-3.875,140.375];let scale=4.;let mut combined=0f32;
    let upper=PanelContour {neighbors:vec![([0.,120.9,144.,55.1],[18.;4])],bounds:[0.,0.,144.,114.9],radii:[18.;4],
        anchors:vec![[72.,120.]],width:6.,gaps:vec![[114.9,120.9]],envelope:[0.,176.,18.,18.],clip:[-1e6,117.9]};
    let lower=PanelContour {neighbors:vec![([0.,-120.9,144.,114.9],[18.;4])],bounds:[0.,0.,144.,55.1],radii:[18.;4],
        anchors:vec![[-48.9,-0.9]],width:6.,gaps:vec![[-6.,0.]],envelope:[-120.9,55.1,18.,18.],clip:[-3.,1e6]};
    for (name,c,origin) in [("upper",upper,24.),("lower",lower,144.9)] {
        let mut global=c.clone();global.clip=[-1e6,1e6];
        let evaluator=global.prepare();
        let mesh=crate::panel_mesh_with_coverage(&packed(&c),|p|evaluator.field(p)<=0.);
        let p=[target[0],target[1]-origin];
        eprintln!("probe {name} local={p:?} field={} triangles={}",evaluator.field(p),mesh.len());
        for (index,t) in mesh.iter().enumerate() {
            let points=triangle_points(t);let mut coverage=1f32;let mut distances=[0.;3];let mut lines=[0.;3];let mut limiting=None;
            for i in 0..3 {
                let a=points[i];let b=points[(i+1)%3];
                let distance=cross(a,b,p)/(b[0]-a[0]).hypot(b[1]-a[1]).max(0.0001)*scale;
                lines[i]=distance;distances[i]=distance;
                if t[6]&(1<<i)==0 {if distance< -0.0001 {coverage=0.;}}
                else {
                    let dx=b[0]-a[0];let dy=b[1]-a[1];
                    let projection=(((p[0]-a[0])*dx+(p[1]-a[1])*dy)/(dx*dx+dy*dy).max(1e-8)).clamp(0.,1.);
                    let finite=(p[0]-a[0]-projection*dx).hypot(p[1]-a[1]-projection*dy)*scale;
                    distances[i]=if distance<0. {-finite} else {finite};
                    let edge=(0.5+distances[i]).clamp(0.,1.);if edge<coverage {coverage=edge;limiting=Some(i);}
                }
            }
            if coverage>0. {
                combined=coverage+combined*(1.-coverage);
                eprintln!("nonzero {name} triangle={index} packed={t:?} points={points:?} distances_px={distances:?} infinite_lines_px={lines:?} coverage={coverage} limiting_edge={limiting:?}");
                if let Some(i)=limiting {
                    let a=points[i];let b=points[(i+1)%3];let dx=b[0]-a[0];let dy=b[1]-a[1];let length=dx.hypot(dy);
                    let middle=[(a[0]+b[0])/2.,(a[1]+b[1])/2.];
                    let outside=[middle[0]+0.05*dy/length,middle[1]-0.05*dx/length];
                    let projection=(((p[0]-a[0])*dx+(p[1]-a[1])*dy)/(length*length)).clamp(0.,1.);
                    let distance=(p[0]-a[0]-projection*dx).hypot(p[1]-a[1]-projection*dy)*scale;
                    let values=[0.,0.25,0.5,0.75,1.].map(|v|evaluator.field([a[0]+v*dx,a[1]+v*dy]));
                    eprintln!("limiting edge same_x={} clip_edge={} field_along={values:?} outward_field={} segment_distance_px={distance}",
                        dx.abs()<0.001,(a[1]-c.clip[0]).abs()<0.001 && (b[1]-c.clip[0]).abs()<0.001 || (a[1]-c.clip[1]).abs()<0.001 && (b[1]-c.clip[1]).abs()<0.001,evaluator.field(outside));
                }
            }
        }
    }
    eprintln!("composed triangle coverage={combined} RGB={}",combined*255.);
    assert!(combined>0.999,"interior point was faded by an exterior edge extension");
}
