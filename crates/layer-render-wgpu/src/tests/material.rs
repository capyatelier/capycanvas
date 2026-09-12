use super::*;

fn material_renderer() -> WgpuRasterizer {
    #[cfg(not(target_os = "windows"))]
    return WgpuRasterizer::new_headless().expect("physical GPU required");
    #[cfg(target_os = "windows")]
    pollster::block_on(async {
        // The default shared headless adapter can prefer Vulkan on Windows.
        // Exercise the same optimized D3D12 shader backend as the WinUI host.
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
        let instance = wgpu::Instance::new(descriptor);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
                ..Default::default()
            })
            .await
            .expect("physical D3D12 GPU required");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .unwrap();
        WgpuRasterizer::from_wgpu_inner(adapter, device.into(), queue, false).unwrap()
    })
}

// Compare pipeline specialization to the original uniform-dispatched shader,
// using the same arithmetic and bindings. This exercises host operation
// selection, attachment variants, persistent state and both prediction paths.
fn use_uniform_dispatch(renderer: &mut WgpuRasterizer) {
    let source = include_str!("../material_brush.wgsl")
        .replace("override MATERIAL_OPERATION: u32;", "")
        .replace("MATERIAL_OPERATION", "style.operation.z");
    let shader = renderer
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("material uniform-dispatch reference"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                &source,
                include_str!("../brush_coverage.wgsl"),
                include_str!("../selection_clip.wgsl"),
            ])),
        });
    let bindings: [_; 4] =
        std::array::from_fn(|i| renderer.pipelines.material[0].get_bind_group_layout(i as u32));
    let layout = renderer
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("material reference layout"),
            bind_group_layouts: &bindings.each_ref().map(Some),
            immediate_size: 0,
        });
    let pipelines: [_; MaterialPipelineKind::COUNT] = std::array::from_fn(|kind| {
        let color = Some(wgpu::ColorTargetState {
            format: COLOR_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        });
        let coverage = (kind == 1 || kind >= 3).then_some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R8Unorm,
            blend: None,
            write_mask: wgpu::ColorWrites::RED,
        });
        let wetness = (kind >= 2).then_some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R8Unorm,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Max,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Max,
                },
            }),
            write_mask: wgpu::ColorWrites::RED,
        });
        fullscreen_pipeline_targets_with_constants(
            &renderer.device,
            &layout,
            &shader,
            "fragment_main",
            &[color, coverage, wetness],
            &[],
            "material uniform-dispatch reference",
        )
    });
    renderer.pipelines.material = std::array::from_fn(|index| {
        let pipeline = pipelines[index % MaterialPipelineKind::COUNT].clone();
        Deferred::new(move || pipeline)
    });
}

#[test]
fn specialized_material_matches_uniform_dispatch_across_pages_and_prediction() {
    let mut specialized = material_renderer();
    eprintln!(
        "material comparison backend={:?}",
        specialized.adapter.get_info().backend
    );
    let mut reference = WgpuRasterizer::from_wgpu_inner(
        specialized.adapter.clone(),
        specialized.device.clone(),
        specialized.queue.clone(),
        false,
    )
    .unwrap();
    use_uniform_dispatch(&mut reference);
    specialized.resize_surface(384, 128).unwrap();
    reference.resize_surface(384, 128).unwrap();
    let layer = Layer::paint(LayerId(1), "Material equivalence");
    let damage = Rect {
        min: Point { x: 180., y: 0. },
        max: Point { x: 330., y: 128. },
    };
    let view = ViewState {
        width_px: 384,
        ..test_view()
    };
    let mut maximum_error = 0;
    let mut changed = [false; 6];
    for (operation, did_change) in changed.iter_mut().enumerate() {
        for variant in 0..4 {
            let execution = match operation {
                0 | 1 => BrushExecution::Dry,
                2 => BrushExecution::Liquify,
                3 => BrushExecution::Smudge,
                4 => BrushExecution::Wet,
                _ => BrushExecution::Watercolor,
            };
            let mut style = test_style(execution);
            style.alpha_locked = variant & 1 != 0;
            style.mode = if variant & 2 == 0 {
                DabMode::Paint
            } else {
                DabMode::Erase
            };
            style.rendering.blend_mode = BrushBlendMode::Multiply;
            if operation == 1 || operation == 5 {
                style.rendering.accumulation = BrushAccumulation::Uniform;
            }
            style.wet_mix.amount_of_paint = 0.65;
            style.wet_mix.dilution = 0.4;
            style.wet_mix.pull = 0.8;
            style.wet_mix.blur = 0.6;
            style.wet_mix.wetness = if variant & 1 != 0 { 0.7 } else { 0. };
            style.wet_mix.mix_space = ColorMixSpace::Oklab;
            assert_eq!(
                BrushPassPlan::for_style(&style).material as usize,
                operation
            );
            let base = DabBatch {
                material_update: 0,
                stroke_id: StrokeId(1),
                layer_id: layer.id,
                kind: DabBatchKind::Persistent,
                stroke_start: true,
                stroke_end: true,
                first_dab: 0,
                dab_count: 2,
                style: test_style(BrushExecution::Dry),
                damage,
            };
            let seed = [
                test_dab([242., 64.], [0.9, 0.08, 0.03, 0.75], 1.),
                test_dab([269., 64.], [0.08, 0.75, 0.12, 0.85], 1.),
            ];
            let mut first = test_dab([253., 64.], [0.03, 0.1, 0.9, 0.8], 0.65);
            first.radii = [28., 24.];
            first.hardness = 0.6;
            first.motion = [13., 1.5];
            first.material = [0., 0.8, 0.9, 0.7];
            let mut second = first;
            second.center.x += 9.;
            second.center.y += 3.;
            let contacts = [first, second];
            let mut seed_pixels = Vec::new();
            // Single prediction uses persistent color directly; two prediction
            // batches use private ping-pong pages. Following commits verify
            // that preview did not modify coverage, wetness or source pixels.
            for phase in 0..5 {
                let dabs = if phase == 0 { &seed } else { &contacts };
                let mut batches = vec![base.clone()];
                if phase != 0 {
                    batches[0].style = style.clone();
                    batches[0].stroke_id = StrokeId(2);
                    batches[0].stroke_start = phase != 4;
                    batches[0].stroke_end = phase == 4;
                    batches[0].kind = if phase <= 2 {
                        DabBatchKind::Preview
                    } else {
                        DabBatchKind::Persistent
                    };
                    if phase == 2 {
                        batches[0].dab_count = 1;
                        let mut next = batches[0].clone();
                        next.first_dab = 1;
                        next.stroke_start = false;
                        batches.push(next);
                    }
                    if phase == 4 {
                        batches[0].material_update = 1;
                    }
                }
                let packet = FramePacket {
                    view,
                    document_extent: [384, 128],
                    layers: std::slice::from_ref(&layer),
                    dabs,
                    dab_batches: &batches,
                    reset_layers: phase == 0,
                    time_seconds: 0.,
                    composite_all: phase == 0,
                };
                specialized.submit(packet).unwrap();
                reference.submit(packet).unwrap();
                let actual = specialized.readback_srgb_rgba8().unwrap();
                let expected = reference.readback_srgb_rgba8().unwrap();
                assert_eq!(actual.len(), expected.len());
                let error = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                maximum_error = maximum_error.max(error);
                assert!(
                    error <= 1,
                    "operation={operation}, variant={variant}, phase={phase}, max channel error={error}"
                );
                if phase == 0 {
                    seed_pixels = actual;
                } else if phase >= 3 && actual != seed_pixels {
                    *did_change = true;
                }
            }
        }
    }
    assert!(
        changed.into_iter().all(|value| value),
        "every material must affect the painted reference"
    );
    eprintln!(
        "24 material cases, 120 full-image comparisons; maximum channel error={maximum_error}"
    );
}
