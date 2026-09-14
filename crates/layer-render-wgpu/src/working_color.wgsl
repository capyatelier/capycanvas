// Shared color arithmetic for compositing and materials. Scalar wetness/slop
// thresholds are separate from the question of whether pigment has coverage.
fn working_unassociate(c:vec4<f32>)->vec3<f32> {
    if WORKING_EXTENDED {
        if c.a>0. {return c.rgb/c.a;}
        return vec3<f32>(0.);
    }
    return c.rgb/max(c.a,.000001);
}
fn working_has_color(alpha:f32)->bool {
    if WORKING_EXTENDED {return alpha>0.;}
    return alpha>.000001;
}
fn working_ratio(value:f32,denominator:f32)->f32 {
    if WORKING_EXTENDED {
        if denominator>0. {return value/denominator;}
        return 0.;
    }
    return value/max(denominator,.000001);
}
fn working_clamp(c:vec4<f32>)->vec4<f32> {
    if WORKING_EXTENDED {
        if c.a<=0. {return vec4<f32>(0.);}
        return vec4<f32>(c.rgb,clamp(c.a,0.,1.));
    }
    return clamp(c,vec4<f32>(0.),vec4<f32>(1.));
}
fn working_mix(a:vec3<f32>,b:vec3<f32>,amount:f32)->vec3<f32> {
    if WORKING_EXTENDED {
        if amount==0. {return a;}
        if amount==1. {return b;}
    }
    return mix(a,b,amount);
}
// Texture coordinates are texel centers; callers retain their edge policy.
fn working_sample_float(image:texture_2d<f32>,point:vec2<f32>)->vec4<f32> {
    let p=point-.5;let origin=vec2<i32>(floor(p));let f=fract(p);
    let maximum=vec2<i32>(textureDimensions(image))-1;
    let a=textureLoad(image,clamp(origin,vec2<i32>(0),maximum),0);
    let b=textureLoad(image,clamp(origin+vec2<i32>(1,0),vec2<i32>(0),maximum),0);
    let c=textureLoad(image,clamp(origin+vec2<i32>(0,1),vec2<i32>(0),maximum),0);
    let d=textureLoad(image,clamp(origin+vec2<i32>(1,1),vec2<i32>(0),maximum),0);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
// Oklab conversions adapted from Björn Ottosson's MIT-licensed reference:
// https://bottosson.github.io/posts/oklab/ — see THIRD_PARTY_NOTICES.md.
// Adaptations: WGSL, signed cube roots, and document-primary transforms.
// Native intermediates preserve extended RGB; bounded targets quantize on commit.
fn working_to_oklab(value: vec3<f32>) -> vec3<f32> {
    let color = working_to_srgb(value);
    let lms = vec3<f32>(
        0.4122214708 * color.r + 0.5363325363 * color.g + 0.0514459929 * color.b,
        0.2119034982 * color.r + 0.6806995451 * color.g + 0.1073969566 * color.b,
        0.0883024619 * color.r + 0.2817188376 * color.g + 0.6299787005 * color.b,
    );
    let root = sign(lms) * pow(abs(lms), vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        0.2104542553 * root.x + 0.7936177850 * root.y - 0.0040720468 * root.z,
        1.9779984951 * root.x - 2.4285922050 * root.y + 0.4505937099 * root.z,
        0.0259040371 * root.x + 0.7827717662 * root.y - 0.8086757660 * root.z,
    );
}

fn working_from_oklab(color: vec3<f32>) -> vec3<f32> {
    let root = vec3<f32>(
        color.x + 0.3963377774 * color.y + 0.2158037573 * color.z,
        color.x - 0.1055613458 * color.y - 0.0638541728 * color.z,
        color.x - 0.0894841775 * color.y - 1.2914855480 * color.z,
    );
    let lms = root * root * root;
    let srgb = vec3<f32>(
        4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
       -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
       -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z,
    );
    if WORKING_EXTENDED {return working_from_srgb(srgb);}
    return max(srgb, vec3<f32>(0.));
}
