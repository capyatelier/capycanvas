//! Sparse R8 visibility masks. Geometry and brush instances stay GPU-resident.
use super::*;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

pub(super) struct MaskPage {
    pub view: wgpu::TextureView,
}
pub(super) struct MaskRenderer {
    pub definitions: BTreeMap<LayerId, layer_core::LayerMask>,
    pub pages: BTreeMap<(LayerId, [u32; 2]), MaskPage>,
    brush: [wgpu::RenderPipeline; 4],
    initialize: wgpu::RenderPipeline,
    init_layout: wgpu::BindGroupLayout,
}
impl MaskRenderer {
    pub fn new(
        device: &wgpu::Device,
        style: &wgpu::BindGroupLayout,
        target: &wgpu::BindGroupLayout,
        texture: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mask coverage brush"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                include_str!("brush.wgsl"),
                include_str!("selection_clip.wgsl"),
            ])),
        });
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
            brush_pipeline_format(
                device,
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
        let init_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("polygon selection coverage"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                include_str!("selection.wgsl"),
                include_str!("selection_geometry.wgsl"),
            ])),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection initialization"),
            bind_group_layouts: &[Some(&init_layout)],
            immediate_size: 0,
        });
        let initialize = fullscreen_pipeline(
            device,
            &layout,
            &init_shader,
            "fragment_main",
            None,
            wgpu::TextureFormat::R8Unorm,
            "antialiased mask selection",
        );
        Self {
            definitions: BTreeMap::new(),
            pages: BTreeMap::new(),
            brush,
            initialize,
            init_layout,
        }
    }
    pub fn is_mask(layers: &[Layer], id: LayerId) -> bool {
        layers
            .iter()
            .flat_map(|l| {
                l.mask
                    .iter()
                    .chain(l.operations.iter().map(|o| &o.coverage))
            })
            .any(|m| m.id == id)
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        layers: &[Layer],
        batches: &[DabBatch],
        extent: [u32; 2],
        reset: bool,
    ) {
        if reset {
            self.pages.clear();
        }
        self.pages.retain(|(id, _), _| Self::is_mask(layers, *id));
        self.definitions = layers
            .iter()
            .flat_map(|l| {
                l.mask
                    .iter()
                    .chain(l.operations.iter().map(|o| &o.coverage))
            })
            .map(|m| (m.id, m.clone()))
            .collect();
        for mask in layers.iter().flat_map(|l| {
            l.mask
                .iter()
                .chain(l.operations.iter().map(|o| &o.coverage))
        }) {
            let mut needed = std::collections::BTreeSet::new();
            if let Some(selection) = &mask.initial {
                let mut bounds = layer_core::Rect::EMPTY;
                for p in selection.contours.iter().flat_map(|p| p.iter()) {
                    bounds.include_circle(*p, 1.0);
                }
                needed.extend(page_coordinates(pixel_rect(bounds, extent)));
            }
            for batch in batches.iter().filter(|b| b.layer_id == mask.id) {
                needed.extend(page_coordinates(batch_pixel_rect(batch, extent)));
            }
            // Geometry is uploaded once for a mask initialization, not on each dab.
            let missing: Vec<_> = needed
                .into_iter()
                .filter(|c| !self.pages.contains_key(&(mask.id, *c)))
                .collect();
            if missing.is_empty() {
                continue;
            }
            let mut edges = Vec::<f32>::new();
            if let Some(selection) = &mask.initial {
                for path in selection.contours.iter() {
                    for (a, b) in path
                        .iter()
                        .zip(path.iter().cycle().skip(1))
                        .take(path.len())
                    {
                        edges.extend([a.x, a.y, b.x, b.y]);
                    }
                }
            }
            let count = edges.len() / 4;
            if edges.is_empty() {
                edges.resize(4, 0.0);
            }
            let edge_data: Vec<u8> = edges.iter().flat_map(|v| v.to_ne_bytes()).collect();
            let edges = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mask selection edges"),
                contents: &edge_data,
                usage: wgpu::BufferUsages::STORAGE,
            });
            for coordinate in missing {
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
                let data = [
                    coordinate[0] as f32 * PAGE_SIZE as f32,
                    coordinate[1] as f32 * PAGE_SIZE as f32,
                    count as f32,
                    mask.initial
                        .as_ref()
                        .map_or(mask.default_coverage, |s| f32::from(s.inverted)),
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
                            resource: edges.as_entire_binding(),
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
                self.pages.insert((mask.id, coordinate), MaskPage { view });
            }
        }
    }
}

impl WgpuRasterizer {
    pub(super) fn encode_mask_dabs(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layers: &[Layer],
        batches: &[DabBatch],
    ) -> Result<(), GpuRasterError> {
        for (index, batch) in batches
            .iter()
            .enumerate()
            .filter(|(_, b)| MaskRenderer::is_mask(layers, b.layer_id) && b.dab_count > 0)
        {
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
