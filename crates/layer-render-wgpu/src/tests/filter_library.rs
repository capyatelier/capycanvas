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

/// Immutable pre-migration reference: sample actual full-resolution renders,
/// including masked/clipped transparency, not a CPU reimplementation.
#[test]
fn runtime_filter_pixel_reference() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let sample = [64usize, 48usize];
    let columns = 8;
    let rows = (BuiltinEffect::ALL.len() * 4).div_ceil(columns);
    let extent = [(columns * sample[0]) as u32, (rows * sample[1]) as u32];
    let mut pixels = vec![0; extent[0] as usize * extent[1] as usize * 4];
    for (i, id) in BuiltinEffect::ALL.into_iter().enumerate() {
        for scope in 0..4 {
            let mut effect = filter(id);
            effect.properties.clipped = scope == 1 || scope == 3;
            if scope >= 2 {
                effect.opacity = 0.63;
                let mut mask = layer_core::LayerMask::reveal_all(LayerId(100), Point::default());
                mask.default_coverage = 0.47;
                effect.mask = Some(mask);
            }
            if scope == 3 && effect.effect.as_ref().unwrap().program.time {
                Arc::make_mut(effect.effect.as_mut().unwrap())
                    .set("animate", EffectValue::Toggle(true))
                    .unwrap();
            }
            submit(
                &mut r,
                EXTENT,
                &[effect, base.clone()],
                2.5,
                i == 0 && scope == 0,
                true,
                None,
            );
            let output = image(&mut r);
            let index = i * 4 + scope;
            for y in 0..sample[1] {
                for x in 0..sample[0] {
                    let sx = (2 * x + 1) * EXTENT[0] as usize / (2 * sample[0]);
                    let sy = (2 * y + 1) * EXTENT[1] as usize / (2 * sample[1]);
                    let src = (sy * EXTENT[0] as usize + sx) * 4;
                    let dst = ((index / columns * sample[1] + y) * extent[0] as usize
                        + index % columns * sample[0]
                        + x)
                        * 4;
                    pixels[dst..dst + 4].copy_from_slice(&output[src..src + 4]);
                }
            }
        }
    }
    let path = "tests/fixtures/runtime-filters-v2.png";
    let mut reader = png::Decoder::new(std::fs::File::open(path).unwrap())
        .read_info()
        .unwrap();
    let mut reference = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut reference).unwrap();
    assert_eq!([info.width, info.height], extent);
    let error = reference
        .iter()
        .zip(&pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(
        error <= 1,
        "Runtime migration changed reference pixels: maximum byte error {error}"
    );
}

#[test]
fn gpu_preparation_is_shared_and_dependency_driven() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let mut effect = filter(BuiltinEffect::UnsharpMask);
    let old = effect.effect.take().unwrap();
    let program = Arc::new((*old.program).clone().with_time_controls());
    effect.effect = Some(Arc::new(layer_core::EffectInstance::new(program)));
    let mut layers = vec![effect, base];
    let count = |r: &WgpuRasterizer| r.scene.as_ref().unwrap().effects.preparation_count();
    submit(&mut r, EXTENT, &layers, 0., true, true, None);
    assert_eq!(count(&r), 1, "two image passes share one preparation");
    let bytes = r.scene.as_ref().unwrap().effects.storage_bytes();
    for i in 0..5 {
        submit(
            &mut r,
            EXTENT,
            &layers,
            i as f32,
            false,
            false,
            Some(([192., 128.], 12.)),
        );
        assert_eq!(count(&r), 1, "painting and animation reuse the lookup");
    }
    r.submit(FramePacket {
        time_seconds: 8.,
        view: ViewState {
            width_px: EXTENT[0],
            height_px: EXTENT[1],
            document_to_surface: [1.5, 0., 0., 1.5, 20., 10.],
            ..test_view()
        },
        document_extent: EXTENT,
        layers: &layers,
        dabs: &[],
        dab_batches: &[],
        reset_layers: false,
        composite_all: true,
    })
    .unwrap();
    assert_eq!(count(&r), 1, "panning/zoom must not prepare");
    Arc::make_mut(layers[0].effect.as_mut().unwrap())
        .set("amount", EffectValue::Number(175.))
        .unwrap();
    submit(&mut r, EXTENT, &layers, 9., false, true, None);
    assert_eq!(count(&r), 1, "unrelated value edit must not prepare");
    layers[0].opacity = 0.7;
    submit(&mut r, EXTENT, &layers, 10., false, true, None);
    assert_eq!(count(&r), 1, "layer properties must not prepare");
    Arc::make_mut(layers[0].effect.as_mut().unwrap())
        .set("sigma", EffectValue::Number(8.))
        .unwrap();
    submit(&mut r, EXTENT, &layers, 11., false, true, None);
    assert_eq!(count(&r), 2, "relevant edit prepares exactly once");
    let effect = Arc::make_mut(layers[0].effect.as_mut().unwrap());
    let program = Arc::make_mut(&mut effect.program);
    program.wgsl = format!("{}\n// render-only revision\n", program.wgsl).into();
    submit(&mut r, EXTENT, &layers, 12., false, true, None);
    assert_eq!(count(&r), 2, "render-only code retains preparation");
    let effect = Arc::make_mut(layers[0].effect.as_mut().unwrap());
    let lookup = &mut Arc::make_mut(&mut Arc::make_mut(&mut effect.program).lookups)[0];
    lookup.wgsl = format!("{}\n// preparation revision\n", lookup.wgsl).into();
    submit(&mut r, EXTENT, &layers, 13., false, true, None);
    assert_eq!(count(&r), 3, "preparation code edit prepares once");
    assert_eq!(
        r.scene.as_ref().unwrap().effects.storage_bytes(),
        bytes,
        "no fresh GPU parameter storage on edits"
    );
}

#[test]
fn custom_preparation_replaces_kernel_at_runtime() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let mut layers = vec![filter(BuiltinEffect::GaussianBlur), base];
    submit(&mut r, EXTENT, &layers, 0., true, true, None);
    let gaussian = image(&mut r);
    let effect = Arc::make_mut(layers[0].effect.as_mut().unwrap());
    let lookup = &mut Arc::make_mut(&mut Arc::make_mut(&mut effect.program).lookups)[0];
    // Runtime-authored triangular kernel: the consumer's tap ABI is unchanged,
    // but no host-side algorithm or compiled preparation selector is involved.
    lookup.wgsl = std::fs::read_to_string("tests/fixtures/triangle-prepare.wgsl")
        .unwrap()
        .into();
    lookup.entry = "triangle".into();
    lookup.workgroup_size = [1, 1, 1];
    submit(&mut r, EXTENT, &layers, 0., false, true, None);
    assert_ne!(image(&mut r), gaussian);
    assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(), 2);
    let expected = image(&mut r);
    submit(&mut r, EXTENT, &layers, 0., true, true, None);
    assert_eq!(
        image(&mut r),
        expected,
        "runtime kernel agrees with fresh composition"
    );
}

#[test]
fn prepared_pointwise_filters_still_fuse() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let mut program = (*BuiltinEffect::BrightnessContrast.program()).clone();
    program.id = "runtime_factor".into();
    program.entry = "runtime_factor".into();
    program.wgsl="fn runtime_factor(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(c.rgb*fx_lookup(b,0u,0u).x,c.a);}".into();
    program.lookups=Arc::from([layer_core::EffectLookup {
        wgsl:"fn prepare_factor(local:vec3<u32>,global:vec3<u32>){prep_store(0u,prep_parameter(0u,0u)/100.);}".into(),
        entry:"prepare_factor".into(),dependencies:Arc::from([Arc::from("brightness")]),values:1,workgroup_size:[1,1,1],workgroups:[1,1,1],
    }]);
    let mut first = filter(BuiltinEffect::BrightnessContrast);
    first.effect = Some(Arc::new(layer_core::EffectInstance::new(Arc::new(program))));
    Arc::make_mut(first.effect.as_mut().unwrap())
        .set("brightness", EffectValue::Number(50.))
        .unwrap();
    let mut second = first.clone();
    second.id = LayerId(3);
    Arc::make_mut(second.effect.as_mut().unwrap())
        .set("brightness", EffectValue::Number(80.))
        .unwrap();
    submit(
        &mut r,
        EXTENT,
        &[first.clone(), second, base.clone()],
        0.,
        true,
        true,
        None,
    );
    assert_eq!(
        r.scene.as_ref().unwrap().effects.compilations,
        1,
        "preparation must not introduce image boundaries"
    );
    assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(), 2);
    let pair = image(&mut r);
    Arc::make_mut(first.effect.as_mut().unwrap())
        .set("brightness", EffectValue::Number(40.))
        .unwrap();
    submit(&mut r, EXTENT, &[first, base], 0., false, true, None);
    assert!(
        image(&mut r)
            .iter()
            .zip(pair)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
}

#[test]
fn invalid_preparation_keeps_the_working_gpu_state() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    let effect = filter(BuiltinEffect::GaussianBlur);
    let layers = vec![effect.clone(), base];
    submit(&mut r, EXTENT, &layers, 0., true, true, None);
    let expected = image(&mut r);
    for source in [
        "fn bad(local:vec3<u32>,global:vec3<u32>){this is not WGSL;}",
        "fn bad(local:vec3<u32>,global:vec3<u32>){prep_data[0u]=vec4<f32>(0.);}",
        "@group(0) @binding(1) var<storage,read> extra:array<f32>; fn bad(local:vec3<u32>,global:vec3<u32>){prep_store(0u,vec4<f32>(extra[0u]));}",
    ] {
        let mut candidate = effect.clone();
        let lookup = &mut Arc::make_mut(
            &mut Arc::make_mut(&mut Arc::make_mut(candidate.effect.as_mut().unwrap()).program)
                .lookups,
        )[0];
        lookup.wgsl = source.into();
        lookup.entry = "bad".into();
        let mut scene = r.scene.take().unwrap();
        assert!(
            scene
                .effects
                .prepare(&r, &[&candidate], effects::Execution::Image(0), 0.)
                .is_err()
        );
        assert_eq!(scene.effects.preparation_count(), 1);
        r.scene = Some(scene);
        submit(&mut r, EXTENT, &layers, 0., false, true, None);
        assert_eq!(image(&mut r), expected);
    }
}

#[test]
fn gpu_gaussian_is_normalized_at_parameter_extremes() {
    let mut r = WgpuRasterizer::new().unwrap();
    let extent = [128, 128];
    let asset = AssetId("test:uniform-normalization".into());
    let pixels: [u8; 4] = [140, 90, 180, 255];
    r.prepare_asset(
        &asset,
        HostImage {
            width: 128,
            height: 128,
            stride: 128 * 4,
            format: PixelFormat::Rgba8Srgb,
            bytes: &pixels.repeat(128 * 128),
        },
    )
    .unwrap();
    let mut base = Layer::paint(LayerId(1), "Uniform");
    base.asset = Some(asset);
    let mut layers = vec![filter(BuiltinEffect::GaussianBlur), base];
    for (i, sigma) in [0., 0.000001, 0.1, 0.5, 1., 3., 12., 21.]
        .into_iter()
        .enumerate()
    {
        Arc::make_mut(layers[0].effect.as_mut().unwrap())
            .set("sigma", EffectValue::Number(sigma))
            .unwrap();
        submit(&mut r, extent, &layers, 0., i == 0, true, None);
        assert!(
            image(&mut r)
                .chunks_exact(4)
                .all(|p| p.iter().zip(pixels).all(|(a, b)| a.abs_diff(b) <= 1)),
            "unnormalized Gaussian at sigma {sigma}"
        );
    }
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
fn cached_clipping_matches_tiled_composition() {
    let mut r = WgpuRasterizer::new().unwrap();
    let base = setup(&mut r, EXTENT);
    for case in 0..18 {
        let mut base = base.clone();
        base.opacity = 0.63;
        base.properties.blend = layer_core::LayerBlend::Multiply;
        base.mask = Some(layer_core::LayerMask::reveal_all(
            LayerId(90),
            Point::default(),
        ));
        base.mask.as_mut().unwrap().default_coverage = 0.7;
        let mut effect = filter(BuiltinEffect::HeatHaze);
        effect.properties.clipped = true;
        effect.opacity = 0.54;
        effect.mask = Some(layer_core::LayerMask::reveal_all(
            LayerId(91),
            Point::default(),
        ));
        effect.mask.as_mut().unwrap().default_coverage = 0.43;
        Arc::make_mut(effect.effect.as_mut().unwrap())
            .set("animate", EffectValue::Toggle(true))
            .unwrap();
        let mut layers = vec![effect, base];
        if case >= 3 {
            let mut clip = filter(BuiltinEffect::GaussianBlur);
            clip.id = LayerId(4);
            clip.properties.clipped = true;
            layers.insert(1, clip);
        }
        if case >= 6 {
            let mut backdrop = setup(&mut r, EXTENT);
            backdrop.id = LayerId(3);
            layers.push(backdrop);
        }
        if (9..12).contains(&case) {
            for layer in &mut layers {
                layer.properties.parent = Some(LayerId(7));
            }
            let mut group = Layer::paint(LayerId(7), "Isolated group");
            group.kind = LayerKind::Group;
            group.opacity = 0.73;
            layers.insert(0, group);
        }
        if case >= 12 {
            let mut upper = filter(if case < 15 {
                BuiltinEffect::Curves
            } else {
                BuiltinEffect::GaussianBlur
            });
            upper.id = LayerId(8);
            upper.properties.clipped = case < 15;
            layers.insert(0, upper);
        }
        if case % 3 == 1 {
            layers
                .iter_mut()
                .find(|l| l.id == LayerId(1))
                .unwrap()
                .visible = false;
        }
        if case % 3 == 2 {
            layers
                .iter_mut()
                .find(|l| l.id == LayerId(2))
                .unwrap()
                .mask
                .as_mut()
                .unwrap()
                .show_area = true;
        }
        let render = |r: &mut WgpuRasterizer, reset, time| {
            r.submit(FramePacket {
                time_seconds: time,
                view: ViewState {
                    width_px: EXTENT[0],
                    height_px: EXTENT[1],
                    background_rgba_linear: [0.6, 0.3, 0.15, 0.8],
                    ..test_view()
                },
                document_extent: EXTENT,
                layers: &layers,
                dabs: &[],
                dab_batches: &[],
                reset_layers: reset,
                composite_all: true,
            })
            .unwrap();
        };
        render(&mut r, true, 0.);
        for time in [0., 0.5] {
            r.scene.as_mut().unwrap().set_tiled_composition(false);
            render(&mut r, false, time);
            let optimized = image(&mut r);
            r.scene.as_mut().unwrap().set_tiled_composition(true);
            render(&mut r, false, time);
            let reference = image(&mut r);
            let max_error = optimized
                .iter()
                .zip(&reference)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                max_error <= 1,
                "case {case}, time {time}: {max_error} byte error"
            );
        }
    }
}

#[test]
fn painting_backdrop_updates_only_dirty_tiles_without_rerunning_frozen_filter() {
    let mut r = WgpuRasterizer::new().unwrap();
    let backdrop = setup(&mut r, EXTENT);
    let mut base = backdrop.clone();
    base.id = LayerId(3);
    base.opacity = 0.6;
    let mut heat = filter(BuiltinEffect::HeatHaze);
    heat.properties.clipped = true;
    let mut layers = vec![heat, base, backdrop];
    submit(&mut r, EXTENT, &layers, 0., true, true, None);
    image(&mut r);
    for animate in [false, true] {
        Arc::make_mut(layers[0].effect.as_mut().unwrap())
            .set("animate", EffectValue::Toggle(animate))
            .unwrap();
        submit(&mut r, EXTENT, &layers, 0., false, false, None);
        image(&mut r);
        let storage = r.scene.as_ref().unwrap().image_cache_bytes();
        for (i, point) in [[40., 40.], [255., 128.], [380., 250.]]
            .into_iter()
            .enumerate()
        {
            let work = r.scene.as_ref().unwrap().composition_work();
            let filter = r.scene.as_ref().unwrap().image_work();
            let time = (i + 1) as f32 / 120.;
            submit(
                &mut r,
                EXTENT,
                &layers,
                time,
                false,
                false,
                Some((point, 8.)),
            );
            let local = image(&mut r);
            let after = r.scene.as_ref().unwrap().composition_work();
            assert_eq!(after[0] - work[0], 1, "refresh backdrop once per update");
            let rect = PixelRect {
                min_x: (point[0] - 9.).max(0.) as u32,
                min_y: (point[1] - 9.).max(0.) as u32,
                max_x: (point[0] + 9.) as u32,
                max_y: (point[1] + 9.) as u32,
            }
            .intersect(PixelRect::full(EXTENT));
            let tile_pixels: u64 = page_coordinates(rect)
                .map(|p| page_rect(p).intersect(PixelRect::full(EXTENT)).area())
                .sum();
            assert_eq!(
                after[1] - work[1],
                tile_pixels,
                "refresh only the touched tiles"
            );
            assert_eq!(after[3], work[3], "keep composition bindings and buffers");
            assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), storage);
            let updated_filter = r.scene.as_ref().unwrap().image_work();
            assert_eq!(
                updated_filter[0], filter[0],
                "backdrop is not the filter input"
            );
            assert_eq!(updated_filter[1] - filter[1], u64::from(animate));
            if !animate {
                assert!(after[2] - work[2] < 4096, "blend only local damage");
            }
            r.scene.as_mut().unwrap().force_image_rebuild();
            submit(&mut r, EXTENT, &layers, time, false, true, None);
            assert_eq!(
                image(&mut r),
                local,
                "backdrop paint at {point:?}, animated={animate}"
            );
        }
    }
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
fn clipped_animation_latency() {
    use std::time::Instant;
    let summary = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        format!(
            "{:.6},{:.6},{:.6}",
            v[v.len() / 2],
            v[v.len() * 95 / 100],
            v[v.len() * 99 / 100]
        )
    };
    let mut r = WgpuRasterizer::new().unwrap();
    let mut report = String::from(
        "extent,backdrop,clipped,mode,cpu_median,cpu_p95,cpu_p99,gpu_median,gpu_p95,gpu_p99,complete_median,complete_p95,complete_p99,backdrop_pixels,filter_pixels,cache_bytes\n",
    );
    for extent in [[2048, 1536], [4096, 4096]] {
        let base = setup(&mut r, extent);
        for (backdrop, clipped) in [(false, false), (false, true), (true, false), (true, true)] {
            for mode in [
                "animation",
                "base-paint",
                "backdrop-paint",
                "backdrop-paint-animated",
            ] {
                if !backdrop && mode != "animation" {
                    continue;
                }
                let animate = matches!(mode, "animation" | "backdrop-paint-animated");
                let painting_backdrop = mode.starts_with("backdrop-paint");
                let mut effect = filter(BuiltinEffect::HeatHaze);
                effect.properties.clipped = clipped;
                Arc::make_mut(effect.effect.as_mut().unwrap())
                    .set("animate", EffectValue::Toggle(animate))
                    .unwrap();
                let mut layers = vec![effect, base.clone()];
                if backdrop {
                    layers[1].opacity = 0.65;
                    for i in 0..3 {
                        let mut l = base.clone();
                        l.id = LayerId(3 + i);
                        l.opacity = 0.7;
                        l.properties.blend = layer_core::LayerBlend::Multiply;
                        layers.push(l);
                    }
                }
                if painting_backdrop {
                    layers[1].id = LayerId(3);
                    layers[2].id = LayerId(1);
                }
                submit(&mut r, extent, &layers, 0., true, true, None);
                r.wait_idle().unwrap();
                r.set_telemetry_enabled(true);
                let (mut cpu, mut complete) = (Vec::new(), Vec::new());
                let (mut backdrop_pixels, mut filter_pixels) = (0, 0);
                for i in 0..320 {
                    let scene = r.scene.as_ref().unwrap();
                    let before = [scene.composition_work()[1], scene.image_pass_pixels()];
                    let start = Instant::now();
                    // Cross a tile boundary, so incremental tests cover multiple
                    // dirty tiles and neighborhood expansion, not a best case.
                    let paint = (mode != "animation")
                        .then_some(([extent[0] as f32 / 2., extent[1] as f32 / 2.], 12.));
                    submit(
                        &mut r,
                        extent,
                        &layers,
                        i as f32 / 120.,
                        false,
                        false,
                        paint,
                    );
                    let ms = start.elapsed().as_secs_f64() * 1000.;
                    r.wait_idle().unwrap();
                    if i >= 64 {
                        cpu.push(ms);
                        complete.push(start.elapsed().as_secs_f64() * 1000.);
                    }
                    let scene = r.scene.as_ref().unwrap();
                    backdrop_pixels = scene.composition_work()[1] - before[0];
                    filter_pixels = scene.image_pass_pixels() - before[1];
                }
                let gpu = r
                    .telemetry()
                    .gpu
                    .ordered()
                    .into_iter()
                    .map(f64::from)
                    .collect();
                let line = format!(
                    "{}x{},{backdrop},{clipped},{mode},{},{},{},{backdrop_pixels},{filter_pixels},{}\n",
                    extent[0],
                    extent[1],
                    summary(cpu),
                    summary(gpu),
                    summary(complete),
                    r.scene.as_ref().unwrap().image_cache_bytes()
                );
                eprint!("{line}");
                report.push_str(&line);
            }
        }
    }
    std::fs::create_dir_all("../../artifacts/benchmarks").unwrap();
    std::fs::write("../../artifacts/benchmarks/clipped-animation.csv", report).unwrap();
}

#[test]
#[ignore = "release-mode physical GPU benchmark"]
fn filter_parameter_latency() {
    use std::time::Instant;
    let summary = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        format!(
            "{:.6},{:.6},{:.6}",
            v[v.len() / 2],
            v[v.len() * 95 / 100],
            v[v.len() * 99 / 100]
        )
    };
    let mut r = WgpuRasterizer::new().unwrap();
    let extent = [2048, 1536];
    let base = setup(&mut r, extent);
    let mut report = String::from(
        "filter,mode,cpu_median,cpu_p95,cpu_p99,gpu_median,gpu_p95,gpu_p99,complete_median,complete_p95,complete_p99\n",
    );
    for (label, filters) in [
        ("Unsharp", vec![BuiltinEffect::UnsharpMask]),
        (
            "Five prepared",
            vec![
                BuiltinEffect::Pencil,
                BuiltinEffect::SoftFocus,
                BuiltinEffect::Bloom,
                BuiltinEffect::GaussianBlur,
                BuiltinEffect::UnsharpMask,
            ],
        ),
    ] {
        for mode in ["paint", "relevant", "unrelated"] {
            let mut layers: Vec<_> = filters
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let mut l = filter(*id);
                    l.id = LayerId(i as u64 + 2);
                    l
                })
                .collect();
            layers.push(base.clone());
            submit(&mut r, extent, &layers, 0., true, true, None);
            r.wait_idle().unwrap();
            r.set_telemetry_enabled(true);
            let (mut cpu, mut complete) = (Vec::new(), Vec::new());
            for i in 0..320 {
                let start = Instant::now();
                let effect = Arc::make_mut(layers[filters.len() - 1].effect.as_mut().unwrap());
                match mode {
                    "relevant" => effect
                        .set("sigma", EffectValue::Number(1. + (i % 80) as f32 * 0.1))
                        .unwrap(),
                    "unrelated" => effect
                        .set("amount", EffectValue::Number(50. + (i % 80) as f32))
                        .unwrap(),
                    _ => {}
                }
                submit(
                    &mut r,
                    extent,
                    &layers,
                    0.,
                    false,
                    mode != "paint",
                    (mode == "paint").then_some(([1024., 768.], 12.)),
                );
                let submitted = start.elapsed().as_secs_f64() * 1000.;
                r.wait_idle().unwrap();
                if i >= 64 {
                    cpu.push(submitted);
                    complete.push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
            let gpu = r
                .telemetry()
                .gpu
                .ordered()
                .into_iter()
                .map(f64::from)
                .collect();
            let line = format!(
                "{label},{mode},{},{},{}\n",
                summary(cpu),
                summary(gpu),
                summary(complete)
            );
            eprint!("{line}");
            report.push_str(&line);
        }
    }
    let name = std::env::var("CAPY_FILTER_BENCHMARK_LABEL").unwrap_or_else(|_| "current".into());
    assert!(name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'));
    std::fs::create_dir_all("../../artifacts/benchmarks").unwrap();
    std::fs::write(
        format!("../../artifacts/benchmarks/filter-parameters-{name}.csv"),
        report,
    )
    .unwrap();
}

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
