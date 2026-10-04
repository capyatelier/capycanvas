use super::*;
use std::sync::{Arc, Weak};

pub(crate) struct Resource { pub buffer: wgpu::Buffer, pub _analysis: Option<Arc<crate::effect_analysis::Lease>> }
type LutKey = ([u8;32], [[u32;3];2]);
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<LutKey, Weak<Resource>>,
    empty: Option<Arc<Resource>>,
    pub uploads: u64,
}
impl Cache {
    pub fn bytes(&self) -> u64 { self.empty.as_ref().map_or(0, |r| r.buffer.size()) + self.entries.values().filter_map(Weak::upgrade).map(|r| r.buffer.size()).sum::<u64>() }
    pub fn get(&mut self, device: &PipelineDevice, queue: &wgpu::Queue, resource: Option<&layer_core::Lut3d>) -> Result<Arc<Resource>, GpuRasterError> {
        self.entries.retain(|_, value| value.strong_count() != 0);
        if let Some(resource) = resource {
            let key = (resource.digest(), resource.domain().map(|point| point.map(f32::to_bits)));
            if let Some(value) = self.entries.get(&key).and_then(Weak::upgrade) { return Ok(value); }
            let bytes = resource.payload().ok_or_else(|| GpuRasterError::Effect("Unresolved color lookup".into()))?;
            let size = (96 + bytes.len() as u64).next_multiple_of(16);
            #[cfg(target_arch = "wasm32")]
            let budget = 64 * 1024 * 1024;
            #[cfg(not(target_arch = "wasm32"))]
            let budget = crate::display_memory::resource_budget(device, self.bytes());
            let retained = self.bytes();
            if size > device.limits().max_storage_buffer_binding_size || size > device.limits().max_buffer_size
                || retained.saturating_add(size) > budget {
                return Err(GpuRasterError::Effect("Color lookup exceeds available GPU memory".into()));
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("immutable color lookup"), size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let headers=headers(resource);
            let header:[u8;96]=std::array::from_fn(|i|headers[i/16][i/4%4].to_le_bytes()[i%4]);
            queue.write_buffer(&buffer, 0, &header);
            queue.write_buffer(&buffer, 96, bytes);
            let value = Arc::new(Resource {buffer, _analysis: None});
            self.entries.insert(key, Arc::downgrade(&value)); self.uploads += 1;
            Ok(value)
        } else {
            Ok(self.empty.get_or_insert_with(|| Arc::new(Resource {_analysis: None, buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("empty effect resource"), size: 16, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false,
            })})).clone())
        }
    }
}

fn headers(resource: &layer_core::Lut3d) -> [[f32;4];6] {
    let domain = resource.domain();
    let mut records = [[0.;4];6];
    records[0][0] = resource.size() as f32;
    records[1][..3].copy_from_slice(&domain[0]); records[2][..3].copy_from_slice(&domain[1]);
    for axis in 0..3 {
        let lo = f64::from(domain[0][axis]); let hi = f64::from(domain[1][axis]);
        let exponent = (-(lo.abs().max(hi.abs()).log2().floor() as i32)).clamp(-126,126);
        let scale = 2f64.powi(exponent);
        records[3][axis] = exponent as f32;
        records[4][axis] = (lo*scale) as f32;
        records[5][axis] = (1./(hi*scale-lo*scale)) as f32;
    }
    records
}
