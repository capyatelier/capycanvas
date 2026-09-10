// A normalized discrete Gaussian, paired for bilinear sampling. Preparation
// runs one 64-lane workgroup only when sigma changes; consumers share 33 records.
var<workgroup> weights: array<f32,64>;
var<workgroup> sums: array<f32,64>;

fn capy_prepare_gaussian(local:vec3<u32>, global:vec3<u32>) {
    let i=local.x;
    let sigma=prep_parameter(0u,0u).x;
    let radius=min(u32(ceil(sigma*3.)),63u);
    var w=0.;
    if i==0u { w=1.; }
    else if i<=radius && sigma>0. { let x=f32(i)/sigma; w=exp(-0.5*x*x); }
    weights[i]=w;
    sums[i]=select(2.*w,w,i==0u);
    workgroupBarrier();
    for(var stride=32u;stride>0u;stride/=2u) {
        if i<stride { sums[i]+=sums[i+stride]; }
        workgroupBarrier();
    }
    if i==0u {
        var count=0u;
        for(var j=1u;j<=radius;j+=2u) {
            var pair=weights[j];
            if j<radius {pair+=weights[j+1u];}
            if pair<=1e-20 {break;}
            count+=1u;
        }
        prep_store(0u,vec4<f32>(1./sums[0],f32(count),0.,0.));
    }
    if i<32u {
        let j=i*2u+1u;
        var a=weights[j];var b=0.;
        if j<radius {b=weights[j+1u];}
        var tap=vec4<f32>(0.);
        if j<=radius && a+b>1e-20 {tap=vec4<f32>(f32(j)+b/(a+b),(a+b)/sums[0],0.,0.);}
        prep_store(i+1u,tap);
    }
}
