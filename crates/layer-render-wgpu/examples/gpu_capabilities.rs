//! Headless capability report, also runnable through adb on Android hardware.
fn main() {
    pollster::block_on(async {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(desc);
        for adapter in instance.enumerate_adapters(wgpu::Backends::PRIMARY).await {
            println!(
                "ADAPTER {:?}\nFEATURES {:?}",
                adapter.get_info(),
                adapter.features()
            );
            for format in [
                wgpu::TextureFormat::Rgba32Float,
                wgpu::TextureFormat::Rg32Float,
                wgpu::TextureFormat::R32Float,
                wgpu::TextureFormat::Rgba16Float,
            ] {
                println!(
                    "{format:?}: {:?}",
                    adapter.get_texture_format_features(format)
                );
            }
        }
    });
}
