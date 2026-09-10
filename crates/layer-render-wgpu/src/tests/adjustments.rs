use super::*;
use layer_core::{BuiltinEffect, EffectInstance, EffectValue, LayerMask, Selection};

fn effect(id: u64, kind: BuiltinEffect) -> Layer {
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
fn frame(r: &mut WgpuRasterizer, layers: &[Layer]) {
    r.submit(FramePacket {
        view: test_view(),
        document_extent: [128, 128],
        layers,
        dabs: &[],
        dab_batches: &[],
        reset_layers: false,
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
    for kind in BuiltinEffect::ALL {
        if matches!(
            kind,
            BuiltinEffect::BlackWhite | BuiltinEffect::GradientMap | BuiltinEffect::Posterize
        ) {
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
    let mut fx = effect(2, BuiltinEffect::BrightnessContrast);
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
    let mut layers: Vec<_> = BuiltinEffect::ALL
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
        let mut fx = effect(i + 2, BuiltinEffect::BrightnessContrast);
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
            for case in 0..=BuiltinEffect::ALL.len() + 4 {
                let kind = case
                    .checked_sub(1)
                    .and_then(|i| BuiltinEffect::ALL.get(i))
                    .copied();
                let chain = case > BuiltinEffect::ALL.len();
                let masked = case == BuiltinEffect::ALL.len() + 2;
                let clipped = case == BuiltinEffect::ALL.len() + 3;
                let telemetry = case != BuiltinEffect::ALL.len() + 4;
                let label = if !chain {
                    kind.map_or("Baseline", BuiltinEffect::label)
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
                    for (i, kind) in BuiltinEffect::ALL.into_iter().enumerate() {
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
        abi:EFFECT_ABI,id:"test_gradient".into(),label:"Test gradient".into(),kind:EffectKind::Generator,
        entry:"test_gradient".into(),parameters:Arc::from([]),constraints:Arc::from([]),
        wgsl:"fn test_gradient(c:vec4<f32>,position:vec2<f32>,base:u32)->vec4<f32>{return vec4<f32>(position.x/333.,position.y/291.,.25,.5)*vec4<f32>(.5,.5,.5,1.);}".into(),
    }))));
    let mut fx = effect(2, BuiltinEffect::HueSaturation);
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
            BuiltinEffect::HueSaturation,
            "hue",
            EffectValue::Number(120.),
            [0, 255, 0],
        ),
        (
            BuiltinEffect::HueSaturation,
            "saturation",
            EffectValue::Number(-100.),
            [128, 128, 128],
        ),
        (
            BuiltinEffect::Curves,
            "curve_0",
            EffectValue::Curve(vec![[0., 1.], [1., 0.]]),
            [0, 255, 255],
        ),
        (
            BuiltinEffect::Levels,
            "output_white",
            EffectValue::Number(0.5),
            [128, 0, 0],
        ),
        (
            BuiltinEffect::BrightnessContrast,
            "brightness",
            EffectValue::Number(-50.),
            [128, 0, 0],
        ),
        (
            BuiltinEffect::Exposure,
            "exposure",
            EffectValue::Number(-1.),
            [188, 0, 0],
        ),
        (
            BuiltinEffect::Vibrance,
            "saturation",
            EffectValue::Number(-100.),
            [128, 128, 128],
        ),
        (
            BuiltinEffect::BlackWhite,
            "reds",
            EffectValue::Number(80.),
            [204, 204, 204],
        ),
        (
            BuiltinEffect::GradientMap,
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
            BuiltinEffect::Posterize,
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
    for (i, kind) in BuiltinEffect::ALL.into_iter().enumerate() {
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
