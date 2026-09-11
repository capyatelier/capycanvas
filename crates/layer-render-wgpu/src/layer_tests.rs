//! Pixel assertions and bounded GPU-completion timings for layer composition.
use super::*;
use layer_core::{
    BrushDeform, BrushRendering, BrushWetMix, LayerMask, LayerOperation, LayerOperationKind, Point,
    Rect, Selection, StrokeId,
};
use layer_render::{DabStyle, ViewState};
#[path = "figure_tests.rs"]
mod figures;
#[path = "paint_transform_tests.rs"]
mod transforms;

fn view() -> ViewState {
    ViewState {
        width_px: 128,
        height_px: 128,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}
fn dab(color: [f32; 4]) -> Dab {
    Dab {
        center: Point { x: 64., y: 64. },
        radii: [60., 60.],
        rotation: [1., 0.],
        motion: [0.; 2],
        color_rgba_linear: color,
        flow: 1.,
        hardness: 1.,
        texture_sign: [1.; 2],
        material: [0.; 4],
    }
}
fn batch(id: u64) -> DabBatch {
    DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(id),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: DabStyle {
            alpha_locked: false,
            selection: None,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution: BrushExecution::Dry,
            grain: None,
            dual: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
        },
        damage: Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 128., y: 128. },
        },
    }
}
fn submit(
    r: &mut WgpuRasterizer,
    layers: &[Layer],
    dabs: &[Dab],
    batches: &[DabBatch],
    reset: bool,
) {
    r.submit(FramePacket {
        view: view(),
        document_extent: [128, 128],
        layers,
        dabs,
        dab_batches: batches,
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
}
fn pixel(r: &mut WgpuRasterizer, x: usize, y: usize) -> [u8; 4] {
    r.readback_srgb_rgba8().unwrap()[(y * 128 + x) * 4..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn connected_region_is_immutable_replayable_and_shared_by_paint_and_masks() {
    use layer_render::{RegionRequest, RegionSource};
    let receive = |r: &mut WgpuRasterizer| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            if let Some(result) = r.take_region() {
                break result.unwrap();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "region callback timed out"
            );
            std::thread::yield_now();
        }
    };
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let asset = AssetId::from("test:closed-line");
    let pixels: Vec<_> = (0..128 * 128)
        .flat_map(|i| {
            let (x, y) = (i % 128, i / 128);
            if ((x == 16 || x == 112) && (16..=112).contains(&y))
                || ((y == 16 || y == 112) && (16..=112).contains(&x))
            {
                [0, 0, 0, 255]
            } else {
                [0; 4]
            }
        })
        .collect();
    r.prepare_asset(
        &asset,
        HostImage {
            width: 128,
            height: 128,
            stride: 512,
            format: PixelFormat::Rgba8Srgb,
            bytes: &pixels,
        },
    )
    .unwrap();
    let mut line = Layer::paint(LayerId(1), "Line");
    line.asset = Some(asset);
    let mut fill = Layer::paint(LayerId(2), "Fill");
    submit(&mut r, &[line.clone(), fill.clone()], &[], &[], true);
    let request = RegionRequest {
        request_id: 7,
        source: RegionSource::Layer(line.id),
        position: [64, 64],
        tolerance: 0.,
        limit: None,
    };
    assert!(r.request_region(request.clone()).unwrap());
    assert!(!r.request_region(request).unwrap(), "single flight");
    r.wait_idle().unwrap();
    let result = receive(&mut r);
    assert_eq!(result.request_id, 7);
    assert_eq!(result.pixels.bounds(), [17, 17, 112, 112]);
    assert!(!r.region_pending());
    let selection = Selection::pixels(result.pixels.clone());
    let original_buffer = r.selection_clip.pixel_buffer(&r.device, &result.pixels);
    r.set_selection_outline(Some(&selection)).unwrap();
    assert_eq!(
        r.display_selection.as_ref().unwrap().1,
        original_buffer,
        "outline uses retained GPU output"
    );
    for inverse in [false, true] {
        let mut selected = selection.translated(Point { x: -8., y: 4. });
        selected.inverted = inverse;
        let mut mask = LayerMask::reveal_all(LayerId(8), Point::default());
        mask.default_coverage = f32::from(inverse);
        mask.initial = Some(selected.clone());
        fill.operations = vec![LayerOperation {
            after_stroke: 0,
            coverage: mask.clone(),
            kind: LayerOperationKind::Fill {
                color: [0., 0., 1., 1.],
                alpha_locked: false,
            },
        }];
        let operation = DabBatch {
            kind: DabBatchKind::LayerOperation(0),
            dab_count: 0,
            ..batch(2)
        };
        submit(&mut r, &[fill.clone()], &[], &[operation], true);
        let expected = r.readback_srgb_rgba8().unwrap();
        for (x, y, inside) in [
            (64, 64, true),
            (9, 21, true),
            (8, 20, false),
            (103, 115, true),
            (104, 116, false),
        ] {
            assert_eq!(
                pixel(&mut r, x, y),
                if inside != inverse {
                    [0, 0, 255, 255]
                } else {
                    [0; 4]
                },
                "{x},{y}, inverse={inverse}"
            );
        }
        // The same region clips a large brush and initializes a layer mask.
        fill.operations.clear();
        let mut brush = batch(2);
        brush.style.selection = Some(std::sync::Arc::new(selected));
        let mut d = dab([0., 0., 1., 1.]);
        d.radii = [300.; 2];
        submit(&mut r, &[fill.clone()], &[d], &[brush], true);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), expected);
        fill.mask = Some(mask);
        submit(&mut r, &[fill.clone()], &[d], &[batch(2)], true);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), expected);
        fill.mask = None;
    }
    // Detection is constrained by an existing selection, including an empty
    // answer when the seed lies outside its coverage.
    submit(&mut r, &[line], &[], &[], true);
    for (seed, bounds) in [([32, 64], [17, 17, 64, 112]), ([80, 64], [0; 4])] {
        assert!(
            r.request_region(RegionRequest {
                request_id: 8,
                source: RegionSource::Composite,
                position: seed,
                tolerance: 0.,
                limit: left_mask(9).initial.map(std::sync::Arc::new)
            })
            .unwrap()
        );
        r.wait_idle().unwrap();
        assert_eq!(receive(&mut r).pixels.bounds(), bounds);
    }
    assert_eq!(
        result.pixels.bounds(),
        [17, 17, 112, 112],
        "later detection never rewrites history"
    );
}

#[test]
fn reference_regions_match_isolated_composition_without_changing_visible_canvas() {
    use layer_core::{Document, EffectInstance};
    use layer_render::{RegionRequest, RegionSource};
    use std::sync::Arc;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let receive = |r: &mut WgpuRasterizer| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            if let Some(result) = r.take_region() {
                break result.unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    };
    let mut doc = Document::new("reference test", 128, 128);
    let mut group = Layer::paint(LayerId(10), "Group");
    group.kind = LayerKind::Group;
    group.properties.offset = Point { x: 8., y: 4. };
    let mut line = Layer::paint(LayerId(11), "Reference");
    line.properties.parent = Some(group.id);
    line.mask = Some(left_mask(20));
    let mut filter = Layer::paint(LayerId(12), "Blur");
    filter.kind = LayerKind::Effect;
    filter.properties.parent = Some(group.id);
    filter.properties.clipped = true;
    filter.effect = Some(Arc::new(EffectInstance::new(
        crate::tests::fixture("gaussian_blur").program(),
    )));
    let unrelated = Layer::paint(LayerId(13), "Unrelated");
    let mut dabs = [dab([0.4, 0.1, 0.8, 1.]), dab([0., 1., 0., 1.])];
    dabs[1].radii = [300.; 2];
    let batches = [
        batch(11),
        DabBatch {
            first_dab: 1,
            ..batch(13)
        },
    ];
    doc.layers = vec![group, filter, line, unrelated];
    doc.reference_layers.insert(LayerId(11));
    for filtered in [false, true] {
        doc.layers[1].visible = filtered;
        let refs = doc.reference_snapshot();
        // Oracle: normal canvas composition of exactly the reference snapshot.
        submit(&mut r, &refs, &dabs, &batches, true);
        assert!(
            r.request_region(RegionRequest {
                request_id: 1,
                source: RegionSource::Composite,
                position: [40, 64],
                tolerance: 0.1,
                limit: None
            })
            .unwrap()
        );
        let expected = receive(&mut r);
        assert!(
            expected.pixels.bounds()[2] <= 80,
            "reference mask constrains coverage"
        );
        // A different visible scene has independent cached input boundaries.
        submit(&mut r, &doc.layers, &dabs, &batches, true);
        let before = r.readback_srgb_rgba8().unwrap();
        assert!(
            r.request_region(RegionRequest {
                request_id: 2,
                source: RegionSource::Layers(refs),
                position: [40, 64],
                tolerance: 0.1,
                limit: None
            })
            .unwrap()
        );
        let actual = receive(&mut r);
        assert_eq!(actual.pixels, expected.pixels, "filtered={filtered}");
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            before,
            "capture never replaces live composition"
        );
    }
}

#[test]
#[ignore = "hardware GPU complete region request benchmark; release, serial"]
fn region_request_latency() {
    use layer_render::{RegionRequest, RegionSource, TimingSamples};
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [2048, 1536];
    let asset = AssetId::from("test:region-benchmark");
    let pixels: Vec<_> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            let (x, y) = (i % extent[0], i / extent[0]);
            if x % 200 == 100 || y % 150 == 75 {
                [0, 0, 0, 255]
            } else {
                [0; 4]
            }
        })
        .collect();
    r.prepare_asset(
        &asset,
        HostImage {
            width: extent[0],
            height: extent[1],
            stride: extent[0] * 4,
            format: PixelFormat::Rgba8Srgb,
            bytes: &pixels,
        },
    )
    .unwrap();
    let mut line = Layer::paint(LayerId(1), "Line");
    line.asset = Some(asset);
    let layers = [line];
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    r.resize_surface(extent[0], extent[1]).unwrap();
    r.submit(FramePacket {
        view,
        document_extent: extent,
        layers: &layers,
        dabs: &[],
        dab_batches: &[],
        time_seconds: 0.,
        reset_layers: true,
        composite_all: true,
    })
    .unwrap();
    r.wait_idle().unwrap();
    for (name, source) in [
        ("visible", RegionSource::Composite),
        ("raw layer", RegionSource::Layer(LayerId(1))),
        (
            "references",
            RegionSource::Layers(layers.iter().map(Layer::composite_snapshot).collect()),
        ),
    ] {
        let mut cpu = TimingSamples::default();
        let mut complete = TimingSamples::default();
        for i in 0..150 {
            let start = std::time::Instant::now();
            assert!(
                r.request_region(RegionRequest {
                    request_id: i,
                    source: source.clone(),
                    position: [48, 48],
                    tolerance: 0.1,
                    limit: None
                })
                .unwrap()
            );
            cpu.push(start.elapsed().as_secs_f32() * 1000.);
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            loop {
                if let Some(result) = r.take_region() {
                    assert_eq!(result.unwrap().pixels.bounds(), [0, 0, 100, 75]);
                    break;
                }
                assert!(start.elapsed() < READBACK_TIMEOUT);
                std::thread::yield_now();
            }
            complete.push(start.elapsed().as_secs_f32() * 1000.);
            if i == 0 {
                eprintln!(
                    "{name} cold complete {:.3}ms",
                    start.elapsed().as_secs_f32() * 1000.
                );
                let mut t = telemetry::Telemetry::new(&r.device, &r.queue);
                t.enabled = true;
                r.regions.as_mut().unwrap().timing = Some(t);
            }
        }
        let gpu = r
            .regions
            .as_ref()
            .unwrap()
            .timing
            .as_ref()
            .unwrap()
            .snapshot()
            .gpu;
        for (label, samples) in [
            ("CPU", cpu),
            ("GPU", gpu),
            ("Completed incl. history", complete),
        ] {
            let mut values = samples.ordered();
            values.sort_by(f32::total_cmp);
            assert_eq!(values.len(), 120);
            eprintln!(
                "{name} {label} median/p95/p99 {:.3}/{:.3}/{:.3}ms",
                values[59], values[113], values[118]
            );
        }
        eprintln!(
            "{name} resident query buffers {} bytes",
            r.regions.as_ref().unwrap().storage_bytes()
        );
    }
}

// Inspect persistent pigment/wetness independently of layer-level effects.
// This is test-only readback, never a drawing or selection raster path.
fn page_bytes(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<u8> {
    let row = texture.width() * texture.format().block_copy_size(None).unwrap();
    assert_eq!(row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
    let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test persistent page"),
        size: u64::from(row) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = r.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    let submission = r.queue.submit([encoder.finish()]);
    let (send, receive) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    receive.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    buffer.unmap();
    bytes
}
fn left_mask(id: u64) -> LayerMask {
    let mut m = LayerMask::reveal_all(LayerId(id), Point::default());
    m.default_coverage = 0.;
    m.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 64., y: 0. },
            Point { x: 64., y: 128. },
            Point { x: 0., y: 128. },
        ])
        .unwrap(),
    );
    m
}

fn preset_style(preset: layer_core::DefaultBrushPreset) -> DabStyle {
    let brush = layer_core::default_brush(preset);
    DabStyle {
        alpha_locked: false,
        selection: None,
        tip: brush.tip,
        mode: if preset == layer_core::DefaultBrushPreset::Eraser {
            DabMode::Erase
        } else {
            DabMode::Paint
        },
        execution: brush.execution,
        grain: brush.grain,
        dual: brush.dual,
        rendering: brush.rendering,
        wet_mix: brush.wet_mix,
        transport: brush.transport,
        deform: brush.deform,
    }
}

#[test]
#[ignore = "hardware selection preparation benchmark; release, serial"]
fn affine_selection_preparation_latency() {
    use layer_core::{Affine, SelectionPixels};
    use std::{sync::Arc, time::Instant};
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [2048, 1536];
    let base = Selection::pixels(Arc::new(
        SelectionPixels::new(
            extent,
            [0, 0, extent[0], extent[1]],
            vec![0x44443210; (extent[0] / 8 * extent[1]) as usize],
        )
        .unwrap(),
    ));
    let percentile = |mut samples: Vec<f32>| {
        samples.sort_by(f32::total_cmp);
        [0.5, 0.95, 0.99].map(|q| samples[(samples.len() as f32 * q).ceil() as usize - 1])
    };
    for mode in ["translation", "affine", "unchanged"] {
        let mut timing = telemetry::Telemetry::new(r.device(), r.queue());
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        let mut generation = 0;
        let mut storage = 0;
        for i in 0..160 {
            let t = if mode == "unchanged" {
                0.
            } else {
                i as f32 * 0.03
            };
            let affine = if mode == "translation" {
                Affine::translation(Point {
                    x: t.sin() * 4.,
                    y: t.cos() * 3.,
                })
            } else {
                Affine::around(
                    Point { x: 1024., y: 768. },
                    [0.95, 1.02],
                    0.2 + t * 0.01,
                    Point::default(),
                )
            };
            let selection = Arc::new(base.transformed(affine).unwrap());
            timing.enabled = i >= 40;
            let started = Instant::now();
            let mut encoder = r.device.create_command_encoder(&Default::default());
            timing.begin(&r.device, &mut encoder);
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, &selection)
                .unwrap();
            timing.end(&mut encoder);
            r.last_submission = Some(r.queue.submit([encoder.finish()]));
            timing.submitted();
            let submit_ms = started.elapsed().as_secs_f32() * 1000.;
            r.wait_idle().unwrap();
            if i >= 40 {
                cpu.push(submit_ms);
                completed.push(started.elapsed().as_secs_f32() * 1000.);
                assert_eq!(
                    r.selection_clip.storage_bytes(),
                    storage,
                    "warm storage stable"
                );
                if mode == "unchanged" {
                    assert_eq!(r.selection_clip.generations, generation);
                } else {
                    assert_eq!(r.selection_clip.generations, generation + 1);
                }
            }
            generation = r.selection_clip.generations;
            storage = r.selection_clip.storage_bytes();
        }
        let telemetry = timing.snapshot();
        assert_eq!(telemetry.gpu.count, 120);
        let (cpu, gpu, completed) = (
            percentile(cpu),
            percentile(telemetry.gpu.ordered()),
            percentile(completed),
        );
        eprintln!(
            "selection {mode}: CPU median/p95/p99={cpu:.3?}ms GPU={gpu:.3?}ms completed={completed:.3?}ms retained={storage}B"
        );
        assert!(
            completed[2] < 8.333,
            "selection preparation exceeds 120Hz budget"
        );
    }
}

#[test]
fn packed_brush_selection_matches_mask_coverage_and_reuses_geometry() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let polygon = Selection::polygon(vec![
        Point { x: 5., y: 5. },
        Point { x: 121., y: 27. },
        Point { x: 91., y: 122. },
        Point { x: 19., y: 80. },
    ])
    .unwrap();
    let mut white = dab([1.; 4]);
    white.radii = [200.; 2];
    for inverted in [false, true] {
        let mut selection = polygon.clone();
        selection.inverted = inverted;
        let mut masked = Layer::paint(LayerId(1), "mask reference");
        let mut mask = LayerMask::reveal_all(LayerId(2), Point::default());
        mask.default_coverage = f32::from(inverted);
        mask.initial = Some(selection.clone());
        masked.mask = Some(mask);
        submit(&mut r, &[masked], &[white], &[batch(1)], true);
        let reference = r.readback_srgb_rgba8().unwrap();
        let mut brush = batch(1);
        brush.style.selection = Some(std::sync::Arc::new(selection));
        let plain = [Layer::paint(LayerId(1), "selected stroke")];
        submit(&mut r, &plain, &[white], &[brush.clone()], true);
        let selected = r.readback_srgb_rgba8().unwrap();
        assert!(
            selected
                .iter()
                .zip(reference)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "packed coverage must match four-sample R8 coverage"
        );
        let generations = r.selection_clip.generations;
        for _ in 0..3 {
            submit(&mut r, &plain, &[white], &[brush.clone()], true);
        }
        assert_eq!(
            r.selection_clip.generations, generations,
            "stroke/reset alone do not regenerate geometry"
        );
        assert!(r.selection_clip.bytes <= 128 * 128 / 2 + 48);
    }
    // Two geometries queued together must not share overwritten input headers
    // or edges, even when both use the same reusable output buffer.
    let mut first = batch(1);
    first.style.selection = Some(std::sync::Arc::new(left_mask(5).initial.unwrap()));
    let mut second = first.clone();
    second.first_dab = 1;
    second.stroke_id = StrokeId(2);
    let mut inverse = second.style.selection.as_ref().unwrap().as_ref().clone();
    inverse.inverted = true;
    second.style.selection = Some(std::sync::Arc::new(inverse));
    let mut red = white;
    red.color_rgba_linear = [1., 0., 0., 1.];
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "two selections")],
        &[red, white],
        &[first, second],
        true,
    );
    assert_eq!(pixel(&mut r, 32, 64), [255, 0, 0, 255]);
    assert_eq!(pixel(&mut r, 96, 64), [255, 255, 255, 255]);
}

#[test]
fn affine_raster_selection_matches_linear_reference_in_fill_brush_and_mask() {
    use layer_core::{Affine, SelectionPixels};
    use std::sync::Arc;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    // An asymmetric shape with holes and fractional coverage, not a uniform box.
    let sample = |x: i32, y: i32| -> f32 {
        if !(0..16).contains(&x) || !(0..8).contains(&y) {
            return 0.;
        }
        if (x + y) % 5 == 0 {
            0.
        } else {
            ((x + 2 * y) % 4 + 1) as f32
        }
    };
    let words: Vec<u32> = (0..8)
        .flat_map(|y| {
            (0..2).map(move |w| (0..8).fold(0, |v, i| v | (sample(w * 8 + i, y) as u32) << (i * 4)))
        })
        .collect();
    let pixels = Arc::new(SelectionPixels::new([16, 8], [0, 0, 16, 8], words).unwrap());
    let base = Selection::pixels(pixels.clone());
    let white = Dab {
        radii: [200.; 2],
        ..dab([1.; 4])
    };
    for affine in [
        Affine::translation(Point { x: 30.5, y: 41.25 }),
        Affine::around(
            Point { x: 8., y: 4. },
            [3., 2.],
            0.43,
            Point { x: 50., y: 50. },
        ),
        Affine::around(
            Point { x: 8., y: 4. },
            [-2., 4.],
            -0.72,
            Point { x: 20., y: 32. },
        ),
        Affine::around(
            Point { x: 8., y: 4. },
            [0.7, 1.3],
            0.11,
            Point { x: -2., y: -2. },
        ),
        Affine::around(
            Point { x: 8., y: 4. },
            [2., 2.],
            0.,
            Point { x: 500., y: 500. },
        ),
    ] {
        for inverted in [false, true] {
            let mut selection = base.transformed(affine).unwrap();
            selection.inverted = inverted;
            let mut layer = Layer::paint(LayerId(1), "affine coverage");
            let mut mask = LayerMask::reveal_all(LayerId(2), Point::default());
            mask.default_coverage = f32::from(inverted);
            mask.initial = Some(selection.clone());
            layer.operations = vec![LayerOperation {
                after_stroke: 0,
                coverage: mask.clone(),
                kind: LayerOperationKind::Fill {
                    color: [1.; 4],
                    alpha_locked: false,
                },
            }];
            submit(
                &mut r,
                &[layer.clone()],
                &[],
                &[DabBatch {
                    kind: DabBatchKind::LayerOperation(0),
                    dab_count: 0,
                    ..batch(1)
                }],
                true,
            );
            let filled = r.readback_srgb_rgba8().unwrap();
            let inverse = affine.inverse().unwrap();
            for y in 0..128 {
                for x in 0..128 {
                    let p = inverse.map(Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    });
                    let coverage = if affine.0[..4] == [1., 0., 0., 1.] {
                        sample(p.x.floor() as i32, p.y.floor() as i32)
                    } else {
                        let q = Point {
                            x: p.x - 0.5,
                            y: p.y - 0.5,
                        };
                        let (a, b) = (q.x.floor() as i32, q.y.floor() as i32);
                        let (fx, fy) = (q.x - a as f32, q.y - b as f32);
                        let top = sample(a, b) * (1. - fx) + sample(a + 1, b) * fx;
                        let bottom = sample(a, b + 1) * (1. - fx) + sample(a + 1, b + 1) * fx;
                        (top * (1. - fy) + bottom * fy).round()
                    } * 0.25;
                    let expected =
                        ((if inverted { 1. - coverage } else { coverage }) * 255.).round() as u8;
                    assert!(
                        filled[(y * 128 + x) * 4 + 3].abs_diff(expected) <= 1,
                        "{affine:?}, inverted={inverted}, {x},{y}"
                    );
                }
            }
            let generations = r.selection_clip.generations;
            let storage = r.selection_clip.storage_bytes();
            layer.operations.clear();
            let selected = DabBatch {
                style: DabStyle {
                    selection: Some(Arc::new(selection.clone())),
                    ..batch(1).style
                },
                ..batch(1)
            };
            submit(&mut r, &[layer.clone()], &[white], &[selected], true);
            assert_eq!(r.readback_srgb_rgba8().unwrap(), filled);
            layer.mask = Some(mask);
            submit(&mut r, &[layer], &[white], &[batch(1)], true);
            assert_eq!(r.readback_srgb_rgba8().unwrap(), filled);
            assert_eq!(
                r.selection_clip.generations, generations,
                "changing the consumer does not resample"
            );
            assert_eq!(r.selection_clip.storage_bytes(), storage);
            r.set_selection_outline(Some(&selection)).unwrap();
            assert_eq!(
                r.display_selection.as_ref().unwrap().1,
                r.selection_clip.pixel_buffer(&r.device, &pixels),
                "display uses original coverage, not another copy"
            );
        }
    }
}

#[test]
fn viewport_outline_places_original_selection_without_rebuilding_coverage() {
    use layer_core::{Affine, SelectionPixels};
    use std::sync::Arc;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let layer = Layer::paint(LayerId(1), "selection display");
    submit(&mut r, &[layer], &[], &[], true);
    let target = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test selection display"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut presenter = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
    let rectangle = Selection::pixels(Arc::new(
        SelectionPixels::new([16, 8], [0, 0, 16, 8], vec![0x44444444; 16]).unwrap(),
    ));
    let words: Vec<_> = (0..128)
        .flat_map(|y| {
            (0..16).map(move |word| {
                (0..8).fold(0, |v, x| {
                    v | (if (30..62).contains(&(word * 8 + x)) && (40..64).contains(&y) {
                        4
                    } else {
                        0
                    }) << (x * 4)
                })
            })
        })
        .collect();
    let reference = Selection::pixels(Arc::new(
        SelectionPixels::new([128, 128], [30, 40, 62, 64], words).unwrap(),
    ));
    for inverted in [false, true] {
        let mut reference = reference.clone();
        reference.inverted = inverted;
        r.set_selection_outline(Some(&reference)).unwrap();
        presenter.present(
            &r,
            &target.create_view(&Default::default()),
            view(),
            [0.; 4],
        );
        let expected = page_bytes(&r, &target);
        for matrix in [
            Affine([2., 0., 0., 3., 30., 40.]),
            Affine([-2., 0., 0., -3., 62., 64.]),
        ] {
            let mut transformed = rectangle.transformed(matrix).unwrap();
            transformed.inverted = inverted;
            let generations = r.selection_clip.generations;
            r.set_selection_outline(Some(&transformed)).unwrap();
            presenter.present(
                &r,
                &target.create_view(&Default::default()),
                view(),
                [0.; 4],
            );
            assert_eq!(page_bytes(&r, &target), expected);
            assert_eq!(
                r.selection_clip.generations, generations,
                "presentation never rasterizes the selection"
            );
        }
    }
}

#[test]
fn all_brush_families_preserve_unselected_pigment_and_wetness() {
    use layer_core::DefaultBrushPreset::*;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    for preset in [
        GPen,
        Pencil,
        Eraser,
        DualTexture,
        Marker,
        NaturalBlender,
        WetRound,
        LoadedOil,
        PaletteKnife,
        LiquifyPush,
        LiquifyTwirl,
        WatercolorWash,
        WetWatercolor,
    ] {
        let layers = [Layer::paint(LayerId(1), "selected painting")];
        let mut blue = dab([0., 0.1, 1., 1.]);
        blue.center.x = 32.;
        blue.radii = [35., 48.];
        let mut yellow = blue;
        yellow.center.x = 96.;
        yellow.color_rgba_linear = [1., 0.8, 0., 1.];
        let base = [blue, yellow];
        let base_batch = DabBatch {
            dab_count: 2,
            ..batch(1)
        };
        submit(&mut r, &layers, &base, &[base_batch], true);
        let before = r.readback_srgb_rgba8().unwrap();
        let raw_before = page_bytes(&r, &r.paint_layers[0].pages[0].active().texture);
        let mut b = batch(1);
        b.stroke_id = StrokeId(2);
        b.dab_count = 3;
        b.style = preset_style(preset);
        b.style.selection = Some(std::sync::Arc::new(left_mask(5).initial.unwrap()));
        let watercolor = b.style.execution == BrushExecution::Watercolor;
        let dabs: Vec<_> = [42., 62., 82.]
            .into_iter()
            .map(|x| {
                let mut d = dab([1., 0., 0.1, 1.]);
                d.center.x = x;
                d.radii = [36., 36.];
                d.motion = [20., 0.];
                d.material = [0.6, 0.8, 1., 0.8];
                d
            })
            .collect();
        submit(&mut r, &layers, &dabs, &[b], false);
        let after = r.readback_srgb_rgba8().unwrap();
        let raw_after = page_bytes(&r, &r.paint_layers[0].pages[0].active().texture);
        let channels = raw_before.len() / (256 * 256);
        for y in 0..128 {
            let start = (y * 256 + 64) * channels;
            let end = (y * 256 + 128) * channels;
            assert_eq!(
                &raw_after[start..end],
                &raw_before[start..end],
                "{preset:?} changed persistent unselected pigment at row {y}"
            );
        }
        for page in &r.paint_layers[0].watercolor_wetness_pages {
            let wetness = page_bytes(&r, &page.active().texture);
            for y in 0..128 {
                assert!(
                    wetness[y * 256 + 64..y * 256 + 128].iter().all(|v| *v == 0),
                    "{preset:?} deposited water outside selection at row {y}"
                );
            }
        }
        let mut changed = 0;
        for y in 0..128 {
            for x in 0..128 {
                let i = (y * 128 + x) * 4;
                // Watercolor's non-destructive layer edge effect is applied
                // after painting (like other layer filters), not baked pigment.
                // Its outside band can extend beyond the painted selection.
                if x >= 64 && !watercolor {
                    assert_eq!(
                        &after[i..i + 4],
                        &before[i..i + 4],
                        "{preset:?} altered unselected {x},{y}"
                    );
                } else if x < 64 && after[i..i + 4] != before[i..i + 4] {
                    changed += 1;
                }
            }
        }
        assert!(
            changed > 10,
            "{preset:?} must actually affect selected paint"
        );
    }
}

#[test]
fn selected_wet_brush_does_not_dry_or_advect_unselected_wet_paint() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    for preset in [
        layer_core::DefaultBrushPreset::WetWatercolor,
        layer_core::DefaultBrushPreset::WetRound,
    ] {
        let layers = [Layer::paint(LayerId(1), "wet selection")];
        let mut b = batch(1);
        b.style = preset_style(preset);
        let mut blue = dab([0., 0., 1., 0.7]);
        blue.material = [0.5, 0.8, 1., 0.8];
        submit(&mut r, &layers, &[blue], &[b.clone()], true);
        let state = |r: &WgpuRasterizer| {
            let layer = &r.paint_layers[0];
            let mut pages = vec![page_bytes(r, &layer.pages[0].active().texture)];
            pages.extend(
                layer
                    .watercolor_wetness_pages
                    .iter()
                    .map(|p| page_bytes(r, &p.active().texture)),
            );
            pages.extend(
                layer
                    .material_pages
                    .iter()
                    .map(|p| page_bytes(r, &p.wetness.texture)),
            );
            pages
        };
        let before = state(&r);
        assert!(before.len() > 1, "wet state must actually exist");
        assert!(
            before[1].iter().any(|v| *v != 0),
            "initial paint must be wet"
        );
        b.stroke_id = StrokeId(2);
        b.style.selection = Some(std::sync::Arc::new(left_mask(9).initial.unwrap()));
        let mut red = blue;
        red.color_rgba_linear = [1., 0., 0., 0.7];
        red.motion = [20., 0.];
        for _ in 0..3 {
            submit(&mut r, &layers, &[red], &[b.clone()], false);
        }
        let after = state(&r);
        assert_ne!(before[0], after[0], "selected pigment must change");
        for (before, after) in before.iter().zip(after) {
            let channels = before.len() / (256 * 256);
            for y in 0..128 {
                let start = (y * 256 + 64) * channels;
                let end = (y * 256 + 128) * channels;
                assert_eq!(
                    &before[start..end],
                    &after[start..end],
                    "{preset:?} changed unselected pigment/water at row {y}"
                );
            }
        }
    }
}

#[test]
fn selection_clips_mask_paint_and_disposable_brush_previews() {
    use layer_core::DefaultBrushPreset::*;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let selection = std::sync::Arc::new(left_mask(5).initial.unwrap());
    let mut masked = Layer::paint(LayerId(1), "masked");
    masked.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    let mut erase = batch(9);
    erase.style.mode = DabMode::Erase;
    erase.style.selection = Some(selection.clone());
    erase.first_dab = 1;
    submit(
        &mut r,
        &[masked],
        &[dab([1.; 4]), dab([1.; 4])],
        &[batch(1), erase],
        true,
    );
    assert_eq!(pixel(&mut r, 32, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 96, 64), [255; 4]);

    for opacity in [1., 0.5] {
        for preset in [GPen, Marker, WetRound, WatercolorWash] {
            let mut layer = Layer::paint(LayerId(1), "preview");
            layer.opacity = opacity;
            let layers = [layer];
            submit(&mut r, &layers, &[dab([0., 0., 1., 1.])], &[batch(1)], true);
            let before = r.readback_srgb_rgba8().unwrap();
            let raw_before = page_bytes(&r, &r.paint_layers[0].pages[0].active().texture);
            let mut preview = batch(1);
            preview.kind = DabBatchKind::Preview;
            preview.stroke_id = StrokeId(2);
            preview.stroke_end = false;
            preview.style = preset_style(preset);
            preview.style.selection = Some(selection.clone());
            let mut red = dab([1., 0., 0., 1.]);
            red.material = [0.6, 0.8, 1., 0.8];
            submit(&mut r, &layers, &[red], &[preview], false);
            let after = r.readback_srgb_rgba8().unwrap();
            assert_ne!(before, after, "{preset:?} preview should be visible");
            assert_eq!(
                &before[(64 * 128 + 96) * 4..][..4],
                &after[(64 * 128 + 96) * 4..][..4],
                "{preset:?} unselected preview"
            );
            assert_eq!(
                raw_before,
                page_bytes(&r, &r.paint_layers[0].pages[0].active().texture)
            );
            submit(&mut r, &layers, &[], &[], false);
            assert_eq!(
                before,
                r.readback_srgb_rgba8().unwrap(),
                "{preset:?} preview cancellation"
            );
        }
    }
}

#[test]
fn scanline_selection_handles_holes_crossings_offcanvas_and_wide_rows() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [2048, 128];
    let mut outside = Selection::polygon(vec![
        Point { x: -40., y: -10. },
        Point { x: 2031.5, y: 5. },
        Point { x: 2050., y: 135. },
        Point { x: -1., y: 113. },
    ])
    .unwrap();
    let hole = Selection::polygon(vec![
        Point { x: 7.25, y: 3.75 },
        Point { x: 1950., y: 125. },
        Point { x: 17.25, y: 105. },
        Point { x: 1910., y: 10. },
    ])
    .unwrap();
    outside.shape = layer_core::SelectionShape::Contours(
        vec![outside.contours()[0].clone(), hole.contours()[0].clone()].into(),
    );
    let render = |r: &mut WgpuRasterizer, layer: &Layer, brush: &DabBatch| {
        let mut d = dab([1.; 4]);
        d.center = Point { x: 1024., y: 64. };
        d.radii = [3000.; 2];
        r.submit(FramePacket {
            view: ViewState {
                width_px: 2048,
                ..view()
            },
            document_extent: extent,
            layers: std::slice::from_ref(layer),
            dabs: &[d],
            dab_batches: std::slice::from_ref(brush),
            reset_layers: true,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        r.readback_srgb_rgba8().unwrap()
    };
    for inverted in [false, true] {
        for delta in [
            Point::default(),
            Point {
                x: 0.375,
                y: -0.125,
            },
        ] {
            let mut geometry = outside.translated(delta);
            geometry.inverted = inverted;
            let mut layer = Layer::paint(LayerId(1), "scanline reference");
            let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
            mask.default_coverage = f32::from(inverted);
            mask.initial = Some(geometry.clone());
            layer.mask = Some(mask);
            let mut b = batch(1);
            b.damage = Rect {
                min: Point::default(),
                max: Point { x: 2048., y: 128. },
            };
            let reference = render(&mut r, &layer, &b);
            layer.mask = None;
            b.style.selection = Some(std::sync::Arc::new(geometry));
            let actual = render(&mut r, &layer, &b);
            for (i, (a, b)) in actual.iter().zip(reference).enumerate() {
                assert!(
                    a.abs_diff(b) <= 1,
                    "coverage mismatch at byte {i}: {a} vs {b}, inverted={inverted}"
                );
            }
        }
    }
}

#[test]
fn point_sampling_reads_visible_or_raw_layer_color_without_recompositing() {
    use layer_render::{ColorSampleRequest, ColorSampleSource as Source};
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "paint");
    layer.opacity = 0.5;
    layer.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[layer.clone()],
        &[dab([0.25, 0.5, 1.0, 0.8])],
        &[batch(1)],
        true,
    );
    let revision = r.composite_revision;
    let sample = |r: &mut WgpuRasterizer, source, position| {
        let request = ColorSampleRequest {
            request_id: 77,
            source,
            position,
        };
        assert!(r.request_color_sample(request).unwrap());
        assert!(
            !r.request_color_sample(request).unwrap(),
            "bounded single-flight sample"
        );
        let start = std::time::Instant::now();
        loop {
            r.device.poll(wgpu::PollType::Poll).unwrap();
            if let Some(result) = r.take_color_sample() {
                let sample = result.unwrap();
                assert_eq!(sample.request_id, 77);
                break sample.rgba;
            }
            assert!(start.elapsed().as_secs() < 5);
            std::thread::yield_now();
        }
    };
    let near = |a: [f32; 4], b: [f32; 4]| {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.015, "{a:?} vs {b:?}");
        }
    };
    near(
        sample(&mut r, Source::Composite, [40, 64]),
        [0.25, 0.5, 1.0, 0.4],
    );
    near(sample(&mut r, Source::Composite, [80, 64]), [0.0; 4]);
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [80, 64]),
        [0.25, 0.5, 1.0, 0.8],
    );
    // Bounds and empty paint tiles yield no color; never clamp to an edge pixel.
    near(sample(&mut r, Source::Composite, [500, 64]), [0.0; 4]);
    near(sample(&mut r, Source::Layer(LayerId(1)), [0, 0]), [0.0; 4]);
    assert_eq!(
        r.composite_revision, revision,
        "sampling never invalidates composition"
    );
    // A subsequent edit must be sampled from the current texture, not a cached color.
    submit(
        &mut r,
        &[layer],
        &[dab([1.0, 0.0, 0.0, 1.0])],
        &[batch(1)],
        true,
    );
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [40, 64]),
        [1.0, 0.0, 0.0, 1.0],
    );
    // Raw layers use independently allocated pages, not document-sized textures.
    let mut dot = dab([0.0, 1.0, 0.0, 1.0]);
    dot.center = Point { x: 320.0, y: 320.0 };
    let mut stroke = batch(1);
    stroke.damage = Rect {
        min: Point { x: 256.0, y: 256.0 },
        max: Point { x: 384.0, y: 384.0 },
    };
    r.submit(FramePacket {
        view: view(),
        document_extent: [384, 384],
        layers: &[Layer::paint(LayerId(1), "paint")],
        dabs: &[dot],
        dab_batches: &[stroke],
        reset_layers: true,
        time_seconds: 0.0,
        composite_all: true,
    })
    .unwrap();
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [320, 320]),
        [0.0, 1.0, 0.0, 1.0],
    );
    near(
        sample(&mut r, Source::Composite, [320, 320]),
        [0.0, 1.0, 0.0, 1.0],
    );
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [64, 64]),
        [0.0; 4],
    );
}

#[test]
fn gradients_share_fill_compositing_and_respect_coverage_and_alpha_lock() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    for radial in [false, true] {
        for transparent in [false, true] {
            for alpha_locked in [false, true] {
                for selected in [false, true] {
                    let mut layer = Layer::paint(LayerId(1), "gradient");
                    let start = [1.0, 0.1, 0.0, 0.7];
                    let end = [0.0, 0.2, 1.0, if transparent { 0.0 } else { 0.7 }];
                    let coverage = if selected {
                        left_mask(9)
                    } else {
                        LayerMask::reveal_all(LayerId(9), Point::default())
                    };
                    layer.operations.push(LayerOperation {
                        after_stroke: 1,
                        coverage,
                        kind: LayerOperationKind::Gradient {
                            start: Point { x: 16.0, y: 64.0 },
                            end: Point { x: 112.0, y: 64.0 },
                            colors: [start, end],
                            radial,
                            alpha_locked,
                        },
                    });
                    let mut operation = batch(1);
                    operation.kind = DabBatchKind::LayerOperation(0);
                    operation.dab_count = 0;
                    submit(
                        &mut r,
                        &[layer],
                        &[dab([0.1, 0.2, 0.3, 0.6])],
                        &[batch(1), operation],
                        true,
                    );
                    for (x, y) in [(40, 64), (80, 64), (64, 32), (0, 0)] {
                        let p = [x as f32 + 0.5 - 16.0, y as f32 + 0.5 - 64.0];
                        let t = (if radial {
                            p[0].hypot(p[1]) / 96.0
                        } else {
                            p[0] / 96.0
                        })
                        .clamp(0.0, 1.0);
                        let mask = f32::from(!selected || x < 64);
                        let a = ((1.0 - t) * start[3] + t * end[3]) * mask;
                        let old_alpha = if x == 0 { 0.0 } else { 0.6 };
                        let alpha = if alpha_locked {
                            old_alpha
                        } else {
                            a + old_alpha * (1.0 - a)
                        };
                        let encode = |v: f32| {
                            if v <= 0.0031308 {
                                v * 12.92
                            } else {
                                1.055 * v.powf(1.0 / 2.4) - 0.055
                            }
                        };
                        let mut expected = [0u8; 4];
                        for i in 0..3 {
                            let pigment =
                                ((1.0 - t) * start[i] * start[3] + t * end[i] * end[3]) * mask;
                            let rgb = pigment * if alpha_locked { old_alpha } else { 1.0 }
                                + [0.1, 0.2, 0.3][i] * old_alpha * (1.0 - a);
                            expected[i] = (encode(rgb / alpha.max(0.000001)) * 255.0).round() as u8;
                        }
                        expected[3] = (alpha * 255.0).round() as u8;
                        let actual = pixel(&mut r, x, y);
                        for i in 0..4 {
                            assert!(
                                actual[i].abs_diff(expected[i]) <= 4,
                                "radial {radial} clear {transparent} lock {alpha_locked} selection {selected} at {x},{y}: {actual:?} vs {expected:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn gradient_respects_layer_mask_and_clipping_base_alpha() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut gradient = Layer::paint(LayerId(1), "gradient");
    gradient.properties.clipped = true;
    gradient.mask = Some(left_mask(8));
    gradient.operations.push(LayerOperation {
        after_stroke: 0,
        coverage: LayerMask::reveal_all(LayerId(9), Point::default()),
        kind: LayerOperationKind::Gradient {
            start: Point::default(),
            end: Point { x: 128., y: 0. },
            colors: [[1., 0., 0., 0.8], [0., 0., 1., 0.8]],
            radial: false,
            alpha_locked: false,
        },
    });
    let op = DabBatch {
        kind: DabBatchKind::LayerOperation(0),
        dab_count: 0,
        ..batch(1)
    };
    submit(
        &mut r,
        &[gradient, Layer::paint(LayerId(2), "base")],
        &[dab([0., 0., 1., 0.5])],
        &[batch(2), op],
        true,
    );
    let tinted = pixel(&mut r, 32, 64);
    for (actual, expected) in tinted.into_iter().zip([203_u8, 0, 170, 128]) {
        assert!(
            actual.abs_diff(expected) <= 3,
            "masked clipped gradient: {tinted:?}"
        );
    }
    let masked_out = pixel(&mut r, 96, 64);
    assert_eq!(&masked_out[..3], &[0, 0, 255]);
    assert!(masked_out[3].abs_diff(128) <= 1);
    assert_eq!(pixel(&mut r, 0, 0)[3], 0);
}

#[test]
fn queued_gradients_match_replay_across_tiles_and_inverted_offset_masks() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [512, 384];
    let v = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    for inverted in [false, true] {
        let mut layer = Layer::paint(LayerId(1), "gradient");
        let mut coverage = LayerMask::reveal_all(LayerId(9), Point { x: -20., y: 12. });
        coverage.default_coverage = 0.;
        coverage.inverted = inverted;
        coverage.initial = Some(
            Selection::polygon(vec![
                Point { x: 250., y: 50. },
                Point { x: 410., y: 50. },
                Point { x: 410., y: 280. },
                Point { x: 250., y: 280. },
            ])
            .unwrap(),
        );
        layer.operations.push(LayerOperation {
            after_stroke: 1,
            coverage,
            kind: LayerOperationKind::Gradient {
                start: Point { x: 160., y: 64. },
                end: Point { x: 400., y: 64. },
                colors: [[1., 0., 0., 0.8], [0., 0., 1., 0.8]],
                radial: false,
                alpha_locked: false,
            },
        });
        let mut second = layer.operations[0].clone();
        second.coverage.id = LayerId(10);
        second.coverage.offset.x -= 35.;
        second.kind = LayerOperationKind::Fill {
            color: [0., 1., 0., 0.3],
            alpha_locked: false,
        };
        layer.operations.push(second);
        let dabs = [dab([0.1, 0.1, 0.1, 0.5])];
        let operations: Vec<_> = (0..2)
            .map(|i| DabBatch {
                kind: DabBatchKind::LayerOperation(i),
                dab_count: 0,
                damage: layer.operations[i as usize].bounds(extent),
                ..batch(1)
            })
            .collect();
        let render = |r: &mut WgpuRasterizer, layers: &[Layer], batches: &[DabBatch], reset| {
            r.submit(FramePacket {
                view: v,
                document_extent: extent,
                layers,
                dabs: &dabs,
                dab_batches: batches,
                reset_layers: reset,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        };
        render(
            &mut r,
            &[Layer::paint(LayerId(1), "gradient")],
            &[batch(1)],
            true,
        );
        render(&mut r, &[layer.clone()], &operations, false);
        let queued = r.readback_srgb_rgba8().unwrap();
        // A translated selection crosses the tile boundary, while its inverse
        // also has coverage in tiles with no original mask or paint page.
        for (x, y, inside) in [
            (240, 200, true),
            (256, 200, true),
            (340, 200, true),
            (480, 320, false),
        ] {
            let a = queued[(y * extent[0] as usize + x) * 4 + 3];
            assert_eq!(
                a > 0,
                inside != inverted,
                "coverage {inverted} at {x},{y}: {a}"
            );
        }
        let mut first_only = layer.clone();
        first_only.operations.truncate(1);
        render(
            &mut r,
            &[Layer::paint(LayerId(1), "gradient")],
            &[batch(1)],
            true,
        );
        render(&mut r, &[first_only], &operations[..1], false);
        render(&mut r, &[layer.clone()], &operations[1..], false);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            queued,
            "split frames must match queued operations"
        );
        let replay = [vec![batch(1)], operations].concat();
        render(&mut r, &[layer], &replay, true);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            queued,
            "full replay must match incremental rendering"
        );
    }
}

#[test]
fn navigator_preview_reuses_composition_and_tracks_paint_mask_and_camera() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "paint");
    layer.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[layer.clone()],
        &[dab([1.0, 0.0, 0.0, 0.5])],
        &[batch(1)],
        true,
    );
    let await_preview = |r: &mut WgpuRasterizer| {
        let start = std::time::Instant::now();
        loop {
            r.device.poll(wgpu::PollType::Poll).unwrap();
            if let Some(result) = r.take_canvas_preview() {
                break result.unwrap();
            }
            assert!(start.elapsed().as_secs() < 10, "preview map timed out");
            std::thread::yield_now();
        }
    };
    assert!(r.request_canvas_preview(None).unwrap());
    assert!(
        !r.request_canvas_preview(None).unwrap(),
        "one in-flight map"
    );
    let first = await_preview(&mut r);
    let image = first.image.unwrap();
    assert_eq!([image.width, image.height], [256, 256]);
    let at = |image: &ReadbackImage, x: usize, y: usize| -> [u8; 4] {
        image.bytes[y * image.stride as usize + x * 4..][..4]
            .try_into()
            .unwrap()
    };
    let original = at(&image, 60, 128);
    assert_eq!(&original[..3], &[255, 0, 0]);
    assert!(original[3].abs_diff(128) <= 1);
    assert_eq!(at(&image, 190, 128), [0; 4]);
    let pixels = r.metrics.composited_pixels;
    let mut camera = view();
    camera.document_to_surface = [-1.0, 0.0, 0.0, 1.0, 100.0, 50.0];
    for _ in 0..8 {
        r.submit(FramePacket {
            view: camera,
            layers: &[layer.clone()],
            document_extent: [128, 128],
            dabs: &[],
            dab_batches: &[],
            reset_layers: false,
            composite_all: false,
            time_seconds: 0.0,
        })
        .unwrap();
        assert!(r.request_canvas_preview(Some(first.revision)).unwrap());
        assert!(await_preview(&mut r).image.is_none());
    }
    assert_eq!(
        r.metrics.composited_pixels, pixels,
        "camera and overview never rebuild composition"
    );
    // Live provisional ink changes the overview before pen-up, using the same target.
    let mut b = batch(1);
    b.kind = DabBatchKind::Preview;
    submit(
        &mut r,
        &[layer.clone()],
        &[dab([0.0, 0.0, 1.0, 1.0])],
        &[b],
        false,
    );
    assert!(r.request_canvas_preview(Some(first.revision)).unwrap());
    let painted = await_preview(&mut r);
    assert_ne!(painted.revision, first.revision);
    assert_eq!(
        at(painted.image.as_ref().unwrap(), 60, 128),
        [0, 0, 255, 255]
    );
    assert_eq!(at(painted.image.as_ref().unwrap(), 190, 128), [0; 4]);
    // Removing the provisional stroke restores the persistent masked image.
    submit(&mut r, &[layer], &[], &[], false);
    assert!(r.request_canvas_preview(Some(painted.revision)).unwrap());
    assert_eq!(
        at(await_preview(&mut r).image.as_ref().unwrap(), 60, 128),
        original
    );
}

#[test]
fn clipping_stack_keeps_soft_base_alpha_and_group_opacity_once() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut base = Layer::paint(LayerId(1), "base");
    let mut a = Layer::paint(LayerId(2), "clip a");
    a.properties.clipped = true;
    let mut b = Layer::paint(LayerId(3), "clip b");
    b.properties.clipped = true;
    let mut batches = vec![batch(1), batch(2), batch(3)];
    for (i, b) in batches.iter_mut().enumerate() {
        b.first_dab = i as u32;
    }
    let dabs = [
        dab([1., 0., 0., 0.4]),
        dab([0., 1., 0., 1.]),
        dab([0., 0., 1., 1.]),
    ];
    let mut layers = vec![b.clone(), a.clone(), base.clone()];
    submit(&mut r, &layers, &dabs, &batches, true);
    assert_eq!(pixel(&mut r, 64, 64), [0, 0, 255, 102]); // export is straight-alpha sRGB
    let mut group = Layer::paint(LayerId(4), "group");
    group.kind = LayerKind::Group;
    group.opacity = 0.5;
    for l in [&mut base, &mut a, &mut b] {
        l.properties.parent = Some(group.id);
    }
    layers = vec![group, b, a, base];
    submit(&mut r, &layers, &[], &[], false);
    let p = pixel(&mut r, 64, 64);
    assert!((p[3] as i32 - 51).abs() <= 1, "{p:?}");
    layers[0].visible = false;
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
}

#[test]
fn apply_mask_preserves_pixels_and_does_not_remain_a_live_mask() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let before = r.readback_srgb_rgba8().unwrap();
    let mut mask = l.mask.take().unwrap();
    mask.show_area = false;
    l.operations.push(LayerOperation {
        after_stroke: 1,
        coverage: mask,
        kind: LayerOperationKind::ApplyMask,
    });
    let mut op = batch(1);
    op.dab_count = 0;
    op.kind = DabBatchKind::LayerOperation(0);
    submit(&mut r, &[l.clone()], &[], &[op], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    submit(&mut r, &[l], &[dab([0., 1., 0., 1.])], &[batch(1)], false);
    assert_eq!(
        pixel(&mut r, 90, 64),
        [0, 255, 0, 255],
        "new paint can extend the baked silhouette"
    );
}

#[test]
fn baked_operations_keep_the_ordinary_brush_path() {
    use layer_core::{DefaultBrushPreset::*, Figure, FigurePaint, FigureShape};
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let asset = AssetId::from("test:baked-operation");
    // Nonuniform artwork makes a pure blender observable even after a
    // transform moves the original selection boundary outside its footprint.
    let bytes: Vec<_> = (0..128 * 128)
        .flat_map(|i| [30 + (i % 128) as u8, 60 + (i / 128) as u8, 180, 170])
        .collect();
    r.prepare_asset(
        &asset,
        HostImage {
            width: 128,
            height: 128,
            stride: 512,
            format: PixelFormat::Rgba8Srgb,
            bytes: &bytes,
        },
    )
    .unwrap();
    for kind in [
        LayerOperationKind::Fill {
            color: [0.8, 0.2, 0.1, 0.6],
            alpha_locked: false,
        },
        LayerOperationKind::Gradient {
            start: Point { x: 8., y: 64. },
            end: Point { x: 120., y: 64. },
            colors: [[1., 0., 0., 0.6], [0., 0., 1., 0.6]],
            radial: false,
            alpha_locked: false,
        },
        LayerOperationKind::Figure(Figure {
            shape: FigureShape::Ellipse,
            paint: FigurePaint::Both,
            start: Point { x: 16., y: 24. },
            end: Point { x: 112., y: 104. },
            width: 8.,
            colors: [[1., 0., 0., 0.6], [0., 0., 1., 0.6]],
            alpha_locked: false,
            erase: false,
        }),
        LayerOperationKind::ApplyMask,
        LayerOperationKind::Transform(layer_core::ImageTransform {
            affine: layer_core::Affine::translation(Point { x: 24., y: 8. }),
            ..Default::default()
        }),
    ] {
        for preset in [GPen, NaturalBlender, WatercolorWash] {
            for opacity in [1., 0.45] {
                let mut reference = Vec::new();
                for keep_history in [false, true] {
                    let mut layer = Layer::paint(LayerId(1), "baked paint");
                    layer.asset = Some(asset.clone());
                    layer.opacity = opacity;
                    layer.operations.push(LayerOperation {
                        after_stroke: 0,
                        coverage: left_mask(9),
                        kind: kind.clone(),
                    });
                    let op = DabBatch {
                        kind: DabBatchKind::LayerOperation(0),
                        dab_count: 0,
                        ..batch(1)
                    };
                    submit(&mut r, &[layer.clone()], &[], &[op], true);
                    assert!(
                        r.scene.is_some(),
                        "image import initializes through scene jobs"
                    );
                    let before = r.readback_srgb_rgba8().unwrap();
                    if !keep_history {
                        // Reference: the same already-baked GPU pages without
                        // history. Discarding history must not change rendering.
                        layer.operations.clear();
                    }
                    let mut stroke = batch(1);
                    stroke.stroke_id = StrokeId(2);
                    stroke.style = preset_style(preset);
                    stroke.damage = Rect {
                        min: Point { x: 32., y: 32. },
                        max: Point { x: 96., y: 96. },
                    };
                    let mut d = dab([0.8, 0.1, 0.2, 0.7]);
                    d.radii = [24.; 2];
                    d.motion = [4., 0.];
                    d.material = [0.6, 0.8, 1., 0.8];
                    for committed in [false, true] {
                        stroke.kind = if committed {
                            DabBatchKind::Persistent
                        } else {
                            DabBatchKind::Preview
                        };
                        stroke.stroke_end = committed;
                        submit(&mut r, &[layer.clone()], &[d], &[stroke.clone()], false);
                        assert!(
                            r.scene.is_none(),
                            "baked history cannot require composition jobs"
                        );
                        if preset == GPen && opacity == 1. && !committed {
                            assert!(r.preview_direct_to_composite);
                        }
                        let image = r.readback_srgb_rgba8().unwrap();
                        assert!(
                            image != before,
                            "{kind:?}, {preset:?}, opacity {opacity}, committed {committed}: brush must change the baked image"
                        );
                        if keep_history {
                            assert_eq!(
                                image,
                                reference[usize::from(committed)],
                                "{kind:?}, {preset:?}, opacity {opacity}, committed {committed}"
                            );
                        } else {
                            reference.push(image);
                        }
                        if !committed {
                            submit(&mut r, &[layer.clone()], &[], &[], false);
                            assert_eq!(
                                r.readback_srgb_rgba8().unwrap(),
                                before,
                                "cancel preserves baked pixels"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn inspection_is_not_exported_and_translated_mask_keeps_source() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let before = r.readback_srgb_rgba8().unwrap();
    l.mask.as_mut().unwrap().show_area = true;
    submit(&mut r, &[l.clone()], &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    l.mask.as_mut().unwrap().offset.x = 40.;
    submit(&mut r, &[l], &[], &[], false);
    assert_eq!(pixel(&mut r, 20, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 80, 64), [255, 0, 0, 255]);
}

#[test]
fn imported_texture_is_linearized_premultiplied_and_masked_on_gpu() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let id = AssetId::from("test:image");
    let bytes = [128, 0, 255, 128].repeat(128 * 128);
    r.prepare_asset(
        &id,
        HostImage {
            width: 128,
            height: 128,
            stride: 512,
            format: PixelFormat::Rgba8Srgb,
            bytes: &bytes,
        },
    )
    .unwrap();
    let mut l = Layer::paint(LayerId(1), "texture");
    l.asset = Some(id);
    l.mask = Some(left_mask(9));
    submit(&mut r, &[l.clone()], &[], &[], true);
    let p = pixel(&mut r, 30, 64);
    assert!(
        (p[0] as i32 - 128).abs() <= 2 && p[2] == 255 && p[3] == 128,
        "{p:?}"
    );
    assert_eq!(pixel(&mut r, 90, 64), [0; 4]);
    l.mask = None;
    submit(&mut r, &[l], &[], &[], true);
    assert_eq!(pixel(&mut r, 90, 64), p);
}

#[test]
fn alpha_lock_preserves_partial_alpha_and_eraser_is_noop() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let l = Layer::paint(LayerId(1), "paint");
    submit(
        &mut r,
        std::slice::from_ref(&l),
        &[dab([1., 0., 0., 0.4])],
        &[batch(1)],
        true,
    );
    let mut locked = batch(1);
    locked.style.alpha_locked = true;
    submit(
        &mut r,
        std::slice::from_ref(&l),
        &[dab([0., 0., 1., 0.5])],
        &[locked.clone()],
        false,
    );
    let p = pixel(&mut r, 64, 64);
    assert!(
        (p[0] as i32 - p[2] as i32).abs() <= 1,
        "equal red/blue contributions: {p:?}"
    );
    assert_eq!(p[3], 102);
    locked.style.mode = DabMode::Erase;
    submit(&mut r, &[l], &[dab([1.; 4])], &[locked], false);
    assert_eq!(pixel(&mut r, 64, 64), p);
}

#[test]
fn mask_scene_preview_keeps_pixels_outside_preview_damage() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let mut preview = batch(1);
    preview.kind = DabBatchKind::Preview;
    preview.style.mode = DabMode::Erase;
    preview.damage = Rect {
        min: Point { x: 50., y: 50. },
        max: Point { x: 78., y: 78. },
    };
    let mut d = dab([1.; 4]);
    d.radii = [12.; 2];
    submit(&mut r, &[l.clone()], &[d], &[preview], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 30, 64), [255, 0, 0, 255]);
    submit(&mut r, &[l], &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
}

#[test]
fn mask_scene_destination_preview_preserves_untouched_color() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let mut b = batch(1);
    b.kind = DabBatchKind::Preview;
    b.style.alpha_locked = true;
    b.damage = Rect {
        min: Point { x: 20., y: 50. },
        max: Point { x: 44., y: 78. },
    };
    let mut d = dab([0., 0., 1., 1.]);
    d.center.x = 32.;
    d.radii = [10.; 2];
    submit(&mut r, &[l], &[d], &[b], false);
    assert_eq!(pixel(&mut r, 32, 64), [0, 0, 255, 255]);
    assert_eq!(pixel(&mut r, 50, 64), [255, 0, 0, 255]);
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn selected_brush_latency() {
    use layer_core::DefaultBrushPreset::*;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.set_telemetry_enabled(true);
    let extent = [2048, 1536];
    let v = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    let layers = [Layer::paint(LayerId(1), "selected brush latency")];
    let selection = std::sync::Arc::new(
        Selection::polygon(
            (0..256)
                .map(|i| {
                    let a = i as f32 / 256. * std::f32::consts::TAU;
                    Point {
                        x: 1024. + 1000. * a.cos(),
                        y: 768. + 740. * a.sin(),
                    }
                })
                .collect(),
        )
        .unwrap(),
    );
    for preset in [GPen, NaturalBlender, WatercolorWash] {
        for selected in [false, true] {
            let mut b = batch(1);
            b.style = preset_style(preset);
            b.style.selection = selected.then(|| selection.clone());
            b.dab_count = 8;
            b.damage = Rect {
                min: Point { x: 600., y: 550. },
                max: Point { x: 1050., y: 1000. },
            };
            let dabs: Vec<_> = (0..8)
                .map(|i| {
                    let mut d = dab([0.2, 0.4, 0.8, 0.8]);
                    d.center = Point {
                        x: 800. + i as f32 * 4.,
                        y: 768.,
                    };
                    d.radii = [192.; 2];
                    d.motion = [4., 0.];
                    d.material = [0.6, 0.8, 1., 0.8];
                    d
                })
                .collect();
            let mut completed = Vec::new();
            let mut generation = 0;
            for i in 0..160 {
                let start = std::time::Instant::now();
                b.stroke_id = StrokeId(i + 1);
                r.submit(FramePacket {
                    view: v,
                    document_extent: extent,
                    layers: &layers,
                    dabs: &dabs,
                    dab_batches: std::slice::from_ref(&b),
                    reset_layers: i == 0,
                    time_seconds: 0.,
                    composite_all: false,
                })
                .unwrap();
                r.wait_idle().unwrap();
                if i >= 40 {
                    completed.push(start.elapsed().as_secs_f32() * 1000.);
                }
                if i == 0 {
                    generation = r.selection_clip.generations;
                }
                assert_eq!(
                    r.selection_clip.generations, generation,
                    "unchanged selection is cached across strokes"
                );
            }
            let summary = |mut values: Vec<f32>| {
                values.sort_by(f32::total_cmp);
                [values[60], values[114], values[118]]
            };
            let stats = r.telemetry();
            assert!(stats.gpu_timestamps);
            eprintln!(
                "{preset:?} selected={selected} p50/p95/p99 ms CPU {:.3?} GPU {:.3?} completed {:.3?}",
                summary(stats.cpu.ordered()),
                summary(stats.gpu.ordered()),
                summary(completed)
            );
        }
    }
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn selection_raster_latency() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.ensure_document([2048, 1536], &[]).unwrap();
    for vertices in [4, 256, 4096] {
        let mut times = Vec::new();
        for i in 0..12 {
            let points = (0..vertices)
                .map(|j| {
                    let a = j as f32 / vertices as f32 * std::f32::consts::TAU;
                    Point {
                        x: 1024. + a.cos() * (1000. + i as f32 * 0.1),
                        y: 768. + a.sin() * 740.,
                    }
                })
                .collect();
            let mut style = batch(1).style;
            style.selection = Some(std::sync::Arc::new(Selection::polygon(points).unwrap()));
            let start = std::time::Instant::now();
            let mut encoder = r.device.create_command_encoder(&Default::default());
            r.prepare_selection(&mut encoder, &style).unwrap();
            let submission = r.queue.submit([encoder.finish()]);
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            if i >= 2 {
                times.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "selection {vertices} vertices, {} bytes, prepare + GPU completion median {:.3}ms max {:.3}ms",
            r.selection_clip.bytes, times[5], times[9]
        );
    }
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn paint_operation_latency() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.set_telemetry_enabled(true);
    let extent = [2048, 1536];
    let v = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    // The editor displays paper before accepting the first tool gesture. Exclude
    // device uploads and canvas allocation, but not first operation resources.
    r.submit(FramePacket {
        view: v,
        document_extent: extent,
        layers: &[Layer::paint(LayerId(1), "paint operation")],
        dabs: &[],
        dab_batches: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: false,
    })
    .unwrap();
    r.wait_idle().unwrap();
    for selected in [false, true] {
        for (name, kind) in [
            (
                "fill",
                LayerOperationKind::Fill {
                    color: [0.3, 0.4, 0.8, 0.5],
                    alpha_locked: false,
                },
            ),
            (
                "linear",
                LayerOperationKind::Gradient {
                    start: Point { x: 500., y: 500. },
                    end: Point { x: 1200., y: 900. },
                    colors: [[1., 0., 0., 0.5], [0., 0., 1., 0.5]],
                    radial: false,
                    alpha_locked: false,
                },
            ),
            (
                "radial",
                LayerOperationKind::Gradient {
                    start: Point { x: 500., y: 500. },
                    end: Point { x: 1200., y: 900. },
                    colors: [[1., 0., 0., 0.5], [0., 0., 1., 0.5]],
                    radial: true,
                    alpha_locked: false,
                },
            ),
        ] {
            let mut layer = Layer::paint(LayerId(1), "paint operation");
            let coverage = if selected {
                left_mask(9)
            } else {
                LayerMask::reveal_all(LayerId(9), Point::default())
            };
            layer.operations.push(LayerOperation {
                after_stroke: 0,
                coverage,
                kind,
            });
            let op = DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                damage: layer.operations[0].bounds(extent),
                ..batch(1)
            };
            let mut completed = Vec::new();
            let mut cold = 0.;
            for i in 0..160 {
                let start = std::time::Instant::now();
                r.submit(FramePacket {
                    view: v,
                    document_extent: extent,
                    layers: std::slice::from_ref(&layer),
                    dabs: &[],
                    dab_batches: std::slice::from_ref(&op),
                    reset_layers: i == 0,
                    time_seconds: 0.,
                    composite_all: false,
                })
                .unwrap();
                r.wait_idle().unwrap(); // Test only; production never waits.
                let ms = start.elapsed().as_secs_f64() * 1000.;
                if i == 0 {
                    cold = ms;
                }
                if i >= 40 {
                    completed.push(ms);
                }
            }
            let stats = r.telemetry();
            assert!(stats.gpu_timestamps);
            let summary = |mut values: Vec<f32>| {
                values.sort_by(f32::total_cmp);
                [values[60], values[114], values[118]]
            };
            let cpu = summary(stats.cpu.ordered());
            let gpu = summary(stats.gpu.ordered());
            completed.sort_by(f64::total_cmp);
            eprintln!(
                "{name} selection={selected}: cold completed={cold:.3}ms; CPU {cpu:.3?}; GPU {gpu:.3?}; completed p50/p95/p99={:.3}/{:.3}/{:.3}ms",
                completed[60], completed[114], completed[118]
            );
        }
    }
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn layer_composition_latency() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    for (name, count, masked) in [
        ("plain", 1, false),
        ("masked", 1, true),
        ("24 masks", 24, true),
        ("100 masks", 100, true),
    ] {
        let layers: Vec<_> = (1..=count)
            .map(|id| {
                let mut l = Layer::paint(LayerId(id), "layer");
                if masked {
                    l.mask = Some(left_mask(id + 1000));
                }
                l
            })
            .collect();
        let dabs: Vec<_> = (1..=count).map(|_| dab([0.2, 0.3, 0.4, 0.2])).collect();
        let batches: Vec<_> = (1..=count)
            .map(|id| {
                let mut b = batch(id);
                b.first_dab = (id - 1) as u32;
                b
            })
            .collect();
        submit(&mut r, &layers, &dabs, &batches, true);
        r.wait_idle().unwrap();
        let mut times = Vec::new();
        for i in 0..140 {
            let start = std::time::Instant::now();
            submit(&mut r, &layers, &dabs[..1], &[batch(1)], false);
            r.wait_idle().unwrap();
            if i >= 20 {
                times.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "{name}: completed GPU frame ms p50={:.3} p95={:.3} p99={:.3}",
            times[60], times[114], times[118]
        );
    }
}
