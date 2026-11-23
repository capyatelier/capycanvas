struct Params { bounds: vec4<f32>, count: u32, shape_only: u32, strength: f32, width: u32, height: u32 }
struct Contacts { values: array<Dab,192> }
struct Extents { values: array<vec4<f32>,192> }
@group(0) @binding(0) var<uniform> style: Style;
@group(0) @binding(1) var<storage,read> contacts: Contacts;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var<storage,read_write> output: array<u32>;
@group(0) @binding(4) var<storage,read> extents: Extents;

@compute @workgroup_size(64)
fn stamp(@builtin(global_invocation_id) id: vec3<u32>) {
    let columns=params.width/4u;
    if id.x>=columns || id.y>=params.height {return;}
    var packed=0u;
    for(var sample=0u;sample<4u;sample++) {
        let pixel=vec2<u32>(id.x*4u+sample,id.y);
        let world=mix(params.bounds.xy,params.bounds.zw,(vec2<f32>(pixel)+0.5)/vec2<f32>(f32(params.width),f32(params.height)));
        let field=contact_field(world);
        var alpha=0.0;
        for(var i=0u;i<params.count;i++) {
            let extent=extents.values[i];
            if any(world<extent.xy) || any(world>extent.zw) {continue;}
            let dab=contacts.values[i];
            let coverage=brush_footprint(dab,world,field);
            let next=select(brush_alpha(dab,coverage),coverage*params.strength,params.shape_only!=0u);
            if contact_uniform() {alpha=max(alpha,next);}
            else {alpha+=(1.0-alpha)*next;}
        }
        packed|=u32(round(clamp(alpha*style.canvas_opacity.z,0.0,1.0)*255.0))<<(sample*8u);
    }
    output[id.y*columns+id.x]=packed;
}
