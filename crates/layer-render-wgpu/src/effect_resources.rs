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
    pub fn get(&mut self, device: &PipelineDevice, resource: Option<&layer_core::Lut3d>) -> Result<Arc<Resource>, GpuRasterError> {
        self.entries.retain(|_, value| value.strong_count() != 0);
        if let Some(resource) = resource {
            let key = (resource.digest(), resource.domain().map(|point| point.map(f32::to_bits)));
            if let Some(value) = self.entries.get(&key).and_then(Weak::upgrade) { return Ok(value); }
            let bytes = resource.payload().ok_or_else(|| GpuRasterError::Effect("Unresolved color lookup".into()))?;
            let size = (96 + bytes.len() as u64).next_multiple_of(16);
            if size > device.limits().max_storage_buffer_binding_size || size > device.limits().max_buffer_size {
                return Err(GpuRasterError::Effect("Color lookup exceeds available GPU memory".into()));
            }
            let lease = Arc::new(crate::effect_analysis::Lease::reserve(device, size).map_err(GpuRasterError::Effect)?);
            let headers=headers(resource);
            let header:[u8;96]=std::array::from_fn(|i|headers[i/16][i/4%4].to_le_bytes()[i%4]);
            let mut contents=vec![0;size as usize];
            contents[..96].copy_from_slice(&header);
            contents[96..96+bytes.len()].copy_from_slice(bytes);
            let buffer=crate::initialized_buffer(device,&wgpu::util::BufferInitDescriptor {
                label:Some("immutable color lookup"),contents:&contents,usage:wgpu::BufferUsages::STORAGE,
            })?;
            let value = Arc::new(Resource {buffer, _analysis: Some(lease)});
            self.entries.insert(key, Arc::downgrade(&value)); self.uploads += 1;
            Ok(value)
        } else {
            if self.empty.is_none() {
                let lease = Arc::new(crate::effect_analysis::Lease::reserve(device, 16).map_err(GpuRasterError::Effect)?);
                self.empty = Some(Arc::new(Resource {_analysis: Some(lease), buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("empty effect resource"), size: 16, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false,
                })}));
            }
            Ok(self.empty.as_ref().unwrap().clone())
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
