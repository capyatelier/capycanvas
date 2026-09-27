// Region tolerance compares encoded document RGB weighted by alpha.
// Only artwork enters this comparison, never view/checker/proof overlays.
fn comparison_color(value: vec4<f32>) -> vec4<f32> {
    let straight = working_unassociate(value);
    return vec4<f32>(sdr_encode(straight,WORKING_SPACE)*value.a,value.a);
}
