//! Full-resolution destination-brush samples beyond the adjacent tile window.
//! Gather disjoint source pages into two reusable 256px Float32 fields. Each
//! pass binds at most nine original/paint tiles; no full-photo texture or CPU
//! readback is needed, and source-cache eviction remains queue ordered.
use super::*;
use layer_core::{Point, Rect};
use wgpu::util::DeviceExt;

pub(super) struct Gather {
    fields: [(wgpu::Texture, wgpu::TextureView); 2],
    pages: wgpu::Buffer,
    complete: wgpu::Buffer,
}
impl Gather {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            fields: std::array::from_fn(|_| {
                create_target(
                    device,
                    [PAGE_SIZE; 2],
                    wgpu::TextureFormat::Rgba32Float,
                    "full-resolution material sample field",
                )
            }),
            pages: descriptor(device, 1, &[]),
            complete: descriptor(device, 2, &[]),
        }
    }
    pub(super) fn storage_bytes(&self) -> u64 {
        self.fields
            .iter()
            .map(|(texture, _)| texture_bytes(texture))
            .sum::<u64>()
            + 320
    }
}

// WGSL uniform: header (mode, count), followed by nine vec4 tile coordinates.
fn descriptor_bytes(mode: u32, pages: &[[u32; 2]]) -> [u8; 160] {
    let mut words = [0u32; 40];
    words[0] = mode;
    words[1] = pages.len() as u32;
    for (i, page) in pages.iter().enumerate() {
        words[4 + i * 4..6 + i * 4].copy_from_slice(page);
    }
    let mut bytes = [0u8; 160];
    for (destination, word) in bytes.chunks_exact_mut(4).zip(words) {
        destination.copy_from_slice(&word.to_le_bytes());
    }
    bytes
}
fn descriptor(device: &wgpu::Device, mode: u32, pages: &[[u32; 2]]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("material source page coordinates"),
        contents: &descriptor_bytes(mode, pages),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

/// Conservative bounds of every backtrace and blur tap from this output page.
/// Coverage lies in [0,1], so each step includes both zero and full displacement.
/// The shader still computes the exact nonlinear coordinate for every pixel.
fn sample_bounds(batch: &DabBatch, dabs: &[Dab], at: [u32; 2], extent: [u32; 2]) -> PixelRect {
    let to_local = batch.style.brush_to_layer;
    let Some(to_brush) = to_local.inverse() else {
        return PixelRect::full(extent);
    };
    let mut bounds = to_brush.bounds(Rect {
        min: Point {
            x: (at[0] * PAGE_SIZE) as f32,
            y: (at[1] * PAGE_SIZE) as f32,
        },
        max: Point {
            x: ((at[0] + 1) * PAGE_SIZE) as f32,
            y: ((at[1] + 1) * PAGE_SIZE) as f32,
        },
    });
    for dab in dabs.iter().rev() {
        let shift = if batch.style.execution == BrushExecution::Smudge {
            let pull = dab.material[1].clamp(0., 1.);
            [-dab.motion[0] * pull, -dab.motion[1] * pull]
        } else {
            let amount = dab.material[3].clamp(0., 1.);
            let motion = dab.motion.map(|v| v * (1. + batch.style.deform.momentum));
            match batch.style.deform.mode {
                LiquifyMode::Push => [-motion[0] * amount, -motion[1] * amount],
                LiquifyMode::TwirlClockwise | LiquifyMode::TwirlCounterClockwise => {
                    let direction = if batch.style.deform.mode == LiquifyMode::TwirlClockwise {
                        1.
                    } else {
                        -1.
                    };
                    bounds = swept_rotation(bounds, dab.center, direction * amount * 0.75);
                    [0.; 2]
                }
                LiquifyMode::Pinch | LiquifyMode::Expand => {
                    let direction = if batch.style.deform.mode == LiquifyMode::Pinch {
                        -1.
                    } else {
                        1.
                    };
                    let scale = 1. + direction * amount * 0.35;
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
                LiquifyMode::Edge | LiquifyMode::Reconstruct => {
                    bounds = expand(bounds, motion[0].hypot(motion[1]) * amount);
                    [0.; 2]
                }
            }
        };
        bounds.min.x += shift[0].min(0.);
        bounds.max.x += shift[0].max(0.);
        bounds.min.y += shift[1].min(0.);
        bounds.max.y += shift[1].max(0.);
    }
    if batch.style.execution == BrushExecution::Smudge {
        bounds = expand(bounds, batch.style.wet_mix.blur.clamp(0., 1.) * 4.);
    }
    let local = expand(to_local.bounds(bounds), 1.); // Manual bilinear neighbors.
    if [local.min.x, local.min.y, local.max.x, local.max.y]
        .into_iter()
        .any(|v| !v.is_finite())
    {
        PixelRect::full(extent)
    } else {
        pixel_rect(local, extent)
    }
}
fn expand(r: Rect, amount: f32) -> Rect {
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

#[cfg(test)]
mod bounds_tests {
    use super::*;

    #[test]
    fn partial_rotation_bounds_cover_every_intermediate_contact_strength() {
        for center in [
            Point::default(),
            Point { x: 100., y: -37. },
            Point { x: -14., y: 53. },
        ] {
            for (min, max) in [
                ([-8., -8.], [8., 8.]),
                ([31., -52.], [48., -29.]),
                ([-53., 19.], [-21., 72.]),
            ] {
                let bounds = Rect {
                    min: Point {
                        x: min[0],
                        y: min[1],
                    },
                    max: Point {
                        x: max[0],
                        y: max[1],
                    },
                };
                for angle in [-0.75, -0.54, 0., 0.35, 0.75] {
                    let swept = swept_rotation(bounds, center, angle);
                    for ix in 0..9 {
                        for iy in 0..9 {
                            for step in 0..65 {
                                let x = f64::from(
                                    min[0] + (max[0] - min[0]) * ix as f32 / 8. - center.x,
                                );
                                let y = f64::from(
                                    min[1] + (max[1] - min[1]) * iy as f32 / 8. - center.y,
                                );
                                let a = f64::from(angle) * step as f64 / 64.;
                                let px = f64::from(center.x) + x * a.cos() - y * a.sin();
                                let py = f64::from(center.y) + x * a.sin() + y * a.cos();
                                assert!(
                                    px >= f64::from(swept.min.x) - 0.0001
                                        && px <= f64::from(swept.max.x) + 0.0001
                                );
                                assert!(
                                    py >= f64::from(swept.min.y) - 0.0001
                                        && py <= f64::from(swept.max.y) + 0.0001
                                );
                            }
                        }
                    }
                }
            }
        }
        let bounded = swept_rotation(
            Rect {
                min: Point { x: 100., y: 0. },
                max: Point { x: 108., y: 8. },
            },
            Point::default(),
            0.54,
        );
        assert!(bounded.max.x - bounded.min.x < 30.);
        assert!(bounded.max.y - bounded.min.y < 65.);
    }
}

impl WgpuRasterizer {
    pub(super) fn material_source_binding(
        &mut self,
        batch_index: usize,
        batch: &DabBatch,
        dabs: &[Dab],
        coordinate: [u32; 2],
        dab_range: std::ops::Range<u32>,
        preview: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<wgpu::BindGroup, GpuRasterError> {
        let timing = self.telemetry.enabled;
        let started = timing.then(web_time::Instant::now);
        let plan = BrushPassPlan::for_style(&batch.style);
        let operation = match batch.style.execution {
            BrushExecution::Liquify => Some(0),
            BrushExecution::Smudge => Some(1),
            _ => None,
        };
        let pages = operation.map(|_| {
            page_coordinates(sample_bounds(
                batch,
                dabs,
                coordinate,
                self.target_extent(batch.layer_id),
            ))
            .collect::<Vec<_>>()
        });
        let distant = pages.as_ref().is_some_and(|pages| {
            pages
                .iter()
                .any(|p| p[0].abs_diff(coordinate[0]) > 1 || p[1].abs_diff(coordinate[1]) > 1)
        });
        self.metrics.material_cpu_ms[0] += elapsed(started);
        if !distant {
            let started = timing.then(web_time::Instant::now);
            let result = self.material_bind_group(
                batch.layer_id,
                coordinate,
                batch.stroke_id,
                plan.state.watercolor_wetness,
                preview,
                None,
                encoder,
            );
            // Keep the original contact order, but do not evaluate a whole
            // fast stroke at every pixel of every touched tile. Only dry
            // contacts are local; smudge/liquify/wet updates retain their full
            // dependency sequence. Use the same swept bounds as allocation.
            if batch.style.contact.is_some() && batch.style.execution == BrushExecution::Dry {
                let header = [0u32, 0, dab_range.start, dab_range.len() as u32];
                let bytes: Vec<_> = header.into_iter().flat_map(u32::to_le_bytes).collect();
                self.uploads.write(encoder, &self.queue, &self.material_source_meta, &bytes)?;
            }
            self.metrics.material_cpu_ms[4] += elapsed(started);
            return result;
        }
        if self.material_gather.is_none() {
            self.material_gather = Some(Gather::new(&self.device));
        }
        self.metrics.material_sample_jobs += 1;
        self.metrics.material_sample_storage_bytes =
            self.material_gather.as_ref().unwrap().storage_bytes();
        let mut current = 0;
        {
            let gather = self.material_gather.as_ref().unwrap();
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear material sample field"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gather.fields[current].1,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let key = Self::texture_set_key(&batch.style);
        for pages in pages.unwrap().chunks(9) {
            let started = timing.then(web_time::Instant::now);
            self.metrics.material_sample_passes += 1;
            let offsets = std::array::from_fn::<_, 9, _>(|i| {
                pages.get(i).map_or([-1_000_000; 2], |p| {
                    [
                        p[0] as i32 - coordinate[0] as i32,
                        p[1] as i32 - coordinate[1] as i32,
                    ]
                })
            });
            self.prepare_raw_neighborhood(batch.layer_id, coordinate, offsets, preview, encoder)?;
            self.metrics.material_cpu_ms[1] += elapsed(started);
            let started = timing.then(web_time::Instant::now);
            // Reuse one uniform in command order. Queue::write_buffer would
            // replace every pass's coordinates before any of them executes.
            self.uploads.write(
                encoder,
                &self.queue,
                &self.material_gather.as_ref().unwrap().pages,
                &descriptor_bytes(1, pages),
            )?;
            let layer = self
                .paint_layers
                .iter()
                .find(|l| l.id == batch.layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
            let views = self.raw_layer_neighborhood(layer, coordinate, offsets, preview);
            let gather = self.material_gather.as_ref().unwrap();
            let binding = create_material_bind_group(
                &self.device,
                &self.material_layout,
                &views,
                &self.dab_buffer,
                &self.empty_scalar_view,
                &gather.fields[current].1,
                &gather.pages,
            );
            self.metrics.material_cpu_ms[2] += elapsed(started);
            let started = timing.then(web_time::Instant::now);
            let next = 1 - current;
            let textures = self
                .texture_sets
                .iter()
                .find(|s| s.key == key)
                .expect("material textures are prepared before gathering");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gather full-resolution material source pages"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gather.fields[next].1,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.material_gather[operation.unwrap()]);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.layer_id, coordinate)],
            );
            pass.set_bind_group(2, &binding, &[]);
            pass.set_bind_group(3, &textures.bind_group, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            self.metrics.material_cpu_ms[3] += elapsed(started);
            current = next;
        }
        let gather = self.material_gather.as_ref().unwrap();
        let (samples, meta) = (gather.fields[current].1.clone(), gather.complete.clone());
        let started = timing.then(web_time::Instant::now);
        let result = self.material_bind_group(
            batch.layer_id,
            coordinate,
            batch.stroke_id,
            plan.state.watercolor_wetness,
            preview,
            Some((&samples, &meta)),
            encoder,
        );
        self.metrics.material_cpu_ms[4] += elapsed(started);
        result
    }
}

fn elapsed(started: Option<web_time::Instant>) -> f64 {
    started.map_or(0., |at| at.elapsed().as_secs_f64() * 1000.)
}
