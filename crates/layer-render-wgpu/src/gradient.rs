use super::*;

pub(crate) const SOURCE: &str = concat!(include_str!("dither.wgsl"), "\n", include_str!("gradient.wgsl"));

pub(crate) fn quantum(depth:layer_core::color::SampleDepth)->f32 {
    if depth.is_float() {0.} else {1./((1u32<<depth.bits())-1) as f32}
}
pub(crate) fn texture(r:&WgpuRasterizer,gradient:&layer_core::GradientDefinition)->Result<wgpu::TextureView,GpuRasterError> {
    let records=gradient.parameters(r.document_color.space).map_err(GpuRasterError::Effect)?;
    let bytes:Vec<u8>=records.into_iter().flatten().flat_map(f32::to_le_bytes).collect();
    let extent=wgpu::Extent3d {width:65,height:1,depth_or_array_layers:1};
    let texture=r.device.create_texture(&wgpu::TextureDescriptor {label:Some("gradient stops"),size:extent,
        mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba32Float,
        usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
    r.queue.write_texture(wgpu::TexelCopyTextureInfo {texture:&texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
        &bytes,wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(65*16),rows_per_image:Some(1)},extent);
    Ok(texture.create_view(&Default::default()))
}
