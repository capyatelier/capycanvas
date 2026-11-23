use super::*;

#[test]
#[ignore = "writes SPIR-V of a preset's material kernel for the Mali Offline Compiler"]
fn material_kernel_spirv() {
    let out = std::env::var("CAPY_KERNEL_SPIRV").expect("CAPY_KERNEL_SPIRV names the output file");
    let id = std::env::var("CAPY_KERNEL_PRESET").map_or(35, |id| id.parse::<u32>().unwrap());
    let preset = layer_core::DefaultBrushPreset::ALL.into_iter().find(|p| *p as u32 == id).unwrap();
    let brush = layer_core::default_brush(preset);
    let uniform = brush.rendering.accumulation == BrushAccumulation::Uniform;
    let extra = std::env::var("CAPY_KERNEL_EXTRA").map_or(0, |flags| flags.parse::<u32>().unwrap());
    let r = WgpuRasterizer::new_native_headless(layer_core::color::DocumentColor::default()).unwrap();
    let style = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
    let transported = destination_material::transported(&style);
    let flags = dry_material::contact_flags(brush.contact) | if uniform { 128 } else { 0 } | extra;
    let (operation, entries, entry) = if transported {
        let operation = BrushPassPlan::for_style(&style).material;
        let entry = if BrushPassPlan::for_style(&style).state.coverage { destination_material::Entry::Coverage } else { destination_material::Entry::Color };
        (operation, destination_material::module(operation, 0, destination_material::blender(&style), destination_material::Taps::Near), entry.name())
    } else {
        let start = if r.pipelines.dry_material.in_place() { dry_material::Start::Stored } else { dry_material::Start::Sampled };
        (if uniform { MaterialOperation::Coverage } else { MaterialOperation::Deposit }, dry_material::Entries::Kernel(start, 0),
            if uniform { "compute_coverage" } else { "compute_color" })
    };
    let source = dry_material::material_source(r.device.working_space(), r.device.float_sampling_wgsl(), entries, flags, operation);
    if let Ok(path) = std::env::var("CAPY_KERNEL_WGSL") { std::fs::write(path, source.as_bytes()).unwrap(); }
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap();
    let pipeline = naga::back::spv::PipelineOptions { shader_stage: naga::ShaderStage::Compute, entry_point: entry.into() };
    let options = naga::back::spv::Options { lang_version: (1, 3), ..Default::default() };
    let (module, info) = naga::back::pipeline_constants::process_overrides(&module, &info,
        Some((naga::ShaderStage::Compute, &pipeline.entry_point)), &Default::default()).unwrap();
    let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline)).unwrap();
    std::fs::write(out, words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
}
