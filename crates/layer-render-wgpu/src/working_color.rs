//! Shared working-color shader contract, selected before pipeline compilation.
use crate::PipelineDevice;
use layer_core::color::RgbSpace;

pub(crate) fn shader(device: &PipelineDevice) -> String {
    source(device.working_space())
}
pub(crate) fn source(space: RgbSpace) -> String {
    let mut shader = crate::view_color::transform(
        "working_to_srgb",
        space,
        RgbSpace::Srgb,
    );
    shader.push_str(&crate::view_color::transform(
        "working_from_srgb",
        RgbSpace::Srgb,
        space,
    ));
    let space_id = RgbSpace::ALL.iter().position(|s| *s == space).unwrap();
    shader.push_str(&format!("const WORKING_SPACE:u32={space_id}u;\n"));
    shader.push_str(include_str!("sdr_color.wgsl"));
    shader.push_str(include_str!("working_color.wgsl"));
    shader
}
