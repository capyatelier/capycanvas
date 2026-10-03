use super::*;
use std::sync::{Arc, Weak};

pub(crate) struct Resource { pub buffer: wgpu::Buffer, pub _analysis: Option<Arc<crate::effect_analysis::Lease>> }
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<[u8;32], Weak<Resource>>,
    empty: Option<Arc<Resource>>,
    pub uploads: u64,
}
impl Cache {
    pub fn bytes(&self) -> u64 { self.empty.as_ref().map_or(0, |r| r.buffer.size()) + self.entries.values().filter_map(Weak::upgrade).map(|r| r.buffer.size()).sum::<u64>() }
    pub fn get(&mut self, device: &PipelineDevice, queue: &wgpu::Queue, resource: Option<&layer_core::Lut3d>) -> Result<Arc<Resource>, GpuRasterError> {
        self.entries.retain(|_, value| value.strong_count() != 0);
        if let Some(resource) = resource {
            if let Some(value) = self.entries.get(&resource.digest()).and_then(Weak::upgrade) { return Ok(value); }
            let bytes = resource.payload().ok_or_else(|| GpuRasterError::Effect("Unresolved color lookup".into()))?;
            let size = bytes.len() as u64;
            #[cfg(target_arch = "wasm32")]
            let budget = 64 * 1024 * 1024;
            #[cfg(not(target_arch = "wasm32"))]
            let budget = crate::display_memory::resource_budget(device, self.bytes());
            let retained = self.bytes();
            if size > device.limits().max_storage_buffer_binding_size as u64 || size > device.limits().max_buffer_size
                || retained.saturating_add(size) > budget {
                return Err(GpuRasterError::Effect("Color lookup exceeds available GPU memory".into()));
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("immutable color lookup"), size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            queue.write_buffer(&buffer, 0, bytes);
            let value = Arc::new(Resource {buffer, _analysis: None});
            self.entries.insert(resource.digest(), Arc::downgrade(&value)); self.uploads += 1;
            Ok(value)
        } else {
            Ok(self.empty.get_or_insert_with(|| Arc::new(Resource {_analysis: None, buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("empty effect resource"), size: 16, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false,
            })})).clone())
        }
    }
}
