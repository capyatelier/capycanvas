// Separate pipeline: ordinary color-region queries never execute tonal work.
struct TonalParameters {
    bands: array<vec4<f32>,16>,
    weights: vec4<f32>,
    probe: vec4<u32>,
    flags: vec4<u32>, // band count, invert, probe enabled, point probe
}
@group(0) @binding(20) var<uniform> tonal: TonalParameters;
@group(0) @binding(21) var<storage,read_write> tone_stats: array<atomic<u32>>;
fn tonal_ramp(distance:f32, width:f32)->f32 {
    if distance >= 0. { return 1.; }
    if width <= 0. { return 0.; }
    let t=clamp(1.+distance/width,0.,1.);
    return t*t*(3.-2.*t);
}
@compute @workgroup_size(64)
fn tonal_tile(@builtin(global_invocation_id) id:vec3<u32>) {
    let tile=batch.tiles[id.z];
    let stride=(tile.size.x+3u)/4u;
    let p=vec2<u32>((id.x%stride)*4u,id.x/stride);
    if p.y>=tile.size.y { return; }
    var packed=0u;
    for(var i=0u;i<4u && p.x+i<tile.size.x;i++) {
        let local=p+vec2<u32>(i,0u);
        let world=tile.origin+local;
        let color=raw_color(id.z,local);
        var coverage=0.;
        if color.a>0. {
            let rgb=color.rgb/color.a;
            let y=rgb.g+tonal.weights.x*(rgb.r-rgb.g)+tonal.weights.z*(rgb.b-rgb.g);
            var stop=-1000.;
            if y>0. { stop=log2(y); }
            for(var band=0u;band<tonal.flags.x;band++) {
                let b=tonal.bands[band];
                coverage=max(coverage,tonal_ramp(stop-b.x,b.z)*tonal_ramp(b.y-stop,b.w));
            }
            if tonal.flags.y!=0u { coverage=1.-coverage; }
            coverage*=color.a;
            if tonal.flags.z!=0u && all(world>=tonal.probe.xy) && all(world<tonal.probe.zw) {
                var bin=0u;
                if y>0. { bin=1u+u32(clamp(floor((stop+149.)*16.),0.,4432.)); }
                atomicAdd(&tone_stats[bin],1u);
                if tonal.flags.w!=0u {
                    let offset=world-tonal.probe.xy;
                    let index=4448u+2u*(offset.y*5u+offset.x);
                    atomicStore(&tone_stats[index],bitcast<u32>(y));
                    atomicStore(&tone_stats[index+1u],bitcast<u32>(color.a));
                }
            }
        }
        packed|=u32(round(clamp(coverage,0.,1.)*255.))<<(8u*i);
    }
    eligibility[8u+(tile.origin.y+p.y)*((tile.extent.x+3u)/4u)+(tile.origin.x+p.x)/4u]=packed;
}
