// Separate pipeline: ordinary color-region queries never execute tonal work.
struct TonalParameters {
    bands: array<vec4<f32>,16>,
    weights: vec4<f32>,
    probe: vec4<u32>,
    flags: vec4<u32>, // band count, invert, probe (0 none, 1 rectangle, 2 quad), point
    quad: array<vec4<f32>,2>,
    cache: vec4<u32>, // chunk texels, image extent, opaque paired luminance
}
@group(0) @binding(20) var<uniform> tonal: TonalParameters;
@group(0) @binding(21) var<storage,read_write> tone_stats: array<atomic<u32>>;
fn tonal_ramp(distance:f32, width:f32)->f32 {
    if distance >= 0. { return 1.; }
    if width <= 0. { return 0.; }
    let t=clamp(1.+distance/width,0.,1.);
    return t*t*(3.-2.*t);
}
fn tonal_probe_contains(world:vec2<u32>)->bool {
    if tonal.flags.z==0u || any(world<tonal.probe.xy) || any(world>=tonal.probe.zw) { return false; }
    if tonal.flags.z==1u { return true; }
    let q=array<vec2<f32>,4>(tonal.quad[0].xy,tonal.quad[0].zw,tonal.quad[1].xy,tonal.quad[1].zw);
    let p=vec2<f32>(world)+vec2<f32>(0.5);
    var positive=true;
    var negative=true;
    for(var i=0u;i<4u;i++) {
        let a=q[(i+1u)%4u]-q[i];
        let b=p-q[i];
        let cross=a.x*b.y-a.y*b.x;
        positive=positive && cross>=0.;
        negative=negative && cross<=0.;
    }
    return positive || negative;
}
fn tonal_coverage(y:f32, alpha:f32, world:vec2<u32>)->u32 {
    var coverage=0.;
    if alpha>0. {
        var stop=-1000.;
        if y>0. { stop=log2(y); }
        for(var band=0u;band<tonal.flags.x;band++) {
            let b=tonal.bands[band];
            coverage=max(coverage,tonal_ramp(stop-b.x,b.z)*tonal_ramp(b.y-stop,b.w));
        }
        if tonal.flags.y!=0u { coverage=1.-coverage; }
        coverage*=alpha;
        if tonal_probe_contains(world) {
            var bin=0u;
            if y>0. { bin=1u+u32(clamp(floor((stop+149.)*16.),0.,4432.)); }
            // Distribute broad/uniform probes across cache lines instead of
            // making every image pixel contend on the same histogram bin.
            let shard=((world.y*tonal.cache.y+world.x)/256u)%64u;
            atomicAdd(&tone_stats[shard*4434u+bin],1u);
            if tonal.flags.w!=0u {
                let offset=world-tonal.probe.xy;
                let index=283776u+2u*(offset.y*5u+offset.x);
                atomicStore(&tone_stats[index],bitcast<u32>(y));
                atomicStore(&tone_stats[index+1u],bitcast<u32>(alpha));
            }
        }
    }
    return u32(round(clamp(coverage,0.,1.)*255.));
}
@compute @workgroup_size(64)
fn tonal_tile(@builtin(global_invocation_id) id:vec3<u32>) {
    let tile=batch.tiles[id.z];
    let stride=(tile.size.x+3u)/4u;
    let p=vec2<u32>((id.x%stride)*4u,id.x/stride);
    if p.y>=tile.size.y { return; }
    var packed=0u;
    var pair=vec2<f32>(0.);
    for(var i=0u;i<4u && p.x+i<tile.size.x;i++) {
        let local=p+vec2<u32>(i,0u);
        let world=tile.origin+local;
        let color=raw_color(id.z,local);
        var y=0.;
        if color.a>0. {
            let rgb=color.rgb/color.a;
            y=rgb.g+tonal.weights.x*(rgb.r-rgb.g)+tonal.weights.z*(rgb.b-rgb.g);
        }
        if tonal.cache.x!=0u {
            if tonal.cache.w!=0u {
                // Opaque RGB needs only one scalar per pixel. Each invocation
                // owns whole pairs, including odd-width zero padding.
                pair[i%2u]=y;
                if i%2u==1u || p.x+i+1u==tile.size.x {
                    cache_store(world.y*((tile.extent.x+1u)/2u)+world.x/2u,pair);
                    pair=vec2<f32>(0.);
                }
            } else { cache_store(world.y*tile.extent.x+world.x,vec2<f32>(y,color.a)); }
        }
        packed|=tonal_coverage(y,color.a,world)<<(8u*i);
    }
    eligibility[8u+(tile.origin.y+p.y)*((tile.extent.x+3u)/4u)+(tile.origin.x+p.x)/4u]=packed;
}
@compute @workgroup_size(64)
fn tonal_cached(@builtin(global_invocation_id) id:vec3<u32>) {
    let extent=tonal.cache.yz;
    let stride=(extent.x+3u)/4u;
    if id.x>=stride || id.y>=extent.y {return;}
    var packed=0u;
    for(var i=0u;i<4u && id.x*4u+i<extent.x;i++) {
        let world=vec2<u32>(id.x*4u+i,id.y);
        let value=cache_load(world);
        packed|=tonal_coverage(value.x,value.y,world)<<(8u*i);
    }
    eligibility[8u+id.y*stride+id.x]=packed;
}
