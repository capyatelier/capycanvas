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
    retained_limit: AtomicU64,
    pub(super) priority: Arc<deferred::PresentationPriority>,
}
impl BufferPool {
    pub fn set_retained_limit(&self, bytes: u64) { self.retained_limit.store(bytes.min(crate::memory::RASTER_BUFFER_BYTES), Ordering::Release); }
    pub fn trim_unused(&self) -> u64 {
        let limit = self.retained_limit.load(Ordering::Acquire);
        let mut resources = self.resources.lock().unwrap();
        let mut released = 0;
        let mut retired = Vec::new();
        for _ in 0..crate::native_tiles::MAX_BATCH_TILES {
            if self.bytes.load(Ordering::Relaxed) <= limit { break; }
            if resources.is_empty() { break; }
            let resource = resources.remove(0);
            let bytes = resource.bytes();
            self.bytes.fetch_sub(bytes, Ordering::Relaxed);
            released += bytes;
            retired.push(resource);
        }
        drop(resources);
        drop(retired);
        released
    }
    pub fn memory_bytes(&self) -> [u64; 2] {
        let metadata = self.resources.lock().unwrap().capacity() as u64 * std::mem::size_of::<Resource>() as u64;
        [metadata.saturating_add(self.working.load(Ordering::Acquire)),
            self.bytes.load(Ordering::Acquire).saturating_add(self.transfer.load(Ordering::Acquire))]
    }
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
        if descriptor.channels <= 2 {
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
        let limit = self.retained_limit.load(Ordering::Acquire);
        while self.bytes.load(Ordering::Relaxed).saturating_add(size) > limit {
            if resources.is_empty() { break; }
            let oldest = resources.remove(0);
            self.bytes.fetch_sub(oldest.bytes(), Ordering::Relaxed);
        }
        if self.bytes.load(Ordering::Relaxed).saturating_add(size) <= limit {
            self.bytes.fetch_add(size, Ordering::Relaxed);
            resources.push(resource);
        }
    }
}
