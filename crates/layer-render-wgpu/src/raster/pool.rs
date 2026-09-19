//! Shared bounded retention for readback buffers and immutable native outputs.
use super::*;
use std::sync::Mutex;

pub(super) enum Resource {
    Buffer(wgpu::Buffer),
    Texture(wgpu::Texture),
}
impl Resource {
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Buffer(buffer) => buffer.size(),
            Self::Texture(texture) => texture_bytes(texture),
        }
    }
}
#[derive(Default)]
pub(crate) struct BufferPool {
    resources: Mutex<Vec<Resource>>,
    pub bytes: AtomicU64,
    pub working: AtomicU64,
    pub transfer: AtomicU64,
    pub(super) priority: Arc<deferred::PresentationPriority>,
}
impl BufferPool {
    fn take_matching(&self, matches: impl Fn(&Resource) -> bool) -> Option<Resource> {
        let mut resources = self.resources.lock().unwrap();
        let index = resources.iter().position(matches)?;
        let resource = resources.remove(index);
        self.bytes.fetch_sub(resource.bytes(), Ordering::Relaxed);
        Some(resource)
    }
    pub fn take(&self, device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        self.take_buffer(
            device,
            size,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        )
    }
    fn take_buffer(
        &self,
        device: &wgpu::Device,
        size: u64,
        usage: wgpu::BufferUsages,
    ) -> wgpu::Buffer {
        if let Some(Resource::Buffer(buffer)) = self.take_matching(|resource| {
            matches!(resource, Resource::Buffer(buffer) if buffer.size() == size && buffer.usage() == usage)
        }) {
            return buffer;
        }
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded raster capture buffer"),
            size,
            usage,
            mapped_at_creation: false,
        })
    }
    pub(super) fn take_native(&self, device: &wgpu::Device, descriptor: PixelDescriptor) -> Resource {
        if descriptor.channels == 1 {
            return Resource::Buffer(self.take_buffer(
                device,
                descriptor.byte_len([PAGE_SIZE; 2]).unwrap() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            ));
        }
        let format = if descriptor.bits_per_channel == 32 {
            wgpu::TextureFormat::Rgba32Uint
        } else if descriptor.bits_per_channel == 16 {
            wgpu::TextureFormat::Rgba16Uint
        } else {
            wgpu::TextureFormat::Rgba8Uint
        };
        let usage = wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let size = wgpu::Extent3d {
            width: PAGE_SIZE,
            height: PAGE_SIZE,
            depth_or_array_layers: 1,
        };
        if let Some(resource) = self.take_matching(|resource| {
            matches!(resource, Resource::Texture(texture) if texture.format() == format && texture.size() == size && texture.usage() == usage)
        }) {
            return resource;
        }
        Resource::Texture(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("immutable native raster output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        }))
    }
    pub fn put(&self, buffer: wgpu::Buffer) {
        self.put_resource(Resource::Buffer(buffer));
    }
    pub(super) fn put_resource(&self, resource: Resource) {
        let mut resources = self.resources.lock().unwrap();
        let size = resource.bytes();
        if size == STATUS_BYTES
            && resources
                .iter()
                .filter(|r| r.bytes() == STATUS_BYTES)
                .count()
                >= 16
        {
            return;
        }
        while self.bytes.load(Ordering::Relaxed) + size > 64 * 1024 * 1024 {
            if resources.is_empty() {
                break;
            }
            let old = resources.remove(0);
            self.bytes.fetch_sub(old.bytes(), Ordering::Relaxed);
        }
        if self.bytes.load(Ordering::Relaxed) + size <= 64 * 1024 * 1024 {
            self.bytes.fetch_add(size, Ordering::Relaxed);
            resources.push(resource);
        }
    }
}
