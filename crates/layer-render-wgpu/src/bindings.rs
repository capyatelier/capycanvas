//! Bindings live with the resources that bound their lifetime. Replacing a
//! dependent buffer/view replaces the one retained binding, rather than growing
//! a global cache that can keep retired tiles or stroke state alive.
use std::cell::RefCell;

pub(super) struct CachedBinding<K>(RefCell<Option<Box<(K, wgpu::BindGroup)>>>);
impl<K> Default for CachedBinding<K> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}
impl<K> CachedBinding<K> {
    pub fn clear(&self) {
        *self.0.borrow_mut() = None;
    }
}
impl<K: PartialEq> CachedBinding<K> {
    pub fn get(&self, key: K, create: impl FnOnce() -> wgpu::BindGroup) -> wgpu::BindGroup {
        let mut entry = self.0.borrow_mut();
        if let Some((previous, binding)) = entry.as_deref()
            && *previous == key
        {
            return binding.clone();
        }
        let binding = create();
        *entry = Some(Box::new((key, binding.clone())));
        binding
    }
}

pub(super) type MaterialInput = CachedBinding<([wgpu::Buffer; 2], [wgpu::TextureView; 2])>;
pub(super) type MaterialOutput =
    CachedBinding<(wgpu::Buffer, wgpu::TextureView, Option<wgpu::TextureView>, bool)>;

pub(super) fn texture(binding: u32, visibility: wgpu::ShaderStages, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    texture_of(binding, visibility, wgpu::TextureSampleType::Float { filterable }, wgpu::TextureViewDimension::D2)
}
pub(super) fn texture_of(
    binding: u32,
    visibility: wgpu::ShaderStages,
    sample_type: wgpu::TextureSampleType,
    view_dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    let ty = wgpu::BindingType::Texture { sample_type, view_dimension, multisampled: false };
    wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None }
}
pub(super) fn storage_texture(
    binding: u32,
    visibility: wgpu::ShaderStages,
    format: wgpu::TextureFormat,
    access: wgpu::StorageTextureAccess,
) -> wgpu::BindGroupLayoutEntry {
    let ty = wgpu::BindingType::StorageTexture { access, format, view_dimension: wgpu::TextureViewDimension::D2 };
    wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None }
}
pub(super) fn buffer(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BufferBindingType,
    has_dynamic_offset: bool,
    min_binding_size: Option<wgpu::BufferSize>,
) -> wgpu::BindGroupLayoutEntry {
    let ty = wgpu::BindingType::Buffer { ty, has_dynamic_offset, min_binding_size };
    wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None }
}
pub(super) fn sampler(binding: u32, visibility: wgpu::ShaderStages, ty: wgpu::SamplerBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Sampler(ty), count: None }
}
pub(super) fn layout(device: &wgpu::Device, label: &str, entries: &[wgpu::BindGroupLayoutEntry]) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
}
pub(super) fn group<'a>(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    resources: impl IntoIterator<Item = wgpu::BindingResource<'a>>,
) -> wgpu::BindGroup {
    let entries: Vec<_> = (0..).zip(resources).map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource }).collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
}
