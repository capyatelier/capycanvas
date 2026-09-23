struct Params { extent: vec2<u32>, mask_side: u32, unused: u32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read_write> output: BrushSelection;
@compute @workgroup_size(64) fn merge(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = ((params.extent.x+255u)/256u)*64u;
    if id.x >= (params.extent.x+3u)/4u || id.y >= params.extent.y { return; }
    let index = id.y*stride+id.x;
    var word = output.values[index];
    for (var i=0u; i<4u; i++) {
        let p = vec2<f32>(f32(id.x*4u+i)+.5, f32(id.y)+.5);
        let coverage = brush_selection_at(p);
        let tint = select(coverage,1.-coverage,params.mask_side != 0u);
        let byte = u32(round(clamp(tint,0.,1.)*255.));
        let old = (word >> (i*8u)) & 255u;
        word = (word & ~(255u << (i*8u))) | (max(old,byte) << (i*8u));
    }
    output.values[index] = word;
}
