//! Dry pages have no cross-page dependencies: dispatch their unchanged pixel
//! evaluator together instead of opening a render pass for every page.
use super::*;

pub(super) type Job = (wgpu::BindGroup, wgpu::BindGroup, [u32; 2], bool, u32);

pub(super) struct Pipelines {
    in_place: bool,
    layouts: [wgpu::BindGroupLayout; 2],
    pub kernels: [Deferred<wgpu::ComputePipeline>; 4],
    variants: std::collections::BTreeMap<u32, [Deferred<wgpu::ComputePipeline>; 4]>,
}

pub(super) fn contact_flags(contact: Option<layer_core::BrushContact>) -> u32 {
    let Some(c) = contact else {
        return 0;
    };
    1 | (u32::from(c.paper > 0.) << 1)
        | (u32::from(c.edge_roughness > 0.) << 2)
        | (u32::from(c.fiber_strength > 0.) << 3)
        | (u32::from(c.pooling > 0.) << 4)
        | (u32::from(c.depletion > 0.) << 5)
        | (u32::from(c.tip_bias > 0. || c.tilt_shading > 0.) << 6)
}

impl Pipelines {
    #[cfg(test)]
    pub(super) fn use_unculled_reference(&mut self) {
        self.variants.clear();
    }

    pub fn new(
        device: &PipelineDevice,
        shared: &PipelineLayouts<'_>,
        shader: &Deferred<wgpu::ShaderModule>,
        in_place: bool,
    ) -> Self {
        let layouts = std::array::from_fn(|coverage| {
            let mut entries = vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
                },
                count: None,
            }];
            for binding in 1..=1 + coverage as u32 {
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: if in_place && binding == 1 { wgpu::StorageTextureAccess::ReadWrite }
                            else { wgpu::StorageTextureAccess::WriteOnly },
                        format: if binding == 1 {
                            wgpu::TextureFormat::Rgba32Float
                        } else {
                            wgpu::TextureFormat::R32Float
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                });
            }
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("dry material outputs"),
                entries: &entries,
            })
        });
        let make_kernels = |flags: u32| {
            std::array::from_fn(|index| {
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("dry material pages"),
                    bind_group_layouts: &[
                        Some(&layouts[index % 2]),
                        Some(shared.target),
                        Some(shared.material),
                        Some(shared.advanced_texture),
                    ],
                    immediate_size: 0,
                });
                let (device, shader) = (device.clone(), shader.clone());
                Deferred::pipeline(move |mode| {
                    mode.compute(
                        &device,
                        &wgpu::ComputePipelineDescriptor {
                            label: Some("dry material pages"),
                            layout: Some(&layout),
                            module: &shader,
                            entry_point: Some(if index % 2 == 0 {
                                "compute_color"
                            } else {
                                "compute_coverage"
                            }),
                            compilation_options: wgpu::PipelineCompilationOptions {
                                constants: &[
                                    ("MATERIAL_OPERATION", (index / 2) as f64),
                                    ("CONTACT_FLAGS", f64::from(flags)),
                                    ("MATERIAL_IN_PLACE", f64::from(in_place)),
                                    // The generic kernel also serves as an
                                    // independent reference for film culling.
                                    ("CONTACT_FILM_CULL", f64::from(flags != u32::MAX)),
                                ],
                                ..Default::default()
                            },
                            cache: None,
                        },
                    )
                })
            })
        };
        let kernels = make_kernels(u32::MAX);
        let mut flags = layer_core::CONTACT_BRUSH_PRESETS
            .into_iter()
            .map(|p| contact_flags(layer_core::default_brush(p).contact))
            .collect::<std::collections::BTreeSet<_>>();
        flags.insert(0);
        flags.extend(flags.clone().into_iter().map(|flags| flags | 128));
        let variants = flags.into_iter().map(|f| (f, make_kernels(f))).collect();
        Self {
            in_place,
            layouts,
            kernels,
            variants,
        }
    }

    pub fn for_style(
        &self,
        style: &layer_render::DabStyle,
    ) -> &[Deferred<wgpu::ComputePipeline>; 4] {
        // Flow integration and maximum-film deposition have different kernels.
        // Resolve that uniform branch at compilation, including its register
        // requirements, rather than carrying both models through every pixel.
        let flags = contact_flags(style.contact)
            | if style.rendering.accumulation == BrushAccumulation::Uniform { 128 } else { 0 };
        self.variants
            .get(&flags)
            .unwrap_or(&self.kernels)
    }

    pub fn output(
        &self,
        r: &WgpuRasterizer,
        color: &PageSurface,
        coverage: Option<&PageSurface>,
    ) -> wgpu::BindGroup {
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &r.style_buffer,
                    offset: 0,
                    size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&color.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&coverage.unwrap_or(color).view),
            },
        ];
        coverage.unwrap_or(color).material_output.get(
            (
                r.style_buffer.clone(),
                color.view.clone(),
                coverage.map(|p| p.view.clone()),
                self.in_place,
            ),
            || {
                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("dry material page output"),
                    layout: &self.layouts[usize::from(coverage.is_some())],
                    entries: &entries[..2 + usize::from(coverage.is_some())],
                })
            },
        )
    }
}

impl WgpuRasterizer {
    pub(super) fn in_place_dry_material(&self, batch: &DabBatch) -> bool {
        batch.kind == DabBatchKind::Persistent && self.compute_dry_material(batch)
            && self.pipelines.dry_in_place.is_some()
    }

    pub(super) fn compute_dry_material(&self, batch: &DabBatch) -> bool {
        // Adreno's compute destination-blend specialization corrupts predicted
        // Multiply batches. Non-normal blends use the ordered fragment path.
        dry_material_compute_eligible(&batch.style) && self.pipelines.dry_material.is_some()
    }

    pub(super) fn encode_dry_material_jobs(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        jobs: &[Job],
    ) {
        if jobs.is_empty() {
            return;
        }
        let operation = BrushPassPlan::for_device(&batch.style, &self.device).material;
        let texture_key = Self::texture_set_key(&batch.style);
        let textures = self
            .texture_sets
            .iter()
            .find(|set| set.key == texture_key)
            .unwrap();
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("dry material pages"),
            timestamp_writes: None,
        });
        pass.set_bind_group(3, &textures.bind_group, &[]);
        let mut active_coverage = None;
        for (output, source, coordinate, coverage, record_offset) in jobs {
            let pipeline = &if self.in_place_dry_material(batch) {
                &self.pipelines.dry_in_place
            } else { &self.pipelines.dry_material }
                .as_ref()
                .unwrap()
                .for_style(&batch.style)[operation as usize * 2 + usize::from(*coverage)];
            if active_coverage != Some(*coverage) {
                pass.set_pipeline(pipeline);
                active_coverage = Some(*coverage);
            }
            pass.set_bind_group(0, output, &[batch_index as u32 * self.style_stride as u32]);
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.layer_id, *coordinate)],
            );
            pass.set_bind_group(2, source, &[*record_offset]);
            pass.dispatch_workgroups(PAGE_SIZE.div_ceil(32), PAGE_SIZE.div_ceil(2), 1);
        }
    }
}

fn dry_material_compute_eligible(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Dry && style.rendering.blend_mode == BrushBlendMode::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_normal_dry_material_uses_the_fragment_path() {
        let normal = crate::tests::test_style(BrushExecution::Dry);
        assert!(dry_material_compute_eligible(&normal));

        let mut multiply = normal;
        multiply.rendering.blend_mode = BrushBlendMode::Multiply;
        assert!(!dry_material_compute_eligible(&multiply));
    }
}
