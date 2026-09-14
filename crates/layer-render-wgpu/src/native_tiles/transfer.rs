//! Native integer transfer values and final quantization decisions. The same
//! table body covers both native depths; sRGB and P3 share their transfer curve.
use crate::GpuRasterError;
use layer_core::color::RgbSpace;

pub(crate) const TABLE_BYTES: u64 = 65_536 * 8;
#[derive(Default)]
pub(crate) struct Tables([Option<NativeTransfer>; 3]);
impl Tables {
    pub fn index(space: RgbSpace) -> usize {
        match space {
            RgbSpace::Srgb | RgbSpace::DisplayP3 => 0,
            RgbSpace::AdobeRgb => 1,
            RgbSpace::ProPhoto => 2,
        }
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.0
            .iter()
            .flatten()
            .map(NativeTransfer::storage_bytes)
            .sum()
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        space: RgbSpace,
    ) -> Result<&NativeTransfer, GpuRasterError> {
        let index = Self::index(space);
        if self.0[index].is_none() {
            self.0[index] = Some(NativeTransfer::new(device, space)?);
        }
        Ok(self.0[index].as_ref().unwrap())
    }
}

/// A depth-independent SDR transfer curve. Primaries are supplied by the
/// working-space owner; sRGB and P3 intentionally use the same curve identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTransfer {
    pub(crate) table: wgpu::Buffer,
    pub(crate) curve: u32,
}
impl NativeTransfer {
    pub fn new(device: &wgpu::Device, space: RgbSpace) -> Result<Self, GpuRasterError> {
        let mut bytes = Vec::with_capacity(TABLE_BYTES as usize);
        for code in 0..=65_535 {
            let decoded = space.decode(f64::from(code) / 65_535.) as f32;
            let boundary = space.decode((f64::from(code) + 0.5) / 65_535.);
            let mut decision = boundary as f32;
            if f64::from(decision) < boundary {
                decision = decision.next_up();
            }
            bytes.extend(decoded.to_le_bytes());
            bytes.extend(decision.to_le_bytes());
        }
        let table = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shared native SDR transfer table"),
            size: TABLE_BYTES,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: true,
        });
        table
            .get_mapped_range_mut(..)
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .copy_from_slice(&bytes);
        table.unmap();

        Ok(Self {
            table,
            curve: match space {
                RgbSpace::Srgb | RgbSpace::DisplayP3 => 0,
                RgbSpace::AdobeRgb => 2,
                RgbSpace::ProPhoto => 3,
            },
        })
    }
    pub fn storage_bytes(&self) -> u64 {
        self.table.size()
    }
    pub(crate) fn as_entire_binding(&self) -> wgpu::BindingResource<'_> {
        self.table.as_entire_binding()
    }
}
