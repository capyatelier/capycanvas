struct Settings {
    rect: vec4<f32>,
    extent: vec4<f32>,
    options: vec4<f32>,
    color: vec4<f32>,
    source_over: vec4<f32>,
    backdrop: vec4<f32>,
    operation_linear: vec4<f32>,
    operation_offset: vec4<f32>,
    query_grid: vec4<u32>,
}
@group(0) @binding(0) var<uniform> settings: Settings;
@group(1) @binding(0) var front: texture_2d<f32>;
@group(1) @binding(1) var back: texture_2d<f32>;
@group(1) @binding(2) var sampling: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
fn query_grid_begin(origin:vec2<u32>)->vec2<u32> {
    let extent=settings.query_grid.zw;let size=min(extent,vec2(256u));
    return vec2<u32>(max(vec2(0), (2*vec2<i32>(size*origin)-vec2<i32>(extent)+2*vec2<i32>(extent)-1)/(2*vec2<i32>(extent))));
}
fn query_grid_size()->vec2<u32> {
    return min(query_grid_begin(settings.query_grid.xy+vec2(256u)),min(settings.query_grid.zw,vec2(256u)))-query_grid_begin(settings.query_grid.xy);
}
fn query_grid_pixel(index:vec2<u32>)->vec2<f32> {
    let extent=settings.query_grid.zw;let size=min(extent,vec2(256u));
    return vec2<f32>(((2u*(query_grid_begin(settings.query_grid.xy)+index)+1u)*extent)/(2u*size)-settings.query_grid.xy);
}
@vertex fn vertex_main(@builtin(vertex_index) i: u32, @builtin(instance_index) instance:u32) -> Vertex {
    if settings.query_grid.z!=0u {
        let size=query_grid_size();
        let corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
        let p=query_grid_pixel(vec2(instance%size.x,instance/size.x))+corners[i];
        return Vertex(vec4(p.x/settings.extent.x*2.-1.,1.-p.y/settings.extent.y*2.,0.,1.),(p-settings.rect.xy)/settings.rect.zw);
    }
    var corners = array<vec2<f32>,3>(vec2<f32>(0.,0.),vec2<f32>(2.,0.),vec2<f32>(0.,2.));
    let uv = corners[i];
    let p = settings.rect.xy + uv*settings.rect.zw;
    var o: Vertex;
    o.position = vec4<f32>(p.x/settings.extent.x*2.-1.,1.-p.y/settings.extent.y*2.,0.,1.);
    o.uv = uv; return o;
}
// operation_offset.w converts the color a layer draw contributes: 1 encodes
// linear layer pixels into the Perceptual composite, 2 decodes the composite.
// 3 keeps a composite's coverage within 0–1 for a layer's pages, and 4 also
// decodes it.
fn scene_space(c:vec4<f32>)->vec4<f32> {
    return scene_convert(c,settings.operation_offset.w);
}
fn scene_convert(c:vec4<f32>,convert:f32)->vec4<f32> {
    if convert==1. {return working_encode(c);}
    if convert==2. {return working_decode(c);}
    if convert==3. {return working_clamp(c);}
    if convert==4. {return working_clamp(working_decode(c));}
    return c;
}
fn scene_sample(image:texture_2d<f32>,uv:vec2<f32>)->vec4<f32> {
    return working_sample_float(image,uv*vec2<f32>(textureDimensions(image)));
}
// A one-to-one copy has exact half-pixel coordinates. Reconstructing them
// from interpolated UVs introduces a crop-size-dependent bilinear footprint.
fn scene_read(image:texture_2d<f32>,v:Vertex)->vec4<f32> {
    let local=v.position.xy-settings.rect.xy;
    let dimensions=vec2<f32>(textureDimensions(image));
    if all(dimensions==settings.rect.zw) {
        if all(fract(settings.rect.xy)==vec2<f32>(0.)) {
            return textureLoad(image,clamp(vec2<i32>(floor(local)),vec2<i32>(0),vec2<i32>(dimensions)-1),0);
        }
        return working_sample_float(image,local);
    }
    return working_sample_float(image,local*(dimensions/settings.rect.zw));
}
fn scene_image_texel(p:vec2<i32>)->vec4<f32> {
    if any(p<vec2(0)) || any(p>=vec2<i32>(textureDimensions(front))) {return vec4(0.);}
    return textureLoad(front,p,0);
}
fn scene_image_bilinear(world:vec2<f32>)->vec4<f32> {
    let m=settings.operation_linear;
    let local=vec2(m.x*world.x+m.z*world.y,m.y*world.x+m.w*world.y)+settings.operation_offset.xy;
    if any(local<vec2(-.5)) || any(local>vec2<f32>(textureDimensions(front))+vec2(.5)) {return vec4(0.);}
    let p=local-vec2(.5);let base=vec2<i32>(floor(p));let t=fract(p);
    // Almost every footprint is interior. Prove its four loads valid once,
    // keeping transparent-edge handling out of each interior texture fetch.
    if all(base>=vec2(0)) && all(base+vec2(1)<vec2<i32>(textureDimensions(front))) {
        return mix(mix(textureLoad(front,base,0),textureLoad(front,base+vec2(1,0),0),t.x),
            mix(textureLoad(front,base+vec2(0,1),0),textureLoad(front,base+vec2(1,1),0),t.x),t.y);
    }
    return mix(mix(scene_image_texel(base),scene_image_texel(base+vec2(1,0)),t.x),
        mix(scene_image_texel(base+vec2(0,1)),scene_image_texel(base+vec2(1,1)),t.x),t.y);
}
fn scene_image(v:Vertex)->vec4<f32> {
    let world=(v.position.xy-settings.rect.xy)+settings.color.xy;
    let m=settings.operation_linear;
    let count=vec2<u32>(clamp(floor(vec2(length(m.xy),length(m.zw))+.5),vec2(1.),vec2(4.)));
    var sum=vec4(0.);
    for (var j=0u;j<count.y;j++) {
        for (var i=0u;i<count.x;i++) {
            sum+=scene_image_bilinear(world+(vec2(f32(i),f32(j))+.5)/vec2<f32>(count)-.5);
        }
    }
    return sum/f32(count.x*count.y);
}
// Closest point on an ellipse from the Lagrange multiplier equation:
// sum((axis * point / (axis^2 + t))^2) = 1. Bounded bisection avoids
// eccentricity-dependent convergence and keeps outline width in image pixels.
fn ellipse_distance(point: vec2<f32>, axes: vec2<f32>) -> f32 {
    let swap = axes.y > axes.x;
    let r = select(axes, axes.yx, swap);
    let p = abs(select(point, point.yx, swap));
    let inside = dot(p/r,p/r) < 1.;
    if r.x-r.y < r.x*.000001 { return length(p)-r.x; }
    let r2 = r*r;
    var q: vec2<f32>;
    if p.y <= r.y*.000001 {
        let x = min(r.x,r2.x*p.x/(r2.x-r2.y));
        q = vec2<f32>(x,r.y*sqrt(max(0.,1.-x*x/r2.x)));
    } else {
        var low = r.y*p.y-r2.y;
        var high = 0.;
        if !inside { low = max(0.,low); high = length(r*p); }
        for (var i=0u;i<24u;i++) {
            let t = (low+high)*.5;
            let v = r*p/max(r2+t,vec2<f32>(.000000000001));
            if dot(v,v)>1. { low=t; } else { high=t; }
        }
        q = r2*p/max(r2+(low+high)*.5,vec2<f32>(.000000000001));
    }
    return select(length(p-q),-length(p-q),inside);
}
fn figure_color(p: vec2<f32>) -> vec4<f32> {
    let code = u32(settings.options.y)%16u;
    let shape = code%3u;
    let paint = code/3u;
    let a = settings.backdrop.xy;
    let b = settings.backdrop.zw;
    let half_width = settings.options.z*.5;
    var d: f32;
    if shape==0u {
        let delta=b-a;
        let t=clamp(dot(p-a,delta)/max(dot(delta,delta),.000001),0.,1.);
        d=length(p-a-delta*t)-half_width;
    } else {
        let radius=abs(b-a)*.5;
        let local=p-(a+b)*.5;
        if shape==1u {
            let q=abs(local)-radius;
            d=length(max(q,vec2<f32>(0.)))+min(max(q.x,q.y),0.);
        } else { d=ellipse_distance(local,radius); }
    }
    let foreground=vec4<f32>(settings.color.rgb*settings.color.a,settings.color.a);
    if shape==0u { return foreground*clamp(.5-d,0.,min(1.,settings.options.z)); }
    if paint==1u { return foreground*clamp(.5-d,0.,1.); }
    let outer=clamp(.5+half_width-d,0.,1.);
    let inner=clamp(.5-half_width-d,0.,1.);
    let outline=outer-inner;
    if paint==0u { return foreground*outline; }
    let interior=inner;
    let background=vec4<f32>(settings.source_over.rgb*settings.source_over.a,settings.source_over.a);
    return foreground*outline+background*interior;
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    if settings.options.x!=12. && (any(v.uv<vec2<f32>(0.)) || any(v.uv>vec2<f32>(1.))) { discard; }
    return scene_value(v);
}
fn scene_value(v: Vertex) -> vec4<f32> {
    if settings.options.x==12. {return scene_space(scene_image(v))*settings.options.y;}
    let op = u32(settings.options.x);
    if op == 0u { return settings.color; }
    if op == 9u {
        let p=v.uv;
        let bands=.5+.5*cos(vec3<f32>(0.,2.,4.)+p.x*8.);
        let light=.25+.65*p.y;
        let detail=select(.8,1.,(u32(p.x*48.)+u32(p.y*12.))%2u==0u);
        return vec4<f32>(bands*light*detail,1.);
    }
    if op == 10u {
        let p=settings.color.xy+v.uv*settings.color.zw;
        let ink=scene_sample(front,p/vec2<f32>(textureDimensions(front)));
        let mask=scene_read(back,v).a;
        return hdr_map_sdr(ink,settings.operation_linear,settings.operation_offset)*mask;
    }
    let raw = scene_read(front,v);
    if op == 6u || op == 11u || op == 18u {
        // Constant fills are the degenerate case (equal endpoint colors).
        let local = settings.extent.zw + v.uv * settings.rect.zw;
        let p = mat2x2<f32>(settings.operation_linear.xy, settings.operation_linear.zw) * local + settings.operation_offset.xy;
        var src: vec4<f32>;
        if op == 11u { src = figure_color(p)*raw.a; }
        else if op==18u {
            let flags=u32(settings.options.z);
            let t=gradient_shape(p-settings.backdrop.xy,settings.backdrop.zw-settings.backdrop.xy,flags%4u,flags>=4u);
            let sample=gradient_sample(0xffffffffu,t);
            let color=gradient_dither(sample.color,p,settings.options.y,sample.dither);
            src=vec4(color.rgb*color.a,color.a)*settings.color.x*raw.a;
        } else {src=vec4(settings.color.rgb*settings.color.a,settings.color.a)*raw.a;}
        let dst = scene_read(back,v);
        if op==11u && settings.options.y>=16. {
            return select(dst*(1.-src.a),dst,settings.options.w>.5);
        }
        if settings.options.w > .5 {
            return vec4<f32>(src.rgb * dst.a + dst.rgb * (1. - src.a), dst.a);
        }
        return src + dst * (1. - src.a);
    }
    if op == 1u || op == 7u || op == 13u || op == 14u || op == 17u || op == 19u { return scene_pointwise(raw,v); }
    // Store pooled mask coverage in alpha to avoid an sRGB encode/decode
    // round trip quantizing feather coverage through a color channel.
    if op == 2u { let m = mix(raw.r,1.-raw.r,settings.options.z); return vec4<f32>(m); }
    let dst = scene_read(back,v);
    if op == 20u { return working_decode(vec4<f32>(vec3<f32>(raw.r), 1.)); }
    if op == 21u {
        let gray = clamp(dot(sdr_encode(working_to_srgb(working_unassociate(raw)), 0u), vec3<f32>(.2126, .7152, .0722)), 0., 1.) * raw.a;
        return vec4<f32>(clamp(gray + dst.r * (1. - raw.a), 0., 1.));
    }
    if op == 22u { return vec4<f32>(raw.r * (1. - dst.a)); }
    if op == 16u { return raw * settings.options.y + dst * settings.options.z; }
    if op == 3u { return raw*mix(dst.a,1.-dst.a,settings.options.z); }
    if op == 5u { let a = (1.-raw.a)*.42; return scene_space(vec4<f32>(.46,.12,.8,1.)*a); }
    let src = select(raw*settings.options.y,raw,settings.options.y==1.);
    if settings.options.w > .5 { return blend_clip(src,dst,u32(settings.options.z)); }
    return blend_composite(src,dst,u32(settings.options.z));
}

fn scene_pointwise(raw: vec4<f32>, v: Vertex) -> vec4<f32> {
    let op = u32(settings.options.x);
    if op == 1u { return scene_space(raw)*settings.options.y; }
    if op == 14u { return scene_space(raw + scene_read(back,v) * (1. - raw.a)) * settings.options.y; }
    if op == 19u {
        let size=vec2<f32>(textureDimensions(front));
        let p=(v.position.xy-settings.rect.xy)*size/settings.rect.zw;
        let preview=textureLoad(front,clamp(vec2<i32>(floor(p)),vec2(0),vec2<i32>(size)-1),0);
        let base=scene_read(back,v);
        if settings.options.z>.5 {
            let result=working_encode(preview)+working_encode(base)*(1.-preview.a);
            if settings.operation_offset.w==1. {return result*settings.options.y;}
            return scene_space(working_decode(result))*settings.options.y;
        }
        return scene_space(preview+base*(1.-preview.a))*settings.options.y;
    }
    if op == 17u {
        let original=working_unassociate(raw);
        let low=working_unassociate(working_encode(scene_read(back,v)));
        return vec4<f32>((.5+(original-low)*.5)*raw.a,raw.a);
    }
    return scene_normal(raw,v);
}

fn scene_normal(raw: vec4<f32>, v: Vertex) -> vec4<f32> {
    var m = settings.options.z;
    if settings.options.w>=2. {
        let mask=scene_read(back,v).r;
        m=mix(mask,1.-mask,settings.options.w-2.);
    }
    let src = scene_space(raw) * m * settings.options.y;
    if settings.options.x == 13. { return scene_convert(src + settings.source_over * (1. - src.a),settings.extent.w); }
    return src;
}
