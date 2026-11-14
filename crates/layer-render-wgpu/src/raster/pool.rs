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
type Retained = (Resource,crate::residency::Charge,Arc<crate::residency::Lease>);
pub(crate) struct BufferPool {
    resources: Mutex<Vec<Retained>>,
    pub working: Arc<crate::residency::Tier>,
    pub transfer: Arc<crate::residency::Tier>,
    pub retained: Arc<crate::residency::Tier>,
    evicted: Arc<std::sync::atomic::AtomicBool>,
    pub(super) priority: Arc<deferred::PresentationPriority>,
}
impl BufferPool {
    pub(crate) fn within(raster:&Arc<crate::residency::Tier>)->Self {
        Self {resources:Default::default(),working:Default::default(),transfer:crate::residency::Tier::within(raster),
            retained:crate::residency::Tier::within(raster),evicted:Default::default(),priority:Default::default()}
    }
    pub(crate) fn charge(&self,bytes:u64)->crate::residency::Charge {self.transfer.charge(bytes)}
    pub fn release_evicted(&self) {
        if !self.evicted.swap(false,Ordering::AcqRel) {return;}
        let retired=self.resources.lock().unwrap().extract_if(..,|(_,_,lease)|lease.evicted()).collect::<Vec<_>>();
        drop(retired);
    }
    pub fn memory_bytes(&self) -> [u64; 2] {
        let resources=self.resources.lock().unwrap();
        let metadata=resources.capacity() as u64*std::mem::size_of::<Retained>() as u64;
        let gpu=self.retained.used().saturating_add(self.transfer.used());
        [metadata.saturating_add(self.working.used()),gpu]
    }
    fn take_matching(&self, matches: impl Fn(&Resource) -> bool) -> Option<Resource> {
        let mut resources = self.resources.lock().unwrap();
        let index = resources.iter().position(|(resource, _, _)| matches(resource))?;
        Some(resources.remove(index).0)
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
    #[cfg(test)]
    pub fn put(&self, buffer: wgpu::Buffer) {
        self.put_resource(Resource::Buffer(buffer));
    }
    pub(super) fn put_charged(&self,resource:Resource,charge:crate::residency::Charge) {drop(charge);self.retain(resource);}
    #[cfg(test)]
    pub(super) fn put_resource(&self,resource:Resource) {self.retain(resource);}
    fn retain(&self,resource:Resource) {
        let mut resources = self.resources.lock().unwrap();
        let size = resource.bytes();
        while self.retained.used().saturating_add(size) > crate::memory::RASTER_BUFFER_BYTES {
            if resources.is_empty() { break; }
            drop(resources.remove(0));
        }
        if self.retained.used().saturating_add(size) <= crate::memory::RASTER_BUFFER_BYTES {
            let lease=self.retained.heap().lease(&self.evicted);lease.offer(size,0);
            resources.push((resource, self.retained.charge(size), lease));
        }
    }
}
