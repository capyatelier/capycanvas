//! Sparse R8 visibility masks. Geometry and brush instances stay GPU-resident.
use super::*;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

pub(super) struct MaskPage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}
impl MaskPage {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sparse layer mask R8 page"),
            size: wgpu::Extent3d {
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view }
    }
}
pub(super) struct MaskRenderer {
    pub definitions: BTreeMap<LayerId, layer_core::LayerMask>,
    pub pages: BTreeMap<(LayerId, [u32; 2]), MaskPage>,
    pub(super) brush: [Deferred<wgpu::RenderPipeline>; 4],
    pub(super) initialize: Deferred<wgpu::RenderPipeline>,
    init_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
}
impl MaskRenderer {
    pub fn new(
        device: &PipelineDevice,
        style: &wgpu::BindGroupLayout,
        target: &wgpu::BindGroupLayout,
        texture: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = {
            let device = device.clone();
            Deferred::new(move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("mask coverage brush"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                        include_str!("brush.wgsl"),
                        include_str!("selection_clip.wgsl"),
                    ])),
                })
            })
        };
        let analytic = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask analytic layout"),
            bind_group_layouts: &[Some(style), Some(target)],
            immediate_size: 0,
        });
        let tip = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask tip layout"),
            bind_group_layouts: &[Some(style), Some(target), Some(texture)],
            immediate_size: 0,
        });
        let brush = std::array::from_fn(|i| {
            let blend = wgpu::BlendComponent {
                src_factor: if i % 2 == 0 {
                    wgpu::BlendFactor::One
                } else {
                    wgpu::BlendFactor::Zero
                },
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            };
            let (device, analytic, tip, shader) = (
                device.clone(),
                analytic.clone(),
                tip.clone(),
                shader.clone(),
            );
            Deferred::new(move || {
                brush_pipeline_format(
                    &device,
                    if i < 2 { &analytic } else { &tip },
                    &shader,
                    if i < 2 {
                        "analytic_fragment"
                    } else {
                        "mask_fragment"
                    },
                    wgpu::BlendState {
                        color: blend,
                        alpha: blend,
                    },
                    wgpu::TextureFormat::R8Unorm,
                    "mask brush R8",
                )
            })
        });
        let init_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection coverage layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let init_shader = {
            let device = device.clone();
            Deferred::new(move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("polygon selection coverage"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                        include_str!("selection.wgsl"),
                        &include_str!("selection_clip.wgsl").replace("@group(1)", "@group(0)"),
                    ])),
                })
            })
        };
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection initialization"),
            bind_group_layouts: &[Some(&init_layout)],
            immediate_size: 0,
        });
        let initialize = {
            let (device, layout, init_shader) =
                (device.clone(), layout.clone(), init_shader.clone());
            Deferred::new(move || {
                fullscreen_pipeline(
                    &device,
                    &layout,
                    &init_shader,
                    "fragment_main",
                    None,
                    wgpu::TextureFormat::R8Unorm,
                    "antialiased mask selection",
                )
            })
        };
        Self {
            definitions: BTreeMap::new(),
            pages: BTreeMap::new(),
            brush,
            initialize,
            init_layout,
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("empty mask selection"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }
    pub fn compile_all(&self) {
        self.initialize.compile();
        for pipeline in &self.brush {
            pipeline.compile();
        }
    }

    pub fn is_mask(layers: &[Layer], id: LayerId) -> bool {
        Self::masks(layers).any(|m| m.id == id)
    }
    fn masks(layers: &[Layer]) -> impl Iterator<Item = &layer_core::LayerMask> {
        layers.iter().flat_map(Layer::masks)
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        inputs: (&[Layer], &[DabBatch]),
        extent: [u32; 2],
        reset: bool,
        selections: &mut selection_clip::SelectionClip,
    ) -> Result<(), GpuRasterError> {
        let (layers, batches) = inputs;
        if reset {
            self.pages.clear();
        }
        self.pages.retain(|(id, _), _| Self::is_mask(layers, *id));
        self.definitions = Self::masks(layers).map(|m| (m.id, m.clone())).collect();
        for mask in Self::masks(layers) {
            let mut needed = std::collections::BTreeSet::new();
            if let Some(selection) = &mask.initial {
                needed.extend(page_coordinates(pixel_rect(selection.bounds(), extent)));
            }
            for batch in batches.iter().filter(|b| b.layer_id == mask.id) {
                needed.extend(page_coordinates(batch_pixel_rect(batch, extent)));
            }
            // Initialize only missing pages. Polygon and connected-region masks
            // consume the same packed coverage used by brush clipping.
            let missing: Vec<_> = needed
                .into_iter()
                .filter(|c| !self.pages.contains_key(&(mask.id, *c)))
                .collect();
            if missing.is_empty() {
                continue;
            }
            if let Some(selection) = &mask.initial {
                selections.prepare(
                    device,
                    encoder,
                    extent,
                    &std::sync::Arc::new(selection.clone()),
                )?;
            }
            let coverage = if mask.initial.is_some() {
                selections.buffer.as_ref().unwrap()
            } else {
                &self.empty_selection
            };
            for coordinate in missing {
                let MaskPage { texture, view } = MaskPage::new(device);
                let data = [
                    coordinate[0] as f32 * PAGE_SIZE as f32,
                    coordinate[1] as f32 * PAGE_SIZE as f32,
                    f32::from(mask.initial.is_some()),
                    mask.default_coverage,
                ];
                let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mask page initialization"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mask initial coverage"),
                    layout: &self.init_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: coverage.as_entire_binding(),
                        },
                    ],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mask initialize page"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
                pass.set_pipeline(&self.initialize);
                pass.set_bind_group(0, &binding, &[]);
                pass.draw(0..3, 0..1);
                drop(pass);
                self.pages
                    .insert((mask.id, coordinate), MaskPage { texture, view });
            }
        }
        Ok(())
    }
}

impl WgpuRasterizer {
    pub(super) fn encode_mask_dabs(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        layers: &[Layer],
        batches: &[DabBatch],
        committed: &[(LayerId, u32)],
    ) -> Result<(), GpuRasterError> {
        for (index, batch) in batches
            .iter()
            .enumerate()
            .filter(|(_, b)| MaskRenderer::is_mask(layers, b.layer_id))
        {
            if let DabBatchKind::LayerOperation(op) = batch.kind {
                if !committed.contains(&(batch.layer_id, op)) {
                    let operation = &layers
                        .iter()
                        .find_map(|l| l.target_history(batch.layer_id))
                        .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?
                        .1[op as usize];
                    let mut transforms = self.transforms.take().unwrap();
                    let result = transforms.apply(
                        self,
                        encoder,
                        batch.layer_id,
                        operation,
                        self.document_extent,
                    );
                    self.transforms = Some(transforms);
                    result?;
                }
                continue;
            }
            if batch.dab_count == 0 {
                continue;
            }
            self.prepare_selection(encoder, &batch.style)?;
            let damage = batch_pixel_rect(batch, self.document_extent);
            for coordinate in page_coordinates(damage) {
                let Some(page) = self.layer_masks.pages.get(&(batch.layer_id, coordinate)) else {
                    continue;
                };
                let texture_tip = matches!(batch.style.tip, BrushTip::Mask(_));
                let pipeline = &self.layer_masks.brush[usize::from(texture_tip) * 2
                    + usize::from(batch.style.mode == DabMode::Erase)];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("incremental visibility mask brush"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &page.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let local = damage
                    .intersect(page_rect(coordinate))
                    .page_local(coordinate);
                pass.set_scissor_rect(local.min_x, local.min_y, local.width(), local.height());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(
                    0,
                    &self.style_bind_group,
                    &[index as u32 * self.style_stride as u32],
                );
                pass.set_bind_group(
                    1,
                    self.paint_target_binding(&batch.style),
                    &[self.target_offset(coordinate)],
                );
                if let BrushTip::Mask(id) = &batch.style.tip {
                    pass.set_bind_group(2, &self.mask(id)?.bind_group, &[]);
                }
                let start = batch.first_dab as u64 * mem::size_of::<Dab>() as u64;
                pass.set_vertex_buffer(
                    0,
                    self.dab_buffer.slice(
                        start..start + batch.dab_count as u64 * mem::size_of::<Dab>() as u64,
                    ),
                );
                pass.draw(0..4, 0..batch.dab_count);
            }
        }
        Ok(())
    }
}
