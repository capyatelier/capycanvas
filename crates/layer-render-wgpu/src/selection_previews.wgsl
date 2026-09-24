struct Params { extent: vec2<u32>, mask_side: u32, unused: u32, color: vec4<f32> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read_write> output: BrushSelection;
@compute @workgroup_size(64) fn merge(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = ((params.extent.x+63u)/64u)*64u;
    if id.x >= params.extent.x || id.y >= params.extent.y { return; }
    let index = id.y*stride+id.x;
    let old = unpack4x8unorm(output.values[index]);
    let coverage = brush_selection_at(vec2<f32>(id.xy)+vec2(.5));
    let alpha = clamp(select(coverage,1.-coverage,params.mask_side != 0u)*params.color.a,0.,1.);
    output.values[index] = pack4x8unorm(vec4(params.color.rgb*alpha,alpha)+old*(1.-alpha));
}
