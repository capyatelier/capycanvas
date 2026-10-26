@group(2) @binding(0) var<storage,read> panel_data: array<vec4<f32>>;
@group(2) @binding(1) var<storage,read> panel_profiles: array<vec2<f32>>;
@group(2) @binding(2) var panel_backdrop: texture_2d<f32>;
@group(2) @binding(3) var panel_sampler: sampler;
@group(2) @binding(4) var panel_coverage: texture_2d<f32>;

struct PanelVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) header: u32,
};

@vertex fn panel_vertex(@builtin(vertex_index) vertex: u32,@builtin(instance_index) instance: u32) -> PanelVertex {
    let corner=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let index=3u+instance*6u;
    let area=panel_data[index];
    return PanelVertex(surface_clip(area.xy+corner[vertex]*area.zw),index);
}

fn pnl_phase(value: f32) -> f32 {
    let t=clamp(value,0.,1.);
    return t*t*t*(10.+t*(-15.+6.*t));
}
fn pnl_norm(v: vec2<f32>) -> f32 { let q=v*v;return sqrt(sqrt(dot(q,q))); }
fn pnl_radii(bounds: vec4<f32>,radii: vec4<f32>) -> vec4<f32> {
    let size=vec4(bounds.z,bounds.z,bounds.w,bounds.w);
    let sums=vec4(radii.x+radii.y,radii.z+radii.w,radii.x+radii.w,radii.y+radii.z);
    let factors=size/max(sums,vec4(.0001));
    return max(radii,vec4(0.))*min(1.,min(min(factors.x,factors.y),min(factors.z,factors.w)));
}
fn pnl_rect(p: vec2<f32>,bounds: vec4<f32>,radii: vec4<f32>) -> f32 {
    let half=bounds.zw*.5;
    let q=p-bounds.xy-half;
    let r=pnl_radii(bounds,radii);
    var field=max(abs(q).x-half.x,abs(q).y-half.y);
    let far=bounds.xy+bounds.zw;
    if p.x<bounds.x+r.x && p.y<bounds.y+r.x { field=max(field,pnl_norm(p-bounds.xy-vec2(r.x))-r.x); }
    if p.x>far.x-r.y && p.y<bounds.y+r.y { field=max(field,pnl_norm(p-vec2(far.x-r.y,bounds.y+r.y))-r.y); }
    if p.x>far.x-r.z && p.y>far.y-r.z { field=max(field,pnl_norm(p-far+vec2(r.z))-r.z); }
    if p.x<bounds.x+r.w && p.y>far.y-r.w { field=max(field,pnl_norm(p-vec2(bounds.x+r.w,far.y-r.w))-r.w); }
    return field;
}
fn pnl_smooth_max(a: f32,b: f32,width: f32) -> f32 {
    if width<=0.00001 { return max(a,b); }
    let v=max(width-abs(a-b),0.);
    return max(a,b)+v*v*v/(6.*width*width);
}
fn pnl_profile(u: f32,delta: f32) -> vec2<f32> {
    if delta==0. || u>=3. { return vec2(u,1.); }
    if u<=-5. { return vec2(u+1.41421356237*delta,1.); }
    let uv=vec2((u+5.)*511./8.,clamp(delta,0.,4.)*64.);
    let low=vec2<u32>(floor(uv));
    let high=min(low+vec2(1u),vec2(511u,256u));
    let f=fract(uv);
    return mix(mix(panel_profiles[low.y*512u+low.x],panel_profiles[low.y*512u+high.x],f.x),
        mix(panel_profiles[high.y*512u+low.x],panel_profiles[high.y*512u+high.x],f.x),f.y);
}
fn pnl_upper(p: vec2<f32>,panel_top: f32,tile_top: f32,radius: f32) -> f32 {
    if tile_top<=panel_top || radius<=0. { return min(tile_top,panel_top)-p.y; }
    var delta=(tile_top-panel_top)/radius;
    var y=(p.y-panel_top)/radius;
    if delta>4. {
        if y>=2. && y<=delta-2. { return -p.x; }
        if y>delta-2. { y-=delta-4.; }
        delta=4.;
    }
    let x=p.x/radius;
    let u=(x-y)*.707106781187;
    let v=(x+y)*.707106781187;
    let profile=pnl_profile(u,delta);
    return (profile.x-v)*radius/sqrt(1.+profile.y*profile.y);
}
fn pnl_gap(p: vec2<f32>,gap: vec2<f32>,anchor: vec2<f32>,width: f32,scale: f32) -> f32 {
    let h=max(gap.y-gap.x,0.001*scale);
    let top=pnl_phase((gap.y-anchor.x)/h);
    let bottom=pnl_phase((anchor.y-gap.x)/h);
    let depth=min(2.*width/3.,2.*h/3.);
    let offset=depth*(1.-top*bottom);
    let q=(p.y-(gap.x+gap.y)*.5)/(h*.5);
    let x=max(offset-p.x,0.)/max(depth,0.001*scale);
    let value=1.-x*x*x-q*q;
    let gradient=length(vec2(3.*x*x/max(depth,0.001*scale),2.*q/(h*.5)));
    var distance=value/max(gradient,1./max(depth,h));
    let smoothing=min(width/4.,h/4.);
    distance=pnl_smooth_max(distance,anchor.x-p.y,smoothing*(1.-top));
    distance=pnl_smooth_max(distance,p.y-anchor.y,smoothing*(1.-bottom));
    return min(distance,min(p.y-gap.x,gap.y-p.y));
}
fn pnl_attachment(bounds: vec4<f32>,h: f32,anchor: vec2<f32>) -> f32 {
    return pnl_phase((bounds.y+bounds.w+h-anchor.x)/h)*pnl_phase((anchor.y+h-bounds.y)/h);
}
fn pnl_field(point: vec2<f32>,header: u32) -> f32 {
    let sidebar=panel_data[header+1u];
    let geometry=panel_data[header+3u];
    let counts=panel_data[header+5u];
    let p=vec2((point.x-geometry.x)*geometry.z,point.y);
    var panels=1e12;
    var radius=0.;
    for (var i=0u;i<u32(counts.y);i++) {
        let index=u32(counts.x)+i*7u;
        let b=panel_data[index];
        let r=panel_data[index+1u];
        radius=max(radius,max(max(r.x,r.y),max(r.z,r.w)));
        panels=min(panels,pnl_rect(point,b,r));
    }
    if panels < -2.*radius { return panels; }
    var joined=panels;
    var outer_top=1e12;
    var outer_bottom=1e12;
    if counts.y>0. {
        let first=u32(counts.x);
        let last=first+(u32(counts.y)-1u)*7u;
        let first_bounds=panel_data[first];
        let last_bounds=panel_data[last];
        let first_radii=pnl_radii(first_bounds,panel_data[first+1u]);
        let last_radii=pnl_radii(last_bounds,panel_data[last+1u]);
        let first_radius=select(first_radii.y,first_radii.x,geometry.z>0.);
        let last_radius=select(last_radii.z,last_radii.w,geometry.z>0.);
        for (var i=0u;i<u32(counts.w);i++) {
            let a=panel_data[u32(counts.z)+i];
            let anchor=vec2(a.y,a.y+a.w);
            let top=pnl_upper(p,first_bounds.y,anchor.x,first_radius);
            let bottom=pnl_upper(vec2(p.x,-p.y),-last_bounds.y-last_bounds.w,-anchor.y,last_radius);
            outer_top=min(outer_top,top);
            outer_bottom=min(outer_bottom,bottom);
            var bridge=1e12;
            for (var j=0u;j<u32(counts.y);j++) {
                let index=first+j*7u;
                let bounds=panel_data[index];
                if anchor.x>=bounds.y+bounds.w+6.*geometry.w || anchor.y<=bounds.y-6.*geometry.w { continue; }
                let radii=pnl_radii(bounds,panel_data[index+1u]);
                let upper_radius=select(radii.y,radii.x,geometry.z>0.);
                let lower_radius=select(radii.z,radii.w,geometry.z>0.);
                let upper=pnl_upper(p,bounds.y,anchor.x,upper_radius);
                let lower=pnl_upper(vec2(p.x,-p.y),-bounds.y-bounds.w,-anchor.y,lower_radius);
                let extent=max(upper_radius,lower_radius);
                let candidate=max(max(upper,lower),max(-geometry.y-p.x,p.x-extent));
                let attachment=pnl_attachment(bounds,6.*geometry.w,anchor);
                var receiver_field=candidate;
                if attachment<1. {
                    let rounded=pnl_rect(point,bounds,radii);
                    receiver_field=mix(rounded,min(rounded,candidate),attachment);
                }
                bridge=min(bridge,receiver_field);
            }
            for (var j=0u;j+1u<u32(counts.y);j++) {
                let upper=panel_data[first+j*7u];
                let lower=panel_data[first+(j+1u)*7u];
                if lower.y>upper.y+upper.w {
                    bridge=max(bridge,pnl_gap(p,vec2(upper.y+upper.w,lower.y),anchor,geometry.y,geometry.w));
                }
            }
            if first_bounds.y>anchor.x {
                bridge=max(bridge,pnl_gap(p,vec2(first_bounds.y-6.*geometry.w,first_bounds.y),anchor,geometry.y,geometry.w));
            }
            if last_bounds.y+last_bounds.w<anchor.y {
                bridge=max(bridge,pnl_gap(p,vec2(last_bounds.y+last_bounds.w,last_bounds.y+last_bounds.w+6.*geometry.w),anchor,geometry.y,geometry.w));
            }
            joined=min(joined,bridge);
        }
    }
    if counts.w>0. { joined=max(joined,max(outer_top,outer_bottom)); }
    if sidebar.z>0. && sidebar.w>0. { return min(joined,pnl_rect(point,sidebar,panel_data[header+2u])); }
    return joined;
}

fn pnl_ui(rgba: vec4<f32>) -> vec4<f32> {
    var rgb=view_ui_rgb(sdr_decode(rgba.rgb,0u));
    if camera.viewport.z>.5 { rgb=sdr_encode(rgb,0u); }
    return vec4(view_store(vec4(rgb,1.)).rgb*rgba.a,rgba.a);
}
fn pnl_foot(p: vec2<f32>,origin: vec2<f32>,radius: f32,direction: f32) -> f32 {
    let q=vec2((p.x-origin.x)*direction,origin.y-p.y);
    let rectangle=max(max(-q.x,q.x-radius),max(-q.y,q.y-radius));
    let corner=radius-pnl_norm(vec2(radius-q.x,radius-q.y));
    return max(rectangle,corner);
}
fn pnl_tint(p: vec2<f32>,header: u32) -> vec4<f32> {
    let counts=panel_data[header+5u];
    let scale=panel_data[header+3u].w;
    var color=pnl_ui(panel_data[header+4u]);
    for (var i=0u;i<u32(counts.y);i++) {
        let index=u32(counts.x)+i*7u;
        let b=panel_data[index];
        if p.x>=b.x && p.x<b.x+b.z && p.y>=b.y && p.y<b.y+b.w {
            color=pnl_ui(panel_data[index+5u]);
            let bar=panel_data[index+2u];
            if bar.z>0. && bar.w>0. {
                let r=min(18.*scale,min(bar.z*.5,bar.w));
                let coverage=clamp(.5-pnl_rect(p,bar,vec4(r,r,0.,0.)),0.,1.);
                let tab=panel_data[index+3u];
                let clip=panel_data[index+4u];
                var selected_coverage=0.;
                if tab.z>0. && p.x>=clip.x && p.x<clip.x+clip.z && p.y>=clip.y && p.y<clip.y+clip.w {
                    let tr=min(18.*scale,min(tab.z*.5,tab.w));
                    var field=pnl_rect(p,tab,vec4(tr,tr,0.,0.));
                    field=min(field,pnl_foot(p,vec2(tab.x,tab.y+tab.w),6.*scale,-1.));
                    field=min(field,pnl_foot(p,tab.xy+tab.zw,6.*scale,1.));
                    selected_coverage=clamp(.5-field,0.,1.);
                }
                color=mix(color,pnl_ui(panel_data[index+6u]),coverage*(1.-selected_coverage));
            }
            break;
        }
    }
    return color;
}

fn pnl_coverage(point: vec2<f32>,header: u32) -> vec2<f32> {
    let field=pnl_field(point,header);
    let coverage=clamp(.5-field,0.,1.);
    let scale=panel_data[header+3u].w;
    let shadow_distance=max(pnl_field(point-vec2(0.,2.*scale),header),0.);
    let shadow=.16*exp(-shadow_distance*shadow_distance/(32.*scale*scale));
    return vec2(coverage,shadow);
}

@vertex fn panel_mask_vertex(@builtin(vertex_index) vertex: u32,@builtin(instance_index) instance: u32) -> PanelVertex {
    let corner=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    return PanelVertex(vec4(corner[vertex]*vec2(2.,-2.)+vec2(-1.,1.),0.,1.),3u+instance*6u);
}

@fragment fn panel_mask_fragment(in: PanelVertex) -> @location(0) vec2<f32> {
    let origin=max(floor(panel_data[in.header].xy),vec2(0.));
    return pnl_coverage(origin+in.position.xy,in.header);
}

@fragment fn panel_fragment(in: PanelVertex) -> @location(0) vec4<f32> {
    let point=logical_surface(in.position.xy*camera.rotation.w);
    let origin=max(floor(panel_data[in.header].xy),vec2(0.));
    let mask=textureLoad(panel_coverage,vec2<i32>(floor(point-origin)),0).xy;
    let coverage=mask.x;
    let shadow=mask.y;
    var tint=pnl_tint(point,in.header);
    if panel_data[0].z>0. {
        let transform=panel_data[1];
        let offset=panel_data[2].xy;
        let p=in.position.xy;
        let uv=(vec2(transform.x*p.x+transform.z*p.y,transform.y*p.x+transform.w*p.y)+offset)*panel_data[0].xy;
        let blurred=textureSampleLevel(panel_backdrop,panel_sampler,uv,0.);
        tint=vec4(tint.rgb+blurred.rgb/max(blurred.a,1e-4)*(1.-tint.a),1.);
    }
    let alpha=tint.a*coverage+shadow*(1.-coverage);
    return vec4(tint.rgb*coverage,alpha)*window_coverage(point);
}
