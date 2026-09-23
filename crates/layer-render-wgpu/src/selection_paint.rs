//! Sparse float footprints and one asynchronous capture per selection gesture.
//! Brush sampling is shared with visibility masks; no host raster or per-dab readback.
use super::*;
use layer_core::Selection;
use layer_render::{SelectionPaint, SelectionPaintMode, SelectionPaintResult};
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

pub(super) struct SelectionPainter {
    layout: wgpu::BindGroupLayout,
    pipelines: [Deferred<wgpu::ComputePipeline>; 3],
    pub active: Option<Painting>,
    pending: Option<wgpu::Buffer>,
    tx: mpsc::Sender<Result<SelectionPaintResult, GpuRasterError>>,
    rx: mpsc::Receiver<Result<SelectionPaintResult, GpuRasterError>>,
}
pub(super) struct Painting {
    id: u64,
    extent: [u32; 2],
    before: wgpu::Buffer,
    pub output: wgpu::Buffer,
    pages: BTreeMap<[u32; 2], wgpu::Buffer>,
}
impl SelectionPainter {
    pub(super) fn storage_bytes(&self) -> u64 {
        self.pending.as_ref().map_or(0, |b| b.size())
            + self.active.as_ref().map_or(0, |p| {
                p.before.size() + p.output.size() + p.pages.values().map(|b| b.size()).sum::<u64>()
            })
    }
    pub fn new(device: &PipelineDevice, textures: &wgpu::BindGroupLayout) -> Self {
        let entries: Vec<_> = [0, 1, 2, 3, 6, 7, 8]
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding <= 1 || binding == 8 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: !matches!(binding, 2 | 3),
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .into();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection paint"),
            entries: &entries,
        });
        let shader_source = |name: &str, binding: u32| {
            include_str!("selection_clip.wgsl")
                .replace(
                    "@group(1) @binding(1)",
                    &format!("@group(0) @binding({binding})"),
                )
                .replace("BrushSelection", &format!("{name}Coverage"))
                .replace("brush_selection", name)
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("incremental selection coverage"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                include_str!("brush_types.wgsl"),
                &include_str!("brush_textures.wgsl").replace("@group(3)", "@group(1)"),
                include_str!("analytic_coverage.wgsl"),
                include_str!("brush_coverage.wgsl"),
                include_str!("contact.wgsl"),
                include_str!("brush_footprint.wgsl"),
                &shader_source("before", 6),
                &shader_source("enclosed", 7),
                include_str!("selection_paint.wgsl"),
            ])),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection paint"),
            bind_group_layouts: &[Some(&layout), Some(textures)],
            immediate_size: 0,
        });
        let pipelines = ["initialize", "paint", "bounds"].map(|entry| {
            let (device, layout, shader) =
                (device.clone(), pipeline_layout.clone(), shader.clone());
            Deferred::pipeline(move |mode| {
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&layout),
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
            })
        });
        let (tx, rx) = mpsc::channel();
        Self {
            layout,
            pipelines,
            active: None,
            pending: None,
            tx,
            rx,
        }
    }

    fn binding(
        &self,
        r: &WgpuRasterizer,
        params: &[u32; 20],
        contacts: &wgpu::Buffer,
        page: &wgpu::Buffer,
        active: &Painting,
        enclosed: &wgpu::Buffer,
        style: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let params = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("selection paint parameters"),
                contents: &params
                    .iter()
                    .flat_map(|v| v.to_ne_bytes())
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let entries: Vec<_> = [
            (0, &params),
            (1, contacts),
            (2, page),
            (3, &active.output),
            (6, &active.before),
            (7, enclosed),
            (8, style),
        ]
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        })
        .into();
        r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("selection paint"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    fn update(
        &mut self,
        r: &mut WgpuRasterizer,
        update: &SelectionPaint,
    ) -> Result<bool, GpuRasterError> {
        if self.pending.is_some() {
            return Ok(false);
        }
        if !update.is_valid() {
            return Err(GpuRasterError::InvalidImage);
        }
        // A uniform contact block keeps the portable four-storage-buffer limit.
        // Partitioning retains the same float footprint and blend order.
        if update.dabs.len() > 64 {
            for (i, dabs) in update.dabs.chunks(64).enumerate() {
                let last = (i + 1) * 64 >= update.dabs.len();
                let part = SelectionPaint {
                    id: update.id,
                    restart: update.restart && i == 0,
                    before: update.before.clone(),
                    mode: update.mode,
                    opacity: update.opacity,
                    gray: update.gray,
                    style: update.style.clone(),
                    gradient: update.gradient,
                    dabs: dabs.to_vec(),
                    finish: update.finish && last,
                    enclosed: if last { update.enclosed.clone() } else { None },
                };
                if !self.update(r, &part)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        if let Some(startup) = &r.startup {
            startup.compiler.check()?;
            let mut ready = true;
            for pipeline in self.pipelines.iter().chain([
                &r.selection_clip.crossings,
                &r.selection_clip.fill,
                &r.selection_clip.resample,
            ]) {
                startup.compiler.pipeline(pipeline, startup::BRUSH);
                ready &= pipeline.ready();
            }
            if !ready {
                return Ok(false);
            }
        }
        let extent = r.document_extent;
        let size = 32 + u64::from(extent[0].div_ceil(4)) * u64::from(extent[1]) * 4;
        if extent.contains(&0)
            || size + 32 > r.device.limits().max_storage_buffer_binding_size
            || extent[1] > r.device.limits().max_compute_workgroups_per_dimension
            || extent[0].div_ceil(256) > r.device.limits().max_compute_workgroups_per_dimension
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("selection paint"),
            },
        );
        let fresh = update.restart
            || self
                .active
                .as_ref()
                .is_none_or(|a| a.id != update.id || a.extent != extent);
        if fresh {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, &update.before)?;
            let input = r.selection_clip.buffer.as_ref().unwrap();
            let before = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("selection paint immutable input"),
                size: input.size(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(input, 0, &before, 0, input.size());
            let output = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("selection paint preview and capture"),
                size: size + 32,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let header: Vec<_> = [
                0, 0, extent[0], extent[1], 0, 2, 0, 0, extent[0], extent[1], 0, 0, 0, 0, 0, 0,
            ]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
            let header = r
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("selection coverage header"),
                    contents: &header,
                    usage: wgpu::BufferUsages::COPY_SRC,
                });
            encoder.copy_buffer_to_buffer(&header, 0, &output, 0, 32);
            encoder.copy_buffer_to_buffer(&header, 32, &output, size, 32);
            self.active = Some(Painting {
                id: update.id,
                extent,
                before,
                output,
                pages: BTreeMap::new(),
            });
        }
        let enclosed = if let Some(area) = &update.enclosed {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, area)?;
            r.selection_clip.buffer.as_ref().unwrap().clone()
        } else {
            self.active.as_ref().unwrap().before.clone()
        };
        let key = WgpuRasterizer::texture_set_key(&update.style);
        r.ensure_texture_set(key.clone())?;
        let textures = r
            .texture_sets
            .iter()
            .find(|t| t.key == key)
            .unwrap()
            .bind_group
            .clone();
        let style = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("selection brush style"),
                contents: style_bytes(&StyleGpu::for_brush(
                    extent,
                    &update.style,
                    0,
                    update.dabs.len() as u32,
                )),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let data: Vec<DabGpu> = update.dabs.iter().copied().map(DabGpu::from).collect();
        // Uniform storage is portable down to 16 KiB. Pad with an existing
        // valid contact; the count excludes unused records.
        let bytes = dab_bytes(&data).to_vec();
        let mut data = bytes;
        data.resize(64 * mem::size_of::<DabGpu>(), 0);
        let contacts = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("selection contacts"),
                contents: &data,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let mut params = [
            extent[0],
            extent[1],
            0,
            0,
            update.dabs.len() as u32,
            match update.mode {
                SelectionPaintMode::Add => 0,
                SelectionPaintMode::Subtract => 1,
                SelectionPaintMode::Gray => 2,
            },
            0,
            u32::from(update.style.rendering.accumulation == BrushAccumulation::Uniform),
            update.opacity.to_bits(),
            update.gray.to_bits(),
            u32::from(update.enclosed.is_some()),
            0,
            update.gradient.map_or(0, |g| g.start.x.to_bits()),
            update.gradient.map_or(0, |g| g.start.y.to_bits()),
            update.gradient.map_or(0, |g| g.end.x.to_bits()),
            update.gradient.map_or(0, |g| g.end.y.to_bits()),
            update
                .gradient
                .map_or(0., |g| if g.radial { 2_f32 } else { 1. })
                .to_bits(),
            update.gradient.map_or(0, |g| g.background.to_bits()),
            update
                .gradient
                .map_or(0., |g| f32::from(g.transparent))
                .to_bits(),
            0,
        ];
        let dummy = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("selection initialization scratch"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let binding = self.binding(
            r,
            &params,
            &contacts,
            &dummy,
            self.active.as_ref().unwrap(),
            &enclosed,
            &style,
        );
        if fresh {
            dispatch(
                &mut encoder,
                &self.pipelines[0],
                &binding,
                &textures,
                [extent[0].div_ceil(4).div_ceil(64), extent[1]],
            );
        }
        let damage = paint_damage(update, extent);
        for coordinate in page_coordinates(damage) {
            let active = self.active.as_mut().unwrap();
            let fresh_page = !active.pages.contains_key(&coordinate);
            let page = active
                .pages
                .entry(coordinate)
                .or_insert_with(|| {
                    r.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("sparse selection float footprint"),
                        size: 256 * 256 * 4,
                        usage: wgpu::BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    })
                })
                .clone();
            params[2] = coordinate[0] * 256;
            params[3] = coordinate[1] * 256;
            params[6] = u32::from(fresh_page);
            let binding = self.binding(
                r,
                &params,
                &contacts,
                &page,
                self.active.as_ref().unwrap(),
                &enclosed,
                &style,
            );
            dispatch(
                &mut encoder,
                &self.pipelines[1],
                &binding,
                &textures,
                [1, 256],
            );
        }
        if update.finish {
            dispatch(
                &mut encoder,
                &self.pipelines[2],
                &binding,
                &textures,
                [extent[0].div_ceil(4).div_ceil(64), extent[1]],
            );
            let readback = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("painted selection history"),
                size: size + 32,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(
                &self.active.as_ref().unwrap().output,
                0,
                &readback,
                0,
                size + 32,
            );
            self.pending = Some(readback);
        }
        r.uploads.finish(&encoder);
        r.last_submission = Some(encoder.submit(&r.queue));
        if let Some(readback) = &self.pending {
            selection_readback::capture_selection(
                readback,
                size + 32,
                size,
                extent,
                true,
                update.id,
                self.tx.clone(),
                |result, changed| SelectionPaintResult {
                    request_id: result.request_id,
                    pixels: result.pixels,
                    changed,
                },
            );
        }
        Ok(true)
    }
}

fn paint_damage(update: &SelectionPaint, extent: [u32; 2]) -> PixelRect {
    let mut damage = update.dabs.iter().fold(PixelRect::EMPTY, |bounds, d| {
        bounds.union(pixel_rect(d.bounds(), extent))
    });
    if let Some(area) = &update.enclosed {
        damage = damage.union(if area.inverted {
            PixelRect::full(extent)
        } else {
            pixel_rect(area.bounds(), extent)
        });
    }
    damage
}

fn dispatch(
    encoder: &mut crate::submission::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    binding: &wgpu::BindGroup,
    textures: &wgpu::BindGroup,
    groups: [u32; 2],
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("selection coverage"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.set_bind_group(1, textures, &[]);
    pass.dispatch_workgroups(groups[0], groups[1], 1);
}

impl WgpuRasterizer {
    pub fn selection_paint_pending(&self) -> bool {
        self.selection_painter
            .as_ref()
            .is_some_and(|p| p.pending.is_some())
    }
    pub(super) fn update_selection_paint(
        &mut self,
        update: &SelectionPaint,
    ) -> Result<bool, GpuRasterError> {
        let mut painter = self
            .selection_painter
            .take()
            .unwrap_or_else(|| SelectionPainter::new(&self.device, &self.advanced_texture_layout));
        let result = painter.update(self, update);
        if matches!(&result, Ok(true)) {
            self.selection_paint_revision = self.selection_paint_revision.wrapping_add(1);
            let extent = self.document_extent;
            self.selection_paint_damage = if update.restart {
                PixelRect::full(extent)
            } else {
                paint_damage(update, extent)
            };
            self.display_selection = Some((
                Selection::empty(),
                painter.active.as_ref().unwrap().output.clone(),
            ));
        }
        self.selection_painter = Some(painter);
        result
    }
    pub(super) fn poll_selection_paint(
        &mut self,
    ) -> Option<Result<SelectionPaintResult, GpuRasterError>> {
        let painter = self.selection_painter.as_mut()?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        let result = painter.rx.try_recv().ok()?;
        painter.pending = None;
        if let Ok(reply) = &result {
            self.selection_clip.remember_pixels(
                &reply.pixels,
                painter.active.as_ref().unwrap().output.clone(),
            );
        }
        painter.active = None;
        Some(result)
    }
}
