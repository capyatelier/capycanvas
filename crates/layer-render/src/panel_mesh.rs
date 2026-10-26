pub fn panel_mesh(commands: &[[i32;7]]) -> Vec<[i32;7]> {
    panel_mesh_with_coverage(commands, |_|false)
}

pub fn panel_mesh_with_coverage(commands: &[[i32;7]],covered: impl Fn([f32;2])->bool) -> Vec<[i32;7]> {
    let mut loops=Vec::new();let mut points=Vec::new();let mut point=[0.;2];
    for c in commands {
        let p=|i|[c[i] as f32/1024.,c[i+1] as f32/1024.];
        match c[0] {
            0=>{points=vec![p(1)];point=p(1);},
            1=>{point=p(1);points.push(point);},
            2=>{flatten([point,p(1),p(3),p(5)],0,&mut points);point=p(5);},
            3=>{if points.len()>2 {loops.push(std::mem::take(&mut points));}},
            _=>{},
        }
    }
    let contains=|p: [f32;2]|loops.iter().any(|polygon| {
        let n=polygon.len();let mut inside=false;
        for i in 0..n {let a=polygon[i];let b=polygon[(i+1)%n];
            if (a[1]>p[1])!=(b[1]>p[1]) && p[0]<(b[0]-a[0])*(p[1]-a[1])/(b[1]-a[1])+a[0] {inside=!inside;}
        }
        inside
    });
    let exterior=|a: [f32;2],b: [f32;2]| {
        let dx=b[0]-a[0];let dy=b[1]-a[1];let length=dx.hypot(dy).max(0.001);
        let outside=[(a[0]+b[0])/2.+0.05*dy/length,(a[1]+b[1])/2.-0.05*dx/length];
        !contains(outside) && !covered(outside)
    };
    let mut result=Vec::new();
    for original in loops.iter().take(loops.len().saturating_sub(1)) {
        let mut polygon=original.clone();
        let area=(0..polygon.len()).map(|i|cross([0.;2],polygon[i],polygon[(i+1)%polygon.len()])).sum::<f32>();
        if area<0. {polygon.reverse();}
        let start=(0..polygon.len()).min_by(|&a,&b|polygon[a][0].total_cmp(&polygon[b][0]).then(polygon[a][1].total_cmp(&polygon[b][1]))).unwrap();
        polygon.rotate_left(start);
        let min=polygon.iter().map(|p|p[0]).fold(f32::INFINITY,f32::min);
        let max=polygon.iter().map(|p|p[0]).fold(f32::NEG_INFINITY,f32::max);
        if max-min<0.0001 {continue;}
        let right=polygon.iter().position(|p|p[0]==max).unwrap();
        let last=polygon.iter().rposition(|p|p[0]==max).unwrap();
        let mut first=polygon[..=right].to_vec();
        let mut second=polygon[last..].iter().rev().copied().collect::<Vec<_>>();
        let order=|chain: &mut Vec<[f32;2]>| {
            if chain.first().unwrap()[0]>chain.last().unwrap()[0] {chain.reverse();}
            let mut previous=min;
            for p in chain.iter_mut() {p[0]=p[0].clamp(previous,max);previous=p[0];}
        };
        order(&mut first);order(&mut second);
        if first.first().unwrap()[1]>second.first().unwrap()[1] {std::mem::swap(&mut first,&mut second);}
        let mut xs=first.iter().chain(&second).map(|p|(p[0]*1024.).round() as i32).collect::<Vec<_>>();
        xs.sort_unstable();xs.dedup();
        let interpolate=|chain: &[[f32;2]],x: f32| {
            let i=chain.partition_point(|p|p[0]<x).min(chain.len()-1);
            if i==0 {return chain[0][1];}
            let a=chain[i-1];let b=chain[i];
            let t=((x-a[0])/(b[0]-a[0]).max(0.000001)).clamp(0.,1.);
            a[1]+t*(b[1]-a[1])
        };
        for (index,w) in xs.windows(2).enumerate() {
            let x0=w[0] as f32/1024.;let x1=w[1] as f32/1024.;
            let a=[x0,interpolate(&first,x0)];let b=[x1,interpolate(&first,x1)];
            let c=[x1,interpolate(&second,x1)];let d=[x0,interpolate(&second,x0)];
            let upper=exterior(a,b);let lower=exterior(c,d);
            let left=index==0 && exterior(d,a);let right=index+2==xs.len() && exterior(b,c);
            for (points,flags) in [([a,b,c],u32::from(upper)|(u32::from(right)<<1)),
                ([a,c,d],(u32::from(lower)<<1)|(u32::from(left)<<2))] {
                let mut packed=[0;7];packed[6]=flags as i32;
                for j in 0..3 {for k in 0..2 {packed[j*2+k]=(points[j][k]*1024.).round() as i32;}}
                let area=(packed[2] as i64-packed[0] as i64)*(packed[5] as i64-packed[1] as i64)
                    -(packed[3] as i64-packed[1] as i64)*(packed[4] as i64-packed[0] as i64);
                if area<0 {packed.swap(0,4);packed.swap(1,5);packed[6]=(packed[6]&4)|((packed[6]&1)<<1)|((packed[6]&2)>>1);}
                if area!=0 {result.push(packed);}
            }
        }
    }
    result
}

fn cross(a: [f32;2],b: [f32;2],c: [f32;2]) -> f32 {
    (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
}

fn flatten(p: [[f32;2];4],depth: u32,out: &mut Vec<[f32;2]>) {
    let length=(p[3][0]-p[0][0]).hypot(p[3][1]-p[0][1]).max(0.0001);
    let error=cross(p[0],p[3],p[1]).abs().max(cross(p[0],p[3],p[2]).abs())/length;
    if error<=0.025 || depth==10 {out.push(p[3]);return;}
    let mid=|a: [f32;2],b: [f32;2]|[(a[0]+b[0])/2.,(a[1]+b[1])/2.];
    let a=mid(p[0],p[1]);let b=mid(p[1],p[2]);let c=mid(p[2],p[3]);let d=mid(a,b);let e=mid(b,c);let f=mid(d,e);
    flatten([p[0],a,d,f],depth+1,out);flatten([f,e,c,p[3]],depth+1,out);
}
