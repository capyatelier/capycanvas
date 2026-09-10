//! Independent catalog-wide pixel, incremental, animation and performance gates.
use super::*;
use layer_core::{BuiltinEffect, EffectAlpha, EffectValue};
const EXTENT: [u32; 2] = [384, 256];

fn fixture([width, height]: [u32; 2]) -> Vec<u8> {
    // Original test artwork: gradients, curved silhouettes, bright highlights,
    // fine texture and transparent edges. No external image/licensing inputs.
    (0..width * height)
        .flat_map(|i| {
            let x = i % width;
            let y = i / width;
            let u = x as f32 / width as f32;
            let v = y as f32 / height as f32;
            let mut color = [
                0.15 + 0.5 * u,
                0.25 + 0.45 * (1. - v),
                0.65 + 0.2 * (1. - u),
            ];
            if (u - 0.72).hypot(v - 0.22) < 0.095 {
                color = [1., 0.94, 0.67];
            }
            let ridge = 0.52 + 0.08 * (u * 11.).sin() + 0.04 * (u * 29.).cos();
            if v > ridge {
                color = [0.12 + 0.15 * u, 0.30 + 0.3 * (1. - v), 0.22 + 0.15 * u];
            }
            if v > 0.76 + 0.05 * (u * 8.).sin() {
                color = [0.4 + 0.25 * u, 0.20 + 0.1 * v, 0.09];
            }
            if (u - 0.25).abs() < 0.09 && (0.44..0.78).contains(&v) {
                color = [0.82, 0.23, 0.14];
            }
            if (x / 5 + y / 7) % 13 == 0 {
                for c in &mut color {
                    *c = (*c * 0.75 + 0.1).min(1.);
                }
            }
            let noise =
                ((x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)) & 31) as f32 / 255. - 0.06;
            let alpha = if x < 8 || y < 8 || x + 8 >= width || y + 8 >= height {
                0
            } else {
                255
            };
            [
                (255. * (color[0] + noise).clamp(0., 1.)) as u8,
                (255. * (color[1] + noise).clamp(0., 1.)) as u8,
                (255. * (color[2] + noise).clamp(0., 1.)) as u8,
                alpha,
            ]
        })
        .collect()
}
fn setup(r: &mut WgpuRasterizer, extent: [u32; 2]) -> Layer {
    let asset = AssetId("test:filter-library-art".into());
    let bytes = fixture(extent);
    r.prepare_asset(
        &asset,
        HostImage {
            width: extent[0],
            height: extent[1],
            stride: extent[0] * 4,
            format: PixelFormat::Rgba8Srgb,
            bytes: &bytes,
        },
    )
    .unwrap();
    let mut layer = Layer::paint(LayerId(1), "Artwork");
    layer.asset = Some(asset);
    layer
}
fn filter(id: BuiltinEffect) -> Layer {
    let mut layer = Layer::paint(LayerId(2), id.label());
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(id.preview()));
    layer
}
fn submit(
    r: &mut WgpuRasterizer,
    extent: [u32; 2],
    layers: &[Layer],
    time: f32,
    reset: bool,
    all: bool,
    paint: Option<([f32; 2], f32)>,
) {
    let mut dabs = Vec::new();
    let mut batches = Vec::new();
    if let Some((center, radius)) = paint {
        let mut dab = test_dab(center, [0.75, 0.06, 0.8, 1.], 1.);
        dab.radii = [radius; 2];
        dabs.push(dab);
        batches.push(DabBatch {
            stroke_id: StrokeId(77),
            layer_id: LayerId(1),
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: test_style(BrushExecution::Dry),
            damage: Rect {
                min: Point {
                    x: center[0] - radius - 1.,
                    y: center[1] - radius - 1.,
                },
                max: Point {
                    x: center[0] + radius + 1.,
                    y: center[1] + radius + 1.,
                },
            },
        });
    }
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        background_rgba_linear: [0.; 4],
        ..test_view()
    };
    r.submit(FramePacket {
        time_seconds: time,
        view,
        document_extent: extent,
        layers,
        dabs: &dabs,
        dab_batches: &batches,
        reset_layers: reset,
        composite_all: all,
    })
    .unwrap();
}
fn image(r: &mut WgpuRasterizer) -> Vec<u8> {
    r.readback_srgb_rgba8().unwrap()
}
fn png(path: &str, extent: [u32; 2], bytes: &[u8]) {
    let path = std::path::Path::new(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut encoder = png::Encoder::new(std::fs::File::create(path).unwrap(), extent[0], extent[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(bytes)
        .unwrap();
}

#[test]
fn entire_filter_catalog_renders_masks_freezes_and_animates() {
    let mut r = WgpuRasterizer::new().expect("physical GPU required");
    let base = setup(&mut r, EXTENT);
    submit(
        &mut r,
        EXTENT,
        std::slice::from_ref(&base),
        0.,
        true,
        true,
        None,
    );
    let original = image(&mut r);
    let directory = "../../artifacts/filter-library";
    png(&format!("{directory}/source.png"), EXTENT, &original);
    let mut rendered = Vec::new();
    for id in BuiltinEffect::ALL {
        let mut layers = vec![filter(id), base.clone()];
        submit(&mut r, EXTENT, &layers, 0., false, true, None);
        let output = image(&mut r);
        assert_ne!(
            output,
            original,
            "{} preview must demonstrate its effect",
            id.label()
        );
        assert!(
            output.chunks_exact(4).any(|p| p[3] > 0),
            "{} must not erase the image",
            id.label()
        );
        if layers[0].effect.as_ref().unwrap().program.alpha == EffectAlpha::Preserve {
            assert!(
                output
                    .chunks_exact(4)
                    .zip(original.chunks_exact(4))
                    .all(|(a, b)| a[3] == b[3]),
                "{} preserves alpha",
                id.label()
            );
        }
        png(&format!("{directory}/{}.png", id.id()), EXTENT, &output);
        rendered.push(output.clone());
        let before = r.scene.as_ref().map_or([0, 0], |s| s.image_work());
        submit(&mut r, EXTENT, &layers, 20., false, false, None);
        assert_eq!(image(&mut r), output, "{} frozen result", id.label());
        assert_eq!(
            r.scene.as_ref().map_or([0, 0], |s| s.image_work()),
            before,
            "{} frozen frame does no image work",
            id.label()
        );
        layers[0].opacity = 0.;
        submit(&mut r, EXTENT, &layers, 20., false, true, None);
        assert_eq!(
            image(&mut r),
            original,
            "{} zero opacity is identity",
            id.label()
        );
        layers[0].opacity = 1.;
        layers[0].mask = Some(layer_core::LayerMask::reveal_all(
            LayerId(100),
            Point::default(),
        ));
        layers[0].mask.as_mut().unwrap().default_coverage = 0.;
        submit(&mut r, EXTENT, &layers, 20., false, true, None);
        assert_eq!(
            image(&mut r),
            original,
            "{} zero mask is identity",
            id.label()
        );
        layers[0].mask = None;
        layers[0].properties.clipped = true;
        submit(&mut r, EXTENT, &layers, 20., false, true, None);
        assert!(
            image(&mut r)
                .chunks_exact(4)
                .zip(original.chunks_exact(4))
                .all(|(a, b)| a[3] == b[3]),
            "{} clipping preserves base coverage",
            id.label()
        );
        if layers[0].effect.as_ref().unwrap().program.time {
            Arc::make_mut(layers[0].effect.as_mut().unwrap())
                .set("animate", EffectValue::Toggle(true))
                .unwrap();
            submit(&mut r, EXTENT, &layers, 0., false, true, None);
            let first = image(&mut r);
            submit(&mut r, EXTENT, &layers, 1., false, false, None);
            let second = image(&mut r);
            assert_ne!(first, second, "{} animation must change pixels", id.label());
        }
    }
    // Contact-sheet assembly only copies complete GPU-rendered rows.
    let columns = 5;
    let rows = 8;
    let width = EXTENT[0] as usize * columns;
    let height = EXTENT[1] as usize * rows;
    let mut sheet = vec![0; width * height * 4];
    for (i, output) in rendered.iter().enumerate() {
        for y in 0..EXTENT[1] as usize {
            let start = ((i / columns * EXTENT[1] as usize + y) * width
                + i % columns * EXTENT[0] as usize)
                * 4;
            sheet[start..start + EXTENT[0] as usize * 4].copy_from_slice(
                &output[y * EXTENT[0] as usize * 4..(y + 1) * EXTENT[0] as usize * 4],
            );
        }
    }
    png(
        &format!("{directory}/contact-sheet.png"),
        [width as u32, height as u32],
        &sheet,
    );
    std::fs::write(
        format!("{directory}/order.txt"),
        BuiltinEffect::ALL
            .into_iter()
            .map(|id| id.label())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
}

#[test]
fn every_filter_incremental_update_matches_a_forced_full_rebuild() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    for id in BuiltinEffect::ALL {
        let layers = vec![filter(id), base.clone()];
        submit(&mut r, EXTENT, &layers, 0., true, true, None);
        image(&mut r);
        let before = r.scene.as_ref().map_or(0, |s| s.image_pass_pixels());
        submit(
            &mut r,
            EXTENT,
            &layers,
            0.,
            false,
            false,
            Some(([255., 128.], 12.)),
        );
        let incremental = image(&mut r);
        let pixels = r.scene.as_ref().map_or(0, |s| s.image_pass_pixels()) - before;
        if let Some(radius) = layers[0].effect.as_ref().unwrap().damage_radius()
            && radius < 64
            && layers[0].effect.as_ref().unwrap().program.image_boundary()
        {
            assert!(
                pixels < (EXTENT[0] * EXTENT[1]) as u64,
                "{} must recompute its local footprint, got {pixels} pixels",
                id.label()
            );
        }
        // Do not compare the cache with itself: discard every full-image
        // checkpoint while preserving the painted GPU pages.
        if let Some(scene) = &mut r.scene {
            scene.force_image_rebuild();
        }
        submit(&mut r, EXTENT, &layers, 0., false, true, None);
        let rebuilt = image(&mut r);
        assert_eq!(
            incremental,
            rebuilt,
            "{}: local update must match uncached full rendering",
            id.label()
        );
    }
}

/// The full and local cases paint the identical dab into resident artwork.
/// Only the dirty region changes, so the comparison includes tracking overhead
/// without conflating it with allocation, compilation or GPU readback.
#[test]
#[ignore = "release-mode physical GPU benchmark"]
fn filter_library_latency() {
    use std::time::Instant;
    fn summary(mut v: Vec<f64>) -> [f64; 3] {
        v.sort_by(f64::total_cmp);
        [v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() * 99 / 100]]
    }
    let mut r = WgpuRasterizer::new().unwrap();
    let extent = [4096, 4096];
    let base = setup(&mut r, extent);
    let mut report = String::from(
        "filter,mode,cold_ms,cpu_median,cpu_p95,cpu_p99,complete_median,complete_p95,complete_p99,gpu_median,gpu_p95,gpu_p99,image_pixels,cache_bytes\n",
    );
    eprintln!("{:?}", r.adapter.get_info());
    let mut cases: Vec<_> = BuiltinEffect::ALL
        .into_iter()
        .map(|id| (id.label(), vec![filter(id)]))
        .collect();
    cases.insert(0, ("Baseline", vec![]));
    cases.push((
        "Five expensive",
        [
            BuiltinEffect::Denoise,
            BuiltinEffect::Painterly,
            BuiltinEffect::DomainWarp,
            BuiltinEffect::GaussianBlur,
            BuiltinEffect::MotionBlur,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let mut l = filter(id);
            l.id = LayerId(2 + i as u64);
            l
        })
        .collect(),
    ));
    for (label, mut layers) in cases {
        layers.push(base.clone());
        let cold = Instant::now();
        submit(&mut r, extent, &layers, 0., true, true, None);
        r.wait_idle().unwrap();
        let cold = cold.elapsed().as_secs_f64() * 1000.;
        for mode in ["full", "local", "broad", "animation", "cached"] {
            let timed = layers
                .iter()
                .any(|l| l.effect.as_ref().is_some_and(|e| e.program.time));
            if mode == "animation" && !timed {
                continue;
            }
            for l in &mut layers {
                if let Some(e) = l.effect.as_mut().filter(|e| e.program.time) {
                    Arc::make_mut(e)
                        .set("animate", EffectValue::Toggle(mode == "animation"))
                        .unwrap();
                }
            }
            let paint = match mode {
                "cached" | "animation" => None,
                "broad" => Some(([2048.; 2], 1800.)),
                _ => Some(([2048.; 2], 12.)),
            };
            let mut cpu = Vec::new();
            let mut complete = Vec::new();
            let mut pixels = 0;
            r.set_telemetry_enabled(true);
            for i in 0..120 {
                let before = r.scene.as_ref().map_or(0, |s| s.image_pass_pixels());
                let start = Instant::now();
                submit(
                    &mut r,
                    extent,
                    &layers,
                    if mode == "animation" {
                        i as f32 / 120.
                    } else {
                        0.
                    },
                    false,
                    mode == "full",
                    paint,
                );
                let submitted = start.elapsed().as_secs_f64() * 1000.;
                r.wait_idle().unwrap();
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                if i >= 24 {
                    cpu.push(submitted);
                    complete.push(elapsed);
                }
                pixels = r.scene.as_ref().map_or(0, |s| s.image_pass_pixels()) - before;
            }
            let telemetry = r.telemetry();
            let gpu = summary(telemetry.gpu.ordered().into_iter().map(f64::from).collect());
            let cpu = summary(cpu);
            let complete = summary(complete);
            let triple = |x: [f64; 3]| format!("{:.6},{:.6},{:.6}", x[0], x[1], x[2]);
            report.push_str(&format!(
                "{label},{mode},{cold:.3},{},{},{},{pixels},{}\n",
                triple(cpu),
                triple(complete),
                triple(gpu),
                r.scene.as_ref().map_or(0, |s| s.image_cache_bytes())
            ));
            eprintln!("{label}/{mode}: complete={complete:?} gpu={gpu:?} pixels={pixels}");
        }
    }
    std::fs::create_dir_all("../../artifacts/benchmarks").unwrap();
    std::fs::write("../../artifacts/benchmarks/filter-library.csv", report).unwrap();
}

#[test]
fn unrelated_layers_do_not_invalidate_filter_inputs() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let mut source = base.clone();
    source.id = LayerId(3);
    let mut clipped = filter(BuiltinEffect::GaussianBlur);
    clipped.properties.clipped = true;
    let mut grouped = filter(BuiltinEffect::Painterly);
    grouped.properties.parent = Some(LayerId(7));
    let mut child = source.clone();
    child.properties.parent = Some(LayerId(7));
    let mut group = Layer::paint(LayerId(7), "Isolated");
    group.kind = LayerKind::Group;
    for (label, layers) in [
        (
            "above",
            vec![
                base.clone(),
                filter(BuiltinEffect::GaussianBlur),
                source.clone(),
            ],
        ),
        ("below clipping base", vec![clipped, source, base.clone()]),
        ("outside isolated group", vec![group, grouped, child, base]),
    ] {
        submit(&mut r, EXTENT, &layers, 0., true, true, None);
        image(&mut r);
        let before = r.scene.as_ref().unwrap().image_work();
        submit(
            &mut r,
            EXTENT,
            &layers,
            0.,
            false,
            false,
            Some(([255., 128.], 12.)),
        );
        let local = image(&mut r);
        assert_eq!(
            r.scene.as_ref().unwrap().image_work(),
            before,
            "painting {label} is not a filter input"
        );
        r.scene.as_mut().unwrap().force_image_rebuild();
        submit(&mut r, EXTENT, &layers, 0., false, true, None);
        assert_eq!(
            image(&mut r),
            local,
            "{label}: unchanged cache must still be correct"
        );
    }
}

#[test]
fn filter_parameter_limits_are_valid_and_do_not_recompile_shaders() {
    let mut r = WgpuRasterizer::new().unwrap();
    let extent = [64, 64];
    let base = setup(&mut r, extent);
    for id in BuiltinEffect::ALL.into_iter().skip(10) {
        let mut layers = vec![filter(id), base.clone()];
        submit(&mut r, extent, &layers, 0., true, true, None);
        image(&mut r);
        let compiled = r.scene.as_ref().unwrap().effects.compilations;
        let defaults = id.preview();
        for p in defaults.program.parameters.iter() {
            if let layer_core::EffectParameterKind::Number { min, max, .. } = p.kind {
                for value in [min, max] {
                    let mut effect = defaults.clone();
                    effect.set(&p.key, EffectValue::Number(value)).unwrap();
                    effect.validate().unwrap();
                    layers[0].effect = Some(Arc::new(effect));
                    submit(&mut r, extent, &layers, 0., false, true, None);
                    image(&mut r);
                    assert_eq!(
                        r.scene.as_ref().unwrap().effects.compilations,
                        compiled,
                        "{} {} changes uniforms/tables, never the pipeline",
                        id.label(),
                        p.key
                    );
                }
            }
        }
    }
}

#[test]
fn expensive_filter_chain_incremental_matches_full_at_document_and_tile_edges() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let mut layers: Vec<_> = [
        BuiltinEffect::Denoise,
        BuiltinEffect::Painterly,
        BuiltinEffect::DomainWarp,
        BuiltinEffect::GaussianBlur,
        BuiltinEffect::MotionBlur,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, id)| {
        let mut l = filter(id);
        l.id = LayerId(2 + i as u64);
        l
    })
    .collect();
    layers.push(base);
    submit(&mut r, EXTENT, &layers, 0., true, true, None);
    image(&mut r);
    for point in [[255., 128.], [2., 2.], [380., 252.]] {
        submit(
            &mut r,
            EXTENT,
            &layers,
            0.,
            false,
            false,
            Some((point, 12.)),
        );
        let local = image(&mut r);
        r.scene.as_mut().unwrap().force_image_rebuild();
        submit(&mut r, EXTENT, &layers, 0., false, true, None);
        assert_eq!(image(&mut r), local, "chained neighborhoods at {point:?}");
    }
}
