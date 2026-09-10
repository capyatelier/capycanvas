use super::*;
use layer_core::{EffectInstance, EffectValue, LayerMask, Selection};

// Keep the established ten-filter baseline stable as the catalog grows.
fn pointwise_baseline() -> [&'static layer_core::EffectDefinition; 10] {
    [
        fixture("curves"),
        fixture("levels"),
        fixture("brightness_contrast"),
        fixture("hue_saturation"),
        fixture("color_balance"),
        fixture("exposure"),
        fixture("vibrance"),
        fixture("black_white"),
        fixture("gradient_map"),
        fixture("posterize"),
    ]
}

fn effect(id: u64, kind: &layer_core::EffectDefinition) -> Layer {
    let mut l = Layer::paint(LayerId(id), kind.label());
    l.kind = LayerKind::Effect;
    l.effect = Some(Arc::new(EffectInstance::new(kind.program())));
    l
}
fn set(layer: &mut Layer, key: &str, value: EffectValue) {
    Arc::make_mut(layer.effect.as_mut().unwrap())
        .set(key, value)
        .unwrap();
}

#[test]
fn image_passes_cross_tiles_cache_inputs_and_freeze_animation() {
    use layer_core::{EffectKind, EffectPass, EffectSampling};
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let mut source = effect(1, fixture("brightness_contrast"));
    let mut generator = (*source.effect.as_ref().unwrap().program).clone();
    generator.kind = EffectKind::Generator;
    generator.wgsl = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(select(0.,1.,p.x<256.),select(1.,0.,p.x<256.),p.y/291.,1.);}".into();
    generator.entry = "pattern".into();
    source.effect = Some(Arc::new(EffectInstance::new(Arc::new(generator))));
    let mut filter = effect(2, fixture("brightness_contrast"));
    let mut program = (*filter.effect.as_ref().unwrap().program)
        .clone()
        .with_time_controls();
    program.entry = "horizontal".into();
    program.wgsl = "fn horizontal(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p+vec2<f32>(-2.,0.))+c+fx_sample(p+vec2<f32>(2.,0.)))/3.;}\nfn vertical(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let a=(fx_sample(p+vec2<f32>(0.,-2.))+c+fx_sample(p+vec2<f32>(0.,2.)))/3.;return vec4<f32>(a.rg,clamp(a.b+fx_time(b)*.1,0.,1.),a.a);}".into();
    program.passes = ["horizontal", "vertical"]
        .map(|entry| EffectPass {
            entry: entry.into(),
            sampling: EffectSampling::Neighborhood { radius: 2 },
        })
        .into();
    filter.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    let mut layers = vec![filter, source];
    let mut view = test_view();
    view.width_px = 333;
    view.height_px = 291;
    view.background_rgba_linear = [0.; 4];
    let render = |r: &mut WgpuRasterizer, layers: &[Layer], time, all| {
        r.submit(FramePacket {
            view,
            document_extent: [333, 291],
            layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: false,
            time_seconds: time,
            composite_all: all,
        })
        .unwrap();
        let mut bytes = vec![0; 333 * 291 * 4];
        r.copy_rgba8_srgb(&mut bytes, 333 * 4).unwrap();
        bytes
    };
    let initial = render(&mut r, &layers, 0., true);
    let at = |image: &[u8], x: usize, y: usize| {
        <[u8; 4]>::try_from(&image[(y * 333 + x) * 4..][..4]).unwrap()
    };
    let edge = at(&initial, 255, 150);
    assert!(
        edge[0] > 200 && edge[0] < 220 && edge[1] > 150 && edge[1] < 165,
        "neighbor sampling must cross x=256: {edge:?}"
    );
    assert_eq!(r.scene.as_ref().unwrap().image_work(), [1, 2]);
    let later = render(&mut r, &layers, 1., false);
    assert_ne!(initial, later);
    assert_eq!(
        r.scene.as_ref().unwrap().image_work(),
        [1, 4],
        "time changes must not rebuild the source"
    );
    let compiled = r.scene.as_ref().unwrap().effects.compilations;
    set(&mut layers[0], "animate", EffectValue::Toggle(false));
    set(&mut layers[0], "time", EffectValue::Number(1.));
    let frozen = render(&mut r, &layers, 20., true);
    assert_eq!(frozen, later);
    let work = r.scene.as_ref().unwrap().image_work();
    let again = render(&mut r, &layers, 21., true);
    assert_eq!(again, frozen);
    assert_eq!(
        r.scene.as_ref().unwrap().image_work(),
        work,
        "unchanged frozen effects do no image work"
    );
    assert_eq!(r.scene.as_ref().unwrap().effects.compilations, compiled);
    layers[0].mask = Some(LayerMask::reveal_all(LayerId(20), Point::default()));
    layers[0].mask.as_mut().unwrap().default_coverage = 0.;
    let hidden = render(&mut r, &layers, 21., true);
    assert_eq!(
        at(&hidden, 255, 150)[1],
        0,
        "mask applies only after both passes"
    );
    layers[0].mask = None;
    let mut next = layers[0].clone();
    next.id = LayerId(3);
    layers.insert(0, next);
    let work = r.scene.as_ref().unwrap().image_work();
    render(&mut r, &layers, 21., true);
    assert_eq!(
        r.scene.as_ref().unwrap().image_work()[0] - work[0],
        1,
        "adjacent stages must share their image without recomposing it"
    );
    assert_eq!(
        r.scene.as_ref().unwrap().image_cache_bytes(),
        333 * 291 * 4 * 4,
        "one original, two results, one shared intermediate"
    );
    let work = r.scene.as_ref().unwrap().image_work();
    set(&mut layers[0], "time", EffectValue::Number(2.));
    render(&mut r, &layers, 21., true);
    assert_eq!(
        r.scene.as_ref().unwrap().image_work(),
        [work[0], work[1] + 2],
        "editing the upper stage reuses all upstream pixels"
    );
    let work = r.scene.as_ref().unwrap().image_work();
    set(&mut layers[1], "time", EffectValue::Number(2.));
    render(&mut r, &layers, 21., true);
    assert_eq!(
        r.scene.as_ref().unwrap().image_work(),
        [work[0], work[1] + 4],
        "editing the lower stage updates both without copying the input"
    );
    layers.remove(0);
    set(&mut layers[0], "time", EffectValue::Number(0.));
    Arc::make_mut(&mut Arc::make_mut(layers[0].effect.as_mut().unwrap()).program).alpha =
        layer_core::EffectAlpha::Filter;
    Arc::make_mut(&mut Arc::make_mut(layers[1].effect.as_mut().unwrap()).program).wgsl="fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return select(vec4<f32>(0.),vec4<f32>(1.,0.,0.,1.),p.x<256.);}".into();
    let blurred = render(&mut r, &layers, 21., true);
    assert!(
        (80..90).contains(&at(&blurred, 256, 150)[3]),
        "blur must extend coverage across a transparent edge"
    );
    layers[0].properties.clipped = true;
    let clipped = render(&mut r, &layers, 21., true);
    assert_eq!(
        at(&clipped, 256, 150)[3],
        0,
        "clipping preserves original base coverage even for spatial filters"
    );
}

#[test]
fn image_boundary_matches_fused_mask_clip_and_group_semantics() {
    use layer_core::{EffectPass, EffectSampling};
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let base = Layer::paint(LayerId(1), "Paint");
    paint(
        &mut r,
        std::slice::from_ref(&base),
        1,
        [0.7, 0.25, 0.1, 0.6],
        45.,
    );
    let mut effect = effect(2, fixture("hue_saturation"));
    set(&mut effect, "hue", EffectValue::Number(100.));
    effect.opacity = 0.7;
    for grouped in [false, true] {
        for clipped in [false, true] {
            for moved in [false, true] {
                let mut paint = base.clone();
                paint.opacity = 0.6;
                let mut fx = effect.clone();
                fx.properties.clipped = clipped;
                let mut mask = LayerMask::reveal_all(
                    LayerId(4),
                    Point {
                        x: if moved { 13. } else { 0. },
                        y: 0.,
                    },
                );
                mask.default_coverage = 0.;
                mask.initial = Some(
                    Selection::polygon(vec![
                        Point { x: 0., y: 0. },
                        Point { x: 100., y: 15. },
                        Point { x: 30., y: 128. },
                    ])
                    .unwrap(),
                );
                fx.mask = Some(mask);
                let mut layers = vec![];
                if grouped {
                    let mut group = Layer::paint(LayerId(3), "Group");
                    group.kind = LayerKind::Group;
                    group.opacity = 0.75;
                    paint.properties.parent = Some(group.id);
                    fx.properties.parent = Some(group.id);
                    layers.push(group);
                }
                layers.extend([fx, paint]);
                frame(&mut r, &layers);
                let mut expected = vec![0; 128 * 128 * 4];
                r.copy_rgba8_srgb(&mut expected, 512).unwrap();
                let index = usize::from(grouped);
                let instance = Arc::make_mut(layers[index].effect.as_mut().unwrap());
                let program = Arc::make_mut(&mut instance.program);
                program.passes = Arc::from([EffectPass {
                    entry: program.entry.clone(),
                    sampling: EffectSampling::Neighborhood { radius: 0 },
                }]);
                frame(&mut r, &layers);
                let mut actual = vec![0; 128 * 128 * 4];
                r.copy_rgba8_srgb(&mut actual, 512).unwrap();
                assert!(
                    actual
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.abs_diff(*b) <= 2),
                    "grouped={grouped} clipped={clipped} moved={moved}"
                );
            }
        }
    }
}
fn frame(r: &mut WgpuRasterizer, layers: &[Layer]) {
    r.submit(FramePacket {
        view: test_view(),
        document_extent: [128, 128],
        layers,
        dabs: &[],
        dab_batches: &[],
        reset_layers: false,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
}
fn paint(r: &mut WgpuRasterizer, layers: &[Layer], id: u64, color: [f32; 4], radius: f32) {
    let mut dab = test_dab([64., 64.], color, 1.);
    dab.radii = [radius; 2];
    let batch = DabBatch {
        stroke_id: StrokeId(id),
        layer_id: LayerId(id),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 128., y: 128. },
        },
    };
    r.submit(FramePacket {
        view: test_view(),
        document_extent: [128, 128],
        layers,
        dabs: &[dab],
        dab_batches: &[batch],
        reset_layers: false,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
}
#[test]
fn adjustment_defaults_masks_clipping_and_parameter_updates() {
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let base = Layer::paint(LayerId(1), "Paint");
    paint(
        &mut r,
        std::slice::from_ref(&base),
        1,
        [0.2, 0.4, 0.7, 1.],
        32.,
    );
    let original = pixel(&mut r, 64, 64);
    for kind in pointwise_baseline() {
        if matches!(kind.id(), "black_white" | "gradient_map" | "posterize") {
            continue;
        }
        let fx = effect(2, kind);
        frame(&mut r, &[fx, base.clone()]);
        let actual = pixel(&mut r, 64, 64);
        assert!(
            actual.iter().zip(original).all(|(a, b)| a.abs_diff(b) <= 2),
            "{} default: {actual:?} vs {original:?}",
            kind.label()
        );
    }
    let mut fx = effect(2, fixture("brightness_contrast"));
    set(&mut fx, "brightness", EffectValue::Number(-50.));
    frame(&mut r, &[fx.clone(), base.clone()]);
    assert!(pixel(&mut r, 4, 64)[0] < 150, "unclipped adjusts backdrop");
    fx.properties.clipped = true;
    frame(&mut r, &[fx.clone(), base.clone()]);
    assert_eq!(
        pixel(&mut r, 4, 64),
        [255; 4],
        "clipped effect leaves backdrop alone"
    );
    assert!(pixel(&mut r, 64, 64)[0] < original[0] - 50);
    let mut mask = LayerMask::reveal_all(LayerId(8), Point::default());
    mask.default_coverage = 0.;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 64., y: 0. },
            Point { x: 64., y: 128. },
            Point { x: 0., y: 128. },
        ])
        .unwrap(),
    );
    fx.mask = Some(mask);
    frame(&mut r, &[fx.clone(), base.clone()]);
    assert!(pixel(&mut r, 48, 64)[0] < original[0] - 50);
    assert!(
        pixel(&mut r, 80, 64)[0].abs_diff(original[0]) <= 2,
        "masked adjustment must preserve original, not make it transparent"
    );
    let compiled = r.scene.as_ref().unwrap().effects.compilations;
    set(&mut fx, "brightness", EffectValue::Number(20.));
    frame(&mut r, &[fx.clone(), base.clone()]);
    assert_eq!(
        compiled,
        r.scene.as_ref().unwrap().effects.compilations,
        "parameter edits reuse pipeline"
    );
    assert!(pixel(&mut r, 48, 64)[0] > original[0] + 20);
    fx.mask.as_mut().unwrap().offset.x = 64.;
    frame(&mut r, &[fx.clone(), base.clone()]);
    assert!(
        pixel(&mut r, 48, 64)[0].abs_diff(original[0]) <= 2,
        "translated mask exposes the original at its old position"
    );
    assert!(
        pixel(&mut r, 80, 64)[0] > original[0] + 20,
        "translated mask applies the effect at its new position"
    );
    fx.visible = false;
    frame(&mut r, &[fx, base]);
    assert_eq!(
        pixel(&mut r, 64, 64),
        original,
        "hiding restores unmodified source"
    );
}

#[test]
fn adjustment_chain_is_fused_and_preserves_clip_base() {
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let base = Layer::paint(LayerId(1), "Paint");
    paint(
        &mut r,
        std::slice::from_ref(&base),
        1,
        [0.2, 0.4, 0.7, 1.],
        32.,
    );
    let mut layers: Vec<_> = pointwise_baseline()
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            let mut l = effect(i as u64 + 2, kind);
            l.properties.clipped = true;
            l
        })
        .collect();
    set(&mut layers[2], "brightness", EffectValue::Number(-50.));
    layers.push(base);
    frame(&mut r, &layers);
    assert_eq!(
        r.scene.as_ref().unwrap().effects.compilations,
        1,
        "one compiled fused chain"
    );
    assert_eq!(pixel(&mut r, 4, 64), [255; 4]);
    assert!(pixel(&mut r, 64, 64)[0] < 50);
}

#[test]
fn masked_chain_crosses_portable_texture_limit_without_losing_coverage() {
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let base = Layer::paint(LayerId(1), "White");
    paint(&mut r, std::slice::from_ref(&base), 1, [1.; 4], 128.);
    let mut layers = Vec::new();
    for i in 0..17 {
        let mut fx = effect(i + 2, fixture("brightness_contrast"));
        set(&mut fx, "brightness", EffectValue::Number(-1.));
        let mut mask = LayerMask::reveal_all(LayerId(30 + i), Point::default());
        mask.default_coverage = 0.;
        mask.initial = Some(
            Selection::polygon(vec![
                Point { x: 0., y: 0. },
                Point { x: 64., y: 0. },
                Point { x: 64., y: 128. },
                Point { x: 0., y: 128. },
            ])
            .unwrap(),
        );
        fx.mask = Some(mask);
        layers.push(fx);
    }
    layers.push(base);
    frame(&mut r, &layers);
    assert!(pixel(&mut r, 48, 64)[0].abs_diff(212) <= 2);
    assert_eq!(pixel(&mut r, 80, 64), [255; 4]);
    assert_eq!(
        r.scene.as_ref().unwrap().effects.compilations,
        2,
        "split the chain only when the guaranteed texture inputs are exhausted"
    );
}

#[test]
#[ignore = "release-mode physical GPU benchmark"]
fn adjustment_latency() {
    use std::time::Instant;
    fn summary(mut v: Vec<f64>) -> [f64; 3] {
        v.sort_by(f64::total_cmp);
        [v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() * 99 / 100]]
    }
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let mut report = String::from(
        "size,incremental,case,cpu_median,cpu_p95,cpu_p99,complete_median,complete_p95,complete_p99,gpu_median,gpu_p95,gpu_p99,passes\n",
    );
    eprintln!("adapter {:?}", r.adapter.get_info());
    for size in [128, 2048, 4096] {
        for incremental in [false, true] {
            for case in 0..=pointwise_baseline().len() + 4 {
                let kind = case
                    .checked_sub(1)
                    .and_then(|i| pointwise_baseline().get(i).copied());
                let chain = case > pointwise_baseline().len();
                let masked = case == pointwise_baseline().len() + 2;
                let clipped = case == pointwise_baseline().len() + 3;
                let telemetry = case != pointwise_baseline().len() + 4;
                let label = if !chain {
                    kind.map_or("Baseline", layer_core::EffectDefinition::label)
                } else if masked {
                    "Ten masked"
                } else if clipped {
                    "Ten clipped"
                } else if telemetry {
                    "Ten fused"
                } else {
                    "Ten fused, stats off"
                };
                let mut layers = vec![Layer::paint(LayerId(1), "Paint")];
                if let Some(kind) = kind {
                    layers.insert(0, effect(2, kind));
                }
                if chain {
                    for (i, kind) in pointwise_baseline().into_iter().enumerate() {
                        let mut fx = effect(2 + i as u64, kind);
                        fx.properties.clipped = clipped;
                        if masked {
                            let mut mask =
                                LayerMask::reveal_all(LayerId(30 + i as u64), Point::default());
                            mask.default_coverage = 0.;
                            mask.initial = Some(
                                Selection::polygon(vec![
                                    Point { x: 0., y: 0. },
                                    Point {
                                        x: size as f32,
                                        y: size as f32 * 0.2,
                                    },
                                    Point {
                                        x: size as f32 * 0.4,
                                        y: size as f32,
                                    },
                                ])
                                .unwrap(),
                            );
                            mask.inverted = i % 2 == 0;
                            fx.mask = Some(mask);
                        }
                        layers.insert(0, fx);
                    }
                }
                let view = ViewState {
                    width_px: size,
                    height_px: size,
                    ..test_view()
                };
                let mut dab = test_dab([size as f32 / 2.; 2], [0.15, 0.35, 0.65, 1.], 1.);
                dab.radii = [size as f32; 2];
                let mut batch = DabBatch {
                    stroke_id: StrokeId(1),
                    layer_id: LayerId(1),
                    kind: DabBatchKind::Persistent,
                    stroke_start: true,
                    stroke_end: true,
                    first_dab: 0,
                    dab_count: 1,
                    style: test_style(BrushExecution::Dry),
                    damage: Rect {
                        min: Point { x: 0., y: 0. },
                        max: Point {
                            x: size as f32,
                            y: size as f32,
                        },
                    },
                };
                // Resident painted pixels, not an empty-canvas-only benchmark.
                r.submit(FramePacket {
                    view,
                    document_extent: [size, size],
                    layers: &layers[layers.len() - 1..],
                    dabs: &[dab],
                    dab_batches: std::slice::from_ref(&batch),
                    reset_layers: true,
                    time_seconds: 0.,
                    composite_all: true,
                })
                .unwrap();
                r.wait_idle().unwrap();
                dab.center = Point { x: 64., y: 64. };
                dab.radii = [30.; 2];
                batch.damage = Rect {
                    min: Point { x: 32., y: 32. },
                    max: Point { x: 96., y: 96. },
                };
                let mut cold = 0.;
                let mut cpu = Vec::new();
                let mut complete = Vec::new();
                r.set_telemetry_enabled(telemetry);
                for i in 0..145 {
                    let start = Instant::now();
                    r.submit(FramePacket {
                        view,
                        document_extent: [size, size],
                        layers: &layers,
                        dabs: if incremental {
                            std::slice::from_ref(&dab)
                        } else {
                            &[]
                        },
                        dab_batches: if incremental {
                            std::slice::from_ref(&batch)
                        } else {
                            &[]
                        },
                        reset_layers: false,
                        time_seconds: 0.,
                        composite_all: !incremental || i == 0,
                    })
                    .unwrap();
                    let submit = start.elapsed().as_secs_f64() * 1000.;
                    r.wait_idle().unwrap();
                    let total = start.elapsed().as_secs_f64() * 1000.;
                    if i == 0 {
                        cold = total;
                    }
                    if i >= 25 {
                        cpu.push(submit);
                        complete.push(total);
                    }
                }
                let t = r.telemetry();
                let gpu: Vec<_> = t.gpu.ordered().into_iter().map(f64::from).collect();
                let cpu = summary(cpu);
                let complete = summary(complete);
                let gpu = if telemetry { Some(summary(gpu)) } else { None };
                let triple = |v: [f64; 3]| format!("{:.6},{:.6},{:.6}", v[0], v[1], v[2]);
                report.push_str(&format!(
                    "{size},{incremental},\"{label}\",{},{},{},{}\n",
                    triple(cpu),
                    triple(complete),
                    gpu.map_or(",,".into(), triple),
                    t.effect_passes
                ));
                eprintln!(
                    "{size}, incremental={incremental}, {}, cold_ms={cold:.3}, cpu={:?}, complete={:?}, gpu={:?}, passes={}",
                    label, cpu, complete, gpu, t.effect_passes
                );
            }
        }
    }
    std::fs::create_dir_all("../../artifacts/benchmarks").unwrap();
    std::fs::write("../../artifacts/benchmarks/adjustments.csv", report).unwrap();
}

#[test]
fn programmable_generator_and_adjustment_share_runtime_without_tile_seams() {
    use layer_core::{EFFECT_ABI, EffectKind, EffectProgram};
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let mut generator = Layer::paint(LayerId(1), "Procedural gradient");
    generator.kind = LayerKind::Effect;
    generator.effect=Some(Arc::new(EffectInstance::new(Arc::new(EffectProgram{
        abi:EFFECT_ABI,id:"test_gradient".into(),label:"Test gradient".into(),kind:EffectKind::Generator,alpha:layer_core::EffectAlpha::Filter,
        entry:"test_gradient".into(),parameters:Arc::from([]),constraints:Arc::from([]),passes:Arc::from([]),time:false,lookups:Arc::from([]),
        wgsl:"fn test_gradient(c:vec4<f32>,position:vec2<f32>,base:u32)->vec4<f32>{return vec4<f32>(position.x/333.,position.y/291.,.25,.5)*vec4<f32>(.5,.5,.5,1.);}".into(),
    }))));
    let mut fx = effect(2, fixture("hue_saturation"));
    set(&mut fx, "hue", EffectValue::Number(120.));
    let mut view = test_view();
    view.width_px = 333;
    view.height_px = 291;
    view.background_rgba_linear = [0.; 4];
    let render = |r: &mut WgpuRasterizer, layers: &[Layer]| {
        r.submit(FramePacket {
            view,
            document_extent: [333, 291],
            layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: false,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        let mut bytes = vec![0; 333 * 291 * 4];
        r.copy_rgba8_srgb(&mut bytes, 333 * 4).unwrap();
        bytes
    };
    let original = render(&mut r, std::slice::from_ref(&generator));
    let adjusted = render(&mut r, &[fx.clone(), generator.clone()]);
    assert!(
        adjusted.chunks_exact(4).all(|p| p[3].abs_diff(128) <= 1),
        "adjustments preserve translucent alpha"
    );
    for y in [0, 128, 255, 256, 290] {
        for x in [0, 128, 255, 256, 332] {
            let offset = (y * 333 + x) * 4;
            let p = &adjusted[offset..offset + 4];
            assert!(
                p[..3]
                    .iter()
                    .zip(&original[offset..offset + 3])
                    .any(|(a, b)| a.abs_diff(*b) > 10),
                "hue transforms generator RGB"
            );
            if x == 255 {
                assert!(
                    p[..3]
                        .iter()
                        .zip(&adjusted[offset + 4..offset + 7])
                        .all(|(a, b)| a.abs_diff(*b) <= 4),
                    "continuous across tile boundary"
                );
            }
        }
    }
    fx.opacity = 0.;
    assert_eq!(
        render(&mut r, &[fx, generator]),
        original,
        "zero opacity restores original color and alpha"
    );
}

#[test]
fn builtin_adjustments_have_known_color_results() {
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let base = Layer::paint(LayerId(1), "Red");
    paint(
        &mut r,
        std::slice::from_ref(&base),
        1,
        [1., 0., 0., 1.],
        40.,
    );
    let cases = [
        (
            fixture("hue_saturation"),
            "hue",
            EffectValue::Number(120.),
            [0, 255, 0],
        ),
        (
            fixture("hue_saturation"),
            "saturation",
            EffectValue::Number(-100.),
            [128, 128, 128],
        ),
        (
            fixture("curves"),
            "curve_0",
            EffectValue::Curve(vec![[0., 1.], [1., 0.]]),
            [0, 255, 255],
        ),
        (
            fixture("levels"),
            "output_white",
            EffectValue::Number(0.5),
            [128, 0, 0],
        ),
        (
            fixture("brightness_contrast"),
            "brightness",
            EffectValue::Number(-50.),
            [128, 0, 0],
        ),
        (
            fixture("exposure"),
            "exposure",
            EffectValue::Number(-1.),
            [188, 0, 0],
        ),
        (
            fixture("vibrance"),
            "saturation",
            EffectValue::Number(-100.),
            [128, 128, 128],
        ),
        (
            fixture("black_white"),
            "reds",
            EffectValue::Number(80.),
            [204, 204, 204],
        ),
        (
            fixture("gradient_map"),
            "gradient",
            EffectValue::Gradient(vec![
                layer_core::GradientStop {
                    position: 0.,
                    color: [0., 0., 1., 1.],
                },
                layer_core::GradientStop {
                    position: 1.,
                    color: [0., 0., 1., 1.],
                },
            ]),
            [0, 0, 255],
        ),
        (
            fixture("posterize"),
            "levels",
            EffectValue::Number(2.),
            [255, 0, 0],
        ),
    ];
    for (kind, key, value, expected) in cases {
        let mut fx = effect(2, kind);
        set(&mut fx, key, value);
        frame(&mut r, &[fx, base.clone()]);
        let actual = pixel(&mut r, 64, 64);
        assert!(
            actual[..3]
                .iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "{} {key}: {actual:?}, expected {expected:?}",
            kind.label()
        );
    }
}

#[test]
fn all_effects_incremental_masks_groups_and_clipping_match_full_recomposition() {
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let mut base = Layer::paint(LayerId(1), "Translucent paint");
    let mut group = Layer::paint(LayerId(20), "Isolated group");
    group.kind = LayerKind::Group;
    base.properties.parent = Some(group.id);
    let mut layers = vec![group];
    for (i, kind) in pointwise_baseline().into_iter().enumerate() {
        let mut fx = effect(i as u64 + 2, kind);
        fx.properties.parent = Some(LayerId(20));
        fx.properties.clipped = true;
        fx.opacity = 0.7;
        if i % 2 == 0 {
            let mut mask = LayerMask::reveal_all(LayerId(40 + i as u64), Point::default());
            mask.default_coverage = 0.4;
            fx.mask = Some(mask);
        }
        layers.push(fx);
    }
    layers.push(base);
    let mut view = test_view();
    view.width_px = 333;
    view.height_px = 291;
    view.background_rgba_linear = [0.; 4];
    let mut dab = test_dab([255., 150.], [0.8, 0.2, 0.1, 0.65], 1.);
    dab.radii = [45.; 2];
    let batch = DabBatch {
        stroke_id: StrokeId(1),
        layer_id: LayerId(1),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 209., y: 104. },
            max: Point { x: 301., y: 196. },
        },
    };
    let render = |r: &mut WgpuRasterizer, all, dabs: &[Dab], batches: &[DabBatch]| {
        r.submit(FramePacket {
            view,
            document_extent: [333, 291],
            layers: &layers,
            dabs,
            dab_batches: batches,
            reset_layers: false,
            time_seconds: 0.,
            composite_all: all,
        })
        .unwrap();
        let mut bytes = vec![0; 333 * 291 * 4];
        r.copy_rgba8_srgb(&mut bytes, 333 * 4).unwrap();
        bytes
    };
    render(&mut r, true, &[], &[]);
    let incremental = render(&mut r, false, &[dab], &[batch]);
    let full = render(&mut r, true, &[], &[]);
    assert_eq!(
        incremental, full,
        "damage crossing a tile boundary must match full composite"
    );
    let p = &full[(150 * 333 + 255) * 4..][..4];
    assert!(
        (160..=170).contains(&p[3]),
        "adjustments and masks preserve base alpha: {p:?}"
    );
    assert_eq!(
        &full[..4],
        &[0; 4],
        "clipped adjustments cannot create coverage"
    );
}
