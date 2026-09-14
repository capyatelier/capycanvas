// Region tolerance compares encoded document RGB weighted by alpha.
// Only artwork enters this comparison, never view/checker/proof overlays.
fn comparison_color(value: vec4<f32>) -> vec4<f32> {
    let straight = working_unassociate(value);
    if WORKING_EXTENDED {return vec4<f32>(sdr_encode(straight,WORKING_SPACE)*value.a,value.a);}
    let srgb = select(1.055 * pow(max(straight, vec3<f32>(0.)), vec3<f32>(1./2.4)) - .055,
        straight * 12.92, straight <= vec3<f32>(.0031308));
    return vec4<f32>(srgb * value.a, value.a);
}
