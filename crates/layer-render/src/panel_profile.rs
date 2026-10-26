fn arc(u: f32, radius: f32) -> [f32; 2] {
    let u2=u*u;
    let inner=(8.*u2*u2+2.*radius.powi(4)).sqrt();
    let value=(inner-3.*u2).max(1e-12).sqrt();
    [value,(-3.*u+8.*u*u2/inner)/value]
}

fn base(u: f32, delta: f32) -> [f32; 2] {
    let root=std::f32::consts::SQRT_2;
    let radius=1./3.;
    if delta==0. { return [u,1.]; }
    let mut center=[-radius,delta-radius];
    let mut touch=[0.,delta-radius];
    if delta<1.+radius {
        let sum=1.+radius;
        let width=(sum.powi(4)-(sum-delta).powi(4)).max(0.).sqrt().sqrt();
        center[0]=1.-width;
        touch=[1.-width/sum,delta/sum];
    }
    if u<=(center[0]-delta)/root { return [u+root*delta,1.]; }
    if u<=(touch[0]-touch[1])/root {
        let a=arc(u-(center[0]-center[1])/root,radius);
        return [(center[0]+center[1])/root+a[0],a[1]];
    }
    if delta>=1.+radius && u< -1./root { return [-u,-1.]; }
    if u<1./root { let a=arc(u,1.);return [root-a[0],-a[1]]; }
    [u,1.]
}

pub fn generate() -> Vec<[f32;2]> {
    (0..257).flat_map(|y| (0..512).map(move |x| {
        let u=-5.+8.*x as f32/511.;let delta=y as f32/64.;
        let mut sum=[0.;2];let mut weight=0.;
        for i in 0..32 {
            let offset=-1.+2.*(i as f32+0.5)/32.;let k=(1.-offset*offset).powi(2);
            let a=base(u+offset/12.,delta);
            sum[0]+=k*a[0];sum[1]+=k*a[1];weight+=k;
        }
        [sum[0]/weight,sum[1]/weight]
    })).collect()
}
