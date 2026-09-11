// Original WGSL filter library. Parameters, footprints and pass order are in
// manifest.json. No platform code or filter-specific renderer branches.
const FX_PI:f32=3.141592653589793;
fn fx_axis(degrees:f32)->vec2<f32> {let a=degrees*FX_PI/180.;return vec2<f32>(cos(a),sin(a));}
fn fx_rotate(p:vec2<f32>,axis:vec2<f32>)->vec2<f32> {return vec2<f32>(p.x*axis.x-p.y*axis.y,p.x*axis.y+p.y*axis.x);}
fn fx_straight(c:vec4<f32>)->vec3<f32> {return c.rgb/max(c.a,.000001);}
fn fx_random(p:vec2<f32>,seed:u32)->f32 {
    let q=vec2<u32>(vec2<i32>(floor(p)));
    var h=q.x*1664525u+q.y*1013904223u+seed*747796405u;
    h=(h^(h>>15u))*2246822519u;
    return f32(h^(h>>13u))/4294967295.;
}
fn fx_noise(p:vec2<f32>,seed:u32)->f32 {
    let q=floor(p);let f=fract(p);let t=f*f*(3.-2.*f);
    return mix(mix(fx_random(q,seed),fx_random(q+vec2<f32>(1.,0.),seed),t.x),mix(fx_random(q+vec2<f32>(0.,1.),seed),fx_random(q+1.,seed),t.x),t.y)*2.-1.;
}
fn fx_fbm(p:vec2<f32>,octaves:u32,seed:u32)->f32 {
    var q=p;var total=0.;var weight=.5;var norm=0.;
    for(var i=0u;i<octaves;i+=1u){total+=fx_noise(q,seed+i)*weight;norm+=weight;q=fx_rotate(q,vec2<f32>(.8,.6))*2.07+7.3;weight*=.5;}
    return total/max(norm,.00001);
}
fn fx_bright(c:vec4<f32>,threshold:f32)->vec4<f32> {
    if threshold<0. {return c;}
    let l=fx_luma(fx_straight(c));return c*max(l-threshold,0.)/max(l,.00001);
}
fn fx_blur(p:vec2<f32>,base:u32,axis:vec2<f32>,threshold:f32)->vec4<f32> {
    let info=fx_lookup(base,0u,0u);var sum=fx_bright(fx_sample(p),threshold)*info.x;
    for(var i=1u;i<=u32(info.y);i+=1u){let tap=fx_lookup(base,0u,i);let offset=axis*tap.x;
        sum+=(fx_bright(fx_sample(p-offset),threshold)+fx_bright(fx_sample(p+offset),threshold))*tap.y;
    }return sum;
}
fn capy_blur_h(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_blur(p,b,vec2<f32>(1.,0.),-1.);}
fn capy_gaussian_blur(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_blur(p,b,vec2<f32>(0.,1.),-1.);}
fn capy_unsharp_mask(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let original=fx_original(p);let blurred=capy_gaussian_blur(c,p,b);let detail=fx_straight(original)-fx_straight(blurred);
    let threshold=fx_parameter(b,2u).x/100.;let gate=smoothstep(threshold,threshold+.02,length(detail));
    return vec4<f32>(clamp(fx_straight(original)+detail*(fx_parameter(b,1u).x/100.)*gate,vec3<f32>(0.),vec3<f32>(1.))*original.a,original.a);
}
fn capy_high_pass(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let original=fx_original(p);let blurred=capy_gaussian_blur(c,p,b);
    return fx_rgba(.5+(fx_rgb(original)-fx_rgb(blurred))*fx_parameter(b,1u).x/100.,original.a);
}
fn capy_bloom_h(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_blur(p,b,vec2<f32>(1.,0.),fx_parameter(b,2u).x/100.);}
fn capy_bloom(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let original=fx_original(p);let glow=capy_gaussian_blur(c,p,b)*fx_parameter(b,1u).x/100.;
    let alpha=min(1.,original.a+glow.a);return vec4<f32>(min(original.rgb+glow.rgb,vec3<f32>(alpha)),alpha);
}
fn capy_soft_focus(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let original=fx_original(p);let soft=fx_straight(capy_gaussian_blur(c,p,b))*original.a;
    return vec4<f32>(mix(original.rgb,max(original.rgb,soft),fx_parameter(b,1u).x/100.),original.a);
}
fn capy_pencil(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let original=fx_original(p);let blur=capy_gaussian_blur(c,p,b);
    let ratio=clamp((fx_luma(fx_rgb(original))+.01)/(fx_luma(fx_rgb(blur))+.01),0.,1.);
    let ink=1.-pow(ratio,1.+fx_parameter(b,1u).x*.12);
    return fx_rgba(mix(fx_parameter(b,3u).rgb,fx_parameter(b,2u).rgb,ink),original.a);
}
fn capy_motion_blur(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let distance=fx_parameter(b,0u).x;if distance<.001{return c;}
    let axis=fx_axis(fx_parameter(b,1u).x);let count=max(2u,u32(ceil(distance))+1u);var sum=vec4<f32>(0.);
    for(var i=0u;i<count;i+=1u){sum+=fx_sample(p+axis*distance*(f32(i)/f32(count-1u)-.5));}
    return sum/f32(count);
}
fn capy_denoise(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let amount=fx_parameter(b,1u).x/100.;if amount<.0001{return c;}
    let radius=i32(fx_parameter(b,0u).x);let center=fx_straight(c);let range=.01+amount*.35;
    var sum=vec3<f32>(0.);var total=0.;
    for(var y=-radius;y<=radius;y+=1){for(var x=-radius;x<=radius;x+=1){
        let v=fx_sample(p+vec2<f32>(f32(x),f32(y)));let diff=fx_straight(v)-center;
        let weight=exp2(-dot(diff,diff)/(range*range))/(1.+f32(x*x+y*y))*v.a;
        sum+=fx_straight(v)*weight;total+=weight;
    }}return vec4<f32>(sum/max(total,.000001)*c.a,c.a);
}
fn capy_edge_detect(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let radius=fx_parameter(b,0u).x;var dx=0.;var dy=0.;
    for(var y=-1;y<=1;y+=1){for(var x=-1;x<=1;x+=1){
        let l=fx_luma(fx_straight(fx_sample(p+vec2<f32>(f32(x),f32(y))*radius)));
        dx+=l*f32(x)*select(1.,2.,y==0);dy+=l*f32(y)*select(1.,2.,x==0);
    }}var edge=clamp(length(vec2<f32>(dx,dy))*.25*fx_parameter(b,1u).x/100.,0.,1.);
    if fx_parameter(b,2u).x>.5{edge=1.-edge;}return fx_rgba(vec3<f32>(edge),c.a);
}
fn capy_white_balance(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let t=fx_parameter(b,0u).x/100.;let tint=fx_parameter(b,1u).x/100.;let original=fx_straight(c);
    var rgb=original*exp2(vec3<f32>(t*.8+tint*.25,-tint*.5,-t*.8+tint*.25));
    if fx_parameter(b,2u).x>.5{rgb*=fx_luma(original)/max(fx_luma(rgb),.000001);}
    return vec4<f32>(clamp(rgb,vec3<f32>(0.),vec3<f32>(1.))*c.a,c.a);
}
fn capy_split_tone(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let rgb=fx_rgb(c);let l=fx_luma(rgb);let balance=fx_parameter(b,2u).x/250.;
    let tone=mix(fx_parameter(b,0u).rgb,fx_parameter(b,1u).rgb,smoothstep(.15+balance,.85+balance,l));
    return fx_rgba(mix(rgb,fx_preserve_luma(tone,l),fx_parameter(b,3u).x/100.),c.a);
}
fn capy_vignette(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let center=vec2<f32>(fx_parameter(b,3u).x,fx_parameter(b,4u).x)/100.;
    let d=length((p/fx_extent()-center)*2.);let radius=fx_parameter(b,1u).x/100.;let soft=max(.01,fx_parameter(b,2u).x/100.);
    let mask=smoothstep(radius*(1.-soft),radius,d);return vec4<f32>(c.rgb*exp2(-2.*mask*fx_parameter(b,0u).x/100.),c.a);
}
fn capy_film_grain(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let size=fx_parameter(b,1u).x;let frame=u32(floor(fx_time(b)*24.*fx_parameter(b,3u).x));
    var grain=vec3<f32>(fx_random(p/size,frame));if fx_parameter(b,2u).x>.5{grain=vec3<f32>(grain.x,fx_random(p/size,frame+13u),fx_random(p/size,frame+37u));}
    let rgb=fx_rgb(c);let l=fx_luma(rgb);let amplitude=(.15+.85*sqrt(max(0.,l*(1.-l))*4.))*fx_parameter(b,0u).x/250.;
    return fx_rgba(rgb+(grain-.5)*amplitude,c.a);
}
fn capy_halftone(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let size=fx_parameter(b,0u).x;let axis=fx_axis(fx_parameter(b,1u).x);let q=fx_rotate(p,axis)/size;
    let center=fx_rotate((floor(q)+.5)*size,vec2<f32>(axis.x,-axis.y));let l=fx_luma(fx_rgb(fx_sample(center)));
    let density=clamp((1.-l-.5)*(1.+fx_parameter(b,2u).x/100.)+.5,0.,1.);let radius=.7071068*sqrt(density);
    let dot=1.-smoothstep(radius-.5/size,radius+.5/size,length(fract(q)-.5));
    let ink=select(select(dot,1.,density>=.9999),0.,density<=.0001);
    return fx_rgba(mix(fx_parameter(b,4u).rgb,fx_parameter(b,3u).rgb,ink),c.a);
}
fn capy_crosshatch(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let spacing=fx_parameter(b,0u).x;let width=fx_parameter(b,1u).x;let l=fx_luma(fx_rgb(c));var ink=0.;
    for(var i=0u;i<4u;i+=1u){let axis=fx_axis(fx_parameter(b,2u).x+f32(i)*45.);let distance=abs(fract(dot(p,axis)/spacing)-.5)*spacing;
        let darkness=1.-smoothstep(.7-f32(i)*.18,.9-f32(i)*.18,l);ink=max(ink,(1.-smoothstep(max(0.,width*.5-.5),width*.5+.5,distance))*darkness);}
    return fx_rgba(mix(fx_parameter(b,4u).rgb,fx_parameter(b,3u).rgb,ink),c.a);
}
fn capy_emboss(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let offset=fx_axis(fx_parameter(b,1u).x)*fx_parameter(b,0u).x;
    let slope=fx_luma(fx_rgb(fx_sample(p+offset)))-fx_luma(fx_rgb(fx_sample(p-offset)));
    return fx_rgba(vec3<f32>(.5+slope*fx_parameter(b,2u).x/100.),c.a);
}
fn capy_pixel_mosaic(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let size=fx_parameter(b,0u).x;return fx_sample((floor(p/size)+.5)*size);}
fn capy_chromatic_aberration(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let offset=fx_axis(fx_parameter(b,1u).x)*fx_parameter(b,0u).x;let red=fx_sample(p+offset);let blue=fx_sample(p-offset);
    return vec4<f32>(red.r,c.g,blue.b,max(c.a,max(red.a,blue.a)));
}
// Four overlapping quadrant statistics, using a fixed 3x3 quadrature in each.
// This bounded painterly approximation is independent of radius in tap count.
fn capy_painterly(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let radius=fx_parameter(b,0u).x;var best=1e10;var result=fx_straight(c);
    for(var quadrant=0u;quadrant<4u;quadrant+=1u){
        let sign=vec2<f32>(select(-1.,1.,(quadrant&1u)!=0u),select(-1.,1.,(quadrant&2u)!=0u));
        var sum=vec3<f32>(0.);var square=vec3<f32>(0.);var total=0.;
        for(var y=0u;y<3u;y+=1u){for(var x=0u;x<3u;x+=1u){let v=fx_sample(p+sign*vec2<f32>(f32(x),f32(y))*.5*radius);sum+=v.rgb;square+=v.rgb*fx_straight(v);total+=v.a;}}
        if total>.0001{let mean=sum/total;let variance=dot(max(vec3<f32>(0.),square/total-mean*mean),vec3<f32>(1.));if variance<best{best=variance;result=mean;}}
    }return vec4<f32>(mix(c.rgb,result*c.a,fx_parameter(b,1u).x/100.),c.a);
}
fn capy_solarize(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let rgb=fx_rgb(c);return fx_rgba(mix(rgb,select(rgb,1.-rgb,rgb>vec3<f32>(fx_parameter(b,0u).x/100.)),fx_parameter(b,1u).x/100.),c.a);}
fn capy_kaleidoscope(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let center=fx_extent()*vec2<f32>(fx_parameter(b,2u).x,fx_parameter(b,3u).x)/100.;let delta=p-center;
    let rotation=fx_parameter(b,1u).x*FX_PI/180.;let sector=2.*FX_PI/fx_parameter(b,0u).x;
    let angle=abs(fract((atan2(delta.y,delta.x)-rotation)/sector+.5)-.5)*sector+rotation;
    return fx_sample(center+vec2<f32>(cos(angle),sin(angle))*length(delta));
}
fn capy_swirl(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let center=fx_extent()*vec2<f32>(fx_parameter(b,2u).x,fx_parameter(b,3u).x)/100.;let delta=p-center;
    let radius=min(fx_extent().x,fx_extent().y)*.5*fx_parameter(b,1u).x/100.;let fade=max(0.,1.-length(delta)/max(radius,.001));
    return fx_sample(center+fx_rotate(delta,fx_axis(fx_parameter(b,0u).x*fade*fade)));
}
fn capy_ripple(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let center=fx_extent()*vec2<f32>(fx_parameter(b,3u).x,fx_parameter(b,4u).x)/100.;let delta=p-center;let radius=length(delta);
    // Reduce each term before subtracting: long-running time must not erase
    // the spatial phase. Keep sin inside WGSL's specified accuracy interval.
    let phase=fract(radius/fx_parameter(b,1u).x)-fract(fx_time(b)*fx_parameter(b,2u).x);
    let wave=sin(2.*FX_PI*(phase-floor(phase+.5)));
    return fx_sample(p+delta/max(radius,1.)*wave*fx_parameter(b,0u).x);
}
fn capy_glass(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let q=p/fx_parameter(b,1u).x;let rough=fx_parameter(b,2u).x/100.;
    let coarse=vec2<f32>(fx_noise(q,3u),fx_noise(q,17u));let fine=vec2<f32>(fx_noise(q*4.,7u),fx_noise(q*4.,29u));
    return fx_sample(p+mix(coarse,fine,rough)*fx_parameter(b,0u).x);
}
fn capy_rainy_glass(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let scale=fx_parameter(b,1u).x;let time=fx_time(b)*fx_parameter(b,3u).x;let grid=p/vec2<f32>(scale,scale*1.5);
    let cell=floor(grid);let seed=fx_random(cell,17u);let phase=fract(time*.35+seed);
    let center=vec2<f32>(.25+.5*fx_random(cell,31u),phase);let delta=fract(grid)-center;
    let enabled=1.-step(fx_parameter(b,2u).x/100.,seed);
    let drop=(1.-smoothstep(.12,.27,length(delta*vec2<f32>(1.,1.3))))*sin(FX_PI*phase)*enabled;
    let trail=(1.-smoothstep(.015,.07,abs(delta.x)))*smoothstep(0.,.4,-delta.y)*.35*enabled;
    let offset=sin(delta*2.*FX_PI)*fx_parameter(b,0u).x*max(drop,trail);
    let source=fx_sample(p+offset);let highlight=drop*(1.-drop)*.08;
    return vec4<f32>(min(source.rgb+highlight*source.a,vec3<f32>(source.a)),source.a);
}
fn capy_vhs(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let time=fx_time(b)*fx_parameter(b,3u).x;let frame=u32(floor(time*24.));let distance=fx_parameter(b,0u).x;
    let jitter=(fx_random(vec2<f32>(floor(p.y/7.),0.),frame)*2.-1.)*distance;
    let q=p+vec2<f32>(jitter,0.);let red=fx_sample(q+vec2<f32>(distance*.3,0.));let green=fx_sample(q);let blue=fx_sample(q-vec2<f32>(distance*.3,0.));
    let alpha=max(red.a,max(green.a,blue.a));var rgb=vec3<f32>(red.r,green.g,blue.b);
    rgb*=1.-fx_parameter(b,2u).x/100.*(.5+.5*sin(p.y*FX_PI));
    rgb+=(fx_random(p,frame+91u)-.5)*fx_parameter(b,1u).x/300.*alpha;
    return vec4<f32>(clamp(rgb,vec3<f32>(0.),vec3<f32>(alpha)),alpha);
}
fn capy_crt(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let uv=p/fx_extent()*2.-1.;let q=(uv*(1.+dot(uv,uv)*fx_parameter(b,0u).x/100.)+1.)*.5*fx_extent();
    if any(q<vec2<f32>(0.)) || any(q>fx_extent()){return vec4<f32>(0.);}
    let separation=fx_parameter(b,3u).x;let red=fx_sample(q+vec2<f32>(separation,0.));let green=fx_sample(q);let blue=fx_sample(q-vec2<f32>(separation,0.));
    let alpha=max(red.a,max(green.a,blue.a));var rgb=vec3<f32>(red.r,green.g,blue.b);
    let stripe=u32(floor(p.x))%3u;let phosphor=select(vec3<f32>(1.-fx_parameter(b,2u).x/100.),vec3<f32>(1.),vec3<u32>(0u,1u,2u)==vec3<u32>(stripe));
    rgb*=phosphor*(1.-fx_parameter(b,1u).x/100.*(.5+.5*cos(p.y*FX_PI)));
    rgb*=.98+.02*sin(p.y/fx_extent().y*2.*FX_PI-fx_time(b)*1.4);
    return vec4<f32>(rgb,alpha);
}
fn capy_heat_haze(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let q=p/fx_parameter(b,1u).x+vec2<f32>(0.,-fx_time(b)*fx_parameter(b,2u).x);let detail=fx_parameter(b,3u).x/100.;
    let noise=mix(fx_noise(q,13u),fx_noise(q*3.7,47u),detail*.5);let side=fx_noise(q+11.7,29u);
    let amount=fx_parameter(b,0u).x*smoothstep(0.,1.,p.y/fx_extent().y);
    return fx_sample(p+vec2<f32>(noise,side*.35)*amount);
}
fn capy_iridescence(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let rgb=fx_rgb(c);let l=fx_luma(rgb);let thickness=l*2.+fx_noise(p/fx_parameter(b,1u).x,7u)*.35+fx_time(b)*fx_parameter(b,2u).x;
    let film=.5+.5*cos(thickness*vec3<f32>(5.1,6.4,7.2)+vec3<f32>(0.,1.,2.));
    return fx_rgba(mix(rgb,fx_preserve_luma(film,l),fx_parameter(b,0u).x/100.),c.a);
}
fn capy_domain_warp(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let octaves=u32(fx_parameter(b,2u).x);let time=fx_time(b)*fx_parameter(b,3u).x;let q=p/fx_parameter(b,1u).x+vec2<f32>(time*.17,-time*.23);
    let bend=vec2<f32>(fx_fbm(q,octaves,3u),fx_fbm(q+19.3,octaves,31u));
    let warp=vec2<f32>(fx_fbm(q+bend*1.4,octaves,71u),fx_fbm(q+bend*1.4+7.9,octaves,113u));
    return fx_sample(p+warp*fx_parameter(b,0u).x);
}
