use super::*;

#[test]
#[ignore = "writes SPIR-V of a preset's dry-material kernel for the Mali Offline Compiler"]
fn dry_material_kernel_spirv() {
    let out = std::env::var("CAPY_KERNEL_SPIRV").expect("CAPY_KERNEL_SPIRV names the output file");
    let id = std::env::var("CAPY_KERNEL_PRESET").map_or(35, |id| id.parse::<u32>().unwrap());
    let preset = layer_core::CONTACT_BRUSH_PRESETS.into_iter().find(|p| *p as u32 == id).unwrap();
    let brush = layer_core::default_brush(preset);
    let uniform = brush.rendering.accumulation == BrushAccumulation::Uniform;
    let r = WgpuRasterizer::new_native_headless(layer_core::color::DocumentColor::default()).unwrap();
    let source = dry_material::shader_source(r.device.working_space(), dry_material::Target::Exact, include_str!("material_brush.wgsl"))
        .replace("override CONTACT_FLAGS: u32 = 4294967295u;",
            &format!("const CONTACT_FLAGS: u32 = {}u;", dry_material::contact_flags(brush.contact)))
        .replace("override MATERIAL_OPERATION: u32;", &format!("const MATERIAL_OPERATION: u32 = {}u;", u32::from(uniform)))
        .replace("override MATERIAL_IN_PLACE: bool = false;", "const MATERIAL_IN_PLACE: bool = false;");
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap();
    let pipeline = naga::back::spv::PipelineOptions {
        shader_stage: naga::ShaderStage::Compute,
        entry_point: if uniform { "compute_coverage" } else { "compute_color" }.into(),
    };
    let options = naga::back::spv::Options { lang_version: (1, 3), ..Default::default() };
    let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline)).unwrap();
    std::fs::write(out, words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
}
