//! Physical working-target tests; native document adoption remains separate.
use super::*;
use crate::native_tiles::{NativeEncodeStatus, NativeTileEncoder, NativeTileRequest};
use layer_core::color::{IntegerDepth, RgbSpace, source::*};

fn native_texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Float32 working publication fixture"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        dimension: wgpu::TextureDimension::D2,
        format,
        mip_level_count: 1,
        sample_count: 1,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn packet<'a>(
    layers: &'a [Layer],
    dabs: &'a [Dab],
    batches: &'a [DabBatch],
    reset: bool,
) -> FramePacket<'a> {
    FramePacket {
        view: ViewState {
            width_px: 256,
            height_px: 256,
            background_rgba_linear: [0.; 4],
            ..test_view()
        },
        document_extent: [256; 2],
        layers,
        dabs,
        dab_batches: batches,
        restore_rasters: &[],
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: true,
    }
}
fn ramp(alpha: u16) -> (Layer, Vec<u8>) {
    let mut builder = SourceBuilder::new(
        [256; 2],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: Default::default(),
            profile_assumed: false,
        },
        4 * 1024 * 1024,
    )
    .unwrap();
    let mut reference = Vec::new();
    for y in 0..256u32 {
        let row: Vec<_> = (0..256u32)
            .flat_map(|x| {
                [
                    (x + y * 256) as u16,
                    ((x + y * 256) * 101) as u16,
                    ((x + y * 256) * 237) as u16,
                    alpha,
                ]
            })
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
        reference.extend(row);
    }
    let mut layer = Layer::paint(LayerId(1), "native ramp");
    layer.source = Some(Arc::new(builder.finish().unwrap()));
    (layer, reference)
}
#[test]
fn float32_working_targets_preserve_integer16_through_scene_and_native_publication() {
    let mut r = WgpuRasterizer::new_float32().unwrap();
    let native = NativeTileEncoder::with_device(&r.device);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let encoded = native_texture(&r, wgpu::TextureFormat::Rgba16Uint);
    let canonical = native_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let status = NativeEncodeStatus::new(&r.device);
    for alpha in [1u16, 2, 17, 32768, 65535] {
        let (layer, reference) = ramp(alpha);
        r.submit(packet(std::slice::from_ref(&layer), &[], &[], true))
            .unwrap();
        let composite = r.composite_texture.as_ref().unwrap();
        assert_eq!(composite.format(), wgpu::TextureFormat::Rgba32Float);
        let batch = native
            .prepare(
                &r.device,
                &[NativeTileRequest {
                    working: composite,
                    encoded: &encoded,
                    canonical: &canonical,
                    transfer: &transfer,
                    depth: IntegerDepth::U16,
                    alpha: layer_core::color::AlphaAssociation::Straight,
                    region: [0, 0, 256, 256],
                }],
                &status,
            )
            .unwrap();
        let mut commands = r.device.create_command_encoder(&Default::default());
        status.reset(&mut commands);
        {
            let mut pass = commands.begin_compute_pass(&Default::default());
            native.encode(&mut pass, &batch);
        }
        r.queue.submit([commands.finish()]);
        let actual = crate::layer_tests::page_bytes(&r, &encoded);
        let max_error = actual
            .chunks_exact(2)
            .zip(reference.chunks_exact(2))
            .map(|(a, b)| {
                u16::from_le_bytes(a.try_into().unwrap())
                    .abs_diff(u16::from_le_bytes(b.try_into().unwrap()))
            })
            .max()
            .unwrap();
        assert_eq!(max_error, 0, "scene narrowed alpha={alpha}");
        assert_eq!(r.metrics.composite_storage_bytes, 256 * 256 * 16);
    }
}

#[test]
fn float32_brush_surfaces_keep_low_flow_accumulation_and_actual_residency() {
    let mut r = WgpuRasterizer::new_float32().unwrap();
    let layer = Layer::paint(LayerId(1), "low flow");
    let color = [0.00012345, 0.13712345, 0.7931234, 1.];
    let alpha = 1. / 65535.;
    let mut dab = test_dab([128., 128.], color, alpha);
    dab.radii = [120.; 2];
    let mut batch = DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: layer.id,
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: false,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 256., y: 256. },
        },
    };
    for frame in 0..128 {
        batch.stroke_start = frame == 0;
        r.submit(packet(
            std::slice::from_ref(&layer),
            std::slice::from_ref(&dab),
            std::slice::from_ref(&batch),
            frame == 0,
        ))
        .unwrap();
    }
    let page = r.paint_layers[0].pages[0].active();
    assert_eq!(page.texture.format(), wgpu::TextureFormat::Rgba32Float);
    assert_eq!(
        r.reservoir.primary.texture.format(),
        wgpu::TextureFormat::Rgba32Float
    );
    assert_eq!(
        r.reservoir.secondary.texture.format(),
        wgpu::TextureFormat::Rgba32Float
    );
    let bytes = crate::layer_tests::page_bytes(&r, &page.texture);
    let pixel = &bytes[(128 * 256 + 128) * 16..][..16];
    let actual: [f32; 4] =
        std::array::from_fn(|c| f32::from_ne_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap()));
    let expected_alpha = 1. - (1. - f64::from(alpha)).powi(128);
    assert!(
        (f64::from(actual[3]) - expected_alpha).abs() < 0.00000003,
        "alpha {actual:?} expected {expected_alpha}"
    );
    for c in 0..3 {
        assert!(
            (f64::from(actual[c]) - f64::from(color[c]) * expected_alpha).abs() < 0.00000003,
            "channel {c}: {actual:?}"
        );
    }
    // Uniform accumulation remembers the greatest coverage in the contact.
    // Quantizing that state to R8 would forget each tiny dab and overpaint.
    batch.style.rendering.accumulation = BrushAccumulation::Uniform;
    for frame in 0..128 {
        batch.stroke_start = frame == 0;
        r.submit(packet(
            std::slice::from_ref(&layer),
            std::slice::from_ref(&dab),
            std::slice::from_ref(&batch),
            frame == 0,
        ))
        .unwrap();
    }
    let page = r.paint_layers[0].pages[0].active();
    let bytes = crate::layer_tests::page_bytes(&r, &page.texture);
    let pixel = &bytes[(128 * 256 + 128) * 16..][..16];
    let uniform_alpha = f32::from_ne_bytes(pixel[12..16].try_into().unwrap());
    assert!(
        (uniform_alpha - alpha).abs() < 0.00000003,
        "uniform coverage narrowed: {uniform_alpha}, expected {alpha}"
    );
    assert_eq!(r.metrics.paint_storage_bytes, 256 * 256 * 16);
    assert_eq!(r.metrics.composite_storage_bytes, 256 * 256 * 16);
}

#[test]
fn float32_material_paths_keep_identity_samples_and_working_state_precision() {
    let mut r = WgpuRasterizer::new_float32().unwrap();
    let (layer, reference) = ramp(65535);
    for execution in [
        BrushExecution::Dry,
        BrushExecution::Smudge,
        BrushExecution::Wet,
        BrushExecution::Liquify,
        BrushExecution::Watercolor,
    ] {
        let mut dab = test_dab([128., 128.], [0.12, 0.34, 0.56, 1.], 0.);
        dab.radii = [120.; 2];
        dab.material = [0.; 4];
        let mut style = test_style(execution);
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.wet_mix.wetness = 0.3;
        style.wet_mix.amount_of_paint = 0.;
        style.wet_mix.pull = 0.;
        style.wet_mix.blur = 0.;
        let batch = DabBatch {
            material_update: 0,
            stroke_id: StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 0., y: 0. },
                max: Point { x: 256., y: 256. },
            },
        };
        r.submit(packet(
            std::slice::from_ref(&layer),
            std::slice::from_ref(&dab),
            std::slice::from_ref(&batch),
            true,
        ))
        .unwrap();
        let actual = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
        let mut max_error = 0u16;
        for (pixel, input) in actual.chunks_exact(16).zip(reference.chunks_exact(8)) {
            let v: [f32; 4] = std::array::from_fn(|c| {
                f32::from_ne_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap())
            });
            assert_eq!(v[3], 1., "{execution:?} alpha");
            for c in 0..3 {
                let code = (RgbSpace::Srgb.encode(f64::from(v[c])) * 65535.).round() as u16;
                let expected = u16::from_le_bytes(input[c * 2..c * 2 + 2].try_into().unwrap());
                max_error = max_error.max(code.abs_diff(expected));
            }
        }
        assert_eq!(max_error, 0, "{execution:?} narrowed identity");
        let layer = &r.paint_layers[0];
        for page in &layer.pages {
            assert_eq!(
                page.active().texture.format(),
                wgpu::TextureFormat::Rgba32Float
            );
        }
        for page in &layer.coverage_pages {
            assert_eq!(
                page.active().texture.format(),
                wgpu::TextureFormat::R32Float
            );
        }
        for page in &layer.material_pages {
            assert_eq!(page.wetness.texture.format(), wgpu::TextureFormat::R32Float);
        }
        for page in &layer.watercolor_wetness_pages {
            assert_eq!(page.primary.texture.format(), wgpu::TextureFormat::R32Float);
            assert_eq!(
                page.secondary.texture.format(),
                wgpu::TextureFormat::R32Float
            );
        }
    }
}

#[test]
fn float32_masks_accumulate_small_coverage_without_narrowing() {
    let mut r = WgpuRasterizer::new_float32().unwrap();
    let (mut layer, _) = ramp(65535);
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(2), Point::default());
    mask.default_coverage = 0.;
    layer.mask = Some(mask);
    let alpha = 1. / 65535.;
    let mut dab = test_dab([128., 128.], [1.; 4], alpha);
    dab.radii = [120.; 2];
    let mut batch = DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(2),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: false,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point::default(),
            max: Point { x: 256., y: 256. },
        },
    };
    for frame in 0..128 {
        batch.stroke_start = frame == 0;
        r.submit(packet(
            std::slice::from_ref(&layer),
            std::slice::from_ref(&dab),
            std::slice::from_ref(&batch),
            frame == 0,
        ))
        .unwrap();
    }
    let mask = &r.layer_masks.pages[&(LayerId(2), [0, 0])].texture;
    assert_eq!(mask.format(), wgpu::TextureFormat::R32Float);
    let bytes = crate::layer_tests::page_bytes(&r, mask);
    let actual = f32::from_ne_bytes(bytes[(128 * 256 + 128) * 4..][..4].try_into().unwrap());
    let expected = 1. - (1. - f64::from(alpha)).powi(128);
    assert!(
        (f64::from(actual) - expected).abs() < 0.00000003,
        "mask {actual} expected {expected}"
    );
}
