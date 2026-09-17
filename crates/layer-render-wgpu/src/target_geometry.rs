//! Editable tiles live in each layer's local bounds. The canvas remains the
//! extent of composition and presentation; it is not a limit on photo backing.
use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct TargetGeometry {
    targets: BTreeMap<LayerId, [u32; 2]>,
    bases: BTreeMap<[u32; 2], usize>,
}

impl WgpuRasterizer {
    pub(crate) fn target_extent(&self, id: LayerId) -> [u32; 2] {
        self.target_geometry
            .targets
            .get(&id)
            .copied()
            .unwrap_or(self.document_extent)
    }

    pub(super) fn update_target_geometry(
        &mut self,
        layers: &[Layer],
        resized: bool,
    ) -> Result<(), GpuRasterError> {
        let targets: BTreeMap<_, _> = layers
            .iter()
            .flat_map(|layer| {
                let extent = layer.local_extent(self.document_extent);
                std::iter::once((layer.id, extent))
                    .chain(layer.masks().map(move |mask| (mask.id, extent)))
            })
            .collect();
        if !resized && targets == self.target_geometry.targets {
            return Ok(());
        }
        let mut bases = BTreeMap::new();
        // Keep the canvas grid first for existing composition consumers.
        bases.insert(self.document_extent, 1usize);
        let count = |extent: [u32; 2]| {
            extent[0].div_ceil(PAGE_SIZE) as usize * extent[1].div_ceil(PAGE_SIZE) as usize
        };
        let mut records = 1 + count(self.document_extent);
        for extent in targets.values() {
            if !bases.contains_key(extent) {
                bases.insert(*extent, records);
                records = records
                    .checked_add(count(*extent))
                    .ok_or(GpuRasterError::SizeOverflow)?;
            }
        }
        let capacity = records.next_power_of_two();
        let bytes = (capacity as u64)
            .checked_mul(self.target_stride)
            .ok_or(GpuRasterError::SizeOverflow)?;
        if bytes > self.device.limits().max_buffer_size || bytes > u32::MAX as u64 {
            return Err(GpuRasterError::ExtentUnsupported);
        }
        if records > self.target_capacity {
            self.target_capacity = capacity;
            self.target_buffer = create_target_buffer(&self.device, self.target_stride, capacity);
            self.target_bind_group = create_target_bind_group(
                &self.device,
                &self.target_layout,
                &self.target_buffer,
                &self.unclipped,
            );
            self.selection_clip.binding = None;
        }
        self.target_upload.clear();
        self.target_upload
            .resize(records * self.target_stride as usize, 0);
        let full = TargetGpu::new([0, 0], self.document_extent, self.document_extent);
        self.target_upload[..mem::size_of::<TargetGpu>()].copy_from_slice(target_bytes(&full));
        for (&extent, &base) in &bases {
            let columns = extent[0].div_ceil(PAGE_SIZE);
            for y in 0..extent[1].div_ceil(PAGE_SIZE) {
                for x in 0..columns {
                    let target =
                        TargetGpu::new([x * PAGE_SIZE, y * PAGE_SIZE], [PAGE_SIZE; 2], extent);
                    let offset = (base + (y * columns + x) as usize) * self.target_stride as usize;
                    self.target_upload[offset..offset + mem::size_of::<TargetGpu>()]
                        .copy_from_slice(target_bytes(&target));
                }
            }
        }
        self.queue
            .write_buffer(&self.target_buffer, 0, &self.target_upload);
        self.target_geometry = TargetGeometry { targets, bases };
        Ok(())
    }

    pub(super) fn layer_target_offset(&self, id: LayerId, coordinate: [u32; 2]) -> u32 {
        let extent = self.target_extent(id);
        let base = self.target_geometry.bases[&extent];
        let index = base + (coordinate[1] * extent[0].div_ceil(PAGE_SIZE) + coordinate[0]) as usize;
        (index as u64 * self.target_stride) as u32
    }
}
