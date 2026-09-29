use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let path = PathBuf::from(arguments.next().ok_or("shader_compile FILE ENTRY...")?);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default()
    }))?;
    let (device, _) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::FLOAT32_FILTERABLE,
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    eprintln!("{:?}", adapter.get_info());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shader compile probe"), source: wgpu::ShaderSource::Wgsl(std::fs::read_to_string(path)?.into()),
    });
    for entry in arguments {
        eprintln!("BEGIN {entry}");
        let start = Instant::now();
        let _pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&entry), layout: None, module: &module, entry_point: Some(&entry),
            compilation_options: Default::default(), cache: None,
        });
        eprintln!("END {entry} {:.3} ms", start.elapsed().as_secs_f64()*1000.);
    }
    Ok(())
}
