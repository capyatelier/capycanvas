//! Pixel assertions and bounded GPU-completion timings for layer composition.
use super::*;
use layer_core::{
    BrushDeform, BrushRendering, BrushWetMix, LayerMask, LayerOperation, LayerOperationKind, Point,
    Rect, Selection, StrokeId,
};
use layer_render::{DabStyle, ViewState};
#[path = "tonal_tests.rs"]
mod tonal_selection;
#[path = "selection_option_tests.rs"]
mod selection_options;
#[path = "selection_paint_tests.rs"]
mod selection_painting;
#[path = "submission_tests.rs"]
mod submissions;
#[path = "paint_transform_tests.rs"]
mod transforms;
#[path = "transform_latency_tests.rs"]
mod transform_latency;
#[path = "transform_oracle_tests.rs"]
mod transform_oracles;
#[path = "placement_tests.rs"]
mod placement;

fn view() -> ViewState {
    ViewState {
        width_px: 128,
        height_px: 128,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}
pub(super) fn dab(color: [f32; 4]) -> Dab {
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
        previous: [0.0; 4],
        contact: [0.0; 4],
        previous_contact: [0.0; 4],
    }
}
pub(super) fn batch(id: u64) -> DabBatch {
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
            brush_to_layer: layer_core::Affine::IDENTITY,
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
            contact: None,
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
        restore_rasters: &[],
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
fn cursor_triangle_and_single_pixel_dot_render_at_native_scale() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "Empty")], &[], &[], true);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cursor pixel reference"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let mut presenter =
        crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
    let camera = ViewState {
        background_rgba_linear: [1.; 4],
        ..view()
    };
    for scale in [1., 1.5, 2., 3.] {
        for turns in 0..4 {
            presenter.set_surface_rotation(turns);
            presenter.set_cursor(r.device(), &[], scale);
            presenter
                .present(&r, &target_view, camera, [1.; 4])
                .unwrap();
            let baseline = page_bytes(&r, &target);
            let x = (20.25_f32 * scale).floor() / scale;
            let y = (24.75_f32 * scale).floor() / scale;
            presenter.set_cursor(
                r.device(),
                &[layer_render::CursorSegment {
                    from: [x, y],
                    to: [x + 1. / scale, y + 1. / scale],
                    distance: 0.,
                    marker: 2.,
                    scale: 1.,
                }],
                scale,
            );
            presenter
                .present(&r, &target_view, camera, [1.; 4])
                .unwrap();
            let actual = page_bytes(&r, &target);
            let changed = actual
                .chunks_exact(4)
                .zip(baseline.chunks_exact(4))
                .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 8))
                .count();
            assert_eq!(
                changed, 1,
                "single-pixel dot: scale {scale}, rotation {turns}"
            );
        }
    }
    presenter.set_surface_rotation(0);
    for background in [[0., 0., 0., 1.], [1.; 4]] {
        let mut fill = dab(background);
        fill.radii = [100.; 2];
        submit(&mut r, &[Layer::paint(LayerId(1), "Background")], &[fill], &[batch(1)], true);
        let camera = ViewState {
            background_rgba_linear: background,
            ..view()
        };
        presenter.set_cursor(r.device(), &[], 1.);
        presenter
            .present(&r, &target_view, camera, background)
            .unwrap();
        let baseline = page_bytes(&r, &target);
        presenter.set_cursor(
            r.device(),
            &[layer_render::CursorSegment {
                from: [32., 32.],
                to: [42., 46.],
                distance: 0.,
                marker: 3.,
                scale: 1.,
            }],
            1.,
        );
        presenter
            .present(&r, &target_view, camera, background)
            .unwrap();
        let triangle = page_bytes(&r, &target);
        let changed = triangle
            .chunks_exact(4)
            .zip(baseline.chunks_exact(4))
            .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 8))
            .count();
        assert!(
            changed > 15,
            "triangle is visible on light and dark: {changed}"
        );
        for (x, y) in [(40, 33), (33, 44)] {
            let index = (y * 128 + x) * 4;
            assert_eq!(
                triangle[index..index + 4],
                baseline[index..index + 4],
                "outside triangle"
            );
        }
        if background[0] == 1. {
            assert!(
                triangle[(41 * 128 + 37) * 4] < 16,
                "triangle has a filled interior"
            );
        }
        presenter.set_cursor(r.device(), &[], 1.);
        presenter
            .present(&r, &target_view, camera, background)
            .unwrap();
        assert_eq!(
            page_bytes(&r, &target),
            baseline,
            "hiding the cursor restores the untouched canvas"
        );
    }
}

#[test]
fn retained_scene_viewport_preserves_pixels_outside_local_paint_and_preview_damage() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let make_target = || r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("retained scene viewport regression"),
        size: wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST, view_formats: &[],
    });
    let target = make_target();
    let reference = make_target();
    let buffered = make_target();
    let shared_again = make_target();
    let mut layer = Layer::paint(LayerId(1), "scene paint");
    layer.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    let layers = [layer];
    let camera = ViewState { width_px: 512, height_px: 512,
        background_rgba_linear: [0.2, 0.3, 0.4, 1.], ..view() };
    let mut retained = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
    retained.set_target_retention(true);
    for (i, (x, y, preview)) in [(85., 90., false), (365., 330., true),
        (95., 370., true), (360., 100., false)].into_iter().enumerate() {
        let mut ink = dab([0.8, 0.1, 0.2, 0.7]);
        ink.center = Point { x, y };
        ink.radii = [24.; 2];
        ink.previous = [24., 24., 1., 0.];
        ink.contact = [1., 0., 0., 0.];
        let mut stroke = batch(1);
        stroke.kind = if preview { DabBatchKind::Preview } else { DabBatchKind::Persistent };
        stroke.style = preset_style(layer_core::DefaultBrushPreset::Pencil);
        stroke.damage = ink.bounds();
        r.submit(FramePacket { view: camera, document_extent: [1024; 2], layers: &layers,
            dabs: &[ink], dab_batches: &[stroke], restore_rasters: &[],
            reset_layers: i == 0, composite_all: i == 0, time_seconds: 0., }).unwrap();
        if i > 0 { assert!(r.composite_damage.area() < 1024 * 1024); }
        retained.present(&r, &target.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
        let mut full = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
        full.present(&r, &reference.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
        assert_eq!(page_bytes(&r, &target), page_bytes(&r, &reference), "frame {i}");
    }
    // A swapchain mode change replaces the image even when the scene and view
    // are unchanged. Neither transition may inherit old image damage/history.
    retained.set_target_retention(false);
    retained.present(&r, &buffered.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
    assert_eq!(page_bytes(&r, &buffered), page_bytes(&r, &reference), "new buffered image");
    retained.set_target_retention(true);
    retained.present(&r, &shared_again.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
    assert_eq!(page_bytes(&r, &shared_again), page_bytes(&r, &reference), "new shared image");
}

#[test]
fn cursor_marks_have_dark_centers_light_surrounds_and_matching_silhouettes() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "Empty")], &[], &[], true);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cursor mark reference"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut presenter =
        crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
    for (name, marker, radius) in [("cross", 4., 5.), ("dot", 4., 1.5), ("sight", 5., 7.)] {
        for scale in [1.0_f32, 2., 3.] {
            let center = (64. + (scale % 2.) * 0.5) / scale;
            for light in [false, true] {
                let background = if light { [1.; 4] } else { [0., 0., 0., 1.] };
                let mut fill = dab(background);
                fill.radii = [100.; 2];
                submit(
                    &mut r,
                    &[Layer::paint(LayerId(1), "Background")],
                    &[fill],
                    &[batch(1)],
                    true,
                );
                let camera = ViewState {
                    background_rgba_linear: background,
                    ..view()
                };
                presenter.set_cursor(
                    r.device(),
                    &[layer_render::CursorSegment {
                        from: [center - radius; 2],
                        to: [center + radius; 2],
                        distance: 0.,
                        marker,
                        scale: 1.,
                    }],
                    scale,
                );
                presenter
                    .present(
                        &r,
                        &target.create_view(&Default::default()),
                        camera,
                        background,
                    )
                    .unwrap();
                let pixels = page_bytes(&r, &target);
                let at = |x, y| pixels[(y * 128 + x) * 4];
                assert!(at(64, 64) < 16, "{name} center is dark at scale {scale}");
                if scale == 1. {
                    if name == "dot" {
                        for y in -2_i32..=2 {
                            for x in -2_i32..=2 {
                                if light {
                                    assert_eq!(
                                        at((64 + x) as usize, (64 + y) as usize) < 16,
                                        (x == 0 && y.abs() <= 1) || (y == 0 && x.abs() <= 1),
                                        "Dot is a five-pixel cross, not a square or ring: {x},{y}"
                                    );
                                }
                            }
                        }
                        assert!(at(65, 65) > 180, "light surround at cross corners");
                    } else if name == "sight" {
                        for (x, y) in [(59, 64), (69, 64), (64, 59), (64, 69)] {
                            assert!(at(x, y) < 16, "sight arms are dark");
                        }
                        assert!(at(69, 65) > 180, "sight has a light surround");
                        assert_eq!(
                            at(66, 66),
                            if light { 255 } else { 0 },
                            "sight center gap remains open"
                        );
                    } else {
                        assert!(at(67, 64) < 16);
                        assert!(at(67, 65) > 180);
                    }
                }
                if let Some(dir) = std::env::var_os("CAPY_CURSOR_TEST_ARTIFACTS") {
                    std::fs::create_dir_all(&dir).unwrap();
                    let file = std::fs::File::create(std::path::Path::new(&dir).join(format!(
                        "{name}-{scale}-{}.png",
                        if light { "light" } else { "dark" }
                    )))
                    .unwrap();
                    let mut encoder = png::Encoder::new(file, 128, 128);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    encoder
                        .write_header()
                        .unwrap()
                        .write_image_data(&pixels)
                        .unwrap();
                }
            }
        }
    }
}

#[test]
fn retained_viewport_matches_full_redraw_after_paint_and_preview_replacement() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let layers = [Layer::paint(LayerId(1), "Ink")];
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let make_target = || {
        r.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 128,
                height: 128,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let target = make_target();
    let reference = make_target();
    for retained in [false, true] {
        let mut cached =
            crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb)
                .unwrap();
        cached.set_target_retention(retained);
        cached.gpu_timings(&r, true);
        for (zoom, turns) in [(0.5, 0), (1., 1), (3.7, 2), (8., 3)] {
            let camera = ViewState {
                document_to_surface: [zoom, 0., 0., zoom, 64. - zoom * 64., 64. - zoom * 64.],
                ..view()
            };
            cached.set_surface_rotation(turns);
            for (i, (x, y, preview)) in [
                (55., 61., false),
                (68., 69., false),
                (57., 73., true),
                (71., 62., true),
            ]
            .into_iter()
            .enumerate()
            {
                let mut dab = dab([0.7, 0.1, 0.05, 1.]);
                dab.center = Point { x, y };
                dab.radii = [3., 3.];
                let mut batch = batch(1);
                batch.kind = if preview {
                    DabBatchKind::Preview
                } else {
                    DabBatchKind::Persistent
                };
                batch.damage = Rect {
                    min: Point {
                        x: x - 10.,
                        y: y - 10.,
                    },
                    max: Point {
                        x: x + 10.,
                        y: y + 10.,
                    },
                };
                r.submit(FramePacket {
                    view: camera,
                    document_extent: [128; 2],
                    layers: &layers,
                    dabs: &[dab],
                    dab_batches: &[batch],
                    restore_rasters: &[],
                    reset_layers: i == 0,
                    composite_all: i == 0,
                    time_seconds: 0.,
                })
                .unwrap();
                if i > 0 {
                    assert!(r.composite_damage.area() < 128 * 128);
                }
                let overview = crate::OverviewPlacement {
                    bounds: [if i < 2 { 98. } else { 82. }, 4., 24., 24.],
                    clip: None,
                    work_area: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
                    outline_linear: [0.8, 0.2, 0.3],
                    background_linear: [0.3; 3],
                    scale: 1.,
                    opacity: 0.7,
                };
                let overviews = if i == 3 {
                    &[][..]
                } else {
                    std::slice::from_ref(&overview)
                };
                // Keep paint, Navigator and cursor as three disjoint regions.
                // A middle pass must not have empty timestamp-write indices.
                let marker = [layer_render::CursorSegment {
                    from: [8., 108.], to: [14., 114.], distance: 0., marker: 0., scale: 1.,
                }];
                cached.set_cursor(r.device(), &marker, 1.);
                cached.set_overviews(&r, overviews);
                cached
                    .present(
                        &r,
                        &target.create_view(&Default::default()),
                        camera,
                        [0.2; 4],
                    )
                    .unwrap();
                let mut full =
                    crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb)
                        .unwrap();
                full.set_surface_rotation(turns);
                full.set_overviews(&r, overviews);
                full.set_cursor(r.device(), &marker, 1.);
                full.present(
                    &r,
                    &reference.create_view(&Default::default()),
                    camera,
                    [0.2; 4],
                )
                .unwrap();
                assert_eq!(
                    page_bytes(&r, &target),
                    page_bytes(&r, &reference),
                    "retained {retained}, zoom {zoom}, rotation {turns}, frame {i}"
                );
                if retained {
                    for (x, y, visible) in [
                        (20., 15., true),
                        (114., 105., true),
                        (100., 16., true),
                        (0., 0., false),
                    ] {
                        let cursor = [layer_render::CursorSegment {
                            from: [x, y],
                            to: [x + 5., y + 8.],
                            distance: 0.,
                            marker: 1.,
                            scale: 1.,
                        }];
                        let segments = if visible { &cursor[..] } else { &[] };
                        cached.set_cursor(r.device(), segments, 1.7);
                        cached
                            .present(
                                &r,
                                &target.create_view(&Default::default()),
                                camera,
                                [0.2; 4],
                            )
                            .unwrap();
                        let mut full = crate::ViewportPresenter::for_surface(
                            &r,
                            format,
                            crate::SdrSurfaceColor::Srgb,
                        )
                        .unwrap();
                        full.set_surface_rotation(turns);
                        full.set_overviews(&r, overviews);
                        full.set_cursor(r.device(), segments, 1.7);
                        full.present(
                            &r,
                            &reference.create_view(&Default::default()),
                            camera,
                            [0.2; 4],
                        )
                        .unwrap();
                        assert_eq!(
                            page_bytes(&r, &target),
                            page_bytes(&r, &reference),
                            "cursor restored, zoom {zoom}, rotation {turns}, frame {i}"
                        );
                    }
                }
            }
        }
    }
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
        contiguous: true,
        selection: None,
        request_id: 7,
        source: RegionSource::Layer(line.id),
        position: [64, 64],
        tolerance: 0.,
        refinement: Default::default(),
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
        fill.pending_operations = vec![LayerOperation {
            placement: layer_core::Affine::IDENTITY,
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
        fill.pending_operations.clear();
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
    for (seed, expansion, bounds) in [
        ([32, 64], 0, [17, 17, 64, 112]),
        ([80, 64], 0, [0; 4]),
        ([32, 64], 3, [14, 14, 64, 115]),
        ([80, 64], 3, [0; 4]),
        ([32, 64], -2, [19, 19, 62, 110]),
    ] {
        assert!(
            r.request_region(RegionRequest {
                contiguous: true,
                selection: None,
                request_id: 8,
                source: RegionSource::Composite,
                position: seed,
                tolerance: 0.,
                refinement: layer_render::RegionRefinement {
                    expansion,
                    smoothing: 1.,
                    ..Default::default()
                },
                limit: left_mask(9).initial.map(std::sync::Arc::new)
            })
            .unwrap()
        );
        r.wait_idle().unwrap();
        let refined = receive(&mut r).pixels;
        assert_eq!(refined.bounds(), bounds);
        // The final selection limit is applied after expansion and antialiasing.
        for y in 0..128 {
            assert!(
                refined.words()[y * 16 + 8..y * 16 + 16]
                    .iter()
                    .all(|w| *w == 0)
            );
        }
    }
    assert_eq!(
        result.pixels.bounds(),
        [17, 17, 112, 112],
        "later detection never rewrites history"
    );
}

#[test]
fn refined_region_antialias_survives_fill_and_history_replay() {
    use layer_render::{RegionRefinement, RegionRequest, RegionSource};
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let asset = AssetId::from("test:diagonal-region");
    let pixels: Vec<_> = (0_u32..128 * 128)
        .flat_map(|i| {
            if (i % 128).abs_diff(64) + (i / 128).abs_diff(64) < 40 {
                [255; 4]
            } else {
                [0, 0, 0, 255]
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
    let mut source = Layer::paint(LayerId(1), "Source");
    source.asset = Some(asset);
    submit(&mut r, &[source], &[], &[], true);
    assert!(
        r.request_region(RegionRequest {
            contiguous: true,
            selection: None,
            request_id: 1,
            source: RegionSource::Composite,
            position: [64, 64],
            tolerance: 0.,
            refinement: RegionRefinement {
                smoothing: 1.,
                ..Default::default()
            },
            limit: None,
        })
        .unwrap()
    );
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    let pixels = loop {
        if let Some(result) = r.take_region() {
            break result.unwrap().pixels;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    };
    let mut expected = None;
    for replay in [false, true] {
        // Distinct owned history storage cannot reuse the live GPU result.
        let selected = if replay {
            std::sync::Arc::new(
                layer_core::SelectionPixels::new(
                    pixels.extent(),
                    pixels.bounds(),
                    pixels.words().to_vec(),
                )
                .unwrap(),
            )
        } else {
            pixels.clone()
        };
        let mut fill = Layer::paint(LayerId(2), "Fill");
        let mut coverage = LayerMask::reveal_all(LayerId(3), Point::default());
        coverage.initial = Some(Selection::pixels(selected));
        coverage.default_coverage = 0.;
        fill.pending_operations.push(LayerOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage,
            kind: LayerOperationKind::Fill {
                color: [0., 0., 1., 1.],
                alpha_locked: false,
            },
        });
        submit(
            &mut r,
            &[fill],
            &[],
            &[DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                ..batch(2)
            }],
            true,
        );
        let output = r.readback_srgb_rgba8().unwrap();
        let mut partial = 0;
        for y in 0..128 {
            for x in 0..128 {
                let quarters = pixels.words()[y * 16 + x / 8] >> (x % 8 * 4) & 15;
                let alpha = output[(y * 128 + x) * 4 + 3];
                assert!(
                    (i32::from(alpha) - (quarters as f32 * 63.75).round() as i32).abs() <= 1,
                    "{x},{y}"
                );
                partial += usize::from(quarters > 0 && quarters < 4);
            }
        }
        assert!(partial > 40, "diagonal edges must really be antialiased");
        if let Some(expected) = &expected {
            assert_eq!(&output, expected);
        } else {
            expected = Some(output);
        }
    }
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
                contiguous: true,
                selection: None,
                request_id: 1,
                source: RegionSource::Composite,
                position: [40, 64],
                tolerance: 0.1,
                refinement: Default::default(),
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
                contiguous: true,
                selection: None,
                request_id: 2,
                source: RegionSource::Layers(refs),
                position: [40, 64],
                tolerance: 0.1,
                refinement: Default::default(),
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

// Inspect persistent pigment/wetness independently of layer-level effects.
// This is test-only readback, never a drawing or selection raster path.
pub(super) fn page_bytes(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<u8> {
    let row = texture.width() * texture.format().block_copy_size(None).unwrap();
    let stride = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test persistent page"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    let submission = encoder.submit(&r.queue);
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
    let bytes = buffer.slice(..).get_mapped_range().unwrap()
        .chunks_exact(stride as usize).flat_map(|line| line[..row as usize].iter().copied()).collect();
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

pub(crate) fn preset_style(preset: layer_core::DefaultBrushPreset) -> DabStyle {
    let brush = layer_core::default_brush(preset);
    DabStyle {
        brush_to_layer: layer_core::Affine::IDENTITY,
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
        contact: brush.contact,
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
            let generations = r.selection_clip.generations;
            let mut layer = Layer::paint(LayerId(1), "affine coverage");
            let mut mask = LayerMask::reveal_all(LayerId(2), Point::default());
            mask.default_coverage = f32::from(inverted);
            mask.initial = Some(selection.clone());
            layer.pending_operations = vec![LayerOperation {
                placement: layer_core::Affine::IDENTITY,
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
            let storage = r.selection_clip.storage_bytes();
            layer.pending_operations.clear();
            let selected = DabBatch {
                style: DabStyle {
                    selection: Some(Arc::new(selection.clone())),
                    ..batch(1).style
                },
                ..batch(1)
            };
            submit(&mut r, &[layer.clone()], &[white], &[selected], true);
            // The two physical paths can choose adjacent UNORM alpha
            // values at a half-code boundary; RGB and coverage stay within one code.
            let actual = r.readback_srgb_rgba8().unwrap();
            assert!(actual.iter().zip(&filled).all(|(a, b)| a.abs_diff(*b) <= 1));
            layer.mask = Some(mask);
            submit(&mut r, &[layer], &[white], &[batch(1)], true);
            // The two physical paths can choose adjacent UNORM alpha
            // values at a half-code boundary; RGB and coverage stay within one code.
            let actual = r.readback_srgb_rgba8().unwrap();
            assert!(actual.iter().zip(&filled).all(|(a, b)| a.abs_diff(*b) <= 1));
            assert!(
                r.selection_clip.generations - generations <= 1,
                "all consumers share one preparation; empty fills may skip it entirely"
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
        ).unwrap();
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
            ).unwrap();
            assert_eq!(page_bytes(&r, &target), expected);
            assert_eq!(
                r.selection_clip.generations, generations,
                "presentation never rasterizes the selection"
            );
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
            restore_rasters: &[],
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
            area: Default::default(),
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
        restore_rasters: &[],
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
fn averaged_sampling_crosses_sparse_pages_and_ignores_transparent_rgb() {
    use layer_core::{color::PixelDescriptor, raster::{RasterData, RasterPlane, RasterTile, TileBlob, TileKey}};
    use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.ensure_document([512, 512], &[Layer::paint(LayerId(1), "sample")]).unwrap();
    let mut data = RasterData::default();
    for (coordinate, texel) in [
        ([0, 0], [255, 0, 0, 255]),
        ([1, 0], [0, 188, 0, 128]),
        // Deliberately noncanonical hidden RGB must never tint an average.
        ([0, 1], [0, 0, 255, 0]),
    ] {
        data.tiles.insert(
            TileKey { plane: RasterPlane::Color, coordinate },
            RasterTile::backed(TileBlob::encode(PixelDescriptor::SRGB8_PAINT, &texel.repeat(256 * 256)).unwrap()),
        );
    }
    r.restore_raster(LayerId(1), &RasterData::default(), &data).unwrap();
    let revision = r.composite_revision;
    let mut sample = |position, area| {
        assert!(r.request_color_sample(ColorSampleRequest {
            request_id: 9, source: ColorSampleSource::Layer(LayerId(1)), position, area,
        }).unwrap());
        r.device.poll(wgpu::PollType::Wait {
            submission_index: None, timeout: Some(Duration::from_secs(5)),
        }).unwrap();
        r.take_color_sample().unwrap().unwrap().rgba
    };
    let area = sample([255, 255], ColorSampleArea::Average5);
    // Independent f64 reference: 9 opaque red, 6 half-covered green, 10 clear.
    let alpha = 9. + 6. * 128. / 255.;
    let green = ((188f64 / 255. + 0.055) / 1.055).powf(2.4);
    let expected = [9. / alpha, 6. * green / alpha, 0., alpha / 25.];
    for (actual, expected) in area.into_iter().zip(expected) {
        assert!((f64::from(actual) - expected).abs() < 0.000001, "{area:?}");
    }
    assert_eq!(sample([0, 0], ColorSampleArea::Average3), [1., 0., 0., 1.]);
    assert_eq!(sample([400, 400], ColorSampleArea::Average5), [0.; 4]);
    assert_eq!(sample([u32::MAX, 0], ColorSampleArea::Point), [0.; 4]);
    assert_eq!(r.composite_revision, revision, "inspection must not recompose");
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
                    layer.pending_operations.push(LayerOperation {
                        placement: layer_core::Affine::IDENTITY,
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
    gradient.pending_operations.push(LayerOperation {
        placement: layer_core::Affine::IDENTITY,
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
        layer.pending_operations.push(LayerOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage,
            kind: LayerOperationKind::Gradient {
                start: Point { x: 160., y: 64. },
                end: Point { x: 400., y: 64. },
                colors: [[1., 0., 0., 0.8], [0., 0., 1., 0.8]],
                radial: false,
                alpha_locked: false,
            },
        });
        let mut second = layer.pending_operations[0].clone();
        second.coverage.id = LayerId(10);
        second.coverage.offset.x -= 35.;
        second.kind = LayerOperationKind::Fill {
            color: [0., 1., 0., 0.3],
            alpha_locked: false,
        };
        layer.pending_operations.push(second);
        let dabs = [dab([0.1, 0.1, 0.1, 0.5])];
        let operations: Vec<_> = (0..2)
            .map(|i| DabBatch {
                kind: DabBatchKind::LayerOperation(i),
                dab_count: 0,
                damage: layer.pending_operations[i as usize].bounds(extent),
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
                restore_rasters: &[],
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
        first_only.pending_operations.truncate(1);
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
    l.pending_operations.push(LayerOperation {
        placement: layer_core::Affine::IDENTITY,
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
fn normal_stack_fusion_matches_unfused_layers_with_opacity_and_preview() {
    let mut r = WgpuRasterizer::new_float32().unwrap();
    for masks in 0..4 {
        let mut top = Layer::paint(LayerId(1), "top paint");
        top.opacity = 0.43;
        if masks & 1 != 0 { top.mask = Some(left_mask(8)); }
        let mut lower = Layer::paint(LayerId(2), "lower paint");
        lower.opacity = 0.61;
        // A scalar mask forces scene composition on blendable Float32 GPUs.
        lower.mask = Some(if masks & 2 != 0 { left_mask(9) }
            else { LayerMask::reveal_all(LayerId(9), Point::default()) });
        let layers = [top, lower];
        let mut lower_dab = dab([0.2, 0.7, 0.4, 0.8]);
        lower_dab.radii = [90.; 2];
        let base = dab([0.8, 0.1, 0.3, 0.7]);
        let mut lower_batch = batch(2);
        lower_batch.first_dab = 1;
        submit(&mut r, &layers, &[base, lower_dab], &[batch(1), lower_batch], true);
        let mut ink = dab([0.1, 0.3, 0.9, 0.6]);
        ink.radii = [24., 18.];
        ink.contact = [0.7, 0., 0., 0.];
        let mut stroke = batch(1);
        stroke.stroke_id = StrokeId(2);
        stroke.style = preset_style(layer_core::DefaultBrushPreset::Marker);
        stroke.damage = ink.bounds();
        stroke.kind = DabBatchKind::Preview;
        for preview in [false, true] {
            r.scene.as_mut().unwrap().set_tiled_composition(false);
            submit(&mut r, &layers, if preview { std::slice::from_ref(&ink) } else { &[] },
                if preview { std::slice::from_ref(&stroke) } else { &[] }, false);
            let fused = page_bytes(&r, r.composite_texture.as_ref().unwrap());
            r.scene.as_mut().unwrap().set_tiled_composition(true);
            submit(&mut r, &layers, if preview { std::slice::from_ref(&ink) } else { &[] },
                if preview { std::slice::from_ref(&stroke) } else { &[] }, false);
            let reference = page_bytes(&r, r.composite_texture.as_ref().unwrap());
            let error = fused.chunks_exact(4).zip(reference.chunks_exact(4))
                .map(|(a,b)| (f32::from_le_bytes(a.try_into().unwrap()) - f32::from_le_bytes(b.try_into().unwrap())).abs())
                .fold(0., f32::max);
            assert!(error < 0.00001, "masks={masks}, preview={preview}: {error}");
        }
        r.scene.as_mut().unwrap().set_tiled_composition(false);
    }
}

#[test]
fn small_swept_contact_preview_matches_commit_and_preserves_distant_pixels() {
    use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::Srgb, depth: SampleDepth::U8,
    }).unwrap();
    let layers = [Layer::paint(LayerId(1), "small swept preview")];
    let render = |r: &mut WgpuRasterizer, dabs: &[Dab], batches: &[DabBatch], reset| {
        r.submit(FramePacket {
            view: view(), document_extent: [1024; 2], layers: &layers,
            dabs, dab_batches: batches, restore_rasters: &[], reset_layers: reset,
            time_seconds: 0., composite_all: true,
        }).unwrap();
    };
    for preset in layer_core::CONTACT_BRUSH_PRESETS {
        let mut base = dab([0.7, 0.1, 0.2, 0.8]);
        base.center = Point { x: 512., y: 512. };
        base.radii = [900.; 2];
        let mut base_batch = batch(1);
        base_batch.damage = base.bounds();
        let mut first = dab([0.1, 0.3, 0.8, 0.7]);
        first.center = Point { x: 260., y: 255. };
        first.radii = [9., 4.];
        first.previous = [0.6, 1.2, 0.8, 0.6];
        first.motion = [29., -17.];
        first.contact = [0.9, 0.7, 1., 0.3];
        first.previous_contact = [0.1, 0.2, 0., 0.3];
        let mut last = first;
        last.center = Point { x: 770., y: 790. };
        last.motion = [-21., 31.];
        let mut stroke = batch(1);
        stroke.stroke_id = StrokeId(2);
        stroke.style = preset_style(preset);
        stroke.dab_count = 2;
        stroke.damage = first.bounds().union(last.bounds());
        render(&mut r, &[base], &[base_batch.clone()], true);
        let original = r.readback_srgb_rgba8().unwrap();
        render(&mut r, &[first, last], &[stroke.clone()], false);
        let committed = r.readback_srgb_rgba8().unwrap();
        assert_ne!(original, committed, "{preset:?} must deposit");
        render(&mut r, &[base], &[base_batch], true);
        stroke.kind = DabBatchKind::Preview;
        stroke.stroke_end = false;
        render(&mut r, &[first, last], &[stroke.clone()], false);
        let predicted = r.readback_srgb_rgba8().unwrap();
        let maximum = predicted.iter().zip(&committed).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
        assert!(maximum <= 1, "{preset:?}: predicted vs committed maximum error {maximum}");
        for (x,y) in [(512,512), (100,100), (950,950)] {
            let i = (y*1024+x)*4;
            assert_eq!(&predicted[i..i+4], &original[i..i+4], "{preset:?}: untouched ({x}, {y})");
        }
        // Reuse the prediction pool with the opposite diagonal. The old
        // contacts remain inside the bounding rectangle but outside the new
        // sparse plan; they must resolve to persistent paint, not pooled pixels.
        first.center.y = 790.;
        last.center.y = 255.;
        stroke.damage = first.bounds().union(last.bounds());
        render(&mut r, &[first, last], &[stroke], false);
        let moved = r.readback_srgb_rgba8().unwrap();
        for (x, y) in [(260, 255), (770, 790)] {
            let i = (y * 1024 + x) * 4;
            assert_eq!(&moved[i..i+4], &original[i..i+4], "{preset:?}: retired prediction ({x}, {y})");
        }
        render(&mut r, &[], &[], false);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), original, "{preset:?}: cancel restores pixels");
    }
}

#[test]
fn constant_backdrop_sparse_updates_match_tiled_float_composition() {
    // Odd extents cover partial edge tiles; sparse edits must preserve the
    // distant tiles and the constant backdrop's premultiplied alpha.
    let extent = [519, 391];
    let mut r = WgpuRasterizer::new_float32().unwrap();
    for masked in [false, true] {
        let mut layer = Layer::paint(LayerId(1), "constant backdrop");
        layer.opacity = 0.63;
        if masked {
            let mut mask = left_mask(9);
            mask.inverted = true;
            layer.mask = Some(mask);
        }
        let layers = [layer];
        for (step, center) in [Point { x: 500., y: 365. }, Point { x: 251., y: 250. }, Point { x: 18., y: 22. }].into_iter().enumerate() {
            let mut ink = dab([0.8, 0.1, 0.3, 0.7]);
            ink.center = center;
            ink.radii = [34.; 2];
            ink.contact = [1., 0., 0., 0.];
            let mut stroke = batch(1);
            stroke.stroke_id = StrokeId(step as u64 + 1);
            stroke.style = preset_style(layer_core::DefaultBrushPreset::GPen);
            stroke.damage = ink.bounds();
            let packet = FramePacket {
                view: ViewState { width_px: extent[0], height_px: extent[1],
                    background_rgba_linear: [0.12, 0.25, 0.37, 0.5], ..view() },
                document_extent: extent, layers: &layers, dabs: &[ink], dab_batches: &[stroke],
                restore_rasters: &[], reset_layers: step == 0, composite_all: step == 0, time_seconds: 0.,
            };
            if let Some(scene) = &mut r.scene { scene.set_tiled_composition(false); }
            r.submit(packet).unwrap();
            let sparse = page_bytes(&r, r.composite_texture.as_ref().unwrap());
            // A blendable Float32 device may use the simple compositor for
            // the unmasked case. Instantiate the reference scene explicitly.
            if r.scene.is_none() { r.scene = Some(scene::Scene::new(&r)); }
            r.scene.as_mut().unwrap().set_tiled_composition(true);
            r.submit(FramePacket { dabs: &[], dab_batches: &[], reset_layers: false,
                composite_all: true, ..packet }).unwrap();
            let tiled = page_bytes(&r, r.composite_texture.as_ref().unwrap());
            let error = sparse.chunks_exact(4).zip(tiled.chunks_exact(4))
                .map(|(a,b)| (f32::from_le_bytes(a.try_into().unwrap()) - f32::from_le_bytes(b.try_into().unwrap())).abs())
                .fold(0., f32::max);
            assert!(error < 0.00001, "masked={masked}, step={step}: error={error}");
        }
    }
}
