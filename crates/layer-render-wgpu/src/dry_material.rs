//! Dry pages have no cross-page dependencies: dispatch their unchanged pixel
//! evaluator together instead of opening a render pass for every page.
use super::*;

pub(super) type Job = (wgpu::BindGroup, wgpu::BindGroup, [u32; 2], bool);

pub(super) struct Pipelines {
    layouts: [wgpu::BindGroupLayout; 2],
    pub kernels: [Deferred<wgpu::ComputePipeline>; 4],
}

impl Pipelines {
    pub fn new(
        device: &PipelineDevice,
        shared: &PipelineLayouts<'_>,
        shader: &Deferred<wgpu::ShaderModule>,
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
                        access: wgpu::StorageTextureAccess::WriteOnly,
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
        let kernels = std::array::from_fn(|index| {
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
                            constants: &[("MATERIAL_OPERATION", (index / 2) as f64)],
                            ..Default::default()
                        },
                        cache: None,
                    },
                )
            })
        });
        Self { layouts, kernels }
    }

    pub fn output(
        &self,
        r: &WgpuRasterizer,
        color: &wgpu::TextureView,
        coverage: Option<&wgpu::TextureView>,
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
                resource: wgpu::BindingResource::TextureView(color),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(coverage.unwrap_or(color)),
            },
        ];
        r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dry material page output"),
            layout: &self.layouts[usize::from(coverage.is_some())],
            entries: &entries[..2 + usize::from(coverage.is_some())],
        })
    }
}

impl WgpuRasterizer {
    pub(super) fn compute_dry_material(&self, batch: &DabBatch) -> bool {
        // Normal source-over stays on the compute evaluator. On Adreno, its
        // specialized destination-blend variant disagrees with the legacy fragment path
        // for a predicted Multiply batch.  Keep the established fragment
        // route for every non-normal blend: it has the same ordered dab
        // evaluation and avoids making preview correctness depend on that
        // driver specialization.
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
        for (output, source, coordinate, coverage) in jobs {
            let pipeline = &self.pipelines.dry_material.as_ref().unwrap().kernels
                [operation as usize * 2 + usize::from(*coverage)];
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, output, &[batch_index as u32 * self.style_stride as u32]);
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.layer_id, *coordinate)],
            );
            pass.set_bind_group(2, source, &[]);
            pass.set_bind_group(3, &textures.bind_group, &[]);
            pass.dispatch_workgroups(PAGE_SIZE.div_ceil(8), PAGE_SIZE.div_ceil(8), 1);
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
