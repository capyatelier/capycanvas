//! Bindings live with the resources that bound their lifetime. Replacing a
//! dependent buffer/view replaces a retained binding, rather than growing
//! a global cache that can keep retired tiles or stroke state alive.
use std::cell::RefCell;

pub(super) struct CachedBinding<K, const N: usize = 1>(RefCell<[Option<Box<(K, wgpu::BindGroup)>>; N]>);
impl<K, const N: usize> Default for CachedBinding<K, N> {
    fn default() -> Self {
        Self(RefCell::new(std::array::from_fn(|_| None)))
    }
}
impl<K, const N: usize> CachedBinding<K, N> {
    pub fn clear(&self) {
        *self.0.borrow_mut() = std::array::from_fn(|_| None);
    }
    pub fn forget(&self, binds: impl Fn(&K) -> bool) {
        for entry in self.0.borrow_mut().iter_mut().filter(|entry| entry.as_deref().is_some_and(|(key, _)| binds(key))) {
            *entry = None;
        }
    }
}
impl<K: PartialEq, const N: usize> CachedBinding<K, N> {
    pub fn get(&self, key: K, create: impl FnOnce(&K) -> wgpu::BindGroup) -> wgpu::BindGroup {
        let mut entries = self.0.borrow_mut();
        let found = entries.iter().position(|entry| entry.as_deref().is_some_and(|(previous, _)| *previous == key));
        let slot = found.or_else(|| entries.iter().position(Option::is_none)).unwrap_or(N - 1);
        entries[..=slot].rotate_right(1);
        if found.is_none() {
            let binding = create(&key);
            entries[0] = Some(Box::new((key, binding)));
        }
        entries[0].as_deref().unwrap().1.clone()
    }
}

/// A material job's inputs: the buffers, then the nine neighborhood slots,
/// stroke coverage and starting paint it binds.
pub(super) type MaterialKey = ([wgpu::Buffer; 2], [wgpu::TextureView; 11]);
pub(super) type MaterialOutput =
    CachedBinding<(wgpu::Buffer, wgpu::TextureView, [Option<wgpu::TextureView>; 2], Option<wgpu::Buffer>)>;

pub(super) struct MaterialSources {
    pub key: MaterialKey,
    pub display: Option<MaterialKey>,
    pub page_local: bool,
}

#[derive(Default)]
pub(super) struct MaterialBindings {
    pub input: CachedBinding<MaterialKey, 2>,
    pub display: CachedBinding<MaterialKey>,
    pub gather: CachedBinding<(wgpu::Buffer, [wgpu::TextureView; 3])>,
    pub output: MaterialOutput,
}
impl MaterialBindings {
    pub fn inputs(&self, sources: MaterialSources, create: impl Fn(&MaterialKey) -> wgpu::BindGroup) -> (wgpu::BindGroup, Option<wgpu::BindGroup>) {
        if !sources.page_local {
            return (create(&sources.key), sources.display.as_ref().map(&create));
        }
        (self.input.get(sources.key, &create), sources.display.map(|key| self.display.get(key, &create)))
    }
    pub fn clear(&self) {
        self.input.clear();
        self.display.clear();
        self.gather.clear();
        self.output.clear();
    }
    pub fn forget(&self, views: &[wgpu::TextureView], buffers: &[wgpu::Buffer]) {
        let (view, buffer) = (|view: &wgpu::TextureView| views.contains(view), |buffer: &wgpu::Buffer| buffers.contains(buffer));
        let material = |(bound, sampled): &MaterialKey| bound.iter().any(buffer) || sampled.iter().any(view);
        self.input.forget(material);
        self.display.forget(material);
        self.gather.forget(|(style, written)| buffer(style) || written.iter().any(view));
        self.output.forget(|(style, color, states, changed)| buffer(style) || view(color) || states.iter().flatten().any(view) || changed.as_ref().is_some_and(buffer));
    }
}

pub(super) fn texture(binding: u32, visibility: wgpu::ShaderStages, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    texture_of(binding, visibility, wgpu::TextureSampleType::Float { filterable }, wgpu::TextureViewDimension::D2)
}
pub(super) fn float_texture(binding: u32, visibility: wgpu::ShaderStages, device: &super::PipelineDevice) -> wgpu::BindGroupLayoutEntry {
    texture(binding, visibility, device.float32_filterable())
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
    #[cfg(test)] crate::scene::sources::gpu_object_created();
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
}
