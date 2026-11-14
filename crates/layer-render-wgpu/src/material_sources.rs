use super::*;
use layer_core::{Point, Rect};

/// Aligned material tile uniforms retain their allocation across frames. Each
/// frame's records follow one another, so every batch keeps its own.
#[derive(Default)]
pub(super) struct TileRecords {
    buffer: Option<wgpu::Buffer>,
    stride: u64,
    used: usize,
}
impl TileRecords {
    pub fn restart(&mut self) {
        self.used = 0;
    }

    /// Writes records after the frame's earlier ones, in the encoder's
    /// prologue so no pass waits on them, and returns the index of the first.
    pub fn upload(&mut self, device: &wgpu::Device, uploads: &mut Uploads, encoder: &mut crate::submission::CommandEncoder,
        records: impl Iterator<Item = [u32; RECORD_WORDS]> + Clone) -> Result<usize, GpuRasterError> {
        self.stride = u64::from(device.limits().min_uniform_buffer_offset_alignment.max(256));
        let (first, count) = (self.used, records.clone().count());
        let used = ((first + count) as u64).checked_mul(self.stride)
            .filter(|size| *size <= device.limits().max_buffer_size)
            .ok_or(GpuRasterError::SizeOverflow)?;
        if count == 0 { return Ok(first); }
        if self.buffer.as_ref().is_none_or(|buffer| buffer.size() < used) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("reusable material tile records"),
                size: used.next_power_of_two().min(device.limits().max_buffer_size),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.used = first + count;
        let stride = self.stride as usize;
        uploads.write_mapped(encoder.prologue(), self.buffer.as_ref().unwrap(), first as u64 * self.stride, count as u64 * self.stride, |mapped| {
            mapped.slice(..).fill(0);
            for (index, words) in records.enumerate() {
                for (i, word) in words.into_iter().enumerate() {
                    mapped.slice(index * stride + i * 4..index * stride + i * 4 + 4).copy_from_slice(&word.to_le_bytes());
                }
            }
        })?;
        Ok(first)
    }

    pub fn storage_bytes(&self) -> u64 { self.buffer.as_ref().map_or(0, wgpu::Buffer::size) }

    pub fn offset(&self, index: usize) -> u32 { (index as u64 * self.stride) as u32 }

    pub fn binding(&self) -> wgpu::BufferBinding<'_> {
        wgpu::BufferBinding {
            buffer: self.buffer.as_ref().expect("dry tile records prepared before encoding"),
            offset: 0,
            size: NonZeroU64::new(160),
        }
    }
}

/// Two reusable full-resolution sample fields that gather chunks ping-pong.
pub(super) struct Gather {
    pub(super) fields: [PageSurface; 2],
}
impl Gather {
    pub(super) fn storage_bytes(&self) -> u64 {
        self.fields.iter().map(PageSurface::storage_bytes).sum()
    }
}

pub(super) const RECORD_WORDS: usize = 40;

/// A tile record: its header, then up to nine page words.
pub(super) fn record(header: [u32; 4], pages: impl IntoIterator<Item = [u32; 4]>) -> [u32; RECORD_WORDS] {
    let mut words = [0; RECORD_WORDS];
    words[..4].copy_from_slice(&header);
    for (destination, page) in words[4..].as_chunks_mut::<4>().0.iter_mut().zip(pages) {
        *destination = page;
    }
    words
}

pub(super) fn rect_words(rect: PixelRect) -> [u32; 4] {
    [rect.min_x(), rect.min_y(), rect.max_x(), rect.max_y()]
}

/// A pointwise job's dab range and dirty rectangle, then a retouching job's
/// source mapping.
pub(super) fn dry_record(local: PixelRect, dabs: std::ops::Range<u32>, mapping: [u32; 16]) -> [u32; RECORD_WORDS] {
    record([0, 0, dabs.start, dabs.len() as u32], std::iter::once(rect_words(local)).chain(mapping.as_chunks::<4>().0.iter().copied()))
}

pub(super) fn sample_rect(batch: &DabBatch, dabs: &[Dab], at: [u32; 2]) -> Rect {
    let mut bounds = Rect {
        min: Point {
            x: (at[0] * PAGE_SIZE) as f32,
            y: (at[1] * PAGE_SIZE) as f32,
        },
        max: Point {
            x: ((at[0] + 1) * PAGE_SIZE) as f32,
            y: ((at[1] + 1) * PAGE_SIZE) as f32,
        },
    };
    for dab in dabs.iter().rev() {
        let untouched = bounds;
        bounds = bounds.intersect(dab.bounds());
        if bounds.is_empty() { bounds = untouched; continue; }
        let shift = {
            let amount = dab.material[3].clamp(0., 1.);
            let motion = dab.motion;
            match batch.style.deform.mode {
                LiquifyMode::Push => [-motion[0] * amount, -motion[1] * amount],
                LiquifyMode::TwirlCounterclockwise | LiquifyMode::TwirlClockwise => {
                    let direction = if batch.style.deform.mode == LiquifyMode::TwirlCounterclockwise {
                        1.
                    } else {
                        -1.
                    };
                    bounds = swept_rotation(bounds, dab.center, direction * amount * 0.75);
                    [0.; 2]
                }
                LiquifyMode::Pinch | LiquifyMode::Expand => {
                    let direction = if batch.style.deform.mode == LiquifyMode::Pinch {
                        1.
                    } else {
                        -1.
                    };
                    let steps = dab.previous_contact[0].max(1.);
                    let scale = (1. + direction * amount * 0.35 / steps).powf(steps);
                    let scaled = Rect {
                        min: Point {
                            x: dab.center.x + (bounds.min.x - dab.center.x) * scale,
                            y: dab.center.y + (bounds.min.y - dab.center.y) * scale,
                        },
                        max: Point {
                            x: dab.center.x + (bounds.max.x - dab.center.x) * scale,
                            y: dab.center.y + (bounds.max.y - dab.center.y) * scale,
                        },
                    };
                    bounds = bounds.union(scaled);
                    [0.; 2]
                }
                LiquifyMode::Crystals => {
                    bounds = expand(bounds, 12. * amount * batch.style.deform.distortion.abs());
                    [0.; 2]
                }
                LiquifyMode::Edge => {
                    bounds = expand(bounds, dab.radii[0].max(dab.radii[1]) * ((0.35 * amount).exp() - 1.));
                    [0.; 2]
                }
                LiquifyMode::Reconstruct => [0.; 2],
            }
        };
        bounds.min.x += shift[0].min(0.);
        bounds.max.x += shift[0].max(0.);
        bounds.min.y += shift[1].min(0.);
        bounds.max.y += shift[1].max(0.);
        bounds = bounds.union(untouched);
    }
    bounds
}

pub(super) fn expand(r: Rect, amount: f32) -> Rect {
    Rect {
        min: Point {
            x: r.min.x - amount,
            y: r.min.y - amount,
        },
        max: Point {
            x: r.max.x + amount,
            y: r.max.y + amount,
        },
    }
}

/// Each pixel rotates by an angle between zero and the contact's maximum.
/// At a fixed angle, a rectangle's extrema occur at its corners. Over this
/// partial turn, each corner reaches extrema at endpoints or cardinal angles.
/// Keeping that swept rectangle avoids fetching a full circle for a small twist.
fn swept_rotation(bounds: Rect, center: Point, angle: f32) -> Rect {
    if angle == 0. {
        return bounds;
    }
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let angle = f64::from(angle);
    let (lo, hi) = (angle.min(0.), angle.max(0.));
    let mut result = bounds;
    let mut include = |x: f64, y: f64| {
        let x = (f64::from(center.x) + x) as f32;
        let y = (f64::from(center.y) + y) as f32;
        result.min.x = result.min.x.min(x);
        result.max.x = result.max.x.max(x);
        result.min.y = result.min.y.min(y);
        result.max.y = result.max.y.max(y);
    };
    let (sin, cos) = angle.sin_cos();
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            let (x, y) = (
                f64::from(x) - f64::from(center.x),
                f64::from(y) - f64::from(center.y),
            );
            include(x * cos - y * sin, x * sin + y * cos);
            let direction = y.atan2(x);
            let radius = x.hypot(y);
            for (i, (x, y)) in [(radius, 0.), (0., radius), (-radius, 0.), (0., -radius)]
                .into_iter()
                .enumerate()
            {
                let turn = (i as f64 * FRAC_PI_2 - direction + PI).rem_euclid(TAU) - PI;
                if turn >= lo && turn <= hi {
                    include(x, y);
                }
            }
        }
    }
    result
}

impl WgpuRasterizer {
    /// Upload one batch's tile records into pooled mapped storage, with no
    /// intermediate byte vector or second CPU copy on the drawing path, and
    /// return the index of the first.
    pub(super) fn prepare_records(
        &mut self,
        records: impl Iterator<Item = [u32; RECORD_WORDS]> + Clone,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<usize, GpuRasterError> {
        self.material_records.upload(&self.device, &mut self.uploads, encoder, records)
    }

    pub(super) fn material_source_binding(
        &mut self,
        batch: &DabBatch,
        batch_index:usize,
        coordinate: [u32; 2],
        target: BrushEncodingTarget,
        intent: &crate::raster::contents::Intent,
        display: Option<usize>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(wgpu::BindGroup, Option<wgpu::BindGroup>), GpuRasterError> {
        let started = self.telemetry.enabled.then(web_time::Instant::now);
        let result = self.material_bind_group(batch,batch_index, coordinate, target, intent, MaterialInputs::Neighborhood, display, encoder);
        self.metrics.material_cpu_ms[4] += elapsed(started);
        result
    }

    pub(super) fn ensure_material_gather(&mut self) {
        if self.material_gather.is_none() {
            let gather = Gather { fields: std::array::from_fn(|_| self.create_page_surface(&self.raster_tiers.paint, "full-resolution material sample field")) };
            self.metrics.material_sample_storage_bytes = gather.storage_bytes();
            self.material_gather = Some(gather);
        }
    }

    /// Bind what a retouching batch copies onto page `coordinate`: the pages
    /// `mapping` reads, around the destination.
    pub(super) fn clone_source_binding(
        &mut self,
        batch: &DabBatch,
        batch_index:usize,
        coordinate: [u32; 2],
        mapping: &crate::retouch_sources::Mapping,
        target: BrushEncodingTarget,
        intent: &crate::raster::contents::Intent,
        display: Option<usize>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(wgpu::BindGroup, Option<wgpu::BindGroup>), GpuRasterError> {
        let pages = self.retouch_pages(mapping, encoder)?;
        self.material_bind_group(batch,batch_index, coordinate, target, intent, MaterialInputs::Retouch(&pages), display, encoder)
    }
}

pub(super) fn elapsed(started: Option<web_time::Instant>) -> f64 {
    started.map_or(0., |at| at.elapsed().as_secs_f64() * 1000.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{PassCounts,dab_batch,packet,submit_ready};
    use crate::layer_tests::{dab,preset_style,placement::{paint_document,target}};

    #[test]
    fn unchanged_material_neighborhood_is_accepted_once_for_a_multibatch_frame() {
        let extent=[128;2];
        let document=paint_document(extent,"brush neighborhood reuse");
        let target=target(&document);
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let style=preset_style(layer_core::DefaultBrushPreset::Paintbrush);
        let mut contact=dab([0.8,0.2,0.1,1.]);
        contact.center=Point {x:64.,y:64.};contact.radii=[20.;2];
        if let Some(dry)=style.contact.and_then(|contact|contact.dry) {
            let relief=dry.pose(0.5).relief;
            contact.previous=[20.,20.,1.,0.];contact.motion=[4.,1.];
            contact.contact=[0.7,relief[0],relief[1],relief[2]];contact.previous_contact=contact.contact;
        }
        let contacts=[contact;4];
        let warm:Vec<_>=(0..contacts.len()).map(|index|DabBatch {first_dab:index as u32,stroke_start:index==0,stroke_end:false,
            ..dab_batch(target,style.clone(),contact.bounds())}).collect();
        submit_ready(&mut r,FramePacket {reset_layers:true,dabs:&contacts,dab_batches:&warm,..packet(document.scene(),extent)}).unwrap();
        let batches:Vec<_>=(0..contacts.len()).map(|index|DabBatch {first_dab:index as u32,stroke_start:false,stroke_end:false,
            ..dab_batch(target,style.clone(),contact.bounds())}).collect();
        assert!(r.material_offsets(&batches[0]).iter().all(Option::is_none));
        let before=PassCounts::snapshot();
        submit_ready(&mut r,FramePacket {dabs:&contacts,dab_batches:&batches,..packet(document.scene(),extent)}).unwrap();
        let counts=PassCounts::snapshot().delta(before);
        assert_eq!(counts.material_encode_accepts,0,"brush encoding must never accept inputs");
        assert_eq!(counts.material_neighborhoods,1,"a frame accepts its brush record once");
        assert!(counts.material_bind_groups<=1,"unchanged material resources must reuse their bind group: {counts:?}");
        assert!(r.readback_srgb_rgba8().unwrap().as_chunks::<4>().0.iter().any(|pixel|pixel[3]>0));
    }
    #[test]
    fn brush_material_encoding_requires_a_prepared_record() {
        let extent=[128;2];let document=paint_document(extent,"brush preparation boundary");let target=target(&document);
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let contact=dab([0.2,0.4,0.8,1.]);
        let mut batch=dab_batch(target,preset_style(layer_core::DefaultBrushPreset::Paintbrush),contact.bounds());
        batch.stroke_end=false;
        submit_ready(&mut r,FramePacket {reset_layers:true,dabs:&[contact],dab_batches:&[batch.clone()],..packet(document.scene(),extent)}).unwrap();
        batch.stroke_start=false;
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let intent=r.raster_intent(batch.target,RasterPlane::Color,[0,0]).unwrap();
        let result=r.material_source_binding(&batch,0,[0,0],BrushEncodingTarget::Persistent,&intent,None,&mut encoder);
        assert!(matches!(result,Err(GpuRasterError::Invariant(_))),"material encode must reject a missing prepared record");
    }

}
