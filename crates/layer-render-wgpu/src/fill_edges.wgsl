@group(2) @binding(0) var fill_source:texture_2d<f32>;
@group(2) @binding(2) var fill_lines:texture_2d<f32>;
@group(2) @binding(3) var fill_cover:texture_2d<f32>;
@group(2) @binding(5) var<storage,read> fill_anchors:BrushSelection;
const RAMP_HALF_PIXELS:u32=3u;

fn fill_source_at(p:vec2<i32>)->vec4<f32> {
    return textureLoad(fill_source,p-fill_precision.extent.zw-vec2<i32>(settings.backdrop.xy),0);
}
fn fill_near_seed(pixel:vec4<f32>,seed:vec4<f32>)->bool {
    let delta=fill_sample(pixel).value-fill_sample(seed).value;
    return dot(delta,delta)<settings.operation_linear.x*settings.operation_linear.x;
}
fn fill_feasible_neighbour(p:vec2<i32>,pixel:vec4<f32>,seed:vec4<f32>)->vec4<f32> {
    let radius=i32(settings.operation_linear.y);
    var neighbour=pixel;var score=-1.;
    let local=p-fill_precision.extent.zw-vec2<i32>(settings.backdrop.xy);
    for(var y=-radius;y<=radius;y++){for(var x=-radius;x<=radius;x++) {
        if x*x+y*y>radius*radius {continue;}
        let sample=local+vec2(x,y);
        if any(sample<vec2(0)) || any(sample>=vec2<i32>(settings.backdrop.zw)) {continue;}
        let candidate=textureLoad(fill_source,sample,0);let delta=candidate-seed;let distance=dot(delta,delta);
        if distance>score {score=distance;neighbour=candidate;}
    }}
    if fill_fraction(pixel,seed,neighbour)<0. {return vec4(-1.);}
    return neighbour;
}
fn fill_plateau(at:vec2<i32>,seed:vec4<f32>)->vec4<f32> {
    let p=at+vec2<i32>(settings.backdrop.xy)+fill_precision.extent.zw;
    let local=p-bitcast<vec2<i32>>(fill_anchors.rect.xy);
    if any(local<vec2(0)) || any(local>=vec2<i32>(fill_anchors.rect.zw)) {return vec4(-1.);}
    let packed=fill_anchors.values[u32(local.y)*fill_anchors.rect.z+u32(local.x)];if packed==0xffffffffu {return vec4(-1.);}
    let q=vec2<i32>(i32(packed&65535u),i32(packed>>16u));
    if fill_selection_at(vec2<f32>(q-fill_precision.extent.zw)+.5)==0. {return vec4(-1.);}
    let delta=p-q;let steps=max(abs(delta.x),abs(delta.y));let square=dot(vec2<f32>(delta),vec2<f32>(delta));
    let radius=i32(settings.operation_linear.y);
    if square>f32(radius*radius) || (fill_anchors.info.x!=0u && 4.*square>f32(RAMP_HALF_PIXELS*RAMP_HALF_PIXELS)) {return vec4(-1.);}
    var current=fill_source_at(q);
    if steps==0 {return fill_feasible_neighbour(p,current,seed);}
    for(var i=1;i<=radius;i++) {
        if f32(i*i)*square>f32(radius*radius*steps*steps) {break;}
        let next_at=q+sign(delta)*((abs(delta)*i+vec2(steps/2))/steps);
        if any(next_at<vec2(0)) || any(next_at>=fill_precision.extent.xy) {return fill_feasible_neighbour(p,fill_source_at(p),seed);}
        let next=fill_source_at(next_at);
        if fill_equal(current,next) && !fill_near_seed(current,seed) {
            if i-1<steps {return vec4(-1.);}
            return current;
        }
        current=next;
    }
    return current;
}

fn fill_line_alpha(at:vec2<i32>)->f32 {
    if any(at<vec2(0)) || any(at>=vec2<i32>(settings.backdrop.zw)) {return 0.;}
    let own=textureLoad(fill_source,at,0).a;let other=textureLoad(fill_lines,at,0).a;
    return own+other*(1.-own);
}
fn fill_profile(at:vec2<i32>,axis:vec2<i32>)->vec2<f32> {
    let center=fill_line_alpha(at);if center==0. {return vec2(0.);}
    var peak=center;var left=0.;var right=0.;
    let radius=i32(settings.operation_linear.y);
    for(var side=-1;side<=1;side+=2) {
        for(var step=1;step<=radius;step++) {
            let p=at+axis*(step*side);
            if any(p<vec2(0)) || any(p>=vec2<i32>(settings.backdrop.zw)) {return vec2(0.);}
            let alpha=fill_line_alpha(p);if alpha==0. {break;}
            peak=max(peak,alpha);
            if side<0 {left+=alpha;}else{right+=alpha;}
            if step==radius {return vec2(0.);}
        }
    }
    let position=dot(vec2<f32>(at)+.5,vec2<f32>(axis));
    return vec2(position+(right-left)/(2.*peak),peak);
}
fn fill_half_plane(distance:f32,normal:vec2<f32>)->f32 {
    let sides=abs(normal);let large=max(sides.x,sides.y);let small=min(sides.x,sides.y);
    if small<.00001 {return clamp(distance/large+.5,0.,1.);}
    let z=distance+(large+small)*.5;
    let a=max(0.,z);let b=max(0.,z-large);let c=max(0.,z-small);let d=max(0.,z-large-small);
    return clamp((a*a-b*b-c*c+d*d)/(2.*large*small),0.,1.);
}
fn fill_line_plane(at:vec2<i32>,gradient:vec2<f32>,horizontal:bool)->vec2<f32> {
    let axis=select(vec2(0,1),vec2(1,0),horizontal);let across=axis.yx;
    let size=vec2<i32>(settings.backdrop.zw);
    let low=clamp(at-across,vec2(0),size-1);let high=clamp(at+across,vec2(0),size-1);
    let a=fill_profile(low,axis);let b=fill_profile(high,axis);
    if a.y==0. || b.y==0. {return vec2(0.);}
    let span=f32(dot(high-low,across));if span==0. {return vec2(0.);}
    let slope=(b.x-a.x)/span;
    let center=a.x+slope*f32(dot(at-low,across));
    let direction=sign(select(gradient.y,gradient.x,horizontal));
    if direction==0. {return vec2(0.);}
    let normal=normalize(select(vec2(-slope,1.),vec2(1.,-slope),horizontal))*direction;
    let distance=(dot(vec2<f32>(at)+.5,vec2<f32>(axis))-center)*select(normal.y,normal.x,horizontal);
    return vec2(fill_half_plane(distance,normal),1.);
}
fn fill_line_middle(world:vec2<f32>,at:vec2<i32>,support:bool)->f32 {
    let current=fill_selection_at(world);
    let fallback=select(current,0.,support);
    let gradient=vec2(fill_selection_at(world+vec2(2.,0.))-fill_selection_at(world-vec2(2.,0.)),
        fill_selection_at(world+vec2(0.,2.))-fill_selection_at(world-vec2(0.,2.)));
    if dot(gradient,gradient)==0. {return fallback;}
    let horizontal=abs(gradient.x)>=abs(gradient.y);
    let plane=fill_line_plane(at,gradient,horizontal);if plane.y!=0. {return plane.x;}
    let alternate=fill_line_plane(at,gradient,!horizontal);return select(fallback,alternate.x,alternate.y!=0.);
}

fn fill_edge_value(world:vec2<f32>,pixel:vec4<f32>,paint:vec4<f32>,support:bool)->vec4<f32> {
    let local=vec2<i32>(floor(world-settings.backdrop.xy));
    let seed=textureLoad(fill_source,vec2(0,i32(textureDimensions(fill_source).y)-1),0);
    var amount=paint.a*(1.-textureLoad(fill_cover,local,0).a);
    if seed.a<.99999 || pixel.a<.99999 {
        if support && fill_line_alpha(local)==0. {return pixel;}
        amount*=fill_line_middle(world,local,support)/max(fill_selection_at(world),.000001);
        return pixel+vec4(settings.color.rgb,1.)*amount*select(1.-pixel.a,0.,settings.options.w>.5);
    }
    let delta=fill_sample(pixel).value-fill_sample(seed).value;
    if dot(delta,delta)<settings.operation_linear.x*settings.operation_linear.x {
        return vec4(settings.color.rgb,1.)*amount+pixel*(1.-amount);
    }
    let neighbour=fill_plateau(local,seed);
    if neighbour.a<0. {return pixel;}
    if fill_equal(pixel,neighbour) {return pixel;}
    return fill_reconstruct(pixel,seed,neighbour,vec4(settings.color.rgb,1.),amount);
}
